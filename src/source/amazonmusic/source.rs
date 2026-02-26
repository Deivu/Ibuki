use super::model::*;
use super::*;
use crate::models::{ApiTrack, ApiTrackInfo, ApiTrackPlaylist, ApiTrackResult, Empty};
use crate::util::encoder::encode_track;
use crate::util::errors::ResolverError;
use crate::util::source::{Query, Source};
use crate::util::url::is_url;
use async_trait::async_trait;
use regex::Regex;
use reqwest::Client;
use songbird::input::Input;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;
use uuid::Uuid;

const CONFIG_TTL_MS: u64 = 60_000; // 60 seconds

pub struct AmazonMusic {
    client: Client,
    url_regex: Regex,
    search_prefixes: (&'static str, &'static str),
    config_cache: Arc<Mutex<Option<CachedConfig>>>,
}

impl AmazonMusic {
    pub fn new(client: Option<Client>) -> Self {
        Self {
            client: client.unwrap_or_default(),
            url_regex: Regex::new(
                r"(?i)^https?://(?:music\.amazon\.[a-z.]+/(?:.*/)?(track|album|playlist|artist)s?/([a-z0-9]+)|(?:www\.)?amazon\.[a-z.]+/dp/([a-z0-9]+))"
            )
            .expect("Failed to init AmazonMusic URL RegEx"),
            search_prefixes: ("amazonmusic", "azsearch"),
            config_cache: Arc::new(Mutex::new(None)),
        }
    }

    async fn get_amazon_config(&self) -> Result<CachedConfig, ResolverError> {
        let mut cache = self.config_cache.lock().await;

        if let Some(ref cached) = *cache {
            let elapsed = Instant::now().duration_since(cached.cached_at);
            if elapsed.as_millis() < CONFIG_TTL_MS as u128 {
                return Ok(cached.clone());
            }
        }

        let response = self
            .client
            .get(CONFIG_URL)
            .header("User-Agent", SEARCH_USER_AGENT)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(ResolverError::FailedStatusCode(
                response.status().to_string(),
            ));
        }

        let config: AmazonConfig = response.json().await?;

        if config.csrf.token.is_empty() {
            return Err(ResolverError::MissingRequiredData(
                "Missing CSRF token from Amazon config",
            ));
        }

        let device_id = config
            .device_id
            .filter(|id| !id.starts_with("000"))
            .unwrap_or_else(|| FALLBACK_DEVICE_ID.to_string());

        let session_id = config
            .session_id
            .filter(|id| !id.starts_with("000"))
            .unwrap_or_else(|| FALLBACK_SESSION_ID.to_string());

        let cached_config = CachedConfig {
            access_token: config.access_token.unwrap_or_default(),
            csrf: config.csrf,
            device_id,
            session_id,
            cached_at: Instant::now(),
        };

