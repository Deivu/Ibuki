use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use regex::Regex;
use reqwest::Client;
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use crate::CONFIG;
use crate::util::errors::ResolverError;

const FALLBACK_PLAYER_HASH: &str = "00c52fa0";
const PLAYER_URL_TTL: Duration = Duration::from_secs(86400);


struct CachedPlayerScript {
    url: String,
    signature_timestamp: Option<String>,
    local_cipher: Option<LocalSignatureCipher>,
    fetched_at: Instant,
}

#[derive(Debug, Clone, Copy)]
enum CipherOp {
    Swap(usize),
    Reverse,
    Splice(usize),
}

#[derive(Debug, Clone)]
struct LocalSignatureCipher {
    operations: Vec<CipherOp>,
    n_function: Option<String>,
}

impl LocalSignatureCipher {
    fn decipher_signature(&self, sig: &str) -> String {
        let mut chars: Vec<char> = sig.chars().collect();
        for op in &self.operations {
            match *op {
                CipherOp::Swap(b) => {
                    let pos = b % chars.len();
                    chars.swap(0, pos);
                }
                CipherOp::Reverse => {
                    chars.reverse();
                }
                CipherOp::Splice(b) => {
                    chars = chars[b..].to_vec();
                }
            }
        }
        chars.into_iter().collect()
    }
}

static TIMESTAMP_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:signatureTimestamp|sts):(\d+)").unwrap());

static ACTIONS_OBJECT_RE: LazyLock<Regex> =
    LazyLock::new(|| {
        Regex::new(
            r#"var\s+([$A-Za-z0-9_]+)\s*=\s*\{(?:\s*["']?[a-zA-Z_$][a-zA-Z_0-9$]*["']?\s*:\s*function\s*\([^)]*\)\s*\{[^{}]*(?:\{[^{}]*\}[^{}]*)*\}\s*,){2}\s*["']?[a-zA-Z_$][a-zA-Z_0-9$]*["']?\s*:\s*function\s*\([^)]*\)\s*\{[^{}]*(?:\{[^{}]*\}[^{}]*)*\}\s*\}"#
        ).unwrap()
    });

static SIG_FUNCTION_RE: LazyLock<Regex> =
    LazyLock::new(|| {
        Regex::new(
            r#"function(?:\s+[a-zA-Z_$][a-zA-Z_0-9$]*)?\(([a-zA-Z_$][a-zA-Z_0-9$]*)\)\{[a-zA-Z_$][a-zA-Z_0-9$]*=[a-zA-Z_$][a-zA-Z_0-9$]*\.split\(["']{2}\);\s*((?:[a-zA-Z_$][a-zA-Z_0-9$]*\.[a-zA-Z_$][a-zA-Z_0-9$]*\([^)]*\);?\s*)+)return [a-zA-Z_$][a-zA-Z_0-9$]*\.join\(["']{2}\)\}"#
        ).unwrap()
    });

static SIG_CALL_RE: LazyLock<Regex> =
    LazyLock::new(|| {
        Regex::new(
            r"([a-zA-Z_$][a-zA-Z_0-9$]*)\.([a-zA-Z_$][a-zA-Z_0-9$]*)\(([^)]*)\)"
        ).unwrap()
    });

static ACTION_METHOD_RE: LazyLock<Regex> =
    LazyLock::new(|| {
        Regex::new(
            r#"["']?([a-zA-Z_$][a-zA-Z_0-9$]*)["']?\s*:\s*function\s*\(([^)]*)\)\s*\{([^}]*(?:\{[^}]*\}[^}]*)*)\}"#
        ).unwrap()
    });

static N_FUNCTION_RE: LazyLock<Regex> =
    LazyLock::new(|| {
        Regex::new(
            r"function\(\s*([a-zA-Z_$][a-zA-Z_0-9$]*)\s*\)\s*\{var\s*([a-zA-Z_$][a-zA-Z_0-9$]*)=\1\[[a-zA-Z_$][a-zA-Z_0-9$]*\[\d+\]\]\([a-zA-Z_$][a-zA-Z_0-9$]*\[\d+\]\).*?catch\(\s*\w+\s*\)\s*\{\s*return.*?\+\s*\1\s*\}\s*return\s*\2\[[a-zA-Z_$][a-zA-Z_0-9$]*\[\d+\]\]\([a-zA-Z_$][a-zA-Z_0-9$]*\[\d+\]\)\};"
        ).unwrap()
    });

