use std::time::Duration;

use dashmap::DashMap;
use reqwest::Client;
use serde_json::Value;
use tracing::{debug, info, warn};

use super::api::InnertubeApi;
use super::cipher::CipherManager;
use super::clients::{self, InnertubeClient};
use crate::CONFIG;
use crate::util::errors::ResolverError;

use std::sync::Arc;
use tokio::sync::Mutex;

use super::sabr::Sabr;
use super::oauth::YoutubeOAuth;

pub struct YouTubeManager {
    http: Client,
    api: InnertubeApi,
    cipher: CipherManager,
    sabr: Arc<Mutex<Sabr>>,
    oauth: Arc<Mutex<YoutubeOAuth>>,
    clients: DashMap<String, Box<dyn InnertubeClient>>,
    search_clients: Vec<String>,
    resolve_clients: Vec<String>,
    playback_clients: Vec<String>,
}

impl YouTubeManager {
    pub fn new() -> Self {
        let http = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();

        let api = InnertubeApi::new();
        let cipher = CipherManager::new();
        let clients = DashMap::new();
        let sabr = Arc::new(Mutex::new(Sabr::new(http.clone())));
        let oauth = Arc::new(Mutex::new(YoutubeOAuth::new(http.clone())));
        let youtube_config = CONFIG.youtube_config.as_ref().expect("YouTube config missing");
        
        // Helper to populate clients map
        let mut register_client = |name: &str| {
            if !clients.contains_key(name) {
                if let Some(client) = clients::get_client_by_name(name) {
                    clients.insert(name.to_string(), client);
                } else {
                    warn!("Unknown client requested in config: {}", name);
                }
            }
        };

        // Handle Option defaults if needed, but config struct has Options.
        // Assuming config is populated or we handle None.
        let search_list = youtube_config.clients.as_ref().and_then(|c| c.search.clone()).unwrap_or_default();
        let resolve_list = youtube_config.clients.as_ref().and_then(|c| c.resolve.clone()).unwrap_or_default();
        let playback_list = youtube_config.clients.as_ref().and_then(|c| c.playback.clone()).unwrap_or_default();

        for name in &search_list { register_client(name); }
        for name in &resolve_list { register_client(name); }
        for name in &playback_list { register_client(name); }

        Self {
            http,
            api,
            cipher,
            sabr,
            oauth,
            clients,
            search_clients: search_list,
            resolve_clients: resolve_list,
            playback_clients: playback_list,
        }
    }

    pub async fn setup(&self) {
        info!("Setting up YouTube Manager...");
        
        let mut sabr = self.sabr.lock().await;
        if let Some(visitor_data) = sabr.fetch_visitor_data().await {
            debug!("Initialized Visitor Data: {}", visitor_data);
        }
        
    }

    pub fn get_client(&self) -> Client {
        self.http.clone()
    }

    fn get_innertube_client(&self, name: &str) -> Option<Box<dyn InnertubeClient>> {
        clients::get_client_by_name(name)
    }

    pub async fn search(&self, query: &str) -> Result<Value, ResolverError> {
        let client_name = self.search_clients.first().cloned().unwrap_or("Android".to_string());
        let client = self.get_innertube_client(&client_name).ok_or(ResolverError::Custom("No search client available".to_string()))?;

        // Extract context data
        let visitor_data = self.sabr.lock().await.get_visitor_data();
        let oauth_token = self.oauth.lock().await.get_access_token();

        self.api.search(query, client.as_ref(), None, visitor_data.as_deref(), oauth_token.as_deref()).await
    }

    pub async fn resolve_video(&self, video_id: &str) -> Result<Value, ResolverError> {
        let mut last_error = ResolverError::Custom("No clients configured for resolution".to_string());
        
        // Extract context data once
        let (visitor_data, po_token) = {
            let sabr = self.sabr.lock().await;
            (sabr.get_visitor_data(), sabr.get_po_token())
        };
        let oauth_token = self.oauth.lock().await.get_access_token();

        for client_name in &self.resolve_clients {
            let Some(client) = self.get_innertube_client(client_name) else { continue; };
            debug!("Attempting to resolve {} with {}", video_id, client.name());

            match self.api.player(video_id, client.as_ref(), None, None, visitor_data.as_deref(), po_token.as_deref(), oauth_token.as_deref()).await {
                Ok(response) => {
                     if let Some(status) = response.get("playabilityStatus").and_then(|s| s.get("status")).and_then(|s| s.as_str()) {
                         if status == "OK" {
                             return Ok(response);
                         } else {
                             debug!("Client {} failed for {}: PlayabilityStatus {}", client.name(), video_id, status);
                         }
                     } else {
                         return Ok(response);
                     }
                },
                Err(e) => last_error = e,
            }
        }

        Err(last_error)
    }

    pub async fn make_playable(&self, video_id: &str) -> Result<String, ResolverError> {
         let mut last_error = ResolverError::Custom("No clients configured for playback".to_string());
         
         // Extract context data once
        let (visitor_data, po_token) = {
            let sabr = self.sabr.lock().await;
            (sabr.get_visitor_data(), sabr.get_po_token())
        };
        let oauth_token = self.oauth.lock().await.get_access_token();

         for client_name in &self.playback_clients {
              let Some(client) = self.get_innertube_client(client_name) else { continue; };
              
              let player_response = match self.api.player(video_id, client.as_ref(), None, None, visitor_data.as_deref(), po_token.as_deref(), oauth_token.as_deref()).await {
                  Ok(res) => res,
                  Err(e) => {
                      last_error = e;
                      continue;
                  }
              };

              let Some(streaming_data) = player_response.get("streamingData") else {
                  debug!("No streamingData for {} with {}", video_id, client.name());
                  continue;
              };

              let mut formats = Vec::new();
              if let Some(f) = streaming_data.get("formats").and_then(|v| v.as_array()) {
                  formats.extend(f.iter());
              }
              if let Some(f) = streaming_data.get("adaptiveFormats").and_then(|v| v.as_array()) {
                  formats.extend(f.iter());
              }

              let audio_format = formats.iter()
                 .filter(|f| f.get("mimeType").and_then(|m| m.as_str()).map(|s| s.starts_with("audio")).unwrap_or(false))
                 .max_by_key(|f| f.get("bitrate").and_then(|b| b.as_u64()).unwrap_or(0));
             
              if let Some(fmt) = audio_format {
                  if let Some(url) = fmt.get("url").and_then(|u| u.as_str()) {
                      debug!("Found direct URL with {}", client.name());
                      return Ok(url.to_string());
                  } else if let Some(sig_cipher) = fmt.get("signatureCipher").and_then(|s| s.as_str()) {
                      debug!("Signature cipher found with {}, attempting decipher", client.name());
                      let params: std::collections::HashMap<String, String> = url::form_urlencoded::parse(sig_cipher.as_bytes())
                         .into_owned()
                         .collect();
                     
                     if let (Some(url), Some(sig)) = (params.get("url"), params.get("s")) {
                          match self.cipher.resolve_url(url, Some(sig), params.get("n").map(|s| s.as_str()), "https://www.youtube.com/iframe_api").await {
                              Ok(deciphered) => return Ok(deciphered),
                              Err(e) => {
                                  warn!("Cipher resolution failed: {:?}", e);
                                  continue;
                              }
                          }
                     }
                  }
              }
          }
          
          Err(last_error)
     }
}
