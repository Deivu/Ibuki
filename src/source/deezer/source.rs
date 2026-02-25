use super::MEDIA_BASE;
use super::PRIVATE_API_BASE;
use super::PUBLIC_API_BASE;
use super::model::Tokens;
use super::model::{
    DeezerApiAlbumDetail, DeezerApiArtistDetail, DeezerApiPlaylist, DeezerApiTrack,
    DeezerApiTrackList, DeezerData, DeezerGetListDataBody, DeezerGetMedia, DeezerGetUrlBody,
    DeezerGetUrlMedia, DeezerQualityFormat, DeezerRecommendationBody,
    DeezerApiErrorWrapper, InternalDeezerGetUserData, InternalDeezerListData, InternalDeezerResponse,
    InternalDeezerRecommendationData, InternalDeezerSongData,
};
use super::stream::DeezerHttpStream;
use crate::CONFIG;
use crate::models::{
    ApiPlaylistInfo, ApiTrack, ApiTrackInfo, ApiTrackPlaylist, ApiTrackResult, Empty,
};
use crate::util::encoder::encode_track;
use crate::util::errors::ResolverError;
use crate::util::source::Query;
use crate::util::source::Source;
use crate::util::url::is_url;
use async_trait::async_trait;
use regex::Regex;
use reqwest::Body;
use reqwest::Client;
use songbird::input::{Compose, HttpRequest, Input, LiveInput};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::Instant;

static ISRC_REGEX_PATTERN: &str = r"^(?:isrc:)?([A-Z]{2}-?[A-Z0-9]{3}-?\d{2}-?\d{5})$";

pub struct Deezer {
    client: Client,
    tokens: Arc<Mutex<Option<Tokens>>>,
    url_regex: Regex,
    short_link_regex: Regex,
    isrc_regex: Regex,
    search_prefixes: (&'static str, &'static str, &'static str),
}

#[async_trait]
impl Source for Deezer {
    fn get_name(&self) -> &'static str {
        "deezer"
    }

    fn get_client(&self) -> Client {
        self.client.clone()
    }