fn classify_action(body: &str, param_count: usize) -> Option<&'static str> {
    if body.contains(".reverse()") {
        return Some("reverse");
    }
    if body.contains(".splice(") {
        return Some("splice");
    }
    if param_count == 2
        && (body.contains("var c=") || body.contains("var d=") || body.contains("a[0]"))
    {
        return Some("swap");
    }
    None
}

fn extract_cipher_from_script(script: &str) -> Option<LocalSignatureCipher> {
    let actions_match = ACTIONS_OBJECT_RE.find(script)?;
    let actions_text = actions_match.as_str();
    let actions_name_re = Regex::new(r"var\s+([$A-Za-z0-9_]+)\s*=").ok()?;
    let actions_name = actions_name_re
        .captures(actions_text)?
        .get(1)?
        .as_str()
        .to_string();
    let mut method_types: HashMap<String, String> = HashMap::new();
    for cap in ACTION_METHOD_RE.captures_iter(actions_text) {
        let method_name = cap.get(1)?.as_str().to_string();
        let params = cap.get(2)?.as_str();
        let body = cap.get(3)?.as_str();
        let param_count = params.split(',').count();

        if let Some(op_type) = classify_action(body, param_count) {
            method_types.insert(method_name, op_type.to_string());
        }
    }

    debug!(
        "Parsed cipher actions object '{}': {:?}",
        actions_name, method_types
    );

    let sig_fn_match = SIG_FUNCTION_RE.captures(script)?;
    let calls_text = sig_fn_match.get(2)?.as_str();
    let mut operations = Vec::new();
    let escaped_name = regex::escape(&actions_name);
    let call_re = Regex::new(&format!(
        r"{}\.([a-zA-Z_$][a-zA-Z_0-9$]*)\(\s*[a-zA-Z_$][a-zA-Z_0-9$]*\s*(?:,\s*(\d+))?\s*\)",
        escaped_name
    ))
    .ok()?;

    for cap in call_re.captures_iter(calls_text) {
        let method = cap.get(1)?.as_str();
        let param: usize = cap
            .get(2)
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(0);

        match method_types.get(method).map(|s| s.as_str()) {
            Some("swap") => operations.push(CipherOp::Swap(param)),
            Some("reverse") => operations.push(CipherOp::Reverse),
            Some("splice") => operations.push(CipherOp::Splice(param)),
            _ => {
                warn!(
                    "Unknown cipher action method: {}.{} - skipping",
                    actions_name, method
                );
            }
        }
    }

    if operations.is_empty() {
        warn!("No cipher operations extracted from player script");
        return None;
    }

    debug!("Extracted {} cipher operations: {:?}", operations.len(), operations);

    let n_function = N_FUNCTION_RE.find(script).map(|m| m.as_str().to_string());
    if n_function.is_some() {
        debug!("Extracted n-function from player script");
    }

    Some(LocalSignatureCipher {
        operations,
        n_function,
    })
}

fn extract_timestamp(script: &str) -> Option<String> {
    TIMESTAMP_RE.captures(script).map(|c| c[1].to_string())
}

