use super::model::*;
use super::*;
use crate::models::{
    ApiPlaylistInfo, ApiTrack, ApiTrackInfo, ApiTrackPlaylist, ApiTrackResult, Empty,
};
use crate::util::encoder::encode_track;
use crate::util::errors::ResolverError;
use crate::util::source::{Query, Source};
use crate::util::url::is_url;
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose};
use regex::Regex;
use reqwest::Client;
use songbird::input::Input;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const TOKEN_VALIDITY_BUFFER_MS: u64 = 10_000; // 10 seconds

pub struct AppleMusic {
    client: Client,
    url_regex: Regex,
    search_prefix: &'static str,
    token_cache: Arc<Mutex<Option<CachedToken>>>,
    config_token: Option<String>,
    country: String,
    playlist_page_limit: u32,
    album_page_limit: u32,
    allow_explicit: bool,
}

impl AppleMusic {
    pub fn new(
        client: Option<Client>,
        config: Option<&crate::util::config::AppleMusicConfig>,
    ) -> Self {
        let country = config
            .and_then(|c| c.market.clone())
            .unwrap_or_else(|| "US".to_string());

        let playlist_page_limit = config.and_then(|c| c.playlist_load_limit).unwrap_or(0);

        let album_page_limit = config.and_then(|c| c.album_load_limit).unwrap_or(0);

        let allow_explicit = config.and_then(|c| c.allow_explicit).unwrap_or(true);

        let config_token = config
            .and_then(|c| c.media_api_token.clone())
            .filter(|t| t != "token_here" && !t.is_empty());

        Self {
            client: client.unwrap_or_default(),
            url_regex: Regex::new(
                r"(?i)^https?://(?:www\.)?music\.apple\.com/(?:[a-zA-Z]{2}/)?(album|playlist|artist|song)/[^/]+/([a-zA-Z0-9\-.]+)(?:\?i=(\d+))?"
            )
            .expect("Failed to init AppleMusic URL RegEx"),
            search_prefix: "amsearch",
            token_cache: Arc::new(Mutex::new(None)),
            config_token,
            country,
            playlist_page_limit,
            album_page_limit,
            allow_explicit,
        }
    }

    async fn get_token(&self) -> Result<CachedToken, ResolverError> {
        let mut cache = self.token_cache.lock().await;

        if let Some(ref cached) = *cache {
            if self.is_token_valid(cached) {
                return Ok(cached.clone());
            }
        }

        if let Some(ref token_str) = self.config_token {
            tracing::info!("Using configured Apple Music token from config");
            let (origin, expiry) = self.parse_token(token_str);
            let token = CachedToken {
                token: token_str.clone(),
                origin,
                expiry,
                cached_at: Instant::now(),
            };

            if self.is_token_valid(&token) {
                *cache = Some(token.clone());
                return Ok(token);
            } else {
                tracing::warn!("Configured token is expired, fetching new token");
            }
        }

        let token = self.fetch_new_token().await?;
        *cache = Some(token.clone());
        Ok(token)
    }

    fn is_token_valid(&self, token: &CachedToken) -> bool {
        if let Some(expiry) = token.expiry {
            Instant::now() < expiry
        } else {
            true // No expiry means token is always valid
        }
    }

