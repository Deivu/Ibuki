use crate::CONFIG;
use crate::models::{ApiTrack, ApiTrackInfo, ApiTrackResult};
use crate::util::errors::ResolverError;
use crate::util::source::{Query, Source};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose};
use reqwest::Client;
use serde_json::Value;
use songbird::input::Input;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::Mutex;
use tracing::{debug, warn};

pub struct Spotify {
    http: Client,
    access_token: Arc<Mutex<Option<String>>>,
    token_expiry: Arc<Mutex<SystemTime>>,
}

impl Spotify {
    pub fn new(http: Option<Client>) -> Self {
        Self {
            http: http.unwrap_or_else(|| Client::new()),
            access_token: Arc::new(Mutex::new(None)),
            token_expiry: Arc::new(Mutex::new(SystemTime::UNIX_EPOCH)),
        }
    }

    async fn get_token(&self) -> Option<String> {
        let mut token = self.access_token.lock().await;
        let mut expiry = self.token_expiry.lock().await;

        if let Some(t) = token.as_ref() {
            if *expiry > SystemTime::now() {
                return Some(t.clone());
            }
        }

        if let Some(config) = &CONFIG.spotify_config {
            if let (Some(client_id), Some(client_secret)) =
                (&config.client_id, &config.client_secret)
            {
                let auth =
                    general_purpose::STANDARD.encode(format!("{}:{}", client_id, client_secret));
                let res = self
                    .http
                    .post("https://accounts.spotify.com/api/token")
                    .header("Authorization", format!("Basic {}", auth))
                    .header("Content-Type", "application/x-www-form-urlencoded")
                    .body("grant_type=client_credentials")
                    .send()
                    .await
                    .ok()?;

                if res.status().is_success() {
                    let json: Value = res.json().await.ok()?;
                    if let Some(access_token) = json["access_token"].as_str() {
                        let expires_in = json["expires_in"].as_u64().unwrap_or(3600);
                        *token = Some(access_token.to_string());
                        *expiry =
                            SystemTime::now() + std::time::Duration::from_secs(expires_in - 60);
                        return Some(access_token.to_string());
                    }
                }
            } else if let Some(_sp_dc) = &config.sp_dc {
                // Anonymous/Mobile token logic (simplified for now, ideally needs external auth or cookie support which is complex)
                // For now, let's stick to client credentials as primary, or maybe use a public token generator if available?
                // Lavalink/NodeLink does complex things with "getWebToken".
                // We will implement Client Credentials flow first as it is standard.
            }
        }
        None
    }

    async fn search_track(&self, query: &str, token: &str) -> Option<Vec<ApiTrack>> {
        let res = self
            .http
            .get("https://api.spotify.com/v1/search")
            .header("Authorization", format!("Bearer {}", token))
            .query(&[("q", query), ("type", "track"), ("limit", "10")])
            .send()
            .await
            .ok()?;

        if !res.status().is_success() {
            return None;
        }

        let json: Value = res.json().await.ok()?;
        let items = json["tracks"]["items"].as_array()?;

        let mut tracks = Vec::new();
        for item in items {
            if let Some(track) = self.parse_track(item) {
                tracks.push(track);
            }
        }
        Some(tracks)
    }

