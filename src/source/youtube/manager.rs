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
    pub(crate) http: Client,
    pub(crate) api: InnertubeApi,
    pub(crate) cipher: CipherManager,
    pub(crate) sabr: Arc<Mutex<Sabr>>,
    pub(crate) oauth: Arc<Mutex<YoutubeOAuth>>,
    pub(crate) clients: DashMap<String, Box<dyn InnertubeClient>>,
    pub(crate) search_clients: Vec<String>,
    pub(crate) resolve_clients: Vec<String>,
    pub(crate) playback_clients: Vec<String>,
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

    pub async fn make_playable(&self, video_id: &str) -> Result<(String, Client, reqwest::header::HeaderMap), ResolverError> {
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

            let mut headers = reqwest::header::HeaderMap::new();
            let mut stream_headers = reqwest::header::HeaderMap::new();
            let ua_str = client.context().client.user_agent.clone().unwrap_or_else(|| {
                client.extra_headers()
                    .into_iter()
                    .find(|(k, _)| k.to_lowercase() == "user-agent")
                    .map(|(_, v)| v)
                    .unwrap_or_else(|| "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36".to_string())
            });
            if let Ok(ua) = reqwest::header::HeaderValue::from_str(&ua_str) {
                headers.insert(reqwest::header::USER_AGENT, ua);
            }
            
            let stream_ua = ua_str.replace(" gzip", "");
            if let Ok(ua) = reqwest::header::HeaderValue::from_str(&stream_ua) {
                stream_headers.insert(reqwest::header::USER_AGENT, ua);
            }

            if let Some(visitor_id) = &visitor_data {
                if let Ok(val) = reqwest::header::HeaderValue::from_str(visitor_id) {
                    headers.insert("X-Goog-Visitor-Id", val.clone());
                    stream_headers.insert("X-Goog-Visitor-Id", val);
                }
            }

            for (key, value) in client.extra_headers() {
                if let (Ok(name), Ok(val)) = (
                    reqwest::header::HeaderName::from_bytes(key.as_bytes()),
                    reqwest::header::HeaderValue::from_str(&value),
                ) {
                    headers.insert(name.clone(), val.clone());
                    let lower_key = key.to_lowercase();
                    if lower_key == "user-agent" || lower_key == "referer" || lower_key == "origin" || lower_key == "x-goog-visitor-id" {
                        if !stream_headers.contains_key(&name) {
                            stream_headers.insert(name, val);
                        }
                    }
                }
            }


            let build_stream_client = |headers: reqwest::header::HeaderMap,
                                       bound_ip: Option<std::net::IpAddr>,
                                       url: &str|
             -> reqwest::Client {
                let mut builder = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(30))
                    .default_headers(headers);

                if let Some(ip) = bound_ip {
                    builder = builder.local_address(ip);
                } else if let Ok(parsed_url) = url::Url::parse(url) {
                    if let Some((_, ip_val)) = parsed_url.query_pairs().find(|(k, _)| k == "ip") {
                        if ip_val.parse::<std::net::Ipv4Addr>().is_ok() {
                            tracing::debug!("Binding stream client to IPv4 (0.0.0.0) for URL: {}", url);
                            builder = builder.local_address(std::net::IpAddr::V4(
                                std::net::Ipv4Addr::UNSPECIFIED,
                            ));
                        } else if ip_val.parse::<std::net::Ipv6Addr>().is_ok() {
                            tracing::debug!("Binding stream client to IPv6 (::0) for URL: {}", url);
                            builder = builder.local_address(std::net::IpAddr::V6(
                                std::net::Ipv6Addr::UNSPECIFIED,
                            ));
                        }
                    }
                }

                builder.build().unwrap_or_else(|_| http_client.clone())
            };

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
                                )
                                .await
                            {
                                Ok(deciphered) => final_url = deciphered,
                                Err(e) => {
                                    warn!("N-parameter decipher failed for {}: {:?}", client.name(), e);
                                    // For clients that require cipher (TV/Web), cipher failure means the URL
                                    // will be broken — skip to next client.
                                    // For mobile clients (IOS, AndroidVR) that don't strictly need cipher,
                                    // fall back to raw URL (stream may be throttled but still functional).
                                    if client.needs_cipher() {
                                        last_error = e;
                                        continue;
                                    }
                                    // fall through with raw final_url
                                }
                            }
                        }
                    }

                    if client.requires_pot() {
                        if let Some(po) = &po_token {
                            if final_url.contains('?') {
                                final_url.push_str(&format!("&pot={}", po));
                            } else {
                                final_url.push_str(&format!("?pot={}", po));
                            }
                        }
                    }
                    let stream_client = build_stream_client(stream_headers.clone(), bound_ip, &final_url);
                    let probe_url = if final_url.contains('?') {
                        format!("{}&range=0-0", final_url)
                    } else {
                        format!("{}?range=0-0", final_url)
                    };
                    match stream_client.get(&probe_url).send().await {
                        Ok(resp) if resp.status() == reqwest::StatusCode::FORBIDDEN => {
                            debug!("CDN probe 403 for {} with {} - trying next client", video_id, client.name());
                            last_error = ResolverError::Custom(format!("Stream CDN 403 for {} client", client.name()));
                            continue;
                        }
                        Err(e) => warn!("CDN probe error for {} with {}: {:?} - proceeding anyway", video_id, client.name(), e),
                        _ => {}
                    }
                    return Ok((final_url.clone(), stream_client, stream_headers));
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
                            )
                            .await
                        {
                            Ok(deciphered) => {
                                let mut final_url = deciphered;
                                if client.requires_pot() {
                                    if let Some(po) = &po_token {
                                        if final_url.contains('?') {
                                            final_url.push_str(&format!("&pot={}", po));
                                        } else {
                                            final_url.push_str(&format!("?pot={}", po));
                                        }
                                    }
                                }
                                let stream_client = build_stream_client(stream_headers.clone(), bound_ip, &final_url);
                                let probe_url = if final_url.contains('?') {
                                    format!("{}&range=0-0", final_url)
                                } else {
                                    format!("{}?range=0-0", final_url)
                                };
                                match stream_client.get(&probe_url).send().await {
                                    Ok(resp) if resp.status() == reqwest::StatusCode::FORBIDDEN => {
                                        debug!("CDN probe 403 for {} with {} (cipher) - trying next client", video_id, client.name());
                                        last_error = ResolverError::Custom(format!("Stream CDN 403 for {} client", client.name()));
                                        continue;
                                    }
                                    Err(e) => warn!("CDN probe error for {} with {}: {:?} - proceeding anyway", video_id, client.name(), e),
                                    _ => {}
                                }
                                return Ok((final_url.clone(), stream_client, stream_headers));
                            }
                            Err(e) => {
                                warn!("Cipher resolution failed for {}: {:?}", client.name(), e);
                                // signatureCipher always requires the deciphered sig — there is no
                                // usable raw URL to fall back to, so always skip to next client.
                                last_error = e;
                                continue;
                            }
                        }
                    }
                }
            }
        }

        Err(last_error)
    }

    pub async fn get_refresh_token(&self) -> Option<String> {
        self.oauth.lock().await.get_refresh_token()
    }

    pub async fn update_po_token_and_visitor_data(
        &self,
        po_token: Option<String>,
        visitor_data: Option<String>,
    ) {
        let mut sabr = self.sabr.lock().await;
        sabr.set_po_token(po_token);
        sabr.set_visitor_data(visitor_data);
    }

    pub async fn set_oauth_refresh_token(
        &self,
        refresh_token: String,
        skip_initialization: bool,
    ) {
        let mut oauth = self.oauth.lock().await;
        oauth.set_refresh_token(refresh_token);
        if !skip_initialization {
            match oauth.refresh_if_needed().await {
                Ok(_) => info!("YouTube OAuth updated via REST API."),
                Err(e) => warn!("YouTube OAuth refresh via REST API failed: {:?}", e),
            }
        }
    }

    pub async fn create_access_token_from_refresh(
        &self,
        refresh_token: &str,
    ) -> Result<super::oauth::OauthToken, ResolverError> {
        let oauth = self.oauth.lock().await;
        oauth.create_access_token_from_refresh(refresh_token).await
    }

    pub async fn load_playlist(
        &self,
        playlist_id: &str,
    ) -> Result<(String, Vec<Value>), ResolverError> {
        let (visitor_data, _po_token) = {
            let sabr = self.sabr.lock().await;
            (sabr.get_visitor_data(), sabr.get_po_token())
        };
        let oauth_token = self.oauth.lock().await.get_access_token();

        let (http_client, bound_ip) = crate::get_client();

        let client_name = self
            .resolve_clients
            .first()
            .cloned()
            .unwrap_or_else(|| "Web".to_string());
        let client = self
            .get_innertube_client(&client_name)
            .ok_or_else(|| ResolverError::Custom("No resolve client available".to_string()))?;

        let browse_id = format!("VL{}", playlist_id);

        let mut response = self
            .api
            .browse(
                Some(&browse_id),
                None,
                client.as_ref(),
                visitor_data.as_deref(),
                oauth_token.as_deref(),
                &http_client,
                bound_ip,
            )
            .await?;

        let playlist_name = extract_playlist_name(&response)
            .unwrap_or_else(|| playlist_id.to_string());

        let mut all_videos: Vec<Value> = Vec::new();
        extract_playlist_videos(&response, &mut all_videos);

        for _ in 0..5 {
            let token = match extract_playlist_continuation(&response) {
                Some(t) => t,
                None => break,
            };

            let client2 = self
                .get_innertube_client(&client_name)
                .ok_or_else(|| ResolverError::Custom("No resolve client available".to_string()))?;

            response = self
                .api
                .browse(
                    None,
                    Some(&token),
                    client2.as_ref(),
                    visitor_data.as_deref(),
                    oauth_token.as_deref(),
                    &http_client,
                    bound_ip,
                )
                .await?;

            extract_playlist_videos(&response, &mut all_videos);
        }

        Ok((playlist_name, all_videos))
    }
}