    async fn fetch_new_token(&self) -> Result<CachedToken, ResolverError> {
        tracing::info!("Fetching new Apple Music Media API token...");

        let html_response = self
            .client
            .get("https://music.apple.com/us/browse")
            .send()
            .await?;

        if !html_response.status().is_success() {
            return Err(ResolverError::FailedStatusCode(
                html_response.status().to_string(),
            ));
        }

        let html = html_response.text().await?;

        let script_tag_regex =
            Regex::new(r#"<script\s+type="module"\s+crossorigin\s+src="([^"]+)""#).unwrap();
        let script_tag = script_tag_regex
            .captures(&html)
            .and_then(|cap| cap.get(1))
            .ok_or_else(|| {
                ResolverError::MissingRequiredData(
                    "Module script tag not found in Apple Music HTML",
                )
            })?;

        let script_url = format!("https://music.apple.com{}", script_tag.as_str());
        let js_response = self.client.get(&script_url).send().await?;

        if !js_response.status().is_success() {
            return Err(ResolverError::FailedStatusCode(
                js_response.status().to_string(),
            ));
        }

        let js_data = js_response.text().await?;
        let token_regex = Regex::new(r"(ey[\w-]+\.[\w-]+\.[\w-]+)").unwrap();
        let token = token_regex
            .captures(&js_data)
            .and_then(|cap| cap.get(1))
            .ok_or_else(|| ResolverError::MissingRequiredData("Access token not found in JS file"))?
            .as_str()
            .to_string();

        tracing::info!("Successfully fetched new Apple Music Media API token");

        let (origin, expiry) = self.parse_token(&token);

        Ok(CachedToken {
            token,
            origin,
            expiry,
            cached_at: Instant::now(),
        })
    }

    fn parse_token(&self, token: &str) -> (Option<String>, Option<Instant>) {
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() < 2 {
            return (None, None);
        }

        let payload_b64 = parts[1].replace('-', "+").replace('_', "/");
        let padding = (4 - payload_b64.len() % 4) % 4;
        let padded = format!("{}{}", payload_b64, "=".repeat(padding));

        match general_purpose::STANDARD.decode(padded) {
            Ok(decoded) => {
                if let Ok(json_str) = String::from_utf8(decoded) {
                    if let Ok(payload) = serde_json::from_str::<TokenPayload>(&json_str) {
                        let origin = payload.root_https_origin.and_then(|mut o| o.pop());
                        let expiry = payload.exp.and_then(|exp| {
                            let exp_millis = (exp as u64) * 1000;
                            let now_millis = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .ok()?
                                .as_millis() as u64;
                            let buffer = TOKEN_VALIDITY_BUFFER_MS;
                            let remaining = exp_millis.saturating_sub(now_millis + buffer);
                            Some(Instant::now() + Duration::from_millis(remaining))
                        });
                        return (origin, expiry);
                    }
                }
            }
            Err(_) => return (None, None),
        }

        (None, None)
    }

    async fn api_request<T: for<'de> serde::Deserialize<'de>>(
        &self,
        path: &str,
    ) -> Result<T, ResolverError> {
        self.api_request_with_retry(path, 0).await
    }

    async fn api_request_with_retry<T: for<'de> serde::Deserialize<'de>>(
        &self,
        path: &str,
        retry_count: u32,
    ) -> Result<T, ResolverError> {
        const MAX_RETRIES: u32 = 2;

        let token = self.get_token().await?;

        let url = if path.starts_with("http") {
            path.to_string()
        } else {
            format!("{}{}", API_BASE, path)
        };

        tracing::debug!("Apple Music API request: {} (retry: {})", url, retry_count);

        let mut request = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token.token))
            .header("Accept", "application/json");

        if let Some(ref origin) = token.origin {
            request = request.header("Origin", format!("https://{}", origin));
        }

        let response = request.send().await?;
        let status = response.status();

        tracing::debug!("Apple Music API response status: {}", status);

        if status.as_u16() == 401 {
            if retry_count >= MAX_RETRIES {
                tracing::error!(
                    "Apple Music API returned 401 after {} retries, giving up",
                    retry_count
                );
                return Err(ResolverError::FailedStatusCode(format!(
                    "401 Unauthorized after {} retries",
                    retry_count
                )));
            }

            tracing::warn!(
                "Apple Music API returned 401, clearing cache and retrying (attempt {}/{})",
                retry_count + 1,
                MAX_RETRIES
            );
            let mut cache = self.token_cache.lock().await;
            *cache = None;
            drop(cache);
            return Box::pin(self.api_request_with_retry(path, retry_count + 1)).await;
        }

        if !status.is_success() {
            tracing::error!("Apple Music API error: {} for URL: {}", status, url);
            return Err(ResolverError::FailedStatusCode(status.to_string()));
        }