    fn parse_query(&self, query: &str) -> Option<Query> {
        if self.short_link_regex.is_match(query) {
            return Some(Query::Url(query.to_string()));
        }

        if !is_url(query) {
            if query.starts_with(self.search_prefixes.0)
                || query.starts_with(self.search_prefixes.1)
                || query.starts_with(self.search_prefixes.2)
            {
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
        tracing::info!("Deezer Source initialized successfully");
        Ok(())
    }

    async fn resolve(&self, query: Query) -> Result<Option<ApiTrackResult>, ResolverError> {
        match query {
            Query::Url(url) => self.resolve_url(&url).await,
            Query::Search(input) => {
                if input.starts_with(self.search_prefixes.2) {
                    let term = input.split_at(self.search_prefixes.2.len()).1;
                    return self.get_recommendations(term).await;
                }

                let mut data: Option<Vec<DeezerApiTrack>> = None;

                if input.starts_with(self.search_prefixes.0) {
                    let term = input.split_at(self.search_prefixes.0.len()).1;
                    if let Some(isrc) = self.extract_isrc(term) {
                        tracing::debug!("Deezer ISRC search: {}", isrc);

                        let request = self
                            .client
                            .get(format!("{PUBLIC_API_BASE}/track/isrc:{isrc}"))
                            .build()?;

                        let response = self.client.execute(request).await?;

                        if response.status().is_success() {
                            let text = response.text().await?;
                            if let Ok(err_wrapper) =
                                serde_json::from_str::<DeezerApiErrorWrapper>(&text)
                            {
                                if err_wrapper.error.is_some() {
                                    return Ok(Some(ApiTrackResult::Empty(None)));
                                }
                            }
                            if let Ok(track) = serde_json::from_str::<DeezerApiTrack>(&text) {
                                let _ = data.insert(vec![track]);
                            }
                        }
                    } else {
                        tracing::debug!("Searching Deezer for: \"{}\"", term);

                        let query = [("q", term)];

                        let request = self
                            .client
                            .get(format!("{PUBLIC_API_BASE}/search"))
                            .query(&query)
                            .build()?;

                        let response = self.client.execute(request).await?;

                        if !response.status().is_success() {
                            return Ok(None);
                        }

                        let tracks = response.json::<DeezerData<Vec<DeezerApiTrack>>>().await?;

                        let _ = data.insert(tracks.data);
                    }
                } else if input.starts_with(self.search_prefixes.1) {
                    let isrc = input.split_at(self.search_prefixes.1.len()).1;

                    tracing::debug!("Deezer ISRC search: {}", isrc);

                    let request = self
                        .client
                        .get(format!("{PUBLIC_API_BASE}/track/isrc:{isrc}"))
                        .build()?;

                    let response = self.client.execute(request).await?;

                    if !response.status().is_success() {
                        return Ok(None);
                    }

                    let text = response.text().await?;
                    if let Ok(err_wrapper) = serde_json::from_str::<DeezerApiErrorWrapper>(&text) {
                        if err_wrapper.error.is_some() {
                            return Ok(Some(ApiTrackResult::Empty(None)));
                        }
                    }
                    if let Ok(track) = serde_json::from_str::<DeezerApiTrack>(&text) {
                        let _ = data.insert(vec![track]);
                    }
                }

                let Some(api_tracks) = data else {
                    return Ok(None);
                };

                let tracks = api_tracks
                    .iter()
                    .filter(|t| t.readable)
                    .take(10)
                    .map(|t| self.build_track_from_api(t))
                    .collect::<Result<Vec<ApiTrack>, ResolverError>>()?;

                if tracks.is_empty() {
                    return Ok(Some(ApiTrackResult::Empty(None)));
                }

                Ok(Some(ApiTrackResult::Search(tracks)))
            }
        }
    }

    async fn make_playable(&self, track: ApiTrack) -> Result<Input, ResolverError> {
        let tokens = self.get_token().await?;

        // Use song.getListData (like the JS source) to get track token
        let response = {
            let query = [
                ("method", "song.getListData"),
                ("input", "3"),
                ("api_version", "1.0"),
                ("api_token", tokens.check_form.as_str()),
            ];

            let body = DeezerGetListDataBody {
                sng_ids: vec![track.info.identifier.clone()],
            };

            let request = self
                .client
                .post(PRIVATE_API_BASE)
                .header("Cookie", tokens.create_cookie())
                .body(Body::from(serde_json::to_string(&body)?))
                .query(&query)
                .build()?;

            self.client.execute(request).await?
        };

        if !response.status().is_success() {
            return Err(ResolverError::FailedStatusCode(
                response.status().to_string(),
            ));
        }

        let list_data = response
            .json::<InternalDeezerResponse<InternalDeezerListData>>()
            .await?;

        if !list_data.error.is_null() {
            return Err(ResolverError::Custom(format!("Deezer API error: {:?}", list_data.error)));
        }

        let track_info = list_data
            .results
            .data
            .first()
            .ok_or(ResolverError::MissingRequiredData("track data from getListData"))?;

        // Use all quality formats for fallback (like JS source)
        let response = {
            let body = DeezerGetUrlBody {
                license_token: tokens.license_token.clone(),
                media: vec![DeezerGetUrlMedia {
                    media_type: String::from("FULL"),
                    formats: DeezerQualityFormat::all_formats(),
                }],
                track_tokens: vec![track_info.track_token.clone()],
            };

            let request = self
                .client
                .post(format!("{MEDIA_BASE}/get_url"))
                .header("Cookie", tokens.create_cookie())
                .body(Body::from(serde_json::to_string(&body)?))
                .build()?;

            self.client.execute(request).await?
        };

        if !response.status().is_success() {
            return Err(ResolverError::FailedStatusCode(
                response.status().to_string(),
            ));
        }

        let json = response.json::<DeezerGetMedia>().await?;

        let data = json
            .data
            .ok_or(ResolverError::MissingRequiredData("media.data"))?;

        let media = data
            .first()
            .ok_or(ResolverError::MissingRequiredData("media.data.first()"))?
            .media
            .first()
            .ok_or(ResolverError::MissingRequiredData(
                "media.data.first().media.first()",
            ))?
            .sources
            .first()
            .ok_or(ResolverError::MissingRequiredData(
                "media.data.first().media.first().sources.first()",
            ))?;

        let mut stream = DeezerHttpStream::new(
            HttpRequest::new(self.get_client(), media.url.clone()),
            self.get_track_key(track.info.identifier.clone()),
        );

        let input = Input::Live(LiveInput::Raw(stream.create_async().await?), None);

        Ok(input)
    }
}

impl Deezer {
    pub fn new(client: Option<Client>) -> Self {
        Self {
            client: client.unwrap_or_default(),
            tokens: Arc::new(Mutex::new(None)),
            url_regex: Regex::new(
                r"^https?://(?:www\.)?deezer\.com/(?:[a-z]+(?:-[a-z]+)?/)?(?P<type>track|album|playlist|artist)/(?P<identifier>\d+)"
            ).expect("Failed to init Deezer URL RegEx"),
            short_link_regex: Regex::new(
                r"^(?:https?://)?link\.deezer\.com/s/([a-zA-Z0-9]+)"
            ).expect("Failed to init Deezer short link RegEx"),
            isrc_regex: Regex::new(ISRC_REGEX_PATTERN)
                .expect("Failed to init ISRC RegEx"),
            search_prefixes: ("dzsearch:", "dzisrc:", "dzrec:"),
        }
    }

    /// Extract ISRC from query if it matches the ISRC pattern
    fn extract_isrc(&self, input: &str) -> Option<String> {
        let trimmed = input.trim();
        self.isrc_regex.captures(trimmed).map(|caps| {
            caps.get(1)
                .unwrap()
                .as_str()
                .replace('-', "")
                .to_uppercase()
        })
    }

    /// Resolve a Deezer URL (track/album/playlist/artist or short link)
    async fn resolve_url(&self, url: &str) -> Result<Option<ApiTrackResult>, ResolverError> {
        // Handle short links (link.deezer.com)
        if url.contains("link.deezer.com") {
            return self.resolve_short_link(url).await;
        }

        let captures = match self.url_regex.captures(url) {
            Some(c) => c,
            None => return Ok(Some(ApiTrackResult::Empty(None))),
        };

        let url_type = captures
            .name("type")
            .map(|m| m.as_str())
            .unwrap_or("");
        let id = captures
            .name("identifier")
            .map(|m| m.as_str())
            .unwrap_or("");

        tracing::debug!("Resolving Deezer URL: type={}, id={}", url_type, id);

        match url_type {
            "track" => self.resolve_track(id).await,
            "album" => self.resolve_album(id).await,
            "playlist" => self.resolve_playlist(id).await,
            "artist" => self.resolve_artist(id).await,
            _ => Ok(Some(ApiTrackResult::Empty(None))),
        }
    }

    /// Resolve a short link by following the redirect
    async fn resolve_short_link(
        &self,
        url: &str,
    ) -> Result<Option<ApiTrackResult>, ResolverError> {
        tracing::debug!("Resolving Deezer short link: {}", url);
        
        // Add https:// if not present
        let full_url = if url.starts_with("http://") || url.starts_with("https://") {
            url.to_string()
        } else {
            format!("https://{}", url)
        };
        
        tracing::debug!("Following redirect from: {}", full_url);
        let response = self.client.get(&full_url).send().await?;
        let final_url = response.url().to_string();
        
        tracing::debug!("Redirected to: {}", final_url);

        if let Some(captures) = self.url_regex.captures(&final_url) {
            let url_type = captures.name("type").map(|m| m.as_str()).unwrap_or("");
            let id = captures.name("identifier").map(|m| m.as_str()).unwrap_or("");

            tracing::debug!("Matched URL type: {}, id: {}", url_type, id);

            match url_type {
                "track" => self.resolve_track(id).await,
                "album" => self.resolve_album(id).await,
                "playlist" => self.resolve_playlist(id).await,
                "artist" => self.resolve_artist(id).await,
                _ => {
                    tracing::warn!("Unknown Deezer URL type: {}", url_type);
                    Ok(Some(ApiTrackResult::Empty(None)))
                }
            }
        } else {
            tracing::warn!("Deezer short link redirect didn't match expected pattern. Final URL: {}", final_url);
            Ok(Some(ApiTrackResult::Empty(None)))
        }
    }

    /// Resolve a single track by ID
    async fn resolve_track(&self, id: &str) -> Result<Option<ApiTrackResult>, ResolverError> {
        let request = self
            .client
            .get(format!("{PUBLIC_API_BASE}/track/{id}"))
            .build()?;

        let response = self.client.execute(request).await?;

        if !response.status().is_success() {
            return Ok(Some(ApiTrackResult::Empty(None)));
        }

        let text = response.text().await?;

        // Check for API error
        if let Ok(err_wrapper) = serde_json::from_str::<DeezerApiErrorWrapper>(&text) {
            if let Some(err) = err_wrapper.error {
                if err.code == 800 {
                    return Ok(Some(ApiTrackResult::Empty(None)));
                }
                return Err(ResolverError::Custom(err.message));
            }
        }

        let track: DeezerApiTrack = serde_json::from_str(&text)?;
        let api_track = self.build_track_from_api(&track)?;

        Ok(Some(ApiTrackResult::Track(api_track)))
    }

    /// Resolve an album by ID
    async fn resolve_album(&self, id: &str) -> Result<Option<ApiTrackResult>, ResolverError> {
        let request = self
            .client
            .get(format!("{PUBLIC_API_BASE}/album/{id}"))
            .build()?;

        let response = self.client.execute(request).await?;

        if !response.status().is_success() {
            return Ok(Some(ApiTrackResult::Empty(None)));
        }

        let text = response.text().await?;

        // Check for API error
        if let Ok(err_wrapper) = serde_json::from_str::<DeezerApiErrorWrapper>(&text) {
            if let Some(err) = err_wrapper.error {
                if err.code == 800 {
                    return Ok(Some(ApiTrackResult::Empty(None)));
                }
                return Err(ResolverError::Custom(err.message));
            }
        }

        let album: DeezerApiAlbumDetail = serde_json::from_str(&text)?;

        // Fetch full track list
        let tracklist_url = format!("{}?limit=1000", album.tracklist);

        let tracklist_response = self.client.get(&tracklist_url).send().await?;

        if !tracklist_response.status().is_success() {
            return Err(ResolverError::Custom(
                "Could not fetch album tracks".to_string(),
            ));
        }

        let tracklist: DeezerApiTrackList = tracklist_response.json().await?;

        let tracks = tracklist
            .data
            .iter()
            .map(|t| self.build_track_from_api_with_artwork(t, &album.cover_xl))
            .collect::<Result<Vec<ApiTrack>, ResolverError>>()?;

        Ok(Some(ApiTrackResult::Playlist(ApiTrackPlaylist {
            info: ApiPlaylistInfo {
                name: album.title,
                selected_track: 0,
            },
            plugin_info: Empty,
            tracks,
        })))
    }

    /// Resolve a playlist by ID
    async fn resolve_playlist(&self, id: &str) -> Result<Option<ApiTrackResult>, ResolverError> {
        let request = self
            .client
            .get(format!("{PUBLIC_API_BASE}/playlist/{id}"))
            .build()?;

        let response = self.client.execute(request).await?;

        if !response.status().is_success() {
            return Ok(Some(ApiTrackResult::Empty(None)));
        }

        let text = response.text().await?;

        // Check for API error
        if let Ok(err_wrapper) = serde_json::from_str::<DeezerApiErrorWrapper>(&text) {
            if let Some(err) = err_wrapper.error {
                if err.code == 800 {
                    return Ok(Some(ApiTrackResult::Empty(None)));
                }
                return Err(ResolverError::Custom(err.message));
            }
        }

        let playlist: DeezerApiPlaylist = serde_json::from_str(&text)?;

        // Fetch full track list
        let tracklist_url = format!("{}?limit=1000", playlist.tracklist);

        let tracklist_response = self.client.get(&tracklist_url).send().await?;

        if !tracklist_response.status().is_success() {
            return Err(ResolverError::Custom(
                "Could not fetch playlist tracks".to_string(),
            ));
        }

        let tracklist: DeezerApiTrackList = tracklist_response.json().await?;

        let artwork = playlist.picture_xl.clone().unwrap_or_default();
        let tracks = tracklist
            .data
            .iter()
            .map(|t| {
                self.build_track_from_api_with_artwork(
                    t,
                    &artwork,
                )
            })
            .collect::<Result<Vec<ApiTrack>, ResolverError>>()?;

        Ok(Some(ApiTrackResult::Playlist(ApiTrackPlaylist {
            info: ApiPlaylistInfo {
                name: playlist.title,
                selected_track: 0,
            },
            plugin_info: Empty,
            tracks,
        })))
    }

    /// Resolve an artist by ID (returns top tracks)
    async fn resolve_artist(&self, id: &str) -> Result<Option<ApiTrackResult>, ResolverError> {
        let request = self
            .client
            .get(format!("{PUBLIC_API_BASE}/artist/{id}"))
            .build()?;

        let response = self.client.execute(request).await?;

        if !response.status().is_success() {
            return Ok(Some(ApiTrackResult::Empty(None)));
        }

        let text = response.text().await?;
        if let Ok(err_wrapper) = serde_json::from_str::<DeezerApiErrorWrapper>(&text) {
            if let Some(err) = err_wrapper.error {
                if err.code == 800 {
                    return Ok(Some(ApiTrackResult::Empty(None)));
                }
                return Err(ResolverError::Custom(err.message));
            }
        }

        let artist: DeezerApiArtistDetail = serde_json::from_str(&text)?;

        // Fetch top tracks
        let top_url = format!("{PUBLIC_API_BASE}/artist/{id}/top?limit=25");

        let top_response = self.client.get(&top_url).send().await?;

        if !top_response.status().is_success() {
            return Err(ResolverError::Custom(
                "Could not fetch artist top tracks".to_string(),
            ));
        }

        let top_data: DeezerData<Vec<DeezerApiTrack>> = top_response.json().await?;

        let tracks = top_data
            .data
            .iter()
            .map(|t| self.build_track_from_api_with_artwork(t, &artist.picture_xl))
            .collect::<Result<Vec<ApiTrack>, ResolverError>>()?;

        Ok(Some(ApiTrackResult::Playlist(ApiTrackPlaylist {
            info: ApiPlaylistInfo {
                name: format!("{}'s Top Tracks", artist.name),
                selected_track: 0,
            },
            plugin_info: Empty,
            tracks,
        })))
    }

    /// Get recommendations using Deezer's smart radio/mix APIs
    async fn get_recommendations(&self, query: &str) -> Result<Option<ApiTrackResult>, ResolverError> {
        let tokens = self.get_token().await?;

        let mut method = "song.getSearchTrackMix";
        let mut body = DeezerRecommendationBody {
            sng_id: Some(query.to_string()),
            art_id: None,
            start_with_input_track: Some("true".to_string()),
        };

        // Handle different query formats
        if query.starts_with("artist=") {
            method = "song.getSmartRadio";
            let artist_id = query.split('=').nth(1).unwrap_or("").to_string();
            body = DeezerRecommendationBody {
                sng_id: None,
                art_id: Some(artist_id),
                start_with_input_track: None,
            };
        } else if query.starts_with("track=") {
            let track_id = query.split('=').nth(1).unwrap_or("").to_string();
            body.sng_id = Some(track_id);
        } else if !query.chars().all(char::is_numeric) {
            // Not a numeric ID, search first
            let search_result = self
                .resolve(Query::Search(format!("dzsearch:{}", query)))
                .await?;

            if let Some(ApiTrackResult::Search(tracks)) = search_result {
                if let Some(first) = tracks.first() {
                    body.sng_id = Some(first.info.identifier.clone());
                } else {
                    return Ok(Some(ApiTrackResult::Empty(None)));
                }
            } else {
                return Ok(Some(ApiTrackResult::Empty(None)));
            }
        }

        let query_params = [
            ("method", method),
            ("input", "3"),
            ("api_version", "1.0"),
            ("api_token", tokens.check_form.as_str()),
        ];

        let request = self
            .client
            .post(PRIVATE_API_BASE)
            .header("Cookie", tokens.create_cookie())
            .body(Body::from(serde_json::to_string(&body)?))
            .query(&query_params)
            .build()?;

        let response = self.client.execute(request).await?;

        if !response.status().is_success() {
            return Ok(Some(ApiTrackResult::Empty(None)));
        }

        let rec_data = response
            .json::<InternalDeezerResponse<InternalDeezerRecommendationData>>()
            .await?;

        if rec_data.results.data.is_empty() {
            return Ok(Some(ApiTrackResult::Empty(None)));
        }

        let tracks = rec_data
            .results
            .data
            .iter()
            .map(|t| self.build_track_from_internal(t))
            .collect::<Result<Vec<ApiTrack>, ResolverError>>()?;

        Ok(Some(ApiTrackResult::Playlist(ApiTrackPlaylist {
            info: ApiPlaylistInfo {
                name: "Deezer Recommendations".to_string(),
                selected_track: 0,
            },
            plugin_info: Empty,
            tracks,
        })))
    }

    /// Build ApiTrack from DeezerApiTrack
    fn build_track_from_api(&self, track: &DeezerApiTrack) -> Result<ApiTrack, ResolverError> {
        let info = ApiTrackInfo {
            identifier: track.id.to_string(),
            is_seekable: true,
            author: track.artist.name.clone(),
            length: (track.duration as u64) * 1000,
            is_stream: false,
            position: 0,
            title: track.title.clone(),
            uri: Some(track.link.clone()),
            artwork_url: Some(track.album.thumbnail.clone()),
            isrc: track.isrc.clone(),
            source_name: self.get_name().to_string(),
        };

        Ok(ApiTrack {
            encoded: encode_track(&info)?,
            info,
            plugin_info: Empty, user_data: None
        })
    }

    /// Build ApiTrack from DeezerApiTrack with custom artwork
    fn build_track_from_api_with_artwork(
        &self,
        track: &DeezerApiTrack,
        artwork_url: &str,
    ) -> Result<ApiTrack, ResolverError> {
        let info = ApiTrackInfo {
            identifier: track.id.to_string(),
            is_seekable: true,
            author: track.artist.name.clone(),
            length: (track.duration as u64) * 1000,
            is_stream: false,
            position: 0,
            title: track.title.clone(),
            uri: Some(track.link.clone()),
            artwork_url: Some(artwork_url.to_string()),
            isrc: track.isrc.clone(),
            source_name: self.get_name().to_string(),
        };

        Ok(ApiTrack {
            encoded: encode_track(&info)?,
            info,
            plugin_info: Empty, user_data: None
        })
    }

    /// Build ApiTrack from InternalDeezerSongData (recommendation/private API)
    fn build_track_from_internal(
        &self,
        track: &InternalDeezerSongData,
    ) -> Result<ApiTrack, ResolverError> {
        let info = ApiTrackInfo {
            identifier: track.sng_id.clone(),
            is_seekable: true,
            author: track.art_name.clone(),
            length: track.duration.parse::<u64>().unwrap_or(0) * 1000,
            is_stream: false,
            position: 0,
            title: track.sng_title.clone(),
            uri: Some(format!("https://www.deezer.com/track/{}", track.sng_id)),
            artwork_url: Some(format!(
                "https://e-cdns-images.dzcdn.net/images/cover/{}/1000x1000-000000-80-0-0.jpg",
                track.alb_picture
            )),
            isrc: Some(track.isrc.clone()),
            source_name: self.get_name().to_string(),
        };

        Ok(ApiTrack {
            encoded: encode_track(&info)?,
            info,
            plugin_info: Empty, user_data: None
        })
    }

    fn get_track_key(&self, id: String) -> [u8; 16] {
        let md5 = hex::encode(md5::compute(id).0);
        let hash = md5.as_bytes();

        let mut key: [u8; 16] = [0; 16];

        for i in 0..16 {
            key[i] = hash[i]
                ^ hash[i + 16]
                ^ CONFIG
                    .deezer_config
                    .as_ref()
                    .expect("Unexpected Nullish Config")
                    .decrypt_key
                    .as_bytes()[i];
        }

        key
    }

    async fn get_token(&self) -> Result<Tokens, ResolverError> {
        let mut guard = self.tokens.lock().await;

        if let Some(token) = guard.as_ref() {
            if Instant::now() < token.expire_at {
                return Ok(token.clone());
            }
        }

        let query = [
            ("method", "deezer.getUserData"),
            ("input", "3"),
            ("api_version", "1.0"),
            ("api_token", ""),
        ];

        let request = self
            .client
            .post(PRIVATE_API_BASE)
            .header("Content-Length", "0")
            .header(
                "Cookie",
                format!(
                    "arl={}",
                    CONFIG
                        .deezer_config
                        .as_ref()
                        .expect("Unexpected Nullish Config")
                        .arl
                        .to_owned()
                ),
            )
            .query(&query)
            .build()?;

        let response = self.client.execute(request).await?;

        if !response.status().is_success() {
            return Err(ResolverError::FailedStatusCode(
                response.status().to_string(),
            ));
        }

        let headers = response
            .headers()
            .get_all("Set-Cookie")
            .iter()
            .filter_map(|header| header.to_str().ok())
            .map(String::from)
            .collect::<Vec<String>>();

        tracing::debug!("Deezer API Set-Cookie headers: {:?}", headers);

        let session_id = headers
            .iter()
            .find(|str| str.contains("sid="))
            .ok_or(ResolverError::MissingRequiredData(
                "Missing Deezer Session Id",
            ))?;

        let unique_id = headers
            .iter()
            .find(|str| str.contains("dzr_uniq_id="))
            .ok_or(ResolverError::MissingRequiredData(
                "Missing Deezer Unique Id",
            ))?;

        let data = response
            .json::<InternalDeezerResponse<InternalDeezerGetUserData>>()
            .await?;

        let tokens = Tokens {
            session_id: session_id.to_string(),
            unique_id: unique_id.to_string(),
            check_form: data.results.check_form,
            license_token: data.results.user.options.license_token,
            expire_at: Instant::now()
                .checked_add(Duration::from_secs(3600))
                .ok_or(ResolverError::MissingRequiredData("Invalid Expire At"))?,
        };

        let _ = guard.insert(tokens.clone());

        tracing::info!("Deezer tokens refreshed successfully");

        Ok(tokens)
    }
}

