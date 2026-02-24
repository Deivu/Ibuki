use crate::models::{ApiPlaylistInfo, ApiTrack, ApiTrackInfo, ApiTrackPlaylist, ApiTrackResult, Empty};
use crate::source::soundcloud::model::*;
use crate::source::soundcloud::{BASE_URL, BATCH_SIZE, SOUNDCLOUD_URL};
use crate::playback::hls::handler::start_hls_stream;
use crate::util::encoder::encode_track;
use crate::util::errors::ResolverError;
use crate::util::source::{Query, Source};
use async_trait::async_trait;
use regex::Regex;
use reqwest::Client;
use songbird::input::{HttpRequest, Input};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const CLIENT_ID_CACHE_DURATION: Duration = Duration::from_secs(7 * 24 * 60 * 60); // 7 days

#[derive(Debug, Clone)]
struct CachedClientId {
    id: String,
    cached_at: Instant,
}

#[derive(Clone)]
pub struct SoundCloud {
    client: Client,
    client_id_cache: Arc<Mutex<Option<CachedClientId>>>,
    config_client_id: Option<String>,
    search_prefix: &'static str,
    url_regex: Regex,
    search_url_regex: Regex,
}

impl SoundCloud {
    pub fn new(http: Option<Client>, config: Option<&crate::util::config::SoundCloudConfig>) -> Self {
        let config_client_id = config.and_then(|c| c.client_id.clone());

        let url_regex = Regex::new(
            r"^https?://(?:www\.|m\.)?soundcloud\.com/([^/\s]+)/(?:sets/)?([^/\s]+)(?:\?|$)"
        )
        .unwrap();

        let search_url_regex = Regex::new(
            r"^https?://(?:www\.)?soundcloud\.com/search(?:/(sounds|people|albums|sets))?(?:\?|$)"
        )
        .unwrap();

        Self {
            client: http.unwrap_or_else(|| Client::new()),
            client_id_cache: Arc::new(Mutex::new(None)),
            config_client_id,
            search_prefix: "scsearch",
            url_regex,
            search_url_regex,
        }
    }

    async fn get_client_id(&self) -> Result<String, ResolverError> {
        let mut cache = self.client_id_cache.lock().await;
        if let Some(ref cached) = *cache {
            if cached.cached_at.elapsed() < CLIENT_ID_CACHE_DURATION {
                return Ok(cached.id.clone());
            }
        }
        if let Some(ref id) = self.config_client_id {
            *cache = Some(CachedClientId {
                id: id.clone(),
                cached_at: Instant::now(),
            });
            return Ok(id.clone());
        }
        let client_id = self.fetch_client_id().await?;

        *cache = Some(CachedClientId {
            id: client_id.clone(),
            cached_at: Instant::now(),
        });
        Ok(client_id)
    }

    async fn fetch_client_id(&self) -> Result<String, ResolverError> {
        let response = self.client.get(SOUNDCLOUD_URL).send().await?;
        
        if !response.status().is_success() {
            return Err(ResolverError::FailedStatusCode(response.status().to_string()));
        }

        let html = response.text().await?;

        let client_id_regex = Regex::new(
            r#"(?:[?&/]?(?:client_id)[\s:=&]*"?|"data":\{"id":")([A-Za-z0-9]{32})"?"#
        )
        .unwrap();

        if let Some(caps) = client_id_regex.captures(&html) {
            if let Some(id) = caps.get(1) {
                return Ok(id.as_str().to_string());
            }
        }

        let asset_regex = Regex::new(r"https://a-v2\.sndcdn\.com/assets/[a-zA-Z0-9-]+\.js").unwrap();
        let asset_urls: Vec<String> = asset_regex
            .find_iter(&html)
            .map(|m| m.as_str().to_string())
            .collect();

        if asset_urls.is_empty() {
            return Err(ResolverError::MissingRequiredData(
                "No asset URLs found in SoundCloud main page",
            ));
        }

        for asset_url in asset_urls {
            match self.client.get(&asset_url).send().await {
                Ok(response) if response.status().is_success() => {
                    if let Ok(js_content) = response.text().await {
                        if let Some(caps) = client_id_regex.captures(&js_content) {
                            if let Some(id) = caps.get(1) {
                                return Ok(id.as_str().to_string());
                            }
                        }
                    }
                }
                _ => continue,
            }
        }

        Err(ResolverError::MissingRequiredData(
            "Failed to extract client ID from SoundCloud",
        ))
    }

