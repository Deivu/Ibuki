use std::time::{Duration, Instant};

use reqwest::Client;
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tracing::{debug, error, warn};

use crate::CONFIG;
use crate::util::errors::ResolverError;

const FALLBACK_PLAYER_HASH: &str = "00c52fa0";

const PLAYER_URL_TTL: Duration = Duration::from_secs(86400);

struct CachedPlayerUrl {
    url: String,
    fetched_at: Instant,
}

pub struct CipherManager {
    http: Client,
    server_url: Option<String>,
    auth_token: Option<String>,
    player_url_cache: Mutex<Option<CachedPlayerUrl>>,
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

        let browser_ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Safari/537.36";

        match self
            .http
            .get("https://www.youtube.com/embed/")
            .header("User-Agent", browser_ua)
            .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
            .header("Accept-Language", "en-US,en;q=0.5")
            .send()
            .await
        {
            Ok(resp) => match resp.text().await {
                Ok(text) => {
                    if let Some(start) = text.find("\"jsUrl\":\"") {
                        let rest = &text[start + 9..];
                        if let Some(end) = rest.find('"') {
                            let js_url = &rest[..end];
                            let full_url = if js_url.starts_with("http") {
                                js_url.to_string()
                            } else {
                                format!("https://www.youtube.com{}", js_url)
                            };
                            debug!("Discovered player URL from embed page jsUrl: {}", full_url);
                            return full_url;
                        }
                    }
                    warn!(
                        "embed/ response did not contain jsUrl (len={}); trying fallback sources",
                        text.len()
                    );
                }
                Err(e) => warn!("Failed to read embed/ body: {:?}", e),
            },
            Err(e) => warn!("Failed to fetch embed/ for player URL: {:?}", e),
        }

        let hash_re = regex::Regex::new(r"/s/player/([0-9a-f]{8})/").unwrap();

        let fallback_sources = [
            "https://www.youtube.com/iframe_api",
            "https://www.youtube.com/",
        ];

        for source in &fallback_sources {
            let text = match self
                .http
                .get(*source)
                .header("User-Agent", browser_ua)
                .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
                .header("Accept-Language", "en-US,en;q=0.5")
                .send()
                .await
            {
                Ok(resp) => match resp.text().await {
                    Ok(t) => t,
                    Err(e) => {
                        warn!("Failed to read body from {}: {:?}", source, e);
                        continue;
                    }
                },
                Err(e) => {
                    warn!("Failed to fetch {}: {:?}", source, e);
                    continue;
                }
            };

            if let Some(caps) = hash_re.captures(&text) {
                let url = format!(
                    "https://www.youtube.com/s/player/{}/player_ias.vflset/en_US/base.js",
                    &caps[1]
                );
                debug!("Discovered player hash from {}: {}", source, url);
                return url;
            }

            warn!(
                "Could not extract player hash from {} (len={}); trying next source",
                source,
                text.len()
            );
        }

        warn!(
            "All player URL sources failed; using fallback hash {}",
            FALLBACK_PLAYER_HASH
        );
        fallback
    }

    async fn get_player_url(&self) -> String {
        let mut cache = self.player_url_cache.lock().await;
        if let Some(ref cached) = *cache {
            if cached.fetched_at.elapsed() < PLAYER_URL_TTL {
                return cached.url.clone();
            }
            debug!("Player URL cache expired after 1 day, refreshing");
        }
        drop(cache);

        let url = self.fetch_player_url().await;

        let mut cache = self.player_url_cache.lock().await;
        *cache = Some(CachedPlayerUrl {
            url: url.clone(),
            fetched_at: Instant::now(),
        });
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
        self.resolve_url_with_sig_key(url, sp, n_param, None).await
    }

    pub async fn resolve_url_with_sig_key(
        &self,
        url: &str,
        sp: Option<&str>,
        n_param: Option<&str>,
        sig_key: Option<&str>,
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

        let obj = payload.as_object_mut().unwrap();
        if let Some(s) = sp {
            obj.insert("encrypted_signature".to_string(), json!(s));
        }
        if let Some(n) = n_param {
            obj.insert("n_param".to_string(), json!(n));
        }
        if let Some(key) = sig_key {
            obj.insert("signature_key".to_string(), json!(key));
        }

        let endpoint = format!("{}/resolve_url", base_url.trim_end_matches('/'));
        let mut req = self.http.post(&endpoint).json(&payload);

        if let Some(token) = &self.auth_token {
            req = req.header("Authorization", token);
        }

        let res = req.send().await.map_err(ResolverError::Reqwest)?;

        let status = res.status();
        let body: Value = res.json().await.map_err(ResolverError::Reqwest)?;

        if !status.is_success() || body.get("success").and_then(|v| v.as_bool()) == Some(false) {
            let msg = body
                .get("error")
                .and_then(|e| e.get("error"))
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| {
                    body.get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Unknown cipher error")
                });
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
