use super::USER_AGENT;
use super::model::*;
use crate::models::{ApiTrack, ApiTrackInfo, ApiTrackResult};
use crate::util::encoder::encode_track;
use crate::util::errors::ResolverError;
use crate::util::source::{Query, Source};
use crate::util::url::is_url;
use crate::playback::hls::handler::start_hls_stream;
use async_trait::async_trait;
use regex::Regex;
use reqwest::Client;
use songbird::input::{HttpRequest, Input};
use songbird::tracks::Track;
use std::sync::Arc;
use crate::CONFIG;

pub struct Gaana {
    client: Client,
    regex: Regex,
    search_prefix: &'static str,
    api_url: String,
    stream_quality: String,
    playlist_track_limit: u32,
    album_track_limit: u32,
    artist_track_limit: u32,
}

#[async_trait]
impl Source for Gaana {
    fn get_name(&self) -> &'static str {
        "gaana"
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

        self.regex.captures(query)?;

        Some(Query::Url(query.to_string()))
    }

    async fn init(&self) -> Result<(), ResolverError> {
        // Verify API is accessible
        Ok(())
    }

    async fn resolve(&self, query: Query) -> Result<Option<ApiTrackResult>, ResolverError> {
        match query {
            Query::Url(url) => {
                let captures = self
                    .regex
                    .captures(&url)
                    .ok_or(ResolverError::InvalidUrl)?;
                let url_type = captures
                    .name("type")
                    .map(|m| m.as_str())
                    .ok_or(ResolverError::InvalidUrl)?;
                let seokey = captures
                    .name("seokey")
                    .map(|m| m.as_str())
                    .ok_or(ResolverError::InvalidUrl)?;

                match url_type {
                    "song" => self.get_song(seokey).await,
                    "album" => self.get_album(seokey).await,
                    "playlist" => self.get_playlist(seokey).await,
                    "artist" => self.get_artist(seokey).await,
                    _ => Ok(None),
                }
            }
            Query::Search(input) => {
                if input.starts_with(self.search_prefix) {
                    let term = input.split_at(self.search_prefix.len()).1;
                    self.search(term).await
                } else {
                    Ok(None)
                }
            }
        }
    }

    async fn make_playable(&self, track: ApiTrack) -> Result<Input, ResolverError> {
        let track_id = &track.info.identifier;

        tracing::info!("Attempting to get stream URL for Gaana track: {}", track_id);

        // Get stream info (URL, protocol, format)
        let stream_info = self.get_stream_info(track_id).await?;

        tracing::info!(
            "Got stream info for Gaana track {}: url={}, protocol={}",
            track_id, stream_info.url, stream_info.protocol
        );

        if stream_info.protocol == "hls" {
            tracing::info!("Using HLS handler for Gaana track: {}", track_id);
            let input = start_hls_stream(stream_info.url, self.get_client()).await;
            return Ok(input);
        }

        // Direct HTTP stream
        Ok(Input::from(HttpRequest::new(self.get_client(), stream_info.url)))
    }
}

impl Gaana {
    pub fn new(client: Option<Client>) -> Self {
        let config = CONFIG
            .gaana_config
            .as_ref()
            .expect("Gaana config is required");

        let api_url = config
            .api_url
            .as_ref()
            .expect("Gaana API URL is required")
            .trim_end_matches('/')
            .to_string();

        Self {
            client: client.unwrap_or_default(),
            regex: Regex::new(
                r"(?:https?://)?(?:www\.)?gaana\.com/(?<type>song|album|playlist|artist)/(?<seokey>[\w-]+)"
            )
            .expect("Failed to init Gaana RegEx"),
            search_prefix: "gaanasearch:",
            api_url,
            stream_quality: config.stream_quality.clone().unwrap_or_else(|| "high".to_string()),
            playlist_track_limit: config.playlist_track_limit.unwrap_or(100),
            album_track_limit: config.album_track_limit.unwrap_or(100),
            artist_track_limit: config.artist_track_limit.unwrap_or(100),
        }
    }

    async fn search(&self, query: &str) -> Result<Option<ApiTrackResult>, ResolverError> {
        let url = format!(
            "{}/api/search/songs?q={}&limit=10",
            self.api_url,
            urlencoding::encode(query)
        );

        let response = self.get_json::<Vec<GaanaTrack>>(&url).await?;
        
        let tracks: Vec<ApiTrack> = response
            .into_iter()
            .filter_map(|track| self.map_track(&track))
            .collect();

        if tracks.is_empty() {
            Ok(None)
        } else {
            Ok(Some(ApiTrackResult::Search(tracks)))
        }
    }