    fn parse_track(&self, item: &Value) -> Option<ApiTrack> {
        let id = item["id"].as_str()?;
        let title = item["name"].as_str()?;
        let authors = item["artists"]
            .as_array()?
            .iter()
            .filter_map(|a| a["name"].as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let length = item["duration_ms"].as_u64()?;
        let artwork_url = item["album"]["images"]
            .as_array()
            .and_then(|imgs| imgs.first())
            .and_then(|img| img["url"].as_str())
            .map(|s| s.to_string());
        let isrc = item["external_ids"]["isrc"].as_str().map(|s| s.to_string());
        let uri = item["external_urls"]["spotify"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let info = ApiTrackInfo {
            identifier: id.to_string(),
            is_seekable: true,
            author: authors,
            length,
            is_stream: false,
            position: 0,
            title: title.to_string(),
            uri: Some(uri),
            artwork_url,
            isrc,
            source_name: "spotify".to_string(),
        };

        Some(ApiTrack {
            encoded: crate::util::encoder::encode_track(&info).ok()?,
            info,
            plugin_info: crate::models::Empty,
            user_data: None,
        })
    }

    async fn get_track(&self, id: &str, token: &str) -> Option<ApiTrack> {
        let res = self
            .http
            .get(format!("https://api.spotify.com/v1/tracks/{}", id))
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .ok()?;

        if !res.status().is_success() {
            return None;
        }

        let json: Value = res.json().await.ok()?;
        self.parse_track(&json)
    }

    async fn get_album_tracks(&self, id: &str, token: &str) -> Option<Vec<ApiTrack>> {
        let res = self
            .http
            .get(format!("https://api.spotify.com/v1/albums/{}/tracks", id))
            .header("Authorization", format!("Bearer {}", token))
            .query(&[("limit", "50")])
            .send()
            .await
            .ok()?;

        if !res.status().is_success() {
            return None;
        }

        let json: Value = res.json().await.ok()?;
        let _items = json["items"].as_array()?;
        let album_res = self
            .http
            .get(format!("https://api.spotify.com/v1/albums/{}", id))
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .ok()?;

        let album_json: Value = album_res.json().await.ok()?;
        let artwork_url = album_json["images"]
            .as_array()
            .and_then(|imgs| imgs.first())
            .and_then(|img| img["url"].as_str())
            .map(|s| s.to_string());

        let mut tracks = Vec::new();
        // The album_json also has "tracks" -> "items".
        if let Some(items) = album_json["tracks"]["items"].as_array() {
            for item in items {
                let id = item["id"].as_str()?;
                let title = item["name"].as_str()?;
                let authors = item["artists"]
                    .as_array()?
                    .iter()
                    .filter_map(|a| a["name"].as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                let length = item["duration_ms"].as_u64()?;
                let uri = item["external_urls"]["spotify"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();

                let info = ApiTrackInfo {
                    identifier: id.to_string(),
                    is_seekable: true,
                    author: authors,
                    length,
                    is_stream: false,
                    position: 0,
                    title: title.to_string(),
                    uri: Some(uri),
                    artwork_url: artwork_url.clone(), // Use album artwork
                    isrc: None,                       // Not always available in simple track object
                    source_name: "spotify".to_string(),
                };

                if let Ok(encoded) = crate::util::encoder::encode_track(&info) {
                    tracks.push(ApiTrack {
                        encoded,
                        info,
                        plugin_info: crate::models::Empty,
                        user_data: None,
                    });
                }
            }
        }

        Some(tracks)
    }

    async fn get_playlist_tracks(&self, id: &str, token: &str) -> Option<Vec<ApiTrack>> {
        let res = self
            .http
            .get(format!(
                "https://api.spotify.com/v1/playlists/{}/tracks",
                id
            ))
            .header("Authorization", format!("Bearer {}", token))
            .query(&[("limit", "50")])
            .query(&[(
                "market",
                CONFIG
                    .spotify_config
                    .as_ref()
                    .and_then(|c| c.market.clone())
                    .as_deref()
                    .unwrap_or("US"),
            )])
            .send()
            .await
            .ok()?;

        if !res.status().is_success() {
            return None;
        }

        let json: Value = res.json().await.ok()?;
        let items = json["items"].as_array()?;

        let mut tracks = Vec::new();
        for item in items {
            if let Some(track) = item.get("track") {
                if let Some(parsed) = self.parse_track(track) {
                    tracks.push(parsed);
                }
            }
        }
        Some(tracks)
    }
}

#[async_trait]
impl Source for Spotify {
    fn get_name(&self) -> &'static str {
        "spotify"
    }

    fn get_client(&self) -> Client {
        self.http.clone()
    }

    fn parse_query(&self, query: &str) -> Option<Query> {
        if query.starts_with("spsearch:") {
            return Some(Query::Search(
                query.strip_prefix("spsearch:").unwrap().to_string(),
            ));
        }
        if query.contains("spotify.com") {
            return Some(Query::Url(query.to_string()));
        }
        None
    }

    async fn init(&self) -> Result<(), ResolverError> {
        if let Some(token) = self.get_token().await {
            tracing::info!(
                "Spotify Source Initialized with token: {}...",
                &token[0..10]
            );
        } else {
            tracing::warn!("Spotify Source initialized but no valid token found check Config.");
        }
        Ok(())
    }

    async fn resolve(&self, query: Query) -> Result<Option<ApiTrackResult>, ResolverError> {
        let token = match self.get_token().await {
            Some(t) => t,
            None => return Ok(None),
        };

        match query {
            Query::Search(query) => {
                if let Some(tracks) = self.search_track(&query, &token).await {
                    return Ok(Some(ApiTrackResult::Search(tracks)));
                }
            }
            Query::Url(url) => {
                let parts: Vec<&str> = url.split('/').collect();
                if let Some(type_idx) = parts
                    .iter()
                    .position(|&x| x == "track" || x == "album" || x == "playlist")
                {
                    if type_idx + 1 < parts.len() {
                        let type_str = parts[type_idx];
                        let id = parts[type_idx + 1].split('?').next().unwrap();

                        match type_str {
                            "track" => {
                                if let Some(track) = self.get_track(id, &token).await {
                                    return Ok(Some(ApiTrackResult::Track(track)));
                                }
                            }
                            "album" => {
                                if let Some(tracks) = self.get_album_tracks(id, &token).await {
                                    // For now returning as Search result (list of tracks), though Lavalink spec might want Playlist result
                                    // But ApiTrackResult::Playlist requires ApiPlaylistInfo which we can construct

                                    let name = format!("Spotify Album {}", id); // Should fetch actual name
                                    return Ok(Some(ApiTrackResult::Playlist(
                                        crate::models::ApiTrackPlaylist {
                                            info: crate::models::ApiPlaylistInfo {
                                                name,
                                                selected_track: -1,
                                            },
                                            plugin_info: crate::models::Empty,
                                            tracks,
                                        },
                                    )));
                                }
                            }
                            "playlist" => {
                                if let Some(tracks) = self.get_playlist_tracks(id, &token).await {
                                    let name = format!("Spotify Playlist {}", id);
                                    return Ok(Some(ApiTrackResult::Playlist(
                                        crate::models::ApiTrackPlaylist {
                                            info: crate::models::ApiPlaylistInfo {
                                                name,
                                                selected_track: -1,
                                            },
                                            plugin_info: crate::models::Empty,
                                            tracks,
                                        },
                                    )));
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        Ok(None)
    }

    async fn make_playable(&self, _track: ApiTrack) -> Result<Input, ResolverError> {
        // IMPORTANT: We need to implement this "bridge" logic.
        // Since we can't easily access the Youtube instance from here without circular deps or global map lookup...
        // We CAN access `crate::SOURCES` global map!

        let query = format!("ytsearch:{} - {}", _track.info.author, _track.info.title);
        debug!("Spotify: Searching YouTube for: {}", query);

        if let Some(youtube) = crate::SOURCES.get("youtube") {
            if let Some(Query::Search(q)) = youtube.to_inner_ref().parse_query(&query) {
                debug!("Spotify: Parsed YouTube search query: {:?}", q);
                match youtube.to_inner_ref().resolve(Query::Search(q)).await {
                    Ok(Some(res)) => match res {
                        ApiTrackResult::Search(tracks) => {
                            debug!("Spotify: YouTube returned {} tracks", tracks.len());
                            if let Some(first) = tracks.into_iter().next() {
                                debug!("Spotify: Playing first result: {}", first.info.title);
                                return youtube.to_inner_ref().make_playable(first).await;
                            } else {
                                warn!("Spotify: YouTube search returned empty results");
                            }
                        }
                        _ => {
                            warn!("Spotify: YouTube returned non-search result");
                        }
                    },
                    Ok(None) => {
                        warn!("Spotify: YouTube resolve returned None");
                    }
                    Err(e) => {
                        warn!("Spotify: YouTube resolve failed: {:?}", e);
                    }
                }
            } else {
                warn!("Spotify: Failed to parse YouTube search query");
            }
        } else {
            warn!("Spotify: YouTube source not available");
        }

        Err(ResolverError::Custom(
            "Playback failed: Could not find track on YouTube".to_string(),
        ))
    }
}
