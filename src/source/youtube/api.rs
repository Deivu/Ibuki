use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use std::time::Duration;
use tracing::error;

use super::clients::{InnertubeClient, InnertubeContext, InnertubeThirdParty};
use crate::util::errors::ResolverError;
use crate::util::http::is_bind_error;

pub const YOUTUBE_API_URL: &str = "https://www.youtube.com/youtubei/v1";

pub struct InnertubeApi {
    http: Client,
}

impl InnertubeApi {
    pub fn new() -> Self {
        Self {
            http: Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .expect("Failed to build HTTP client"),
        }
    }

    pub async fn make_request(
        &self,
        endpoint: &str,
        client: &dyn InnertubeClient,
        context: &InnertubeContext,
        payload: Value,
        extra_headers: &[(String, String)],
        http_client: &Client,
        bound_ip: Option<std::net::IpAddr>,
    ) -> Result<Value, ResolverError> {
        let mut req_builder = http_client.post(format!("{}{}", YOUTUBE_API_URL, endpoint))
            .header("Cookie", "CONSENT=YES+cb.20210328-17-p0.en+FX+471");
        
        for (k, v) in extra_headers {
            req_builder = req_builder.header(k, v);
        }

        let mut final_payload = payload;
        if let Some(obj) = final_payload.as_object_mut() {
            obj.insert("context".to_string(), json!(context));
            if let Some(Value::Object(extra_map)) = client.extra_payload() {
                for (key, val) in extra_map {
                    obj.insert(key, val);
                }
            }
        }

        let res = req_builder.json(&final_payload).send().await;

        let (res, fallback_used) = match res {
            Ok(r) => (r, false),
            Err(e) => {
                if !is_bind_error(&e) {
                    return Err(ResolverError::Reqwest(e));
                }

                tracing::error!(
                    "RoutePlanner: System failed to bind to local IP {:?}. Check your 'ipBlocks' in config.json. OS Error: {}",
                    bound_ip,
                    e
                );
                tracing::warn!(
                    "RoutePlanner: Falling back to default system interface for this request."
                );

                if let (Some(planner), Some(ip)) = (&*crate::ROUTE_PLANNER, bound_ip) {
                    planner.ban_ip(ip);
                }

                let mut fallback_builder =
                    crate::REQWEST.post(format!("{}{}", YOUTUBE_API_URL, endpoint));
                for (k, v) in extra_headers {
                    fallback_builder = fallback_builder.header(k, v);
                }

                let fallback_res = fallback_builder
                    .json(&final_payload)
                    .send()
                    .await
                    .map_err(ResolverError::Reqwest)?;

                (fallback_res, true)
            }
        };

        if !fallback_used && res.status() == StatusCode::TOO_MANY_REQUESTS {
            if let (Some(planner), Some(ip)) = (&*crate::ROUTE_PLANNER, bound_ip) {
                planner.ban_ip(ip);
            }
        }

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
        http_client: &Client,
        bound_ip: Option<std::net::IpAddr>,
    ) -> Result<Value, ResolverError> {
        let mut payload = json!({
            "query": query,
        });

        if let Some(p) = params {
            payload
                .as_object_mut()
                .unwrap()
                .insert("params".to_string(), json!(p));
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

        self.make_request(
            "/search",
            client,
            &context,
            payload,
            &headers,
            http_client,
            bound_ip,
        )
        .await
    }

    pub async fn player(
        &self,
        video_id: &str,
        client: &dyn InnertubeClient,
        params: Option<&str>,
        start_time: Option<u64>,
        playlist_id: Option<&str>,
        visitor_data: Option<&str>,
        po_token: Option<&str>,
        oauth_token: Option<&str>,
        signature_timestamp: Option<u32>,
        encrypted_host_flags: Option<&str>,
        http_client: &Client,
        bound_ip: Option<std::net::IpAddr>,
    ) -> Result<Value, ResolverError> {
        let mut payload = json!({
            "videoId": video_id,
            "racyCheckOk": true,
            "contentCheckOk": true,
            "playbackContext": {
                "contentPlaybackContext": {
                    "autoCaptionsDefaultOn": false,
                    "autonavState": "STATE_OFF",
                    "html5Preference": "HTML5_PREF_WANTS",
                    "lactMilliseconds": "-1",
                    "mdxContext": {},
                    "playerWidthPixels": 1280,
                    "playerHeightPixels": 720,
                    "referer": "https://www.youtube.com/"
                }
            }
        });

        if let Some(p) = params {
            payload.as_object_mut().unwrap().insert("params".to_string(), json!(p));
        }

        if let Some(flags) = encrypted_host_flags {
            if let Some(obj) = payload
                .get_mut("playbackContext")
                .and_then(|pc| pc.get_mut("contentPlaybackContext"))
                .and_then(|cc| cc.as_object_mut())
            {
                obj.insert("encryptedHostFlags".to_string(), json!(flags));
            }
        }

        if let Some(pid) = playlist_id {
            payload
                .as_object_mut()
                .unwrap()
                .insert("playlistId".to_string(), json!(pid));
        }

        if let Some(start) = start_time {
            payload
                .as_object_mut()
                .unwrap()
                .insert("startTimeSecs".to_string(), json!(start));
        }

        if let Some(timestamp) = signature_timestamp {
            if let Some(obj) = payload
                .get_mut("playbackContext")
                .and_then(|pc| pc.get_mut("contentPlaybackContext"))
                .and_then(|cc| cc.as_object_mut())
            {
                obj.insert("signatureTimestamp".to_string(), json!(timestamp));
            }
        }

        let mut context = client.context();
        if let Some(vd) = visitor_data {
            context.client.visitor_data = Some(vd.to_string());
        }

        if oauth_token.is_none() {
            context.client.client_screen = Some("EMBED".to_string());
            let mut fields = serde_json::Map::new();
            fields.insert("embedUrl".to_string(), json!("https://google.com"));
            context.third_party = Some(InnertubeThirdParty { fields });
        }

        let mut headers = client.extra_headers();
        
        if let Some(vd) = visitor_data {
            headers.push(("X-Goog-Visitor-Id".to_string(), vd.to_string()));
        }

        if let Some(token) = oauth_token {
            headers.push(("Authorization".to_string(), format!("Bearer {}", token)));
        }

        if let Some(po) = po_token {
            if let Some(p) = payload.as_object_mut() {
                p.insert(
                    "serviceIntegrityDimensions".to_string(),
                    json!({
                        "poToken": po
                    }),
                );
            }
        }

        self.make_request(
            "/player",
            client,
            &context,
            payload,
            &headers,
            http_client,
            bound_ip,
        )
        .await
    }

    pub async fn next(
        &self,
        video_id: Option<&str>,
        playlist_id: Option<&str>,
        continuation: Option<&str>,
        client: &dyn InnertubeClient,
        visitor_data: Option<&str>,
        oauth_token: Option<&str>,
        http_client: &Client,
        bound_ip: Option<std::net::IpAddr>,
    ) -> Result<Value, ResolverError> {
        let mut payload = json!({});

        if let Some(vid) = video_id {
            payload
                .as_object_mut()
                .unwrap()
                .insert("videoId".to_string(), json!(vid));
        }

        if let Some(pid) = playlist_id {
            payload
                .as_object_mut()
                .unwrap()
                .insert("playlistId".to_string(), json!(pid));
        }

        if let Some(cont) = continuation {
            payload
                .as_object_mut()
                .unwrap()
                .insert("continuation".to_string(), json!(cont));
        }

        let mut context = client.context();
        if let Some(vd) = visitor_data {
            context.client.visitor_data = Some(vd.to_string());
        }

        if oauth_token.is_none() {
            context.client.client_screen = Some("EMBED".to_string());
            let mut fields = serde_json::Map::new();
            fields.insert("embedUrl".to_string(), json!("https://google.com"));
            context.third_party = Some(InnertubeThirdParty { fields });
        }

        let mut headers = client.extra_headers();
        
        if let Some(vd) = visitor_data {
            headers.push(("X-Goog-Visitor-Id".to_string(), vd.to_string()));
        }

        if let Some(token) = oauth_token {
            headers.push(("Authorization".to_string(), format!("Bearer {}", token)));
        }

        self.make_request(
            "/next",
            client,
            &context,
            payload,
            &headers,
            http_client,
            bound_ip,
        )
        .await
    }
}
