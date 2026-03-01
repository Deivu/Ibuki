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

use super::oauth::YoutubeOAuth;
use super::sabr::Sabr;

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
        let youtube_config = CONFIG
            .youtube_config
            .as_ref()
            .expect("YouTube config missing");

        let register_client = |name: &str| {
            if !clients.contains_key(name) {
                if let Some(client) = clients::get_client_by_name(name) {
                    clients.insert(name.to_string(), client);
                } else {
                    warn!("Unknown client requested in config: {}", name);
                }
            }
        };

        let search_list = youtube_config
            .clients
            .as_ref()
            .and_then(|c| c.search.clone())
            .unwrap_or_default();
        let resolve_list = youtube_config
            .clients
            .as_ref()
            .and_then(|c| c.resolve.clone())
            .unwrap_or_default();
        let playback_list = youtube_config
            .clients
            .as_ref()
            .and_then(|c| c.playback.clone())
            .unwrap_or_default();

        for name in &search_list {
            register_client(name);
        }
        for name in &resolve_list {
            register_client(name);
        }
        for name in &playback_list {
            register_client(name);
        }

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

        let youtube_config = CONFIG.youtube_config.as_ref();
 
        if let Some(settings) = youtube_config.and_then(|c| c.clients.as_ref()).and_then(|c| c.settings.as_ref()) {
            if let Some(tv_settings) = settings.get("TV") {
                if let Some(token_val) = &tv_settings.refresh_token {
                    let token = if let Some(s) = token_val.as_str() {
                        Some(s.to_string())
                    } else if let Some(a) = token_val.as_array().and_then(|a| a.first()).and_then(|v| v.as_str()) {
                        Some(a.to_string())
                    } else {
                        None
                    };
 
                    if let Some(t) = token {
                        let mut oauth = self.oauth.lock().await;
                        oauth.set_refresh_token(t);
                        match oauth.refresh_if_needed().await {
                            Ok(_) => info!("YouTube OAuth initialized with TV refresh token."),
                            Err(e) => warn!("YouTube OAuth initial refresh failed: {:?}", e),
                        }
                    }
                }
            }
        }
 
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
        let (http_client, bound_ip) = crate::get_client();
        let client_name = self
            .search_clients
            .first()
            .cloned()
            .unwrap_or("Web".to_string());
        let client = self
            .get_innertube_client(&client_name)
            .ok_or(ResolverError::Custom(
                "No search client available".to_string(),
            ))?;

        let (visitor_data, po_token) = {
            let sabr = self.sabr.lock().await;
            (sabr.get_visitor_data(), sabr.get_po_token())
        };
        let oauth_token = self.oauth.lock().await.get_access_token();

        self.api
            .search(
                query,
                client.as_ref(),
                None,
                visitor_data.as_deref(),
                po_token.as_deref(),
                oauth_token.as_deref(),
                &http_client,
                bound_ip,
            )
            .await
    }

    pub async fn fetch_encrypted_host_flags(&self, video_id: &str) -> Option<String> {
        let url = format!("https://www.youtube.com/embed/{}", video_id);
        let res = self.http.get(url)
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .send()
            .await
            .ok()?;
        
        let text = res.text().await.ok()?;
        let re = regex::Regex::new(r#""encryptedHostFlags":"([^"]+)""#).ok()?;
        re.captures(&text).and_then(|cap| cap.get(1).map(|m| m.as_str().to_string()))
    }

    pub async fn resolve_video(&self, video_id: &str) -> Result<Value, ResolverError> {
        let mut last_error =
            ResolverError::Custom("No clients configured for resolution".to_string());

        let (visitor_data, po_token) = {
            let sabr = self.sabr.lock().await;
            (sabr.get_visitor_data(), sabr.get_po_token())
        };
        let oauth_token = self.oauth.lock().await.get_access_token();

        let (http_client, bound_ip) = crate::get_client();
        for client_name in &self.resolve_clients {
            let Some(client) = self.get_innertube_client(client_name) else {
                continue;
            };
            debug!("Attempting to resolve {} with {}", video_id, client.name());

            let encrypted_host_flags = if client.name().to_lowercase().contains("embedded") {
                self.fetch_encrypted_host_flags(video_id).await
            } else {
                None
            };

            match self
                .api
                .player(
                    video_id,
                    client.as_ref(),
                    client.player_params(),
                    None,
                    None,
                    visitor_data.as_deref(),
                    po_token.as_deref(),
                    oauth_token.as_deref(),
                    None,
                    encrypted_host_flags.as_deref(),
                    &http_client,
                    bound_ip,
                )
                .await
            {
                Ok(response) => {
                    let status = response
                        .get("playabilityStatus")
                        .and_then(|s| s.get("status"))
                        .and_then(|s| s.as_str());

                    match status {
                        Some("OK") | None => return Ok(response),
                        Some(status) => {
                            debug!(
                                "Client {} failed for {}: PlayabilityStatus {}",
                                client.name(),
                                video_id,
                                status
                            );
                            last_error = ResolverError::Custom(format!(
                                "PlayabilityStatus {} for video {}",
                                status, video_id
                            ));
                        }
                    }
                }
                Err(e) => last_error = e,
            }
        }

        Err(last_error)
    }

    pub async fn make_playable(&self, video_id: &str) -> Result<(String, Client), ResolverError> {
        let mut last_error =
            ResolverError::Custom("No clients configured for playback".to_string());

        // Extract context data once
        let (visitor_data, po_token) = {
            let sabr = self.sabr.lock().await;
            (sabr.get_visitor_data(), sabr.get_po_token())
        };
        let oauth_token = self.oauth.lock().await.get_access_token();

        let (http_client, bound_ip) = crate::get_client();

        for client_name in &self.playback_clients {
            let Some(client) = self.get_innertube_client(client_name) else {
                continue;
            };

            let mut stream_client_builder = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30));
            if let Some(ip) = bound_ip {
                stream_client_builder = stream_client_builder.local_address(ip);
            }
            let mut headers = crate::util::headers::generate_headers().unwrap_or_default();
            let ua_str = client.context().client.user_agent.unwrap_or_else(|| {
                client.extra_headers()
                    .into_iter()
                    .find(|(k, _)| k.to_lowercase() == "user-agent")
                    .map(|(_, v)| v)
                    .unwrap_or_else(|| "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36".to_string())
            });
            if let Ok(ua) = reqwest::header::HeaderValue::from_str(&ua_str) {
                headers.insert(reqwest::header::USER_AGENT, ua);
            }
            let stream_client = stream_client_builder
                .default_headers(headers)
                .build()
                .unwrap_or_else(|_| http_client.clone());

            let encrypted_host_flags = if client.name().to_lowercase().contains("embedded") {
                self.fetch_encrypted_host_flags(video_id).await
            } else {
                None
            };

            let player_response = match self
                .api
                .player(
                    video_id,
                    client.as_ref(),
                    client.player_params(),
                    None,
                    None,
                    visitor_data.as_deref(),
                    po_token.as_deref(),
                    oauth_token.as_deref(),
                    None,
                    encrypted_host_flags.as_deref(),
                    &http_client,
                    bound_ip,
                )
                .await
            {
                Ok(res) => res,
                Err(e) => {
                    last_error = e;
                    continue;
                }
            };

            let Some(streaming_data) = player_response.get("streamingData") else {
                let status = player_response.get("playabilityStatus").and_then(|s| s.get("status")).and_then(|s| s.as_str()).unwrap_or("UNKNOWN");
                let reason = player_response.get("playabilityStatus").and_then(|s| s.get("reason")).and_then(|s| s.as_str()).unwrap_or("No reason provided");
                debug!("No streamingData for {} with {}. Status: {}, Reason: {}", video_id, client.name(), status, reason);
                continue;
            };

            let mut formats = Vec::new();
            if let Some(f) = streaming_data.get("formats").and_then(|v| v.as_array()) {
                formats.extend(f.iter());
            }
            if let Some(f) = streaming_data
                .get("adaptiveFormats")
                .and_then(|v| v.as_array())
            {
                formats.extend(f.iter());
            }

            let audio_format = formats
                .iter()
                .filter(|f| {
                    f.get("mimeType")
                        .and_then(|m| m.as_str())
                        .map(|s| s.starts_with("audio"))
                        .unwrap_or(false)
                })
                .max_by_key(|f| f.get("bitrate").and_then(|b| b.as_u64()).unwrap_or(0));

            if let Some(fmt) = audio_format {
                if let Some(url) = fmt.get("url").and_then(|u| u.as_str()) {
                    debug!("Found direct URL with {}", client.name());
                    debug!("Raw direct URL: {}", url);
                    let mut final_url = url.to_string();
                    
                    let parsed: Result<url::Url, _> = url::Url::parse(url);
                    if let Ok(parsed_url) = parsed {
                        let query_pairs: std::collections::HashMap<String, String> = parsed_url.query_pairs().into_owned().collect();
                        debug!("Query pairs for direct url keys: {:?}", query_pairs.keys().collect::<Vec<_>>());
                        if let Some(n) = query_pairs.get("n") {
                            debug!("Found 'n' parameter in direct URL, attempting decipher");
                            match self
                                .cipher
                                .resolve_url(
                                    url,
                                    None,
                                    Some(n),
                                    "https://www.youtube.com/iframe_api",
                                )
                                .await
                            {
                                Ok(deciphered) => final_url = deciphered,
                                Err(e) => {
                                    warn!("N-parameter decipher failed for {}: {:?}", client.name(), e);
                                    // if cipher fails, allow the request to potentially try failing with 403, and Lavalink behavior falls back to next client? 
                                    // Wait, we shouldn't continue and abort the current client. We can just use the raw url and let it natively fail, or we can `continue;` to the next client! 
                                    // Lavalink tries the stream format and if cipher fails, it skips it.
                                    last_error = e;
                                    continue;
                                }
                            }
                        }
                    }

                    if let Some(po) = &po_token {
                        if final_url.contains('?') {
                            final_url.push_str(&format!("&pot={}", po));
                        } else {
                            final_url.push_str(&format!("?pot={}", po));
                        }
                    }
                    return Ok((final_url, stream_client));
                } else if let Some(sig_cipher) = fmt.get("signatureCipher").and_then(|s| s.as_str())
                {
                    debug!(
                        "Signature cipher found with {}, attempting decipher",
                        client.name()
                    );
                    let params: std::collections::HashMap<String, String> =
                        url::form_urlencoded::parse(sig_cipher.as_bytes())
                            .into_owned()
                            .collect();

                    if let (Some(url), Some(sig)) = (params.get("url"), params.get("s")) {
                        match self
                            .cipher
                            .resolve_url(
                                url,
                                Some(sig),
                                params.get("n").map(|s| s.as_str()),
                                "https://www.youtube.com/iframe_api",
                            )
                            .await
                        {
                            Ok(deciphered) => {
                                let mut final_url = deciphered;
                                if let Some(po) = &po_token {
                                    if final_url.contains('?') {
                                        final_url.push_str(&format!("&pot={}", po));
                                    } else {
                                        final_url.push_str(&format!("?pot={}", po));
                                    }
                                }
                                return Ok((final_url, stream_client));
                            }
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
