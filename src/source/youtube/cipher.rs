use reqwest::Client;
use serde_json::{json, Value};
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
        let config = CONFIG.youtube_config.as_ref().expect("YouTube config should be present");
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
            return Err(ResolverError::Custom("Cipher Server URL not configured".to_string()));
        };

        let mut payload = json!({
            "url": url,
            "playerUrl": player_url,
        });

        if let Some(s) = sp {
            payload.as_object_mut().unwrap().insert("signature".to_string(), json!(s));
        }
        
        if let Some(n) = n_param {
            payload.as_object_mut().unwrap().insert("n".to_string(), json!(n));
        }

        let mut req = self.http.post(base_url).json(&payload);

        if let Some(token) = &self.auth_token {
            req = req.header("Authorization", token);
        }

        let res = req.send().await.map_err(ResolverError::Reqwest)?;

        if !res.status().is_success() {
             let status = res.status();
             let text = res.text().await.unwrap_or_default();
             error!("Cipher Server Error: {} - {}", status, text);
             return Err(ResolverError::Custom(format!("Cipher Error: {}", status)));
        }

        let body: Value = res.json().await.map_err(ResolverError::Reqwest)?;
        
        if let Some(resolved) = body.get("resolved_url").and_then(|v| v.as_str()) {
            Ok(resolved.to_string())
        } else {
            Err(ResolverError::Custom("Cipher Server returned no resolved_url".to_string()))
        }
    }
}
