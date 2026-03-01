use std::time::Duration;

use regex::Regex;
use reqwest::Client;
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tracing::{debug, error, warn};

use crate::CONFIG;
use crate::util::errors::ResolverError;

/// Fallback player hash used when fetching the current one from YouTube fails.
const FALLBACK_PLAYER_HASH: &str = "00c52fa0";

pub struct CipherManager {
    http: Client,
    server_url: Option<String>,
    auth_token: Option<String>,
    /// Cached base.js player URL. Cleared when the cipher server returns an error
    /// so the next call will re-fetch a potentially updated hash.
    player_url_cache: Mutex<Option<String>>,
}

impl CipherManager {
    pub fn new() -> Self {
        let config = CONFIG
            .youtube_config
            .as_ref()
            .expect("YouTube config should be present");
        let cipher_config = config.cipher.as_ref();

        let server_url = cipher_config.map(|c| c.url.clone());
        let auth_token = cipher_config.and_then(|c| c.token.clone());

        if server_url.is_none() {
            warn!("Cipher Server URL is missing! Signature deciphering will fail.");
        }

        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_default();

        Self {
            http,
            server_url,
            auth_token,
            player_url_cache: Mutex::new(None),
        }
    }

    async fn fetch_player_url(&self) -> String {
        let fallback = format!(
            "https://www.youtube.com/s/player/{}/player_ias.vflset/en_US/base.js",
            FALLBACK_PLAYER_HASH
        );

        let resp = match self
            .http
            .get("https://www.youtube.com/iframe_api")
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64)")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                warn!("Failed to fetch iframe_api for player URL discovery: {:?}", e);
                return fallback;
            }
        };

        let text = match resp.text().await {
            Ok(t) => t,
            Err(e) => {
                warn!("Failed to read iframe_api body: {:?}", e);
                return fallback;
            }
        };

        let re = Regex::new(r"/s/player/([0-9a-f]{8})/").unwrap();
        if let Some(caps) = re.captures(&text) {
            let hash = &caps[1];
            let url = format!(
                "https://www.youtube.com/s/player/{}/player_ias.vflset/en_US/base.js",
                hash
            );
            debug!("Discovered player URL from iframe_api: {}", url);
            return url;
        }

        warn!(
            "Could not extract player hash from iframe_api response (len={}); using fallback hash {}",
            text.len(),
            FALLBACK_PLAYER_HASH
        );
        fallback
    }

    async fn get_player_url(&self) -> String {
        let cached = self.player_url_cache.lock().await.clone();
        if let Some(url) = cached {
            return url;
        }
        let url = self.fetch_player_url().await;
        *self.player_url_cache.lock().await = Some(url.clone());
        url
    }

    async fn invalidate_player_url(&self) {
        *self.player_url_cache.lock().await = None;
    }

    pub async fn resolve_url(
        &self,
        url: &str,
        sp: Option<&str>,
        n_param: Option<&str>,
    ) -> Result<String, ResolverError> {
        let Some(base_url) = &self.server_url else {
            return Err(ResolverError::Custom(
                "Cipher Server URL not configured".to_string(),
            ));
        };

        let player_url = self.get_player_url().await;

        let mut payload = json!({
            "stream_url": url,
            "player_url": player_url,
        });

        if let Some(s) = sp {
            payload
                .as_object_mut()
                .unwrap()
                .insert("encrypted_signature".to_string(), json!(s));
        }

        if let Some(n) = n_param {
            payload
                .as_object_mut()
                .unwrap()
                .insert("n_param".to_string(), json!(n));
        }

        let endpoint = format!("{}/resolve_url", base_url.trim_end_matches('/'));
        let mut req = self.http.post(&endpoint).json(&payload);

        if let Some(token) = &self.auth_token {
            req = req.header("Authorization", token);
        }

        let res = req.send().await.map_err(ResolverError::Reqwest)?;

        let status = res.status();
        let body: Value = res.json().await.map_err(ResolverError::Reqwest)?;

        // Server returns {"success": false, "error": {...}} on failure
        if !status.is_success() || body.get("success").and_then(|v| v.as_bool()) == Some(false) {
            let msg = body
                .get("error")
                .and_then(|e| e.get("error"))
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown cipher error");
            error!("Cipher Server Error: {} - {}", status, msg);
            self.invalidate_player_url().await;
            return Err(ResolverError::Custom(format!("Cipher Error: {}", msg)));
        }

        let resolved = body
            .get("resolved_url")
            .and_then(|v| v.as_str())
            .or_else(|| {
                body.get("data")
                    .and_then(|d| d.get("resolved_url"))
                    .and_then(|v| v.as_str())
            });

        if let Some(url) = resolved {
            Ok(url.to_string())
        } else {
            Err(ResolverError::Custom(
                "Cipher Server returned no resolved_url".to_string(),
            ))
        }
    }
}