    fn parse_search_type(&self, query: &str) -> (String, String) {
        let mut search_query = query.trim().to_string();
        let mut search_type = "tracks".to_string();

        // Remove scsearch prefix
        if let Some(stripped) = search_query.strip_prefix("scsearch:") {
            search_query = stripped.to_string();
        } else if let Some(stripped) = search_query.strip_prefix("scsearch") {
            search_query = stripped.to_string();
        }

        search_query = search_query.trim().to_string();

        // Check for type prefix (e.g., "users:Madonna")
        if let Some(colon_pos) = search_query.find(':') {
            if colon_pos > 0 && colon_pos <= 12 {
                let possible_type = search_query[..colon_pos].to_lowercase();
                let normalized_type = match possible_type.as_str() {
                    "track" | "tracks" | "sounds" | "sound" => "tracks",
                    "user" | "users" | "people" | "artist" | "artists" => "users",
                    "album" | "albums" => "albums",
                    "playlist" | "playlists" | "set" | "sets" => "playlists",
                    "all" | "everything" => "all",
                    _ => "",
                };

                if !normalized_type.is_empty() {
                    search_type = normalized_type.to_string();
                    search_query = search_query[(colon_pos + 1)..].trim().to_string();
                }
            }
        }

        (search_type, search_query)
    }

