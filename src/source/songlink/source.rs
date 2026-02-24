use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;
use songbird::input::Input;

use crate::util::source::{Query, Source};
use crate::models::{ApiTrack, ApiTrackResult};
use crate::util::errors::ResolverError;

pub struct Songlink {
    http: Client,
}

impl Songlink {
    pub fn new(http: Option<Client>) -> Self {
        Self {
            http: http.unwrap_or_else(|| Client::new()),
        }
    }

    async fn fetch_songlink_data(&self, url: &str) -> Option<Value> {
        let encoded_url = urlencoding::encode(url);
        let api_url = format!("https://api.song.link/v1-alpha.1/links?url={}&userCountry=US&songIfSingle=true", encoded_url);

        let res = self.http.get(&api_url).send().await.ok()?;
        
        if !res.status().is_success() {
             return None;
        }

        res.json().await.ok()
    }
}

#[async_trait]
impl Source for Songlink {
    fn get_name(&self) -> &'static str {
        "songlink"
    }

    fn get_client(&self) -> Client {
        self.http.clone()
    }

    fn parse_query(&self, query: &str) -> Option<Query> {
        if query.contains("song.link") || query.contains("album.link") || query.contains("artist.link") || query.contains("odesli.co") {
            return Some(Query::Url(query.to_string()));
        }
        None
    }

    async fn init(&self) -> Result<(), ResolverError> {
        tracing::info!("Songlink Source Initialized");
        Ok(())
    }

    async fn resolve(&self, query: Query) -> Result<Option<ApiTrackResult>, ResolverError> {
        let url = match query {
            Query::Url(u) => u,
            _ => return Ok(None),
        };

        if let Some(json) = self.fetch_songlink_data(&url).await {
            if let Some(links_map) = json.get("linksByPlatform").and_then(|l| l.as_object()) {
                let preferred = vec![
                    "spotify",
                    "appleMusic", 
                    "youtubeMusic",
                    "youtube",
                    "deezer",
                    "tidal",
                    "soundcloud"
                ];
                
                for platform in preferred {
                    if let Some(link_obj) = links_map.get(platform) {
                        if let Some(target_url) = link_obj.get("url").and_then(|u| u.as_str()) {
                             tracing::debug!("Songlink resolved to platform: {} -> {}", platform, target_url);
                             
                             // Resolve using source manager logic (find source for URL)
                             // We don't have direct check logic here, but we can iterate active sources
                             
                             for entry in crate::SOURCES.iter() {
                                 if entry.key() == "songlink" {
                                     continue;
                                 }
                                 
                                 let source = entry.value();
                                 if let Some(q) = source.to_inner_ref().parse_query(target_url) {
                                     // Found a source that handles this!
                                     if let Ok(Some(result)) = source.to_inner_ref().resolve(q).await {
                                         return Ok(Some(result));
                                     }
                                 }
                             }
                        }
                    }
                }
            }
        }

        Ok(None)
    }

    async fn make_playable(&self, _track: ApiTrack) -> Result<Input, ResolverError> {
        Err(ResolverError::Custom("Songlink source does not support direct playback".to_string()))
    }
}