    async fn get_song(&self, seokey: &str) -> Result<Option<ApiTrackResult>, ResolverError> {
        let url = format!("{}/api/songs/{}", self.api_url, urlencoding::encode(seokey));
        
        let response = self.get_json::<GaanaTrackResponse>(&url).await?;
        
        // Try to extract track from different possible response structures
        let track_data = response.data.or_else(|| {
            // If data is None, try to construct from top-level fields
            if response.title.is_some() || response.name.is_some() {
                Some(GaanaTrack {
                    track_id: response.track_id,
                    title: response.title,
                    name: response.name,
                    duration: response.duration,
                    artists: response.artists,
                    seokey: response.seokey,
                    artwork: response.artwork,
                    artwork_url: response.artwork_url,
                    song_url: response.song_url,
                    isrc: response.isrc,
                })
            } else {
                None
            }
        });

        if let Some(track) = track_data {
            if let Some(api_track) = self.map_track(&track) {
                return Ok(Some(ApiTrackResult::Track(api_track)));
            }
        }

        Ok(None)
    }

    async fn get_album(&self, seokey: &str) -> Result<Option<ApiTrackResult>, ResolverError> {
        let url = format!("{}/api/albums/{}", self.api_url, urlencoding::encode(seokey));
        
        let response = self.get_json::<GaanaAlbumResponse>(&url).await?;
        
        let album_data = response.data.or_else(|| {
            if response.title.is_some() || response.name.is_some() {
                Some(GaanaAlbum {
                    title: response.title,
                    name: response.name,
                    seokey: response.seokey,
                    tracks: response.tracks,
                    songs: response.songs,
                    artwork: response.artwork,
                    artwork_url: response.artwork_url,
                })
            } else {
                None
            }
        });

        if let Some(album) = album_data {
            return Ok(Some(self.build_playlist(
                album.title.or(album.name).unwrap_or_else(|| "Gaana Album".to_string()),
                album.tracks.or(album.songs).unwrap_or_default(),
                self.album_track_limit,
            )));
        }

        Ok(None)
    }

    async fn get_playlist(&self, seokey: &str) -> Result<Option<ApiTrackResult>, ResolverError> {
        let url = format!("{}/api/playlists/{}", self.api_url, urlencoding::encode(seokey));
        
        let response = self.get_json::<GaanaPlaylistResponse>(&url).await?;
        
        let playlist_data = response.playlist.or(response.data).or_else(|| {
            if response.title.is_some() || response.name.is_some() || response.playlist_name.is_some() {
                Some(GaanaPlaylist {
                    title: response.title,
                    name: response.name,
                    playlist_name: response.playlist_name,
                    seokey: response.seokey,
                    playlist_id: response.playlist_id,
                    tracks: response.tracks,
                    songs: response.songs,
                    artwork: response.artwork,
                    artwork_url: response.artwork_url,
                })
            } else {
                None
            }
        });

        if let Some(playlist) = playlist_data {
            return Ok(Some(self.build_playlist(
                playlist.title
                    .or(playlist.name)
                    .or(playlist.playlist_name)
                    .unwrap_or_else(|| "Gaana Playlist".to_string()),
                playlist.tracks.or(playlist.songs).unwrap_or_default(),
                self.playlist_track_limit,
            )));
        }

        Ok(None)
    }

    async fn get_artist(&self, seokey: &str) -> Result<Option<ApiTrackResult>, ResolverError> {
        let url = format!("{}/api/artists/{}", self.api_url, urlencoding::encode(seokey));
        
        let response = self.get_json::<GaanaArtistResponse>(&url).await?;
        
        let artist_data = response.data.or_else(|| {
            if response.title.is_some() || response.name.is_some() {
                Some(GaanaArtist {
                    title: response.title,
                    name: response.name,
                    seokey: response.seokey,
                    artist_id: response.artist_id,
                    top_tracks: response.top_tracks,
                    tracks: response.tracks,
                    songs: response.songs,
                    artwork: response.artwork,
                    artwork_url: response.artwork_url,
                })
            } else {
                None
            }
        });

        if let Some(artist) = artist_data {
            let name = artist.title.or(artist.name).unwrap_or_else(|| "Gaana Artist".to_string());
            let tracks = artist.top_tracks
                .or(artist.tracks)
                .or(artist.songs)
                .unwrap_or_default();
            
            return Ok(Some(self.build_playlist(
                format!("{}'s Top Tracks", name),
                tracks,
                self.artist_track_limit,
            )));
        }

        Ok(None)
    }

    fn build_playlist(&self, name: String, tracks: Vec<GaanaTrack>, limit: u32) -> ApiTrackResult {
        let api_tracks: Vec<ApiTrack> = tracks
            .into_iter()
            .filter_map(|track| self.map_track(&track))
            .take(limit as usize)
            .collect();

        ApiTrackResult::Playlist(crate::models::ApiTrackPlaylist {
            info: crate::models::ApiPlaylistInfo {
                name,
                selected_track: 0,
            },
            plugin_info: crate::models::Empty,
            tracks: api_tracks,
        })
    }