pub struct CipherManager {
    http: Client,
    server_url: Option<String>,
    auth_token: Option<String>,
    player_cache: Mutex<Option<CachedPlayerScript>>,
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
            warn!("Cipher Server URL is missing; will rely on local cipher only.");
        }

        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_default();

        Self {
            http,
            server_url,
            auth_token,
            player_cache: Mutex::new(None),
        }
    }

    fn canonical_player_url(hash: &str) -> String {
        format!(
            "https://www.youtube.com/s/player/{}/player_ias.vflset/en_US/base.js",
            hash
        )
    }

    fn extract_hash_from_player_url(url: &str) -> Option<&str> {
        let start = url.find("/s/player/")? + 10;
        let rest = &url[start..];
        let end = rest.find('/')?;
        Some(&rest[..end])
    }

    async fn try_fetch_embed_player_url(&self, browser_ua: &str) -> Option<String> {
        let text = self
            .http
            .get("https://www.youtube.com/embed/")
            .header("User-Agent", browser_ua)
            .header(
                "Accept",
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            )
            .header("Accept-Language", "en-US,en;q=0.5")
            .send()
            .await
            .ok()?
            .text()
            .await
            .ok()?;

        let start = text.find("\"jsUrl\":\"")?;
        let rest = &text[start + 9..];
        let js_url = &rest[..rest.find('"')?];
        let full_url = if js_url.starts_with("http") {
            js_url.to_string()
        } else {
            format!("https://www.youtube.com{}", js_url)
        };
        let canonical = Self::extract_hash_from_player_url(&full_url)
            .map(Self::canonical_player_url)
            .unwrap_or(full_url.clone());
        debug!(
            "Discovered player URL from embed page jsUrl: {} -> {}",
            full_url, canonical
        );
        Some(canonical)
    }

    async fn fetch_player_url(&self) -> String {
        let fallback = Self::canonical_player_url(FALLBACK_PLAYER_HASH);
        let browser_ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Safari/537.36";

        if let Some(url) = self.try_fetch_embed_player_url(browser_ua).await {
            return url;
        }
        warn!("embed/ page did not yield a player URL; trying fallback sources");

        let hash_re = Regex::new(r"/s/player/([0-9a-f]{8})/").unwrap();

        let fallback_sources = [
            "https://www.youtube.com/iframe_api",
            "https://www.youtube.com/",
        ];

        for source in &fallback_sources {
            let resp = match self
                .http
                .get(*source)
                .header("User-Agent", browser_ua)
                .header(
                    "Accept",
                    "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
                )
                .header("Accept-Language", "en-US,en;q=0.5")
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    warn!("Failed to fetch {}: {:?}", source, e);
                    continue;
                }
            };
            let text = match resp.text().await {
                Ok(t) => t,
                Err(e) => {
                    warn!("Failed to read body from {}: {:?}", source, e);
                    continue;
                }
            };

            if let Some(caps) = hash_re.captures(&text) {
                let url = Self::canonical_player_url(&caps[1]);
                debug!("Discovered player hash from {}: {}", source, url);
                return url;
            }
        }

        warn!(
            "All player URL sources failed; using fallback hash {}",
            FALLBACK_PLAYER_HASH
        );
        fallback
    }

    async fn download_player_script(&self, url: &str) -> Option<String> {
        let browser_ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Safari/537.36";
        let resp = self
            .http
            .get(url)
            .header("User-Agent", browser_ua)
            .send()
            .await
            .ok()?;

        if !resp.status().is_success() {
            warn!("Failed to download player script {}: {}", url, resp.status());
            return None;
        }

        resp.text().await.ok()
    }

    async fn ensure_player_script(&self) -> (String, Option<String>, Option<LocalSignatureCipher>) {
        {
            let cache = self.player_cache.lock().await;
            if let Some(ref cached) = *cache {
                if cached.fetched_at.elapsed() < PLAYER_URL_TTL {
                    return (
                        cached.url.clone(),
                        cached.signature_timestamp.clone(),
                        cached.local_cipher.clone(),
                    );
                }
                debug!("Player script cache expired, refreshing");
            }
        }

        let url = self.fetch_player_url().await;
        let (sts, local_cipher) = match self.download_player_script(&url).await {
            Some(script) => {
                let sts = extract_timestamp(&script);
                let cipher = extract_cipher_from_script(&script);

                if let Some(ref ts) = sts {
                    info!("Extracted signatureTimestamp {} from player script", ts);
                }
                if let Some(ref c) = cipher {
                    info!(
                        "Extracted local cipher with {} operations from player script",
                        c.operations.len()
                    );
                } else {
                    warn!("Failed to extract local cipher from player script");
                }

                (sts, cipher)
            }
            None => {
                warn!("Failed to download player script {}", url);
                let remote_sts = self.get_sts_from_remote(&url).await.ok();
                (remote_sts, None)
            }
        };

        let mut cache = self.player_cache.lock().await;
        *cache = Some(CachedPlayerScript {
            url: url.clone(),
            signature_timestamp: sts.clone(),
            local_cipher: local_cipher.clone(),
            fetched_at: Instant::now(),
        });

        (url, sts, local_cipher)
    }

    pub async fn invalidate_player_cache(&self) {
        *self.player_cache.lock().await = None;
    }

    pub async fn get_player_url(&self) -> String {
        self.ensure_player_script().await.0
    }

    pub async fn get_signature_timestamp(&self) -> Option<String> {
        self.ensure_player_script().await.1
    }

    async fn get_sts_from_remote(&self, player_url: &str) -> Result<String, ResolverError> {
        let Some(base_url) = &self.server_url else {
            return Err(ResolverError::Custom(
                "Cipher Server URL not configured".to_string(),
            ));
        };

        let endpoint = format!("{}/get_sts", base_url.trim_end_matches('/'));
        let payload = json!({ "player_url": player_url });
        let mut req = self.http.post(&endpoint).json(&payload);

        if let Some(token) = &self.auth_token {
            req = req.header("Authorization", token);
        }

        let res = req.send().await.map_err(ResolverError::Reqwest)?;
        let body: Value = res.json().await.map_err(ResolverError::Reqwest)?;

        body.get("sts")
            .and_then(|v| v.as_str())
            .or_else(|| body.get("sts").and_then(|v| v.as_u64()).map(|_| ""))
            .map(|s| {
                if s.is_empty() {
                    body.get("sts")
                        .and_then(|v| v.as_u64())
                        .map(|n| n.to_string())
                        .unwrap_or_default()
                } else {
                    s.to_string()
                }
            })
            .ok_or_else(|| {
                ResolverError::Custom("Remote cipher returned no STS".to_string())
            })
    }

    async fn resolve_url_remote(
        &self,
        url: &str,
        player_url: &str,
        sp: Option<&str>,
        n_param: Option<&str>,
        sig_key: Option<&str>,
    ) -> Result<String, ResolverError> {
        let Some(base_url) = &self.server_url else {
            return Err(ResolverError::Custom(
                "Cipher Server URL not configured".to_string(),
            ));
        };

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
            return Err(ResolverError::Custom(format!("Remote Cipher Error: {}", msg)));
        }

        body.get("resolved_url")
            .and_then(|v| v.as_str())
            .or_else(|| {
                body.get("data")
                    .and_then(|d| d.get("resolved_url"))
                    .and_then(|v| v.as_str())
            })
            .map(|u| u.to_string())
            .ok_or_else(|| {
                ResolverError::Custom("Remote Cipher returned no resolved_url".to_string())
            })
    }

    fn resolve_url_local(
        &self,
        url: &str,
        sp: Option<&str>,
        n_param: Option<&str>,
        sig_key: Option<&str>,
        cipher: &LocalSignatureCipher,
    ) -> Result<String, ResolverError> {
        let mut parsed = url::Url::parse(url)
            .map_err(|e| ResolverError::Custom(format!("Invalid URL: {}", e)))?;

        if let Some(encrypted_sig) = sp {
            let deciphered = cipher.decipher_signature(encrypted_sig);
            let key = sig_key.unwrap_or("signature");
            debug!("Local cipher: deciphered sig with key '{}'", key);
            parsed.query_pairs_mut().append_pair(key, &deciphered);
        }

        if n_param.is_some() {
            debug!("Local cipher: n-parameter present but cannot transform locally (throttled playback)");
        }

        Ok(parsed.to_string())
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
        let (player_url, _sts, local_cipher) = self.ensure_player_script().await;

        if self.server_url.is_some() {
            match self
                .resolve_url_remote(url, &player_url, sp, n_param, sig_key)
                .await
            {
                Ok(resolved) => return Ok(resolved),
                Err(e) => {
                    warn!("Remote cipher failed: {:?}; falling back to local cipher", e);
                }
            }
        }

        if let Some(ref cipher) = local_cipher {
            debug!("Using local cipher fallback for URL resolution");
            return self.resolve_url_local(url, sp, n_param, sig_key, cipher);
        }

        if sp.is_some() {
            self.invalidate_player_cache().await;
            return Err(ResolverError::Custom(
                "No cipher available (remote failed, local not extracted). Cannot decipher signature.".to_string(),
            ));
        }
        warn!("No cipher available for n-param transform; URL may be throttled");
        Ok(url.to_string())
    }
}
