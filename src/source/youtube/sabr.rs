use reqwest::Client;
use serde_json::Value;
use tracing::{debug, warn};
use crate::util::errors::ResolverError;
use crate::source::youtube::api::YOUTUBE_API_URL;

pub struct Sabr {
    http: Client,
    visitor_data: Option<String>,
    po_token: Option<String>,
}

impl Sabr {
    pub fn new(http: Client) -> Self {
        Self {
            http,
            visitor_data: None,
            po_token: None,
        }
    }

    pub async fn fetch_visitor_data(&mut self) -> Option<String> {
        // Method 1: Embed Page
        let response = self.http
            .get("https://www.youtube.com/embed")
            .header("Cookie", "YSC=cz5kYp3ZuIE; VISITOR_INFO1_LIVE=U-0T5oUyzf8;")
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/115.0.0.0 Safari/537.36")
            .send()
            .await
            .ok()?;

        if response.status().is_success() {
            if let Ok(body) = response.text().await {
                 let visitor_regex = regex::Regex::new(r#""VISITOR_DATA":"([^"]+)""#).ok()?;
                 if let Some(captures) = visitor_regex.captures(&body) {
                     if let Some(match_) = captures.get(1) {
                         let data = match_.as_str().to_string();
                         debug!("Fetched visitor data from embed: {}", data);
                         self.visitor_data = Some(data.clone());
                         return Some(data);
                     }
                 }
            }
        }

        // Method 2: Guide API (Fallback)
        debug!("Embed visitor data failed, trying API...");
        let payload = serde_json::json!({
            "context": {
                "client": {
                    "clientName": "WEB",
                    "clientVersion": "2.20230728.00.00",
                    "hl": "en",
                    "gl": "US"
                }
            }
        });

        let res = self.http.post(format!("{}/guide", YOUTUBE_API_URL))
            .json(&payload)
            .send()
            .await
            .ok()?;
        
        let body: Value = res.json().await.ok()?;
        if let Some(visitor_data) = body.get("responseContext")
            .and_then(|rc| rc.get("visitorData"))
            .and_then(|v| v.as_str()) 
        {
            debug!("Fetched visitor data from API: {}", visitor_data);
            self.visitor_data = Some(visitor_data.to_string());
            return Some(visitor_data.to_string());
        }

        None
    }

    pub async fn generate_po_token(&mut self) -> Option<String> {
        // Placeholder for PO Token generation logic.
        // For accurate porting, we would need to know the specific fields or have an external generator.
        // 
        // Current Strategy: 
        // 1. If we have a stored token, return it.
        // 2. If not, try to generate/fetch (Not implemented yet w/o implementation details).
        
        if let Some(token) = &self.po_token {
            return Some(token.clone());
        }

        warn!("PoToken generation logic is currently a placeholder. Requests might be throttled.");
        None
    }
    pub fn get_visitor_data(&self) -> Option<String> {
        self.visitor_data.clone()
    }

    pub fn get_po_token(&self) -> Option<String> {
        self.po_token.clone()
    }
}
