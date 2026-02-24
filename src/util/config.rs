use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeezerConfig {
    pub decrypt_key: String,
    pub arl: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct YoutubeClientSettings {
    pub refresh_token: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct YoutubeClientsConfig {
    pub search: Option<Vec<String>>,
    pub playback: Option<Vec<String>>,
    pub resolve: Option<Vec<String>>,
    pub settings: Option<HashMap<String, YoutubeClientSettings>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct YoutubeCipherConfig {
    pub url: String,
    pub token: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct YoutubeConfig {
    pub enabled: Option<bool>,
    pub allow_itag: Option<Vec<u32>>,
    pub target_itag: Option<u32>,
    pub get_oauth_token: Option<bool>,
    pub use_po_token: Option<bool>,
    pub hl: Option<String>,
    pub gl: Option<String>,
    pub cookies: Option<String>,
    pub clients: Option<YoutubeClientsConfig>,
    pub max_search_results: Option<u32>,
    pub cipher: Option<YoutubeCipherConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpConfig {}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AmazonMusicConfig {
    pub enabled: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppleMusicConfig {
    pub enabled: Option<bool>,
    pub media_api_token: Option<String>,
    pub market: Option<String>,
    pub playlist_load_limit: Option<u32>,
    pub album_load_limit: Option<u32>,
    pub playlist_page_load_concurrency: Option<u32>,
    pub album_page_load_concurrency: Option<u32>,
    pub allow_explicit: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JioSaavnConfig {
    pub api_url: String,
    pub secret_key: Option<String>,
    pub playlist_track_limit: Option<u32>,
    pub recommendations_track_limit: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GaanaConfig {
    pub api_url: Option<String>,
    pub stream_quality: Option<String>,
    pub playlist_track_limit: Option<u32>,
    pub album_track_limit: Option<u32>,
    pub artist_track_limit: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotifyConfig {
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub sp_dc: Option<String>,
    pub market: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SonglinkConfig {
    pub fallback_to_any: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SoundCloudConfig {
    pub enabled: Option<bool>,
    pub client_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoggingFileConfig {
    pub enabled: bool,
    pub path: String,
    pub rotation: String,
    pub ttl_days: u32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoggingConfig {
    pub level: String,
    pub file: LoggingFileConfig,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionThresholds {
    pub bad: f64,
    pub average: f64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionConfig {
    pub log_all_checks: bool,
    pub interval: u64,
    pub timeout: u64,
    pub thresholds: ConnectionThresholds,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FiltersEnabled {
    pub tremolo: bool,
    pub vibrato: bool,
    pub lowpass: bool,
    pub highpass: bool,
    pub rotation: bool,
    pub karaoke: bool,
    pub distortion: bool,
    pub channel_mix: bool,
    pub equalizer: bool,
    pub chorus: bool,
    pub compressor: bool,
    pub echo: bool,
    pub phaser: bool,
    pub timescale: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FiltersConfig {
    pub enabled: FiltersEnabled,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioFadingCurve {
    pub duration: u64,
    pub curve: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDucking {
    pub enabled: bool,
    pub duration: u64,
    pub target_volume: f64,
    pub curve: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioFading {
    pub enabled: bool,
    pub track_start: AudioFadingCurve,
    pub track_end: AudioFadingCurve,
    pub track_stop: AudioFadingCurve,
    pub seek: AudioFadingCurve,
    pub ducking: AudioDucking,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioConfig {
    pub quality: String,
    pub encryption: String,
    pub resampling_quality: String,
    pub fading: AudioFading,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutePlannerConfig {
    pub strategy: String,
    pub banned_ip_cooldown: u64,
    pub ip_blocks: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitGlobal {
    pub max_requests: u32,
    pub time_window_ms: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitPerIp {
    pub max_requests: u32,
    pub time_window_ms: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitPerUserId {
    pub max_requests: u32,
    pub time_window_ms: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitPerGuildId {
    pub max_requests: u32,
    pub time_window_ms: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitIgnore {
    pub user_ids: Vec<String>,
    pub guild_ids: Vec<String>,
    pub ips: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitConfig {
    pub enabled: bool,
    pub global: RateLimitGlobal,
    pub per_ip: RateLimitPerIp,
    pub per_user_id: RateLimitPerUserId,
    pub per_guild_id: RateLimitPerGuildId,
    pub ignore_paths: Vec<String>,
    pub ignore: RateLimitIgnore,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DosProtectionThresholds {
    pub burst_requests: u32,
    pub time_window_ms: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DosProtectionMitigation {
    pub delay_ms: u64,
    pub block_duration_ms: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DosProtectionIgnore {
    pub user_ids: Vec<String>,
    pub guild_ids: Vec<String>,
    pub ips: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DosProtectionConfig {
    pub enabled: bool,
    pub thresholds: DosProtectionThresholds,
    pub mitigation: DosProtectionMitigation,
    pub ignore: DosProtectionIgnore,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsAuthorization {
    #[serde(rename = "type")]
    pub auth_type: String,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsConfig {
    pub enabled: bool,
    pub authorization: MetricsAuthorization,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub port: u16,
    pub address: String,
    pub authorization: String,
    pub player_update_secs: Option<u8>,
    pub status_update_secs: Option<u8>,
    pub max_search_results: Option<u32>,
    pub max_album_playlist_length: Option<u32>,
    pub track_stuck_threshold_ms: Option<u64>,
    pub event_timeout_ms: Option<u64>,
    pub zombie_threshold_ms: Option<u64>,
    pub enable_holo_tracks: Option<bool>,
    pub enable_track_stream_endpoint: Option<bool>,
    pub resolve_external_links: Option<bool>,
    pub fetch_channel_info: Option<bool>,
    pub default_search_source: Option<Vec<String>>,
    pub unified_search_sources: Option<Vec<String>>,
    pub logging: Option<LoggingConfig>,
    pub connection: Option<ConnectionConfig>,
    pub filters: Option<FiltersConfig>,
    pub audio: Option<AudioConfig>,
    pub route_planner: Option<RoutePlannerConfig>,
    pub rate_limit: Option<RateLimitConfig>,
    pub dos_protection: Option<DosProtectionConfig>,
    pub metrics: Option<MetricsConfig>,
    pub deezer_config: Option<DeezerConfig>,
    pub youtube_config: Option<YoutubeConfig>,
    pub jiosaavn_config: Option<JioSaavnConfig>,
    pub gaana_config: Option<GaanaConfig>,
    pub http_config: Option<HttpConfig>,
    pub amazonmusic_config: Option<AmazonMusicConfig>,
    pub applemusic_config: Option<AppleMusicConfig>,
    pub spotify_config: Option<SpotifyConfig>,
    pub soundcloud_config: Option<SoundCloudConfig>,
    pub songlink_config: Option<SonglinkConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Config::new()
    }
}

impl Config {
    pub fn new() -> Self {
        let config = fs::read_to_string("./config.json").expect("Missing ./config.json");
        serde_json::from_str::<Config>(&config).unwrap()
    }
}
