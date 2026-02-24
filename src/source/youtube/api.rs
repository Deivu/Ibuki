use reqwest::{Client, Url};
use serde_json::{json, Value};
use tracing::error;

use super::clients::{InnertubeClient, InnertubeContext};
use crate::util::errors::ResolverError;

pub const YOUTUBE_API_URL: &str = "https://www.youtube.com/youtubei/v1";

pub struct InnertubeApi {
    http: Client,
}

impl InnertubeApi {
    pub fn new() -> Self {
        Self {
            http: Client::new(),
        }
    }

    pub async fn make_request(
        &self,
        endpoint: &str,
        client: &dyn InnertubeClient,
        context: &InnertubeContext,
        payload: Value,
        extra_headers: &[(String, String)],
    ) -> Result<Value, ResolverError> {
        let api_key = std::env::var("INNERTUBE_API_KEY")
            .map_err(|_| ResolverError::Custom("Missing INNERTUBE_API_KEY environment variable".to_string()))?;

        let mut url = Url::parse(&format!("{}{}", YOUTUBE_API_URL, endpoint))
            .map_err(|_| ResolverError::Custom("Invalid API URL".to_string()))?;

        url.query_pairs_mut().append_pair("key", &api_key);
        
        let mut req_builder = self.http.post(url);

        for (k, v) in extra_headers {
            req_builder = req_builder.header(k, v);
        }

        // Add Context to Payload
        let mut final_payload = payload;
        if let Some(obj) = final_payload.as_object_mut() {
             obj.insert("context".to_string(), json!(context));
             if let Some(extra) = client.extra_payload() {
                 if let Some(extra_map) = extra.as_object() {
                     for (key, val) in extra_map {
                         obj.insert(key.clone(), val.clone());
                     }
                 }
             }
        }

        let res = req_builder
            .json(&final_payload)
            .send()
            .await
            .map_err(ResolverError::Reqwest)?;

        if !res.status().is_success() {
             let status = res.status();
             let text = res.text().await.unwrap_or_default();
             error!("Innertube API Error: {} - {}", status, text);
             return Err(ResolverError::Custom(format!("API Error: {}", status)));
        }

        let body: Value = res.json().await.map_err(ResolverError::Reqwest)?;
        Ok(body)
    }

    pub async fn search(
        &self,
        query: &str,
        client: &dyn InnertubeClient,
        params: Option<&str>,
        visitor_data: Option<&str>,
        oauth_token: Option<&str>,
    ) -> Result<Value, ResolverError> {
        let mut payload = json!({
            "query": query,
        });

        if let Some(p) = params {
            payload.as_object_mut().unwrap().insert("params".to_string(), json!(p));
        }

        let mut context = client.context();
        if let Some(vd) = visitor_data {
            if let Some(client_info) = &mut context.client.visitor_data {
                *client_info = vd.to_string();
            } else {
                context.client.visitor_data = Some(vd.to_string());
            }
        }

        let mut headers = client.extra_headers();
        if let Some(token) = oauth_token {
            headers.push(("Authorization".to_string(), format!("Bearer {}", token)));
        }

        self.make_request("/search", client, &context, payload, &headers).await
    }

    pub async fn player(
        &self,
        video_id: &str,
        client: &dyn InnertubeClient,
        start_time: Option<u64>,
        playlist_id: Option<&str>,
        visitor_data: Option<&str>,
        po_token: Option<&str>,
        oauth_token: Option<&str>,
    ) -> Result<Value, ResolverError> {
        // Prepare main payload
        let mut payload = json!({
            "videoId": video_id,
            "playbackContext": {
                "contentPlaybackContext": {
                    "autoCaptionsDefaultOn": false,
                    "autonavState": "STATE_OFF",
                    "html5Preference": "HTML5_PREF_WANTS",
                    "lactMilliseconds": "-1",
                    "mdxContext": {},
                    "playerWidthPixels": 1280,
                    "playerHeightPixels": 720,
                    "referer": "https://www.youtube.com/",
                    "signatureTimestamp": 19766
                }
            }
        });

        if let Some(pid) = playlist_id {
            payload.as_object_mut().unwrap().insert("playlistId".to_string(), json!(pid));
        }

        if let Some(start) = start_time {
             payload.as_object_mut().unwrap().insert("startTimeSecs".to_string(), json!(start));
        }
        
        let mut context = client.context();
        if let Some(vd) = visitor_data {
             context.client.visitor_data = Some(vd.to_string());
        }

        let mut headers = client.extra_headers();
        if let Some(token) = oauth_token {
            headers.push(("Authorization".to_string(), format!("Bearer {}", token)));
        }
        
        if let Some(po) = po_token {
             if let Some(p) = payload.as_object_mut() {
                 p.insert("serviceIntegrityDimensions".to_string(), json!({
                     "poToken": po
                 }));
             }
        }

        self.make_request("/player", client, &context, payload, &headers).await
    }

    pub async fn next(
        &self,
        video_id: Option<&str>,
        playlist_id: Option<&str>,
        continuation: Option<&str>,
        client: &dyn InnertubeClient,
        visitor_data: Option<&str>,
        oauth_token: Option<&str>,
    ) -> Result<Value, ResolverError> {
        let mut payload = json!({});

        if let Some(vid) = video_id {
            payload.as_object_mut().unwrap().insert("videoId".to_string(), json!(vid));
        }

        if let Some(pid) = playlist_id {
            payload.as_object_mut().unwrap().insert("playlistId".to_string(), json!(pid));
        }

        if let Some(cont) = continuation {
            payload.as_object_mut().unwrap().insert("continuation".to_string(), json!(cont));
        }
        
        let mut context = client.context();
        if let Some(vd) = visitor_data {
             context.client.visitor_data = Some(vd.to_string());
        }
        
        let mut headers = client.extra_headers();
        if let Some(token) = oauth_token {
            headers.push(("Authorization".to_string(), format!("Bearer {}", token)));
        }

        self.make_request("/next", client, &context, payload, &headers).await
    }
}