        let data = response.json::<T>().await?;
        Ok(data)
    }

    fn build_track(
        &self,
        song: &Song,
        artwork_override: Option<String>,
    ) -> Result<ApiTrack, ResolverError> {
        let attributes = song
            .attributes
            .as_ref()
            .ok_or_else(|| ResolverError::MissingRequiredData("Song attributes missing"))?;

        let artwork = artwork_override.or_else(|| self.parse_artwork(attributes.artwork.as_ref()));
        let is_explicit = attributes.content_rating.as_deref() == Some("explicit");

        let mut track_uri = attributes.url.clone().unwrap_or_default();
        if !track_uri.is_empty() {
            let separator = if track_uri.contains('?') { "&" } else { "?" };
            track_uri = format!("{}{}explicit={}", track_uri, separator, is_explicit);
        }

        let info = ApiTrackInfo {
            identifier: song.id.clone(),
            is_seekable: true,
            author: attributes
                .artist_name
                .clone()
                .unwrap_or_else(|| "Unknown".to_string()),
            length: attributes.duration_in_millis.unwrap_or(0),
            is_stream: false,
            position: 0,
            title: attributes.name.clone(),
            uri: Some(track_uri),
            artwork_url: artwork,
            isrc: attributes.isrc.clone(),
            source_name: "applemusic".to_string(),
        };

        Ok(ApiTrack {
            encoded: encode_track(&info)?,
            info,
            plugin_info: Empty,
            user_data: None,
        })
    }

    fn parse_artwork(&self, artwork: Option<&Artwork>) -> Option<String> {
        artwork.and_then(|art| {
            art.url.as_ref().map(|url| {
                url.replace("{w}", &art.width.unwrap_or(1000).to_string())
                    .replace("{h}", &art.height.unwrap_or(1000).to_string())
            })
        })
    }

    async fn resolve_track(&self, id: &str) -> Result<Option<ApiTrackResult>, ResolverError> {
        let response: ApiResponse<Song> = self
            .api_request(&format!(
                "/catalog/{}/songs/{}?extend=artistUrl",
                self.country, id
            ))
            .await?;

        if let Some(data) = response.data {
            if let Some(song) = data.first() {
                let track = self.build_track(song, None)?;
                return Ok(Some(ApiTrackResult::Track(track)));
            }
        }

        Ok(Some(ApiTrackResult::Empty(None)))
    }

    async fn resolve_album(&self, id: &str) -> Result<Option<ApiTrackResult>, ResolverError> {
        let response: ApiResponse<Album> = self
            .api_request(&format!(
                "/catalog/{}/albums/{}?extend=artistUrl",
                self.country, id
            ))
            .await?;

        let album = response
            .data
            .and_then(|data| data.into_iter().next())
            .ok_or_else(|| ResolverError::MissingRequiredData("Album not found"))?;

        let base_tracks = album
            .relationships
            .as_ref()
            .and_then(|rel| rel.tracks.as_ref())
            .and_then(|tracks| tracks.data.as_ref())
            .map(|v| v.clone())
            .unwrap_or_default();

        let total = album
            .relationships
            .as_ref()
            .and_then(|rel| rel.tracks.as_ref())
            .and_then(|tracks| tracks.meta.as_ref())
            .and_then(|meta| meta.total)
            .unwrap_or(base_tracks.len() as u32);

        let extra_tracks = self
            .paginate(
                &format!("/catalog/{}/albums/{}/tracks", self.country, id),
                total,
                self.album_page_limit,
            )
            .await?;

        let all_tracks = [base_tracks, extra_tracks].concat();

        let artwork = self.parse_artwork(
            album
                .attributes
                .as_ref()
                .and_then(|attr| attr.artwork.as_ref()),
        );

        let tracks = all_tracks
            .iter()
            .map(|song| self.build_track(song, artwork.clone()))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Some(ApiTrackResult::Playlist(ApiTrackPlaylist {
            info: ApiPlaylistInfo {
                name: album
                    .attributes
                    .map(|attr| attr.name)
                    .unwrap_or_else(|| "Unknown Album".to_string()),
                selected_track: 0,
            },
            plugin_info: Empty,
            tracks,
        })))
    }

    async fn resolve_playlist(&self, id: &str) -> Result<Option<ApiTrackResult>, ResolverError> {
        let response: ApiResponse<Playlist> = self
            .api_request(&format!("/catalog/{}/playlists/{}", self.country, id))
            .await?;

        let playlist = response
            .data
            .and_then(|data| data.into_iter().next())
            .ok_or_else(|| ResolverError::MissingRequiredData("Playlist not found"))?;

        let base_tracks = playlist
            .relationships
            .as_ref()
            .and_then(|rel| rel.tracks.as_ref())
            .and_then(|tracks| tracks.data.as_ref())
            .map(|v| v.clone())
            .unwrap_or_default();

        let total = playlist
            .relationships
            .as_ref()
            .and_then(|rel| rel.tracks.as_ref())
            .and_then(|tracks| tracks.meta.as_ref())
            .and_then(|meta| meta.total)
            .unwrap_or(base_tracks.len() as u32);

        let extra_tracks = self
            .paginate(
                &format!(
                    "/catalog/{}/playlists/{}/tracks?extend=artistUrl",
                    self.country, id
                ),
                total,
                self.playlist_page_limit,
            )
            .await?;

        let all_tracks = [base_tracks, extra_tracks].concat();

        let artwork = self.parse_artwork(
            playlist
                .attributes
                .as_ref()
                .and_then(|attr| attr.artwork.as_ref()),
        );

        let tracks = all_tracks
            .iter()
            .map(|song| self.build_track(song, artwork.clone()))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Some(ApiTrackResult::Playlist(ApiTrackPlaylist {
            info: ApiPlaylistInfo {
                name: playlist
                    .attributes
                    .map(|attr| attr.name)
                    .unwrap_or_else(|| "Unknown Playlist".to_string()),
                selected_track: 0,
            },
            plugin_info: Empty,
            tracks,
        })))
    }

    async fn resolve_artist(&self, id: &str) -> Result<Option<ApiTrackResult>, ResolverError> {
        let top_tracks_response: ApiResponse<Song> = self
            .api_request(&format!(
                "/catalog/{}/artists/{}/view/top-songs",
                self.country, id
            ))
            .await?;

        let songs = top_tracks_response
            .data
            .ok_or_else(|| ResolverError::MissingRequiredData("Artist not found"))?;

        let artist_response: ApiResponse<Artist> = self
            .api_request(&format!("/catalog/{}/artists/{}", self.country, id))
            .await?;

        let artist = artist_response
            .data
            .and_then(|data| data.into_iter().next());

        let artist_name = artist
            .as_ref()
            .and_then(|a| a.attributes.as_ref())
            .map(|attr| attr.name.clone())
            .unwrap_or_else(|| "Artist".to_string());

        let artwork = artist
            .as_ref()
            .and_then(|a| a.attributes.as_ref())
            .and_then(|attr| self.parse_artwork(attr.artwork.as_ref()));

        let tracks = songs
            .iter()
            .map(|song| self.build_track(song, artwork.clone()))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Some(ApiTrackResult::Playlist(ApiTrackPlaylist {
            info: ApiPlaylistInfo {
                name: format!("{}'s Top Tracks", artist_name),
                selected_track: 0,
            },
            plugin_info: Empty,
            tracks,
        })))
    }

    async fn paginate(
        &self,
        base_path: &str,
        total_items: u32,
        max_pages: u32,
    ) -> Result<Vec<Song>, ResolverError> {
        let pages = (total_items as f32 / MAX_PAGE_ITEMS as f32).ceil() as u32;

        let allowed_pages = if max_pages > 0 {
            pages.min(max_pages)
        } else {
            pages
        };

        if allowed_pages <= 1 {
            return Ok(Vec::new());
        }

        let mut results = Vec::new();

        for index in 1..allowed_pages {
            let offset = index * MAX_PAGE_ITEMS;
            let separator = if base_path.contains('?') { "&" } else { "?" };
            let path = format!(
                "{}{}limit={}&offset={}",
                base_path, separator, MAX_PAGE_ITEMS, offset
            );

            match self.api_request::<ApiResponse<Song>>(&path).await {
                Ok(response) => {
                    if let Some(data) = response.data {
                        results.extend(data);
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to fetch page {}: {:?}", index, e);
                }
            }
        }

        Ok(results)
    }

    async fn perform_search(&self, query: &str) -> Result<Option<ApiTrackResult>, ResolverError> {
        let limit = 10; // Default limit
        let encoded_query = urlencoding::encode(query);

        let response: ApiResponse<Song> = self
            .api_request(&format!(
                "/catalog/{}/search?term={}&limit={}&types=songs&extend=artistUrl",
                self.country, encoded_query, limit
            ))
            .await?;

        let songs = response
            .results
            .and_then(|results| results.songs)
            .and_then(|songs| songs.data)
            .unwrap_or_default();

        if songs.is_empty() {
            return Ok(Some(ApiTrackResult::Empty(None)));
        }

        let tracks = songs
            .iter()
            .map(|song| self.build_track(song, None))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Some(ApiTrackResult::Search(tracks)))
    }
}

