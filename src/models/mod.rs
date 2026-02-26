use kameo::Reply;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

// Lavalink Types: reduced to what we actually need

fn str_to_u64<'de, T, D>(de: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: std::str::FromStr,
    <T as std::str::FromStr>::Err: std::fmt::Display,
{
    String::deserialize(de)?
        .parse()
        .map_err(serde::de::Error::custom)
}

fn u64_to_str<S>(num: &u64, se: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    se.serialize_str(num.to_string().as_str())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Common,
    Suspicious,
    Fault,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LoadType {
    Track,
    Playlist,
    Search,
    Empty,
    Error,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[serde(tag = "loadType", content = "data")]
pub enum ApiTrackResult {
    Track(ApiTrack),
    Playlist(ApiTrackPlaylist),
    Search(Vec<ApiTrack>),
    Error(ApiTrackLoadException),
    Empty(Option<Empty>),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Empty;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiPlaylistInfo {
    pub name: String,
    pub selected_track: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiTrackPlaylist {
    pub info: ApiPlaylistInfo,
    pub plugin_info: Empty,
    pub tracks: Vec<ApiTrack>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApiTrackLoadException {
    pub message: String,
    pub severity: Severity,
    pub cause: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiVoiceData {
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub endpoint: String,
    #[serde(default)]
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connected: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ping: Option<i32>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ApiPlayerState {
    pub time: u64,
    pub position: u32,
    pub connected: bool,
    pub ping: Option<i32>,
}

#[derive(Clone, Debug, Reply, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiPlayer {
    #[serde(deserialize_with = "str_to_u64", serialize_with = "u64_to_str")]
    pub guild_id: u64,
    pub track: Option<ApiTrack>,
    pub volume: u32,
    pub paused: bool,
    pub state: ApiPlayerState,
    pub voice: ApiVoiceData,
    pub filters: LavalinkFilters,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiTrackInfo {
    pub identifier: String,
    pub is_seekable: bool,
    pub author: String,
    pub length: u64,
    pub is_stream: bool,
    pub position: u64,
    pub title: String,
    pub uri: Option<String>,
    pub artwork_url: Option<String>,
    pub isrc: Option<String>,
    pub source_name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiTrack {
    pub encoded: String,
    pub info: ApiTrackInfo,
    pub plugin_info: Empty,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_data: Option<Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiException {
    #[serde(deserialize_with = "str_to_u64", serialize_with = "u64_to_str")]
    pub guild_id: u64,
    pub message: Option<String>,
    pub severity: String,
    pub cause: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiTrackStart {
    #[serde(deserialize_with = "str_to_u64", serialize_with = "u64_to_str")]
    pub guild_id: u64,
    pub track: ApiTrack,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiTrackEnd {
    #[serde(deserialize_with = "str_to_u64", serialize_with = "u64_to_str")]
    pub guild_id: u64,
    pub track: ApiTrack,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiTrackException {
    #[serde(deserialize_with = "str_to_u64", serialize_with = "u64_to_str")]
    pub guild_id: u64,
    pub track: ApiTrack,
    pub exception: ApiException,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiTrackStuck {
    #[serde(deserialize_with = "str_to_u64", serialize_with = "u64_to_str")]
    pub guild_id: u64,
    pub track: ApiTrack,
    pub threshold_ms: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiWebSocketClosed {
    #[serde(deserialize_with = "str_to_u64", serialize_with = "u64_to_str")]
    pub guild_id: u64,
    pub code: usize,
    pub reason: String,
    pub by_remote: bool,
}

#[allow(clippy::enum_variant_names)]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ApiPlayerEvents {
    TrackStartEvent(ApiTrackStart),
    TrackEndEvent(ApiTrackEnd),
    TrackExceptionEvent(ApiTrackException),
    TrackStuckEvent(ApiTrackStuck),
    WebSocketClosedEvent(ApiWebSocketClosed),
}

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateApiPlayerTrack {
    pub encoded: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_data: Option<Value>,
}

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiPlayerOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track: Option<UpdateApiPlayerTrack>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paused: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice: Option<ApiVoiceData>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<LavalinkFilters>,
}

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct ApiFrameStats {
    pub sent: u64,
    pub nulled: u32,
    pub deficit: i32,
}

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiCpu {
    pub cores: u32,
    pub system_load: f64,
    pub lavalink_load: f64,
}

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct ApiMemory {
    pub free: u64,
    pub used: u64,
    pub allocated: u64,
    pub reservable: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiReady {
    pub resumed: bool,
    pub session_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiPlayerUpdate {
    #[serde(deserialize_with = "str_to_u64", serialize_with = "u64_to_str")]
    pub guild_id: u64,
    pub state: ApiPlayerState,
}

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiStats {
    pub players: u32,
    pub playing_players: u32,
    pub uptime: u64,
    pub memory: ApiMemory,
    pub cpu: ApiCpu,
    pub frame_stats: Option<ApiFrameStats>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op")]
#[serde(rename_all = "camelCase")]
pub enum ApiNodeMessage {
    Ready(Box<ApiReady>),
    PlayerUpdate(Box<ApiPlayerUpdate>),
    Stats(Box<ApiStats>),
    Event(Box<ApiPlayerEvents>),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiSessionBody {
    pub resuming: bool,
    pub timeout: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiSessionInfo {
    pub resuming_key: String,
    pub timeout: u16,
}

/**
 * The structs below is included because Anchorage (https://github.com/Deivu/Anchorage/tree/master) breaks without it
 */

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LavalinkFilters {
    pub volume: Option<f64>,
    pub equalizer: Option<Vec<Equalizer>>,
    pub karaoke: Option<Karaoke>,
    pub timescale: Option<Timescale>,
    pub tremolo: Option<Tremolo>,
    pub vibrato: Option<Vibrato>,
    pub rotation: Option<Rotation>,
    pub distortion: Option<Distortion>,
    pub channel_mix: Option<ChannelMix>,
    pub low_pass: Option<LowPass>,
    pub plugin_filters: Option<Value>,
}

impl LavalinkFilters {
    pub fn merge(&mut self, other: LavalinkFilters) {
        if other.volume.is_some() {
            self.volume = other.volume;
        }
        if other.equalizer.is_some() {
            self.equalizer = other.equalizer;
        }
        if other.karaoke.is_some() {
            self.karaoke = other.karaoke;
        }
        if other.timescale.is_some() {
            self.timescale = other.timescale;
        }
        if other.tremolo.is_some() {
            self.tremolo = other.tremolo;
        }
        if other.vibrato.is_some() {
            self.vibrato = other.vibrato;
        }
        if other.rotation.is_some() {
            self.rotation = other.rotation;
        }
        if other.distortion.is_some() {
            self.distortion = other.distortion;
        }
        if other.channel_mix.is_some() {
            self.channel_mix = other.channel_mix;
        }
        if other.low_pass.is_some() {
            self.low_pass = other.low_pass;
        }
        if other.plugin_filters.is_some() {
            self.plugin_filters = other.plugin_filters;
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tremolo {
    pub frequency: Option<f64>,
    pub depth: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Vibrato {
    pub frequency: Option<f64>,
    pub depth: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Timescale {
    pub speed: Option<f64>,
    pub pitch: Option<f64>,
    pub rate: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rotation {
    pub rotation_hz: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LowPass {
    pub smoothing: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Karaoke {
    pub level: Option<f64>,
    pub mono_level: Option<f64>,
    pub filter_band: Option<f64>,
    pub filter_width: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Equalizer {
    pub band: u16,
    pub gain: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Distortion {
    pub sin_offset: Option<f64>,
    pub sin_scale: Option<f64>,
    pub cos_offset: Option<f64>,
    pub cos_scale: Option<f64>,
    pub tan_offset: Option<f64>,
    pub tan_scale: Option<f64>,
    pub offset: Option<f64>,
    pub scale: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelMix {
    pub left_to_left: Option<f64>,
    pub left_to_right: Option<f64>,
    pub right_to_left: Option<f64>,
    pub right_to_right: Option<f64>,
}
