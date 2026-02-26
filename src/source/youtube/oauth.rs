use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{debug, error, info};

use crate::util::errors::ResolverError;
use crate::util::http::is_bind_error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OauthToken {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: u64,
    pub token_type: String,
}

pub struct YoutubeOAuth {
    http: Client,
    token: Option<OauthToken>,
}

impl YoutubeOAuth {
    pub fn new(http: Client) -> Self {
        Self { http, token: None }
    }

    pub fn set_token(&mut self, token: OauthToken) {
        self.token = Some(token);
    }

    pub fn is_valid(&self) -> bool {
        if let Some(token) = &self.token {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            return token.expires_at > now + 60; // Buffer 60s
        }
        false
    }

    pub fn get_access_token(&self) -> Option<String> {
        self.token.as_ref().map(|t| t.access_token.clone())
    }

    pub fn get_refresh_token(&self) -> Option<String> {
        self.token.as_ref().map(|t| t.refresh_token.clone())
    }

    pub async fn refresh_if_needed(&mut self) -> Result<Option<String>, ResolverError> {
        if self.is_valid() {
            return Ok(self.token.as_ref().map(|t| t.access_token.clone()));
        }

        if let Some(token) = &self.token {
            debug!("Refreshing OAuth token...");
            let new_token = self.refresh_access_token(&token.refresh_token).await?;
            self.token = Some(new_token.clone());
            info!("OAuth token refreshed successfully.");
            Ok(Some(new_token.access_token))
        } else {
            Ok(None)
        }
    }

    async fn refresh_access_token(&self, refresh_token: &str) -> Result<OauthToken, ResolverError> {
        let params = [
            (
                "client_id",
                "861556708454-d6dlm3lh05ig8aa4ea9398830989024p.apps.googleusercontent.com",
            ), // Common YouTube TV client ID
            ("client_secret", "S1M2-4-E34E"), // Common secret? Or none. TV client usually public.
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ];

        let (http_client, bound_ip) = crate::get_client();
        let res = http_client
            .post("https://oauth2.googleapis.com/token")
            .form(&params)
            .send()
            .await;

        let (res, fallback_used) = match res {
            Ok(r) => (r, false),
            Err(e) => {
                if !is_bind_error(&e) {
                    return Err(ResolverError::Reqwest(e));
                }

                tracing::error!(
                    "RoutePlanner: OAuth: System failed to bind to local IP {:?}. Check your 'ipBlocks'. OS Error: {}",
                    bound_ip,
                    e
                );
                tracing::warn!(
                    "RoutePlanner: Falling back to default system interface for OAuth request."
                );

                if let (Some(planner), Some(ip)) = (&*crate::ROUTE_PLANNER, bound_ip) {
                    planner.ban_ip(ip);
                }

                let fallback_res = crate::REQWEST
                    .post("https://oauth2.googleapis.com/token")
                    .form(&params)
                    .send()
                    .await
                    .map_err(ResolverError::Reqwest)?;

                (fallback_res, true)
            }
        };

        if !fallback_used && res.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            if let (Some(planner), Some(ip)) = (&*crate::ROUTE_PLANNER, bound_ip) {
                planner.ban_ip(ip);
            }
        }

        if !res.status().is_success() {
            let text = res.text().await.unwrap_or_default();
            error!("OAuth Refresh Failed: {}", text);
            return Err(ResolverError::Custom(format!(
                "OAuth Refresh Failed: {}",
                text
            )));
        }

        let body: Value = res.json().await.map_err(ResolverError::Reqwest)?;

        let access_token = body
            .get("access_token")
            .and_then(|v| v.as_str())
            .ok_or(ResolverError::Custom("No access_token".to_string()))?
            .to_string();
        let expires_in = body
            .get("expires_in")
            .and_then(|v| v.as_u64())
            .unwrap_or(3600);
        let new_refresh_token = body
            .get("refresh_token")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or(refresh_token.to_string());

        let expires_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + expires_in;

        Ok(OauthToken {
            access_token,
            refresh_token: new_refresh_token,
            expires_at,
            token_type: "Bearer".to_string(),
        })
    }
}
