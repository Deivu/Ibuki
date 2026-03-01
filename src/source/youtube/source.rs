use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;
use songbird::input::Input;
use tracing::{debug, warn};

use super::manager::YouTubeManager;
use crate::models::{ApiTrack, ApiTrackInfo, ApiTrackResult};
use crate::util::errors::ResolverError;
use crate::util::source::{Query, Source};

pub struct Youtube {
    manager: YouTubeManager,
}

impl Youtube {
    pub fn new(_http: Option<Client>) -> Self {
        Self {
            manager: YouTubeManager::new(),
        }
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
        match query {
            Query::Search(search_query) => {
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
                let stripped = if let Some(s) = url.strip_prefix("https://www.youtube.com/watch?v=")
                {
                    s
                } else if let Some(s) = url.strip_prefix("https://youtu.be/") {
                    s
                } else {
                    return Ok(None);
                };

                let video_id = stripped.split(&['&', '?'][..]).next().unwrap_or("");

                if video_id.len() != 11
                    || !video_id
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                {
                    tracing::warn!("YouTube: Invalid video ID extracted: {}", video_id);
                    return Ok(None);
                }

                let info = self.manager.resolve_video(video_id).await?;

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
        let (stream_url, client) = self.manager.make_playable(&track.info.identifier).await?;
        Ok(Input::from(crate::source::youtube::stream::YoutubeHttpStream::new(
            client, 
            stream_url, 
            reqwest::header::HeaderMap::new()
        )))
    }
}
