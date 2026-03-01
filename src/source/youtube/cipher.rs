use reqwest::Client;
use serde_json::{Value, json};
use tracing::{error, warn};

use crate::CONFIG;
use crate::util::errors::ResolverError;

pub struct CipherManager {
    http: Client,
    server_url: Option<String>,
    auth_token: Option<String>,
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

        Self {
            http: Client::new(),
            server_url,
            auth_token,
        }
    }

    pub async fn resolve_url(
        &self,
        url: &str,
        sp: Option<&str>,
        n_param: Option<&str>,
        player_url: &str,
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

        let endpoint = format!(
            "{}/resolve_url",
            base_url.trim_end_matches('/')
        );
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
            return Err(ResolverError::Custom(format!("Cipher Error: {}", msg)));
        }

        // Resolved URL may be at top level or nested under "data"
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