fn extract_playlist_name(json: &Value) -> Option<String> {
    json.get("header")
        .and_then(|h| h.get("playlistHeaderRenderer"))
        .and_then(|r| r.get("title"))
        .and_then(|t| t.get("simpleText"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            json.get("header")
                .and_then(|h| h.get("playlistHeaderRenderer"))
                .and_then(|r| r.get("title"))
                .and_then(|t| t.get("runs"))
                .and_then(|r| r.as_array())
                .and_then(|r| r.first())
                .and_then(|r| r.get("text"))
                .and_then(|t| t.as_str())
                .map(|s| s.to_string())
        })
        .or_else(|| {
            json.get("metadata")
                .and_then(|m| m.get("playlistMetadataRenderer"))
                .and_then(|r| r.get("title"))
                .and_then(|t| t.as_str())
                .map(|s| s.to_string())
        })
}

fn extract_playlist_videos(json: &Value, videos: &mut Vec<Value>) {
    if let Some(items) = json
        .get("contents")
        .and_then(|c| c.get("twoColumnBrowseResultsRenderer"))
        .and_then(|c| c.get("tabs"))
        .and_then(|t| t.as_array())
        .and_then(|t| t.first())
        .and_then(|t| t.get("tabRenderer"))
        .and_then(|t| t.get("content"))
        .and_then(|c| c.get("sectionListRenderer"))
        .and_then(|s| s.get("contents"))
        .and_then(|c| c.as_array())
        .and_then(|c| c.first())
        .and_then(|c| c.get("itemSectionRenderer"))
        .and_then(|c| c.get("contents"))
        .and_then(|c| c.as_array())
        .and_then(|c| c.first())
        .and_then(|c| c.get("playlistVideoListRenderer"))
        .and_then(|pl| pl.get("contents"))
        .and_then(|c| c.as_array())
    {
        for item in items {
            if let Some(video) = item.get("playlistVideoRenderer") {
                videos.push(video.clone());
            }
        }
        return;
    }

    if let Some(items) = json
        .get("contents")
        .and_then(|c| c.get("singleColumnBrowseResultsRenderer"))
        .and_then(|c| c.get("tabs"))
        .and_then(|t| t.as_array())
        .and_then(|t| t.first())
        .and_then(|t| t.get("tabRenderer"))
        .and_then(|t| t.get("content"))
        .and_then(|c| c.get("sectionListRenderer"))
        .and_then(|s| s.get("contents"))
        .and_then(|c| c.as_array())
        .and_then(|c| c.first())
        .and_then(|c| c.get("itemSectionRenderer"))
        .and_then(|c| c.get("contents"))
        .and_then(|c| c.as_array())
        .and_then(|c| c.first())
        .and_then(|c| c.get("playlistVideoListRenderer"))
        .and_then(|pl| pl.get("contents"))
        .and_then(|c| c.as_array())
    {
        for item in items {
            if let Some(video) = item.get("playlistVideoRenderer") {
                videos.push(video.clone());
            }
        }
        return;
    }

    if let Some(actions) = json
        .get("onResponseReceivedActions")
        .and_then(|a| a.as_array())
    {
        for action in actions {
            if let Some(items) = action
                .get("appendContinuationItemsAction")
                .and_then(|a| a.get("continuationItems"))
                .and_then(|c| c.as_array())
            {
                for item in items {
                    if let Some(video) = item.get("playlistVideoRenderer") {
                        videos.push(video.clone());
                    }
                }
            }
        }
    }
}