    fn map_track(&self, track: &GaanaTrack) -> Option<ApiTrack> {
        let title = track.title.as_ref().or(track.name.as_ref())?.clone();
        let duration = track.duration.unwrap_or(0.0) * 1000.0; // Convert to milliseconds

        if duration <= 0.0 {
            return None;
        }

        let author = track.artists.as_ref()
            .map(|a| format_artists(a))
            .unwrap_or_else(|| "Unknown".to_string());

        // Convert track_id from Value to String
        let identifier = track.track_id.as_ref()
            .and_then(|id| match id {
                serde_json::Value::String(s) => Some(s.clone()),
                serde_json::Value::Number(n) => Some(n.to_string()),
                _ => None,
            })
            .or_else(|| track.seokey.clone())?;

        let seokey = track.seokey.as_ref()?;
        let uri = track.song_url.clone()
            .unwrap_or_else(|| format!("https://gaana.com/song/{}", seokey));

        let info = ApiTrackInfo {
            identifier,
            is_seekable: true,
            author,
            length: duration as u64,
            is_stream: false,
            position: 0,
            title,
            uri: Some(uri),
            artwork_url: track.artwork_url.clone().or(track.artwork.clone()),
            isrc: track.isrc.clone(),
            source_name: "gaana".to_string(),
        };

        Some(ApiTrack {
            encoded: encode_track(&info).ok()?,
            info,
            plugin_info: crate::models::Empty,
        })
    }

    async fn get_stream_info(&self, track_id: &str) -> Result<GaanaStreamInfo, ResolverError> {
        let url = format!(
            "{}/api/stream/{}?quality={}",
            self.api_url,
            urlencoding::encode(track_id),
            urlencoding::encode(&self.stream_quality)
        );

        let response = self.get_json::<GaanaStreamResponse>(&url).await?;
        let hls_url = response.data.as_ref()
            .and_then(|d| d.hls_url.clone().or(d.hls_url_alt.clone()))
            .or(response.hls_url.clone())
            .or(response.hls_url_alt.clone());

        let direct_url = response.data.as_ref()
            .and_then(|d| d.url.clone())
            .or(response.url.clone());
        let stream_url = hls_url.clone().or(direct_url);
        let stream_url = stream_url.ok_or(ResolverError::MissingRequiredData("Stream URL"))?;

        let is_hls = hls_url.is_some();

        // Extract segments for non-HLS streams
        let segments: Vec<String> = if !is_hls {
            response.data.as_ref()
                .and_then(|d| d.segments.as_ref())
                .or(response.segments.as_ref())
                .map(|segs| {
                    segs.iter()
                        .filter_map(|seg| {
                            seg.get("url")
                                .and_then(|u| u.as_str())
                                .map(|s| s.to_string())
                                .or_else(|| seg.as_str().map(|s| s.to_string()))
                        })
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        // Extract init URL for segmented streams
        let init_url = if !is_hls {
            response.data.as_ref()
                .and_then(|d| d.init_url.clone().or(d.init_url_alt.clone()))
        } else {
            None
        };

        let format = if is_hls {
            "mpegts".to_string()
        } else {
            response.data.as_ref()
                .and_then(|d| d.format.clone())
                .unwrap_or_else(|| "mp4".to_string())
        };

        Ok(GaanaStreamInfo {
            url: stream_url,
            protocol: if is_hls { "hls".to_string() } else { "https".to_string() },
            format,
            init_url,
            segments,
        })
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T, ResolverError> {
        let response = self
            .client
            .get(url)
            .header("Accept", "application/json")
            .header("User-Agent", USER_AGENT)
            .header("Referer", "https://gaana.com/")
            .send()
            .await?;

        let status = response.status();
        let headers = response.headers().clone();
        
        if !status.is_success() {
            return Err(ResolverError::FailedStatusCode(status.to_string()));
        }

        // Log response headers for debugging
        if let Some(content_type) = headers.get("content-type") {
            tracing::debug!("Gaana API response Content-Type: {:?}", content_type);
        }
        if let Some(content_encoding) = headers.get("content-encoding") {
            tracing::debug!("Gaana API response Content-Encoding: {:?}", content_encoding);
        }

        // Get the response text first for better error handling
        let text = response.text().await?;
        
        if text.is_empty() {
            tracing::warn!("Gaana API returned empty response for URL: {}", url);
            return Err(ResolverError::Custom("Empty response from Gaana API".to_string()));
        }

        // Try to parse as JSON, with better error message
        serde_json::from_str(&text).map_err(|e| {
            // Safe preview that won't panic on UTF-8 boundaries or binary data
            let preview = if text.is_ascii() || text.len() <= 200 {
                text.chars().take(200).collect::<String>()
            } else {
                // For binary/corrupted data, show hex preview
                text.as_bytes()
                    .iter()
                    .take(100)
                    .map(|b| format!("{:02x}", b))
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            
            tracing::error!(
                "Failed to parse Gaana API response as JSON. URL: {}, Error: {}, Response preview: {}", 
                url, e, preview
            );
            ResolverError::Custom(format!("Invalid JSON from Gaana API: {}", e))
        })
    }
}
