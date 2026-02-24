use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UrlType {
    Video,
    Playlist,
    Shorts,
    Unknown,
}

/// Innertube client context sent with every API request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InnertubeContext {
    pub client: InnertubeClientInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<InnertubeUser>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<InnertubeRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InnertubeClientInfo {
    pub client_name: String,
    pub client_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_make: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub android_sdk_version: Option<String>,
    pub hl: String,
    pub gl: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visitor_data: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InnertubeUser {
    pub locked_safety_mode: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InnertubeRequest {
    pub use_ssl: bool,
}

/// Player API request body
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerRequestBody {
    pub context: InnertubeContext,
    pub video_id: String,
    pub content_check_ok: bool,
    pub racy_check_ok: bool,
}

/// Search API request body
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchRequestBody {
    pub context: InnertubeContext,
    pub query: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<String>,
}

/// Innertube player API response (partial — only fields we need)
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerResponse {
    pub playability_status: Option<PlayabilityStatus>,
    pub video_details: Option<VideoDetails>,
    pub streaming_data: Option<StreamingData>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayabilityStatus {
    pub status: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoDetails {
    pub video_id: String,
    pub title: Option<String>,
    pub length_seconds: Option<String>,
    pub channel_id: Option<String>,
    pub short_description: Option<String>,
    pub thumbnail: Option<ThumbnailContainer>,
    pub author: Option<String>,
    pub is_live_content: Option<bool>,
    pub is_live: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbnailContainer {
    pub thumbnails: Option<Vec<Thumbnail>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Thumbnail {
    pub url: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamingData {
    pub adaptive_formats: Option<Vec<Format>>,
    pub formats: Option<Vec<Format>>,
    pub hls_manifest_url: Option<String>,
    pub expires_in_seconds: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Format {
    pub itag: u32,
    pub url: Option<String>,
    pub mime_type: Option<String>,
    pub bitrate: Option<u64>,
    pub quality_label: Option<String>,
    pub audio_quality: Option<String>,
    pub signature_cipher: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub content_length: Option<String>,
}

/// Simplified stream info returned by innertube clients
#[derive(Debug, Clone)]
pub struct StreamInfo {
    pub url: String,
    pub itag: u32,
    pub mime_type: String,
    pub bitrate: u64,
    pub is_audio: bool,
    /// The User-Agent of the innertube client that fetched this stream
    pub user_agent: String,
}

/// Search result video renderer from innertube search API
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    pub contents: Option<SearchContents>,
    pub error: Option<ApiError>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchContents {
    pub section_list_renderer: Option<SectionListRenderer>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionListRenderer {
    pub contents: Option<Vec<SectionContent>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionContent {
    pub item_section_renderer: Option<ItemSectionRenderer>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemSectionRenderer {
    pub contents: Option<Vec<SearchItem>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchItem {
    pub video_renderer: Option<VideoRenderer>,
    pub compact_video_renderer: Option<VideoRenderer>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoRenderer {
    pub video_id: Option<String>,
    pub title: Option<TextRuns>,
    pub owner_text: Option<TextRuns>,
    pub long_byline_text: Option<TextRuns>,
    pub short_byline_text: Option<TextRuns>,
    pub length_text: Option<SimpleText>,
    pub thumbnail: Option<ThumbnailContainer>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TextRuns {
    pub runs: Option<Vec<TextRun>>,
    #[serde(rename = "simpleText")]
    pub simple_text: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TextRun {
    pub text: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimpleText {
    pub simple_text: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiError {
    pub message: Option<String>,
}

pub struct UrlPatterns {
    video: Regex,
    playlist: Regex,
    short_url: Regex,
    shorts: Regex,
    music_video: Regex,
    video_id: Regex,
    playlist_id: Regex,
}

impl UrlPatterns {
    pub fn new() -> Self {
        Self {
            video: Regex::new(r"^https?://(?:music\.)?(?:www\.)?youtube\.com/watch\?v=[\w-]+").unwrap(),
            playlist: Regex::new(r"^https?://(?:music\.)?(?:www\.)?youtube\.com/playlist\?list=[\w-]+").unwrap(),
            short_url: Regex::new(r"^https?://youtu\.be/[\w-]+").unwrap(),
            shorts: Regex::new(r"^https?://(?:www\.)?youtube\.com/shorts/[\w-]+").unwrap(),
            music_video: Regex::new(r"^https?://music\.youtube\.com/watch\?v=[\w-]+").unwrap(),
            video_id: Regex::new(r"(?:v=|shorts/|youtu\.be/)([^&?]+)").unwrap(),
            playlist_id: Regex::new(r"[?&]list=([\w-]+)").unwrap(),
        }
    }

    pub fn check_url_type(&self, url: &str) -> UrlType {
        if self.playlist_id.is_match(url) {
            return UrlType::Playlist;
        }

        if self.video.is_match(url) || self.short_url.is_match(url) || self.music_video.is_match(url) {
            return UrlType::Video;
        }

        if self.shorts.is_match(url) {
            return UrlType::Shorts;
        }

        if self.playlist.is_match(url) {
            return UrlType::Playlist;
        }

        UrlType::Unknown
    }

    /// Extract video ID from a URL
    pub fn extract_video_id(&self, url: &str) -> Option<String> {
        self.video_id
            .captures(url)
            .and_then(|cap| cap.get(1).map(|m| m.as_str().to_string()))
    }

    /// Extract playlist ID from a URL
    pub fn extract_playlist_id(&self, url: &str) -> Option<String> {
        self.playlist_id
            .captures(url)
            .and_then(|cap| cap.get(1).map(|m| m.as_str().to_string()))
    }
}

impl Default for UrlPatterns {
    fn default() -> Self {
        Self::new()
    }
}

pub fn get_runs_text(runs: &Option<TextRuns>) -> Option<String> {
    runs.as_ref().and_then(|tr| {
        if let Some(simple) = &tr.simple_text {
            return Some(simple.clone());
        }
        tr.runs.as_ref().map(|runs| {
            runs.iter()
                .filter_map(|r| r.text.as_ref())
                .cloned()
                .collect::<Vec<_>>()
                .join("")
        })
    })
}

/// Parse a duration string like "3:42" or "1:05:30" into milliseconds
pub fn parse_duration_text(text: &str) -> u64 {
    let parts: Vec<&str> = text.split(':').collect();
    let mut seconds: u64 = 0;

    match parts.len() {
        3 => {
            seconds += parts[0].parse::<u64>().unwrap_or(0) * 3600;
            seconds += parts[1].parse::<u64>().unwrap_or(0) * 60;
            seconds += parts[2].parse::<u64>().unwrap_or(0);
        }
        2 => {
            seconds += parts[0].parse::<u64>().unwrap_or(0) * 60;
            seconds += parts[1].parse::<u64>().unwrap_or(0);
        }
        1 => {
            seconds += parts[0].parse::<u64>().unwrap_or(0);
        }
        _ => {}
    }

    seconds * 1000
}

/// Get the best thumbnail URL from a video details or renderer
pub fn get_best_thumbnail(container: &Option<ThumbnailContainer>) -> Option<String> {
    container.as_ref().and_then(|tc| {
        tc.thumbnails.as_ref().and_then(|thumbs| {
            thumbs.last().map(|t| t.url.split('?').next().unwrap_or(&t.url).to_string())
        })
    })
}

/// Select best audio stream from streaming data
pub fn select_best_audio(streaming_data: &StreamingData, audio_itags: &[u32]) -> Option<StreamInfo> {
    let mut all_formats = Vec::new();

    if let Some(adaptive) = &streaming_data.adaptive_formats {
        all_formats.extend(adaptive.iter());
    }
    if let Some(formats) = &streaming_data.formats {
        all_formats.extend(formats.iter());
    }

    // Filter to audio formats only
    let audio_formats: Vec<&Format> = all_formats
        .iter()
        .filter(|f| {
            f.mime_type
                .as_ref()
                .is_some_and(|mime| mime.starts_with("audio/"))
        })
        .copied()
        .collect();

    // Try preferred itags first
    for itag in audio_itags {
        if let Some(format) = audio_formats.iter().find(|f| f.itag == *itag) {
            if let Some(url) = &format.url {
                return Some(StreamInfo {
                    url: url.clone(),
                    itag: format.itag,
                    mime_type: format.mime_type.clone().unwrap_or_default(),
                    bitrate: format.bitrate.unwrap_or(0),
                    is_audio: true,
                    user_agent: String::new(),
                });
            }
        }
    }

    // Fallback: pick highest bitrate audio
    let best = audio_formats
        .iter()
        .filter(|f| f.url.is_some())
        .max_by_key(|f| f.bitrate.unwrap_or(0));

    best.and_then(|format| {
        format.url.as_ref().map(|url| StreamInfo {
            url: url.clone(),
            itag: format.itag,
            mime_type: format.mime_type.clone().unwrap_or_default(),
            bitrate: format.bitrate.unwrap_or(0),
            is_audio: true,
            user_agent: String::new(),
        })
    })
}

/// Select best video stream from streaming data (used as fallback)
pub fn select_best_video(streaming_data: &StreamingData, video_itags: &[u32]) -> Option<StreamInfo> {
    let mut all_formats = Vec::new();

    if let Some(adaptive) = &streaming_data.adaptive_formats {
        all_formats.extend(adaptive.iter());
    }
    if let Some(formats) = &streaming_data.formats {
        all_formats.extend(formats.iter());
    }

    for itag in video_itags {
        if let Some(format) = all_formats.iter().find(|f| f.itag == *itag) {
            if let Some(url) = &format.url {
                return Some(StreamInfo {
                    url: url.clone(),
                    itag: format.itag,
                    mime_type: format.mime_type.clone().unwrap_or_default(),
                    bitrate: format.bitrate.unwrap_or(0),
                    is_audio: false,
                    user_agent: String::new(),
                });
            }
        }
    }

    None
}