fn extract_playlist_continuation(json: &Value) -> Option<String> {
    let mut candidate_items: Vec<&Value> = Vec::new();
    if let Some(arr) = json
        .get("contents")
        .and_then(|c| c.get("twoColumnBrowseResultsRenderer"))
        .and_then(|c| c.get("tabs"))
        .and_then(|t| t.as_array())
        .and_then(|t| t.first())
        .and_then(|t| t.get("tabRenderer"))
        .and_then(|t| t.get("content"))
        .and_then(|c| c.get("sectionListRenderer"))
        .and_then(|s| s.get("contents"))
        .and_then(|c| c.as_array())
        .and_then(|c| c.first())
        .and_then(|c| c.get("itemSectionRenderer"))
        .and_then(|c| c.get("contents"))
        .and_then(|c| c.as_array())
        .and_then(|c| c.first())
        .and_then(|c| c.get("playlistVideoListRenderer"))
        .and_then(|pl| pl.get("contents"))
        .and_then(|c| c.as_array())
    {
        candidate_items.extend(arr.iter());
    }

    if let Some(actions) = json
        .get("onResponseReceivedActions")
        .and_then(|a| a.as_array())
    {
        for action in actions {
            if let Some(arr) = action
                .get("appendContinuationItemsAction")
                .and_then(|a| a.get("continuationItems"))
                .and_then(|c| c.as_array())
            {
                candidate_items.extend(arr.iter());
            }
        }
    }

    for item in candidate_items {
        if let Some(token) = item
            .get("continuationItemRenderer")
            .and_then(|c| c.get("continuationEndpoint"))
            .and_then(|e| e.get("continuationCommand"))
            .and_then(|c| c.get("token"))
            .and_then(|t| t.as_str())
        {
            return Some(token.to_string());
        }
    }

    None
}