        *cache = Some(cached_config.clone());
        Ok(cached_config)
    }

    fn build_csrf_header(&self, csrf: &CsrfToken) -> String {
        serde_json::json!({
            "interface": "CSRFInterface.v1_0.CSRFHeaderElement",
            "token": csrf.token,
            "timestamp": csrf.timestamp,
            "rndNonce": csrf.nonce
        })
        .to_string()
    }

    fn extract_identifier(&self, url: &str) -> Option<String> {
        if let Some(asin) = self.extract_track_asin(url) {
            return Some(asin);
        }

        let mut end = url.len();
        if let Some(pos) = url.find('?') {
            if pos < end {
                end = pos;
            }
        }
        if let Some(pos) = url.find('#') {
            if pos < end {
                end = pos;
            }
        }

        if let Some(slash_pos) = url[..end].rfind('/') {
            let id = &url[slash_pos + 1..end];
            if !id.is_empty() {
                return Some(id.to_string());
            }
        }

        None
    }

    fn extract_track_asin(&self, url: &str) -> Option<String> {
        let key = "trackAsin=";
        let start = url.find(key)? + key.len();

        let mut end = url.len();
        for delimiter in ["&", "%26", "#"] {
            if let Some(pos) = url[start..].find(delimiter) {
                let abs_pos = start + pos;
                if abs_pos < end {
                    end = abs_pos;
                }
            }
        }

        let id = &url[start..end];
        if id.is_empty() {
            None
        } else {
            Some(id.to_string())
        }
    }

    fn parse_iso8601_duration(&self, duration: &str) -> u64 {
        let re = Regex::new(r"PT(?:(\d+)H)?(?:(\d+)M)?(?:(\d+)S)?").unwrap();
        if let Some(caps) = re.captures(duration) {
            let hours: u64 = caps
                .get(1)
                .and_then(|m| m.as_str().parse().ok())
                .unwrap_or(0);
            let minutes: u64 = caps
                .get(2)
                .and_then(|m| m.as_str().parse().ok())
                .unwrap_or(0);
            let seconds: u64 = caps
                .get(3)
                .and_then(|m| m.as_str().parse().ok())
                .unwrap_or(0);
            return (hours * 3600 + minutes * 60 + seconds) * 1000;
        }
        0
    }

    fn parse_colon_duration(&self, duration: &str) -> u64 {
        let parts: Vec<&str> = duration.split(':').collect();
        let mut seconds = 0u64;

        for part in parts {
            if let Ok(n) = part.parse::<u64>() {
                seconds = seconds * 60 + n;
            } else {
                return 0;
            }
        }

        seconds * 1000
    }

    fn parse_time_string(&self, s: &str) -> u64 {
        let s = s.to_uppercase();
        let mut total = 0u64;
        let mut current_num = 0u64;
        let mut in_number = false;

        let mut i = 0;
        while i < s.len() {
            let ch = s.chars().nth(i).unwrap();
            if ch.is_ascii_digit() {
                current_num = current_num * 10 + (ch as u64 - '0' as u64);
                in_number = true;
            } else if in_number {
                if s[i..].starts_with("HOUR") {
                    total += current_num * 3600;
                } else if s[i..].starts_with("MINUTE") {
                    total += current_num * 60;
                } else if s[i..].starts_with("SECOND") {
                    total += current_num;
                }
                current_num = 0;
                in_number = false;
            }
            i += 1;
        }

        total * 1000
    }

    async fn resolve_url(&self, url: &str) -> Result<Option<ApiTrackResult>, ResolverError> {
        let captures = match self.url_regex.captures(url) {
            Some(c) => c,
            None => return Ok(Some(ApiTrackResult::Empty(None))),
        };

        let url_type = captures
            .get(1)
            .or_else(|| captures.get(3))
            .map(|m| m.as_str())
            .unwrap_or("track");

        let id = captures
            .get(2)
            .or_else(|| captures.get(3))
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();

        tracing::debug!("Resolving Amazon Music URL: type={}, id={}", url_type, id);

        if let Some(track_asin) = self.extract_track_asin(url) {
            return self.resolve_track(url, &track_asin).await;
        }

        match url_type {
            "track" | "dp" => self.resolve_track(url, &id).await,
            "album" => self.resolve_album(url, &id).await,
            "playlist" => self.resolve_playlist(url, &id).await,
            "artist" => self.resolve_artist(url, &id).await,
            _ => Ok(Some(ApiTrackResult::Empty(None))),
        }
    }

    async fn resolve_track(
        &self,
        url: &str,
        id: &str,
    ) -> Result<Option<ApiTrackResult>, ResolverError> {
        if let Some(result) = self.fetch_json_ld(url, Some(id)).await? {
            return Ok(Some(result));
        }

        Ok(Some(ApiTrackResult::Empty(None)))
    }

    async fn resolve_album(
        &self,
        url: &str,
        _id: &str,
    ) -> Result<Option<ApiTrackResult>, ResolverError> {
        if let Some(result) = self.fetch_json_ld(url, None).await? {
            return Ok(Some(result));
        }

        Ok(Some(ApiTrackResult::Empty(None)))
    }

    async fn resolve_playlist(
        &self,
        url: &str,
        _id: &str,
    ) -> Result<Option<ApiTrackResult>, ResolverError> {
        if let Some(result) = self.fetch_json_ld(url, None).await? {
            return Ok(Some(result));
        }

        Ok(Some(ApiTrackResult::Empty(None)))
    }

    async fn resolve_artist(
        &self,
        url: &str,
        _id: &str,
    ) -> Result<Option<ApiTrackResult>, ResolverError> {
        if let Some(result) = self.fetch_json_ld(url, None).await? {
            return Ok(Some(result));
        }

        Ok(Some(ApiTrackResult::Empty(None)))
    }

    async fn fetch_json_ld(
        &self,
        url: &str,
        target_id: Option<&str>,
    ) -> Result<Option<ApiTrackResult>, ResolverError> {
        let response = self
            .client
            .get(url)
            .header("User-Agent", BOT_USER_AGENT)
            .send()
            .await?;

        if !response.status().is_success() {
            return Ok(None);
        }

        let body = response.text().await?;
        let header_artist = self.extract_header_primary_text(&body);
        let header_image = self.extract_header_image(&body);
        let og_image = self.extract_og_image(&body);
        let artwork_url = header_image.or(og_image);
        let json_ld_regex =
            Regex::new(r#"<script [^>]*type="application/ld\+json"[^>]*>([\s\S]*?)</script>"#)
                .unwrap();

        let mut collection: Option<JsonLdData> = None;
        let mut track_data: Option<JsonLdData> = None;

        for cap in json_ld_regex.captures_iter(&body) {
            if let Some(json_str) = cap.get(1) {
                let content = json_str
                    .as_str()
                    .replace("&quot;", "\"")
                    .replace("&amp;", "&");

                if let Ok(data) = serde_json::from_str::<JsonLdData>(&content) {
                    match data.data_type.as_str() {
                        "MusicAlbum" | "MusicGroup" | "Playlist" => {
                            collection = Some(data);
                        }
                        "MusicRecording" => {
                            track_data = Some(data);
                        }
                        _ => {}
                    }
                } else if let Ok(data_arr) = serde_json::from_str::<Vec<JsonLdData>>(&content) {
                    if let Some(data) = data_arr.first() {
                        match data.data_type.as_str() {
                            "MusicAlbum" | "MusicGroup" | "Playlist" => {
                                collection = Some(data.clone());
                            }
                            "MusicRecording" => {
                                track_data = Some(data.clone());
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        let mut tracks = Vec::new();
        let mut collection_name = header_artist.unwrap_or_else(|| "Unknown Artist".to_string());
        let mut collection_image = artwork_url;

        if let Some(ref coll) = collection {
            if let Some(ref artist) = coll.by_artist.as_ref().or(coll.author.as_ref()) {
                if let Some(name) = artist.name() {
                    collection_name = name.to_string();
                }
            }
            if let Some(ref img) = coll.image {
                collection_image = Some(img.clone());
            }
            if let Some(ref track_list) = coll.track {
                for (idx, t) in track_list.iter().enumerate() {
                    let id = self
                        .extract_identifier(t.url.as_deref().unwrap_or(""))
                        .or_else(|| t.id.as_ref().and_then(|id| self.extract_identifier(id)))
                        .unwrap_or_else(|| format!("am-{}", idx));

                    let artist = t
                        .by_artist
                        .as_ref()
                        .or(t.author.as_ref())
                        .and_then(|a| a.name())
                        .unwrap_or(&collection_name);

                    let duration = t
                        .duration
                        .as_ref()
                        .map(|d| self.parse_iso8601_duration(d))
                        .unwrap_or(0);

                    tracks.push(ApiTrackInfo {
                        identifier: id.clone(),
                        is_seekable: true,
                        author: artist.to_string(),
                        length: duration,
                        is_stream: false,
                        position: 0,
                        title: t.name.clone(),
                        uri: Some(
                            t.url
                                .clone()
                                .unwrap_or_else(|| format!("{}/tracks/{}", MUSIC_BASE, id)),
                        ),
                        artwork_url: collection_image.clone(),
                        isrc: t.isrc_code.clone(),
                        source_name: "amazonmusic".to_string(),
                    });
                }
            }
        }

        if tracks.is_empty() {
            tracks = self.parse_html_rows(&body, &collection_name, &collection_image);
        }
        if !tracks.is_empty() {
            if let Some(tid) = target_id {
                if let Some(selected) = tracks.iter().find(|t| {
                    t.identifier == tid || t.uri.as_ref().map(|u| u.contains(tid)).unwrap_or(false)
                }) {
                    return Ok(Some(ApiTrackResult::Track(ApiTrack {
                        encoded: encode_track(selected)?,
                        info: selected.clone(),
                        plugin_info: Empty,
                        user_data: None,
                    })));
                }
            }
            if url.contains("/tracks/") && target_id.is_none() {
                return Ok(Some(ApiTrackResult::Track(ApiTrack {
                    encoded: encode_track(&tracks[0])?,
                    info: tracks[0].clone(),
                    plugin_info: Empty,
                    user_data: None,
                })));
            }
            return Ok(Some(ApiTrackResult::Playlist(ApiTrackPlaylist {
                info: crate::models::ApiPlaylistInfo {
                    name: collection_name,
                    selected_track: 0,
                },
                plugin_info: Empty,
                tracks: tracks
                    .into_iter()
                    .map(|info| ApiTrack {
                        encoded: encode_track(&info).unwrap_or_default(),
                        info,
                        plugin_info: Empty,
                        user_data: None,
                    })
                    .collect(),
            })));
        }

        if let Some(td) = track_data {
            let artist = td
                .by_artist
                .as_ref()
                .or(td.author.as_ref())
                .and_then(|a| a.name())
                .unwrap_or("Unknown Artist");

            let duration = td
                .duration
                .as_ref()
                .map(|d| self.parse_iso8601_duration(d))
                .unwrap_or(0);

            let track_image = td.image.as_ref().or(collection_image.as_ref()).cloned();

            let id = self
                .extract_identifier(url)
                .unwrap_or_else(|| "unknown".to_string());

            let track_info = ApiTrackInfo {
                identifier: id,
                is_seekable: true,
                author: artist.to_string(),
                length: duration,
                is_stream: false,
                position: 0,
                title: td.name.unwrap_or_else(|| "Unknown Track".to_string()),
                uri: Some(url.to_string()),
                artwork_url: track_image,
                isrc: td.isrc_code,
                source_name: "amazonmusic".to_string(),
            };

            return Ok(Some(ApiTrackResult::Track(ApiTrack {
                encoded: encode_track(&track_info)?,
                info: track_info,
                plugin_info: Empty,
                user_data: None,
            })));
        }

        Ok(None)
    }

    fn extract_header_primary_text(&self, html: &str) -> Option<String> {
        let re = Regex::new(r#"<music-detail-header[^>]*primary-text="([^"]+)""#).unwrap();
        re.captures(html)
            .map(|cap| cap.get(1).unwrap().as_str().replace("&amp;", "&"))
    }

    fn extract_header_image(&self, html: &str) -> Option<String> {
        let re = Regex::new(r#"<music-detail-header[^>]*image-src="([^"]+)""#).unwrap();
        re.captures(html)
            .map(|cap| cap.get(1).unwrap().as_str().to_string())
    }

    fn extract_og_image(&self, html: &str) -> Option<String> {
        let re = Regex::new(r#"<meta property="og:image" content="([^"]+)""#).unwrap();
        re.captures(html)
            .map(|cap| cap.get(1).unwrap().as_str().to_string())
    }

    fn parse_html_rows(
        &self,
        html: &str,
        collection_name: &str,
        collection_image: &Option<String>,
    ) -> Vec<ApiTrackInfo> {
        let mut tracks = Vec::new();

        let re = Regex::new(
            r#"<(?:music-image-row|music-text-row)[^>]*primary-text="([^"]+)"[^>]*primary-href="([^"]+)"(?:[^>]*secondary-text-1="([^"]+)")?[^>]*duration="([^"]+)"(?:[^>]*image-src="([^"]+)")?"#
        ).unwrap();

        for cap in re.captures_iter(html) {
            let title = cap.get(1).unwrap().as_str().replace("&amp;", "&");
            let href = cap.get(2).unwrap().as_str();
            let artist = cap
                .get(3)
                .map(|m| m.as_str().replace("&amp;", "&"))
                .unwrap_or_else(|| collection_name.to_string());
            let duration_str = cap.get(4).unwrap().as_str();
            let image = cap
                .get(5)
                .map(|m| m.as_str().to_string())
                .or_else(|| collection_image.clone());

            let id = self
                .extract_identifier(href)
                .unwrap_or_else(|| format!("am-{}", tracks.len()));

            let duration = if duration_str.contains(':') {
                self.parse_colon_duration(duration_str)
            } else {
                0
            };

            tracks.push(ApiTrackInfo {
                identifier: id.clone(),
                is_seekable: true,
                author: artist,
                length: duration,
                is_stream: false,
                position: 0,
                title,
                uri: Some(format!("{}/tracks/{}", MUSIC_BASE, id)),
                artwork_url: image,
                isrc: None,
                source_name: "amazonmusic".to_string(),
            });
        }

        tracks
    }

    async fn fetch_track_duration(&self, track_id: &str) -> Result<u64, ResolverError> {
        let config = self.get_amazon_config().await?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();

        let headers = serde_json::json!({
            "x-amzn-authentication": serde_json::json!({
                "interface": "ClientAuthenticationInterface.v1_0.ClientTokenElement",
                "accessToken": config.access_token
            }).to_string(),
            "x-amzn-device-model": "WEBPLAYER",
            "x-amzn-device-width": "1920",
            "x-amzn-device-family": "WebPlayer",
            "x-amzn-device-id": config.device_id,
            "x-amzn-user-agent": SEARCH_USER_AGENT,
            "x-amzn-session-id": config.session_id,
            "x-amzn-device-height": "1080",
            "x-amzn-request-id": Uuid::new_v4().to_string(),
            "x-amzn-device-language": "en_US",
            "x-amzn-currency-of-preference": "USD",
            "x-amzn-os-version": "1.0",
            "x-amzn-application-version": "1.0.9172.0",
            "x-amzn-device-time-zone": "America/Sao_Paulo",
            "x-amzn-timestamp": now.to_string(),
            "x-amzn-csrf": self.build_csrf_header(&config.csrf),
            "x-amzn-music-domain": "music.amazon.com",
            "x-amzn-page-url": format!("{}/tracks/{}", MUSIC_BASE, track_id),
            "x-amzn-feature-flags": "hd-supported,uhd-supported",
        });

        let payload = serde_json::json!({
            "id": track_id,
            "userHash": r#"{"level":"LIBRARY_MEMBER"}"#,
            "headers": headers.to_string()
        });

        let response = self
            .client
            .post(format!("{}/cosmicTrack/displayCatalogTrack", API_BASE))
            .header("User-Agent", SEARCH_USER_AGENT)
            .header("Content-Type", "text/plain;charset=UTF-8")
            .header("Origin", MUSIC_BASE)
            .header("Referer", format!("{}/", MUSIC_BASE))
            .body(payload.to_string())
            .send()
            .await?;

        if !response.status().is_success() {
            return Ok(0);
        }

        let data: TrackDurationResponse = response.json().await?;

        if let Some(methods) = data.methods {
            if let Some(method) = methods.first() {
                if let Some(template) = &method.template {
                    if let Some(ref text) = template.header_tertiary_text {
                        let duration = self.parse_time_string(text);
                        if duration > 0 {
                            return Ok(duration);
                        }
                    }
                }
            }
        }

        Ok(0)
    }

    async fn perform_search(&self, query: &str) -> Result<Option<ApiTrackResult>, ResolverError> {
        let config = self.get_amazon_config().await?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();

        let headers = serde_json::json!({
            "x-amzn-authentication": serde_json::json!({
                "interface": "ClientAuthenticationInterface.v1_0.ClientTokenElement",
                "accessToken": config.access_token
            }).to_string(),
            "x-amzn-device-model": "WEBPLAYER",
            "x-amzn-device-width": "1920",
            "x-amzn-device-height": "1080",
            "x-amzn-device-family": "WebPlayer",
            "x-amzn-device-id": config.device_id,
            "x-amzn-user-agent": SEARCH_USER_AGENT,
            "x-amzn-session-id": config.session_id,
            "x-amzn-request-id": Uuid::new_v4().to_string(),
            "x-amzn-device-language": "en_US",
            "x-amzn-currency-of-preference": "USD",
            "x-amzn-os-version": "1.0",
            "x-amzn-application-version": "1.0.9172.0",
            "x-amzn-device-time-zone": "America/New_York",
            "x-amzn-timestamp": now.to_string(),
            "x-amzn-csrf": self.build_csrf_header(&config.csrf),
            "x-amzn-music-domain": "music.amazon.com",
            "x-amzn-page-url": format!("{}/search/{}?filter=IsLibrary%7Cfalse&sc=none", MUSIC_BASE, urlencoding::encode(query)),
            "x-amzn-feature-flags": "hd-supported,uhd-supported"
        });

        let payload = serde_json::json!({
            "filter": r#"{"IsLibrary":["false"]}"#,
            "keyword": serde_json::json!({
                "interface": "Web.TemplatesInterface.v1_0.Touch.SearchTemplateInterface.SearchKeywordClientInformation",
                "keyword": ""
            }).to_string(),
            "suggestedKeyword": query,
            "userHash": r#"{"level":"LIBRARY_MEMBER"}"#,
            "headers": headers.to_string()
        });

        let response = self
            .client
            .post(format!("{}/showSearch", API_BASE))
            .header("User-Agent", SEARCH_USER_AGENT)
            .header("Content-Type", "text/plain;charset=UTF-8")
            .header("x-amzn-csrf", &config.csrf.token)
            .header("Origin", MUSIC_BASE)
            .header("Referer", format!("{}/", MUSIC_BASE))
            .body(payload.to_string())
            .send()
            .await?;

        if !response.status().is_success() {
            tracing::error!("Amazon Music search API returned {}", response.status());
            return Ok(Some(ApiTrackResult::Empty(None)));
        }

        let data: SearchResponse = response.json().await?;

        let mut tracks = Vec::new();

        if let Some(methods) = data.methods {
            for method in methods {
                if let Some(template) = method.template {
                    if let Some(widgets) = template.widgets {
                        for widget in widgets {
                            if let Some(items) = widget.items {
                                for item in items {
                                    let is_song = item.label.as_deref() == Some("song");
                                    let is_square = item
                                        .interface
                                        .as_ref()
                                        .map(|i| i.contains("SquareHorizontalItemElement"))
                                        .unwrap_or(false);

                                    if !is_song && !is_square {
                                        continue;
                                    }

                                    if let Some(primary_link) = item.primary_link {
                                        if let Some(deeplink) = primary_link.deeplink {
                                            let identifier = self.extract_identifier(&deeplink);

                                            if let Some(id) = identifier {
                                                if !is_song && !deeplink.contains("trackAsin=") {
                                                    continue;
                                                }

                                                let title = item
                                                    .primary_text
                                                    .map(|t| t.as_str().replace("&amp;", "&"))
                                                    .unwrap_or_else(|| "Unknown Track".to_string());

                                                let author = item
                                                    .secondary_text
                                                    .map(|t| t.as_str().replace("&amp;", "&"))
                                                    .unwrap_or_else(|| {
                                                        "Unknown Artist".to_string()
                                                    });

                                                tracks.push(ApiTrackInfo {
                                                    identifier: id.clone(),
                                                    is_seekable: true,
                                                    author,
                                                    length: 0,
                                                    is_stream: false,
                                                    position: 0,
                                                    title,
                                                    uri: Some(format!(
                                                        "{}/tracks/{}",
                                                        MUSIC_BASE, id
                                                    )),
                                                    artwork_url: item.image,
                                                    isrc: None,
                                                    source_name: "amazonmusic".to_string(),
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if tracks.is_empty() {
            return Ok(Some(ApiTrackResult::Empty(None)));
        }

        let fetch_limit = std::cmp::min(tracks.len(), 5);
        for i in 0..fetch_limit {
            if let Ok(duration) = self.fetch_track_duration(&tracks[i].identifier).await {
                if duration > 0 {
                    tracks[i].length = duration;
                }
            }
        }

        Ok(Some(ApiTrackResult::Search(
            tracks
                .into_iter()
                .map(|info| ApiTrack {
                    encoded: encode_track(&info).unwrap_or_default(),
                    info,
                    plugin_info: Empty,
                    user_data: None,
                })
                .collect(),
        )))
    }
}

#[async_trait]
impl Source for AmazonMusic {
    fn get_name(&self) -> &'static str {
        "amazonmusic"
    }

    fn get_client(&self) -> Client {
        self.client.clone()
    }

    fn parse_query(&self, query: &str) -> Option<Query> {
        if !is_url(query) {
            if query.starts_with(self.search_prefixes.0)
                || query.starts_with(self.search_prefixes.1)
            {
                return Some(Query::Search(query.to_string()));
            } else {
                return None;
            }
        }

        if self.url_regex.is_match(query) {
            return Some(Query::Url(query.to_string()));
        }

        None
    }

    async fn init(&self) -> Result<(), ResolverError> {
        self.get_amazon_config().await?;
        tracing::info!("Amazon Music Source initialized successfully");
        Ok(())
    }

    async fn resolve(&self, query: Query) -> Result<Option<ApiTrackResult>, ResolverError> {
        match query {
            Query::Url(url) => self.resolve_url(&url).await,
            Query::Search(input) => {
                let term = if input.starts_with(self.search_prefixes.0) {
                    input.split_at(self.search_prefixes.0.len()).1
                } else if input.starts_with(self.search_prefixes.1) {
                    input.split_at(self.search_prefixes.1.len()).1
                } else {
                    &input
                };

                let term = term.strip_prefix(':').unwrap_or(term);

                self.perform_search(term).await
            }
        }
    }

    async fn make_playable(&self, track: ApiTrack) -> Result<Input, ResolverError> {
        let query = format!("ytsearch:{} - {}", track.info.author, track.info.title);
        tracing::debug!("AmazonMusic: Searching YouTube for: {}", query);

        if let Some(youtube) = crate::SOURCES.get("youtube") {
            if let Some(Query::Search(q)) = youtube.to_inner_ref().parse_query(&query) {
                tracing::debug!("AmazonMusic: Parsed YouTube search query: {:?}", q);
                match youtube.to_inner_ref().resolve(Query::Search(q)).await {
                    Ok(Some(res)) => match res {
                        ApiTrackResult::Search(tracks) => {
                            tracing::debug!(
                                "AmazonMusic: YouTube returned {} tracks",
                                tracks.len()
                            );
                            if let Some(first) = tracks.into_iter().next() {
                                tracing::debug!(
                                    "AmazonMusic: Playing first result: {}",
                                    first.info.title
                                );
                                return youtube.to_inner_ref().make_playable(first).await;
                            }
                        }
                        _ => {
                            tracing::warn!(
                                "AmazonMusic: Unexpected result type from YouTube search"
                            );
                        }
                    },
                    Ok(None) => {
                        tracing::warn!("AmazonMusic: YouTube search returned no results");
                    }
                    Err(e) => {
                        tracing::error!("AmazonMusic: YouTube search failed: {:?}", e);
                        return Err(e);
                    }
                }
            }
        }

        Err(ResolverError::MissingRequiredData(
            "Failed to find YouTube fallback or no results",
        ))
    }
}