#[async_trait]
impl Source for AppleMusic {
    fn get_name(&self) -> &'static str {
        "applemusic"
    }

    fn get_client(&self) -> Client {
        self.client.clone()
    }

    fn parse_query(&self, query: &str) -> Option<Query> {
        if !is_url(query) {
            if query.starts_with(self.search_prefix) {
                return Some(Query::Search(query.to_string()));
            } else {
                return None;
            }
        }

        if self.url_regex.is_match(query) {
            return Some(Query::Url(query.to_string()));
        }

        None
    }

    async fn init(&self) -> Result<(), ResolverError> {
        self.get_token().await?;
        tracing::info!("Apple Music Source initialized successfully");
        Ok(())
    }

    async fn resolve(&self, query: Query) -> Result<Option<ApiTrackResult>, ResolverError> {
        match query {
            Query::Url(url) => {
                let captures = match self.url_regex.captures(&url) {
                    Some(c) => c,
                    None => return Ok(Some(ApiTrackResult::Empty(None))),
                };

                let url_type = captures.get(1).map(|m| m.as_str()).unwrap_or("");
                let id = captures.get(2).map(|m| m.as_str()).unwrap_or("");
                let alt_track_id = captures.get(3).map(|m| m.as_str());

                tracing::debug!("Resolving Apple Music URL: type={}, id={}", url_type, id);

                match url_type {
                    "song" => self.resolve_track(id).await,
                    "album" => {
                        if let Some(track_id) = alt_track_id {
                            self.resolve_track(track_id).await
                        } else {
                            self.resolve_album(id).await
                        }
                    }
                    "playlist" => self.resolve_playlist(id).await,
                    "artist" => self.resolve_artist(id).await,
                    _ => Ok(Some(ApiTrackResult::Empty(None))),
                }
            }
            Query::Search(input) => {
                let term = if input.starts_with(self.search_prefix) {
                    let stripped = input.strip_prefix(self.search_prefix).unwrap_or(&input);
                    stripped.strip_prefix(':').unwrap_or(stripped)
                } else {
                    &input
                };

                self.perform_search(term).await
            }
        }
    }

    async fn make_playable(&self, track: ApiTrack) -> Result<Input, ResolverError> {
        if crate::SOURCES.get("youtube").is_none() {
            tracing::warn!(
                "AppleMusic needs YouTube source for playback fallbacks, but it is not available"
            );
            return Err(ResolverError::MissingRequiredData(
                "YouTube source not configured - required for Apple Music playback",
            ));
        }

        let is_explicit = track
            .info
            .uri
            .as_ref()
            .and_then(|uri| {
                url::Url::parse(uri).ok().and_then(|u| {
                    u.query_pairs()
                        .find(|(k, _)| k == "explicit")
                        .map(|(_, v)| v == "true")
                })
            })
            .unwrap_or(false);

        let mut search_query = format!("{} {}", track.info.title, track.info.author);
        if is_explicit {
            search_query += if self.allow_explicit {
                " official video"
            } else {
                " clean version"
            };
        }

        tracing::debug!("AppleMusic: Searching for playback: {}", search_query);

        if let Some(ref isrc) = track.info.isrc {
            let isrc_query = format!("ytmsearch:\"{}\"", isrc);
            if let Some(youtube) = crate::SOURCES.get("youtube") {
                if let Some(Query::Search(q)) = youtube.to_inner_ref().parse_query(&isrc_query) {
                    if let Ok(Some(ApiTrackResult::Search(tracks))) =
                        youtube.to_inner_ref().resolve(Query::Search(q)).await
                    {
                        if let Some(first) = tracks.into_iter().next() {
                            tracing::debug!("AppleMusic: Found via ISRC: {}", first.info.title);
                            return youtube.to_inner_ref().make_playable(first).await;
                        }
                    }
                }
            }
        }

        let ytm_query = format!("ytmsearch:{}", search_query);
        if let Some(youtube) = crate::SOURCES.get("youtube") {
            if let Some(Query::Search(q)) = youtube.to_inner_ref().parse_query(&ytm_query) {
                match youtube.to_inner_ref().resolve(Query::Search(q)).await {
                    Ok(Some(ApiTrackResult::Search(tracks))) => {
                        if let Some(first) = tracks.into_iter().next() {
                            tracing::debug!(
                                "AppleMusic: Found via YouTube Music: {}",
                                first.info.title
                            );
                            return youtube.to_inner_ref().make_playable(first).await;
                        }
                    }
                    _ => {}
                }
            }
        }

        let yt_query = format!("ytsearch:{}", search_query);
        if let Some(youtube) = crate::SOURCES.get("youtube") {
            if let Some(Query::Search(q)) = youtube.to_inner_ref().parse_query(&yt_query) {
                match youtube.to_inner_ref().resolve(Query::Search(q)).await {
                    Ok(Some(ApiTrackResult::Search(tracks))) => {
                        if let Some(first) = tracks.into_iter().next() {
                            tracing::debug!("AppleMusic: Found via YouTube: {}", first.info.title);
                            return youtube.to_inner_ref().make_playable(first).await;
                        }
                    }
                    _ => {}
                }
            }
        }

        Err(ResolverError::MissingRequiredData(
            "Failed to find YouTube fallback or no results",
        ))
    }
}
