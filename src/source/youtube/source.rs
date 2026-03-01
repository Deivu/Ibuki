use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;
use songbird::input::Input;
use std::sync::Arc;
use tracing::{debug, warn};

use super::manager::YouTubeManager;
use crate::models::{ApiTrack, ApiTrackInfo, ApiTrackResult};
use crate::util::errors::ResolverError;
use crate::util::source::{Query, Source};

pub struct Youtube {
    manager: Arc<YouTubeManager>,
}

impl Youtube {
    pub fn new(_http: Option<Client>) -> Self {
        let manager = Arc::new(YouTubeManager::new());
        let _ = super::YOUTUBE_MANAGER.set(Arc::clone(&manager));
        Self { manager }
    }

    fn parse_video_details(&self, details: &Value) -> Option<ApiTrackInfo> {
        let id = details.get("videoId")?.as_str()?.to_string();
        let title = details.get("title")?.as_str()?.to_string();
        let author = details
            .get("author")?
            .as_str()
            .unwrap_or("Unknown Author")
            .to_string();
        let length = details
            .get("lengthSeconds")?
            .as_str()?
            .parse::<u64>()
            .unwrap_or(0)
            * 1000;
        let is_stream = details
            .get("isLiveContent")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        Some(ApiTrackInfo {
            title,
            author,
            length,
            identifier: id.clone(),
            is_stream,
            uri: Some(format!("https://www.youtube.com/watch?v={}", id)),
            artwork_url: Some(format!("https://i.ytimg.com/vi/{}/maxresdefault.jpg", id)),
            source_name: "youtube".to_string(),
            isrc: None,
            position: 0,
            is_seekable: !is_stream,
        })
    }

    fn extract_video_renderers(&self, value: &Value, tracks: &mut Vec<ApiTrack>) {
        match value {
            Value::Object(obj) => {
                if obj.contains_key("videoId") && obj.contains_key("title") {
                    if let Some(track) = self.parse_video_renderer(obj) {
                        tracks.push(track);
                        return;
                    }
                }

                for (_, v) in obj.iter() {
                    self.extract_video_renderers(v, tracks);
                }
            }
            Value::Array(arr) => {
                for item in arr {
                    self.extract_video_renderers(item, tracks);
                }
            }
            _ => {}
        }
    }