    async fn perform_search(&self, query: &str) -> Result<Option<ApiTrackResult>, ResolverError> {
        let (search_type, search_query) = self.parse_search_type(query);

        if search_query.is_empty() {
            return Ok(Some(ApiTrackResult::Empty(None)));
        }

        let client_id = self.get_client_id().await?;
        let endpoint = match search_type.as_str() {
            "users" => "/search/users",
            "albums" => "/search/albums",
            "playlists" => "/search/playlists",
            "all" => "/search",
            _ => "/search/tracks",
        };

        let url = format!("{}{}", BASE_URL, endpoint);
        let response = self
            .client
            .get(&url)
            .query(&[
                ("q", search_query.as_str()),
                ("client_id", &client_id),
                ("limit", "50"),
                ("offset", "0"),
                ("linked_partitioning", "1"),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(ResolverError::FailedStatusCode(response.status().to_string()));
        }

        let search_response: SearchResponse = response.json().await?;

        if search_response.collection.is_empty() {
            return Ok(Some(ApiTrackResult::Empty(None)));
        }

        let tracks = self.process_search_results(&search_response.collection, &search_type)?;

        if tracks.is_empty() {
            return Ok(Some(ApiTrackResult::Empty(None)));
        }

        Ok(Some(ApiTrackResult::Search(tracks)))
    }

    fn process_search_results(
        &self,
        collection: &[serde_json::Value],
        search_type: &str,
    ) -> Result<Vec<ApiTrack>, ResolverError> {
        let mut results = Vec::new();

        for item in collection.iter().take(50) {
            let kind = item.get("kind").and_then(|k| k.as_str()).unwrap_or("");

            match search_type {
                "tracks" => {
                    if kind == "track" {
                        if let Ok(track) = serde_json::from_value::<Track>(item.clone()) {
                            results.push(self.build_track(&track));
                        }
                    }
                }
                "users" => {
                    if kind == "user" {
                        if let Ok(user) = serde_json::from_value::<User>(item.clone()) {
                            results.push(self.build_user_track(&user));
                        }
                    }
                }
                "albums" | "playlists" => {
                    if kind == "playlist" {
                        if let Ok(playlist) = serde_json::from_value::<Playlist>(item.clone()) {
                            results.push(self.build_playlist_track(&playlist, search_type == "albums"));
                        }
                    }
                }
                "all" => {
                    match kind {
                        "track" => {
                            if let Ok(track) = serde_json::from_value::<Track>(item.clone()) {
                                results.push(self.build_track(&track));
                            }
                        }
                        "user" => {
                            if let Ok(user) = serde_json::from_value::<User>(item.clone()) {
                                results.push(self.build_user_track(&user));
                            }
                        }
                        "playlist" => {
                            if let Ok(playlist) = serde_json::from_value::<Playlist>(item.clone()) {
                                let is_album = playlist.is_album.unwrap_or(false);
                                results.push(self.build_playlist_track(&playlist, is_album));
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        Ok(results)
    }

    fn build_track(&self, track: &Track) -> ApiTrack {
        let info = ApiTrackInfo {
            identifier: track.id.to_string(),
            is_seekable: true,
            author: track
                .user
                .as_ref()
                .map(|u| u.username.clone())
                .unwrap_or_else(|| "Unknown".to_string()),
            length: track.duration.unwrap_or(0),
            is_stream: false,
            position: 0,
            title: track.title.clone(),
            uri: track.permalink_url.clone(),
            artwork_url: track.artwork_url.clone(),
            isrc: track
                .publisher_metadata
                .as_ref()
                .and_then(|pm| pm.isrc.clone()),
            source_name: "soundcloud".to_string(),
        };

        ApiTrack {
            encoded: encode_track(&info).unwrap_or_default(),
            info,
            plugin_info: Empty,
        }
    }

    fn build_user_track(&self, user: &User) -> ApiTrack {
        let info = ApiTrackInfo {
            identifier: user.id.to_string(),
            is_seekable: false,
            author: "SoundCloud".to_string(),
            length: 0,
            is_stream: false,
            position: 0,
            title: user.username.clone(),
            uri: user.permalink_url.clone(),
            artwork_url: user.avatar_url.clone(),
            isrc: None,
            source_name: "soundcloud".to_string(),
        };

        ApiTrack {
            encoded: encode_track(&info).unwrap_or_default(),
            info,
            plugin_info: Empty,
        }
    }

    fn build_playlist_track(&self, playlist: &Playlist, _is_album: bool) -> ApiTrack {
        let info = ApiTrackInfo {
            identifier: playlist.id.to_string(),
            is_seekable: true,
            author: playlist
                .user
                .as_ref()
                .map(|u| u.username.clone())
                .unwrap_or_else(|| "Unknown".to_string()),
            length: 0,
            is_stream: false,
            position: 0,
            title: playlist.title.clone(),
            uri: playlist.permalink_url.clone(),
            artwork_url: playlist.artwork_url.clone(),
            isrc: None,
            source_name: "soundcloud".to_string(),
        };

        ApiTrack {
            encoded: encode_track(&info).unwrap_or_else(|e| {
                eprintln!("Failed to encode SoundCloud playlist '{}' (id: {}): {:?}", info.title, info.identifier, e);
                String::new()
            }),
            info,
            plugin_info: Empty,
        }
    }

    async fn resolve_track(&self, track_id: &str) -> Result<Option<ApiTrackResult>, ResolverError> {
        let client_id = self.get_client_id().await?;
        let url = format!("{}/tracks/{}", BASE_URL, track_id);

        let response = self
            .client
            .get(&url)
            .query(&[("client_id", &client_id)])
            .send()
            .await?;

        if response.status().as_u16() == 404 {
            return Ok(Some(ApiTrackResult::Empty(None)));
        }

        if !response.status().is_success() {
            return Err(ResolverError::FailedStatusCode(response.status().to_string()));
        }

        let track: Track = response.json().await?;
        Ok(Some(ApiTrackResult::Track(self.build_track(&track))))
    }

    async fn resolve_playlist(&self, url: &str) -> Result<Option<ApiTrackResult>, ResolverError> {
        let client_id = self.get_client_id().await?;
        let resolve_url = format!("{}/resolve", BASE_URL);

        let response = self
            .client
            .get(&resolve_url)
            .query(&[("url", url), ("client_id", &client_id)])
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(ResolverError::FailedStatusCode(response.status().to_string()));
        }

        let playlist: Playlist = response.json().await?;
        let mut complete_tracks: Vec<Track> = Vec::new();
        let mut track_ids = Vec::new();

        // Separate complete tracks from stubs
        if let Some(tracks) = &playlist.tracks {
            for track_or_stub in tracks {
                match track_or_stub {
                    TrackOrStub::Track(track) => complete_tracks.push((*track).clone()),
                    TrackOrStub::Stub(stub) => track_ids.push(stub.id),
                }
            }
        }

        // Fetch missing tracks in batches
        if !track_ids.is_empty() {
            for chunk in track_ids.chunks(BATCH_SIZE) {
                let ids = chunk
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>()
                    .join(",");

                let batch_url = format!("{}/tracks", BASE_URL);
                match self
                    .client
                    .get(&batch_url)
                    .query(&[("ids", &ids), ("client_id", &client_id)])
                    .send()
                    .await
                {
                    Ok(response) if response.status().is_success() => {
                        if let Ok(batch_tracks) = response.json::<Vec<Track>>().await {
                            complete_tracks.extend(batch_tracks);
                        }
                    }
                    _ => {}
                }
            }
        }

        let tracks: Vec<ApiTrack> = complete_tracks
            .iter()
            .take(500) // Max playlist length
            .map(|t| self.build_track(t))
            .collect();

        Ok(Some(ApiTrackResult::Playlist(ApiTrackPlaylist {
            info: ApiPlaylistInfo {
                name: playlist.title.clone(),
                selected_track: 0,
            },
            plugin_info: Empty,
            tracks,
        })))
    }

    async fn select_transcoding(&self, track: &Track) -> Result<(String, String, String), ResolverError> {
        let transcodings = track
            .media
            .as_ref()
            .ok_or_else(|| ResolverError::MissingRequiredData("No media data"))?
            .transcodings
            .as_slice();

        if transcodings.is_empty() {
            return Err(ResolverError::MissingRequiredData("No transcodings available"));
        }

        // Priority order: progressive MP3 > progressive AAC > any progressive > HLS (as fallback)
        // Note: Progressive streams work better than HLS for symphonia probing
        // We accept preview URLs if that's all SoundCloud provides
        let progressive_mp3 = transcodings.iter().find(|t| {
            t.format.protocol == "progressive" 
                && t.format.mime_type.contains("mpeg")
        });

        let progressive_aac = transcodings.iter().find(|t| {
            t.format.protocol == "progressive" 
                && t.format.mime_type.contains("aac")
        });

        let any_progressive = transcodings.iter().find(|t| {
            t.format.protocol == "progressive"
        });

        let hls_aac_high = transcodings.iter().find(|t| {
            t.format.protocol == "hls"
                && (t.format.mime_type.contains("aac") || t.format.mime_type.contains("mp4"))
                && (t.quality.as_deref() == Some("hq")
                    || t.preset.as_ref().map_or(false, |p| p.contains("160"))
                    || t.url.contains("160"))
        });

        let hls_aac_standard = transcodings.iter().find(|t| {
            t.format.protocol == "hls"
                && (t.format.mime_type.contains("aac") || t.format.mime_type.contains("mp4"))
        });

        let any_hls = transcodings.iter().find(|t| {
            t.format.protocol == "hls"
        });

        // Prioritize progressive over HLS since progressive works better with symphonia probe
        let selected = progressive_mp3
            .or(progressive_aac)
            .or(any_progressive)
            .or(hls_aac_high)
            .or(hls_aac_standard)
            .or(any_hls)
            .or_else(|| transcodings.first())
            .ok_or_else(|| ResolverError::MissingRequiredData("No suitable transcoding found"))?;

        let client_id = self.get_client_id().await?;
        let stream_auth_url = format!("{}?client_id={}", selected.url, client_id);

        let response = self.client.get(&stream_auth_url).send().await?;
        let status = response.status();
        let final_url = if response.url().as_str() != stream_auth_url {
            response.url().to_string()
        } else if status.is_redirection() {
            response
                .headers()
                .get("location")
                .and_then(|h| h.to_str().ok())
                .ok_or_else(|| ResolverError::MissingRequiredData("No redirect location"))?
                .to_string()
        } else if status.is_success() {
            let content_type = response
                .headers()
                .get("content-type")
                .and_then(|h| h.to_str().ok())
                .unwrap_or("");

            if content_type.contains("application/json") {
                match response.json::<StreamAuthResponse>().await {
                    Ok(auth_response) => auth_response.url,
                    Err(_) => stream_auth_url.clone(),
                }
            } else {
                stream_auth_url
            }
        } else {
            return Err(ResolverError::FailedStatusCode(status.to_string()));
        };

        let mime_type = selected.format.mime_type.to_lowercase();
        let protocol = selected.format.protocol.clone();
        let format = if mime_type.contains("mpeg") {
            "mp3"
        } else if mime_type.contains("aac") || mime_type.contains("mp4") {
            if protocol == "hls" {
                "aac_hls"
            } else {
                "m4a"
            }
        } else if mime_type.contains("opus") {
            "opus"
        } else {
            "arbitrary"
        };

        Ok((final_url, protocol, format.to_string()))
    }
}

#[async_trait]
impl Source for SoundCloud {
    fn get_name(&self) -> &'static str {
        "soundcloud"
    }

    fn get_client(&self) -> Client {
        self.client.clone()
    }

    async fn init(&self) -> Result<(), ResolverError> {
        let _ = self.get_client_id().await;
        Ok(())
    }

    fn parse_query(&self, url: &str) -> Option<Query> {
        if url.starts_with(self.search_prefix) {
            return Some(Query::Search(url.to_string()));
        }
        if self.url_regex.is_match(url) || self.search_url_regex.is_match(url) {
            return Some(Query::Url(url.to_string()));
        }

        None
    }

    async fn resolve(&self, query: Query) -> Result<Option<ApiTrackResult>, ResolverError> {
        match query {
            Query::Url(url) => {
                if let Some(caps) = self.search_url_regex.captures(&url) {
                    let search_type = caps.get(1).map(|m| m.as_str()).unwrap_or("tracks");
                    if let Ok(parsed_url) = url::Url::parse(&url) {
                        if let Some(query) = parsed_url
                            .query_pairs()
                            .find(|(k, _)| k == "q")
                            .map(|(_, v)| v.to_string())
                        {
                            let search_query = format!("{}:{}", search_type, query);
                            return self.perform_search(&search_query).await;
                        }
                    }
                    return Ok(Some(ApiTrackResult::Empty(None)));
                }

                let client_id = self.get_client_id().await?;
                let resolve_url = format!("{}/resolve", BASE_URL);

                let response = self
                    .client
                    .get(&resolve_url)
                    .query(&[("url", &url), ("client_id", &client_id)])
                    .send()
                    .await?;

                if response.status().as_u16() == 404 {
                    return Ok(Some(ApiTrackResult::Empty(None)));
                }

                if !response.status().is_success() {
                    return Err(ResolverError::FailedStatusCode(
                        response.status().to_string(),
                    ));
                }

                let resolve_response: serde_json::Value = response.json().await?;
                let kind = resolve_response
                    .get("kind")
                    .and_then(|k| k.as_str())
                    .unwrap_or("");

                match kind {
                    "track" => {
                        let track: Track = serde_json::from_value(resolve_response)?;
                        Ok(Some(ApiTrackResult::Track(self.build_track(&track))))
                    }
                    "playlist" => self.resolve_playlist(&url).await,
                    _ => Ok(Some(ApiTrackResult::Empty(None))),
                }
            }
            Query::Search(input) => self.perform_search(&input).await,
        }
    }

    async fn make_playable(&self, track: ApiTrack) -> Result<Input, ResolverError> {
        let client_id = self.get_client_id().await?;
        let tracks_url = format!("{}/tracks/{}", BASE_URL, track.info.identifier);

        let response = self
            .client
            .get(&tracks_url)
            .query(&[("client_id", &client_id)])
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(ResolverError::FailedStatusCode(
                response.status().to_string(),
            ));
        }

        let track_data: Track = response.json().await?;
        let (url, protocol, _format) = self.select_transcoding(&track_data).await?;

        // For progressive streams, use simple HTTP request
        // For HLS streams, use HLS handler (though progressive is preferred due to probe compatibility)
        if protocol == "hls" {
            Ok(start_hls_stream(url, self.client.clone()).await)
        } else {
            Ok(Input::from(HttpRequest::new(self.client.clone(), url)))
        }
    }
}