    fn parse_video_renderer(&self, obj: &serde_json::Map<String, Value>) -> Option<ApiTrack> {
        let id = obj.get("videoId")?.as_str()?;

        let title = obj
            .get("title")
            .and_then(|t| t.get("runs"))
            .and_then(|r| r.as_array())
            .and_then(|r| r.first())
            .and_then(|r| r.get("text"))
            .and_then(|t| t.as_str())
            .or_else(|| obj.get("title").and_then(|t| t.as_str()))?;

        let author = obj
            .get("ownerText")
            .or_else(|| obj.get("shortBylineText"))
            .and_then(|owner| owner.get("runs"))
            .and_then(|r| r.as_array())
            .and_then(|r| r.first())
            .and_then(|r| r.get("text"))
            .and_then(|t| t.as_str())
            .unwrap_or("Unknown")
            .to_string();

        let is_live = obj
            .get("badges")
            .and_then(|b| b.as_array())
            .map(|badges| {
                badges.iter().any(|badge| {
                    badge
                        .get("metadataBadgeRenderer")
                        .and_then(|r| r.get("style"))
                        .and_then(|s| s.as_str())
                        .map(|s| s.contains("LIVE"))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);

        let length = if is_live {
            0
        } else {
            obj.get("lengthText")
                .and_then(|l| l.get("runs"))
                .and_then(|r| r.as_array())
                .and_then(|arr| arr.first())
                .and_then(|first| first.get("text"))
                .and_then(|t| t.as_str())
                .or_else(|| {
                    obj.get("lengthText")
                        .and_then(|l| l.get("simpleText"))
                        .and_then(|t| t.as_str())
                })
                .or_else(|| {
                    obj.get("thumbnailOverlays")
                        .and_then(|overlays| overlays.as_array())
                        .and_then(|arr| {
                            arr.iter().find_map(|overlay| {
                                overlay
                                    .get("thumbnailOverlayTimeStatusRenderer")
                                    .and_then(|r| r.get("text"))
                                    .and_then(|t| t.get("runs"))
                                    .and_then(|r| r.as_array())
                                    .and_then(|arr| arr.first())
                                    .and_then(|first| first.get("text"))
                                    .and_then(|s| s.as_str())
                            })
                        })
                })
                .or_else(|| {
                    obj.get("thumbnailOverlays")
                        .and_then(|overlays| overlays.as_array())
                        .and_then(|arr| {
                            arr.iter().find_map(|overlay| {
                                overlay
                                    .get("thumbnailOverlayTimeStatusRenderer")
                                    .and_then(|r| r.get("text"))
                                    .and_then(|t| t.get("simpleText"))
                                    .and_then(|s| s.as_str())
                            })
                        })
                })
                .and_then(|time_str| {
                    let parts: Vec<&str> = time_str.split(':').collect();
                    match parts.len() {
                        2 => {
                            let mins = parts[0].parse::<u64>().ok()?;
                            let secs = parts[1].parse::<u64>().ok()?;
                            Some((mins * 60 + secs) * 1000)
                        }
                        3 => {
                            let hours = parts[0].parse::<u64>().ok()?;
                            let mins = parts[1].parse::<u64>().ok()?;
                            let secs = parts[2].parse::<u64>().ok()?;
                            Some((hours * 3600 + mins * 60 + secs) * 1000)
                        }
                        _ => None,
                    }
                })
                .or_else(|| {
                    obj.get("lengthSeconds")
                        .and_then(|l| l.as_str())
                        .and_then(|s| s.parse::<u64>().ok())
                        .map(|secs| secs * 1000)
                })
                .unwrap_or_else(|| {
                    tracing::warn!("Could not parse duration for video: {}", id);
                    0
                })
        };

        let info = ApiTrackInfo {
            title: title.to_string(),
            author,
            length,
            identifier: id.to_string(),
            is_stream: is_live,
            uri: Some(format!("https://www.youtube.com/watch?v={}", id)),
            artwork_url: Some(format!("https://i.ytimg.com/vi/{}/mqdefault.jpg", id)),
            source_name: "youtube".to_string(),
            isrc: None,
            position: 0,
            is_seekable: !is_live,
        };

        Some(ApiTrack {
            encoded: crate::util::encoder::encode_track(&info).ok()?,
            info,
            plugin_info: crate::models::Empty,
            user_data: None,
        })
    }
}

#[async_trait]
impl Source for Youtube {
    fn get_name(&self) -> &'static str {
        "youtube"
    }

    fn get_client(&self) -> Client {
        self.manager.get_client()
    }

    fn parse_query(&self, query: &str) -> Option<Query> {
        if query.starts_with("ytsearch:") {
            return Some(Query::Search(
                query.strip_prefix("ytsearch:").unwrap().to_string(),
            ));
        }
        if query.starts_with("spsearch:") {
            return Some(Query::Search(
                query.strip_prefix("spsearch:").unwrap().to_string(),
            ));
        }
        if query.starts_with("scsearch:") {
            return Some(Query::Search(
                query.strip_prefix("scsearch:").unwrap().to_string(),
            ));
        }
        if query.starts_with("ymsearch:") {
            return Some(Query::Search(
                query.strip_prefix("ymsearch:").unwrap().to_string(),
            ));
        }
        if query.contains("youtube.com") || query.contains("youtu.be") {
            return Some(Query::Url(query.to_string()));
        }
        None
    }

    async fn init(&self) -> Result<(), ResolverError> {
        self.manager.setup().await;
        Ok(())
    }

    async fn resolve(&self, query: Query) -> Result<Option<ApiTrackResult>, ResolverError> {
        let yt_cfg = crate::CONFIG.youtube_config.as_ref();
        let allow_search = yt_cfg
            .and_then(|c| c.allow_search)
            .unwrap_or(true);
        let allow_direct_video_ids = yt_cfg
            .and_then(|c| c.allow_direct_video_ids)
            .unwrap_or(true);
        let allow_direct_playlist_ids = yt_cfg
            .and_then(|c| c.allow_direct_playlist_ids)
            .unwrap_or(true);

        match query {
            Query::Search(search_query) => {
                if !allow_search {
                    return Ok(Some(ApiTrackResult::Error(
                        crate::models::ApiTrackLoadException {
                            message: "YouTube search is disabled".to_string(),
                            severity: crate::models::Severity::Common,
                            cause: "allowSearch is false in the YouTube config".to_string(),
                        },
                    )));
                }

                debug!("YouTube: Searching for: {}", search_query);
                let res = match self.manager.search(&search_query).await {
                    Ok(res) => {
                        debug!("YouTube: Search response received");
                        res
                    }
                    Err(e) => {
                        warn!("YouTube: Search failed: {:?}", e);
                        return Err(e);
                    }
                };

                let mut tracks = Vec::new();
                self.extract_video_renderers(&res, &mut tracks);

                if !tracks.is_empty() {
                    debug!("YouTube: Returning {} tracks", tracks.len());
                    return Ok(Some(ApiTrackResult::Search(tracks)));
                } else {
                    warn!("YouTube: No videoRenderer objects found in response");
                }
                Ok(None)
            }
            Query::Url(url) => {
                let parsed = url::Url::parse(&url).ok();

                let playlist_id = parsed.as_ref().and_then(|u| {
                    u.query_pairs()
                        .find(|(k, _)| k == "list")
                        .map(|(_, v)| v.to_string())
                });

                let video_id = parsed.as_ref().and_then(|u| {
                    u.query_pairs()
                        .find(|(k, _)| k == "v")
                        .map(|(_, v)| v.to_string())
                        .or_else(|| {
                            let path_segments: Vec<_> = u
                                .path_segments()
                                .map(|s| s.collect())
                                .unwrap_or_default();
                            if u.host_str() == Some("youtu.be") {
                                path_segments.first().map(|s| s.to_string())
                            } else if path_segments.first().map(|s| *s)
                                == Some("shorts")
                            {
                                path_segments.get(1).map(|s| s.to_string())
                            } else {
                                None
                            }
                        })
                });

                if let Some(pid) = playlist_id {
                    if !pid.starts_with("RD") && allow_direct_playlist_ids {
                        return self.resolve_playlist(&pid).await;
                    }
                }

                if !allow_direct_video_ids {
                    return Ok(Some(ApiTrackResult::Error(
                        crate::models::ApiTrackLoadException {
                            message: "YouTube direct video IDs are disabled".to_string(),
                            severity: crate::models::Severity::Common,
                            cause: "allowDirectVideoIds is false in the YouTube config".to_string(),
                        },
                    )));
                }

                let vid = match video_id {
                    Some(v) => v,
                    None => {
                        warn!("YouTube: Could not extract video ID from URL: {}", url);
                        return Ok(None);
                    }
                };

                if vid.len() != 11
                    || !vid.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                {
                    warn!("YouTube: Invalid video ID extracted: {}", vid);
                    return Ok(None);
                }

                let info = self.manager.resolve_video(&vid).await?;

                if let Some(details) = info.get("videoDetails") {
                    if let Some(track_info) = self.parse_video_details(details) {
                        return Ok(Some(ApiTrackResult::Track(ApiTrack {
                            encoded: crate::util::encoder::encode_track(&track_info)?,
                            info: track_info,
                            plugin_info: crate::models::Empty,
                            user_data: None,
                        })));
                    }
                }
                Ok(None)
            }
        }
    }

    async fn make_playable(&self, track: ApiTrack) -> Result<Input, ResolverError> {
        let (stream_url, client, headers) =
            self.manager.make_playable(&track.info.identifier).await?;
        Ok(Input::from(
            crate::source::youtube::stream::YoutubeHttpStream::new(
                client,
                stream_url,
                headers,
            ),
        ))
    }
}


impl Youtube {
    async fn resolve_playlist(
        &self,
        playlist_id: &str,
    ) -> Result<Option<ApiTrackResult>, ResolverError> {
        debug!("YouTube: Loading playlist {}", playlist_id);

        let (name, video_renderers) = match self.manager.load_playlist(playlist_id).await {
            Ok(r) => r,
            Err(e) => {
                warn!("YouTube: Playlist load failed for {}: {:?}", playlist_id, e);
                return Err(e);
            }
        };

        let mut tracks: Vec<ApiTrack> = Vec::new();
        for renderer in &video_renderers {
            if let Some(obj) = renderer.as_object() {
                if let Some(track) = self.parse_playlist_video_renderer(obj) {
                    tracks.push(track);
                }
            }
        }

        if tracks.is_empty() {
            return Ok(Some(ApiTrackResult::Empty(None)));
        }

        Ok(Some(ApiTrackResult::Playlist(
            crate::models::ApiTrackPlaylist {
                info: crate::models::ApiPlaylistInfo {
                    name,
                    selected_track: 0,
                },
                plugin_info: crate::models::Empty,
                tracks,
            },
        )))
    }

    fn parse_playlist_video_renderer(
        &self,
        obj: &serde_json::Map<String, Value>,
    ) -> Option<ApiTrack> {
        let id = obj.get("videoId")?.as_str()?;

        let is_playable = obj
            .get("isPlayable")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        if !is_playable {
            return None;
        }

        let title = obj
            .get("title")
            .and_then(|t| t.get("runs"))
            .and_then(|r| r.as_array())
            .and_then(|r| r.first())
            .and_then(|r| r.get("text"))
            .and_then(|t| t.as_str())
            .or_else(|| obj.get("title").and_then(|t| t.as_str()))?;

        let author = obj
            .get("shortBylineText")
            .and_then(|t| t.get("runs"))
            .and_then(|r| r.as_array())
            .and_then(|r| r.first())
            .and_then(|r| r.get("text"))
            .and_then(|t| t.as_str())
            .unwrap_or("Unknown")
            .to_string();

        let length = obj
            .get("lengthSeconds")
            .and_then(|l| l.as_str())
            .and_then(|s| s.parse::<u64>().ok())
            .map(|secs| secs * 1000)
            .unwrap_or(0);

        let info = ApiTrackInfo {
            title: title.to_string(),
            author,
            length,
            identifier: id.to_string(),
            is_stream: false,
            uri: Some(format!("https://www.youtube.com/watch?v={}", id)),
            artwork_url: Some(format!("https://i.ytimg.com/vi/{}/mqdefault.jpg", id)),
            source_name: "youtube".to_string(),
            isrc: None,
            position: 0,
            is_seekable: true,
        };

        Some(ApiTrack {
            encoded: crate::util::encoder::encode_track(&info).ok()?,
            info,
            plugin_info: crate::models::Empty,
            user_data: None,
        })
    }
}
