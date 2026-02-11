#![allow(dead_code, unused)]
use crate::CONFIG;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use tokio::time::Instant;

/// Deezer API sometimes returns integer 0 where a string is expected.
/// This deserializer handles both cases gracefully.
fn string_or_number<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    match value {
        Value::String(s) => Ok(s),
        Value::Number(n) => Ok(n.to_string()),
        Value::Null => Ok(String::new()),
        other => Ok(other.to_string()),
    }
}

fn optional_string_or_number<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    match value {
        Value::Null => Ok(None),
        Value::String(s) => Ok(Some(s)),
        Value::Number(n) => Ok(Some(n.to_string())),
        other => Ok(Some(other.to_string())),
    }
}

pub enum DeezerQuality {
    Flac,
    Mp3_320,
    Mp3_256,
    Mp3_128,
}

#[derive(Serialize, Debug)]
pub struct DeezerQualityFormat {
    pub format: String,
    pub cipher: String,
}

#[derive(Serialize, Debug)]
pub struct DeezerGetUrlMedia {
    #[serde(rename = "type")]
    pub media_type: String,
    pub formats: Vec<DeezerQualityFormat>,
}

#[derive(Serialize, Debug)]
pub struct DeezerGetUrlBody {
    pub license_token: String,
    pub media: Vec<DeezerGetUrlMedia>,
    pub track_tokens: Vec<String>,
}

#[derive(Serialize, Debug)]
pub struct DeezerMakePlayableBody {
    #[serde(rename = "SNG_ID")]
    pub sng_id: String,
}

#[derive(Debug, Deserialize)]
pub struct DeezerGetMediaError {
    pub code: u16,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct DeezerGetMedia {
    pub error: Option<Vec<DeezerGetMediaError>>,
    pub data: Option<Vec<DeezerGetMediaData>>,
}

#[derive(Debug, Deserialize)]
pub struct DeezerGetMediaData {
    pub media: Vec<DeezerGetMediaEntry>,
}

#[derive(Debug, Deserialize)]
pub struct DeezerGetMediaEntry {
    pub media_type: String,
    pub cipher: DeezerGetMediaCipher,
    pub format: String,
    pub sources: Vec<DeezerGetMediaSource>,
    pub nbf: u64,
    pub exp: u64,
}

#[derive(Debug, Deserialize)]
pub struct DeezerGetMediaSource {
    pub url: String,
    pub provider: String,
}

#[derive(Debug, Deserialize)]
pub struct DeezerGetMediaCipher {
    #[serde(rename = "type")]
    pub type_name: String,
}

#[derive(Debug, Deserialize)]
pub struct InternalDeezerSongData {
    #[serde(rename = "ALB_ID")]
    pub alb_id: String,
    #[serde(rename = "ALB_PICTURE")]
    pub alb_picture: String,
    #[serde(rename = "ALB_TITLE")]
    pub alb_title: String,
    #[serde(rename = "ARTISTS")]
    pub artists: Vec<InternalDeezerSongArtist>,
    #[serde(rename = "ART_ID")]
    pub art_id: String,
    #[serde(rename = "ART_NAME")]
    pub art_name: String,
    #[serde(rename = "ARTIST_IS_DUMMY")]
    pub artist_is_dummy: bool,
    #[serde(rename = "DIGITAL_RELEASE_DATE")]
    pub digital_release_date: String,
    #[serde(rename = "DISK_NUMBER")]
    pub disk_number: String,
    #[serde(rename = "DURATION")]
    pub duration: String,
    #[serde(rename = "EXPLICIT_LYRICS")]
    pub explicit_lyrics: String,
    #[serde(rename = "EXPLICIT_TRACK_CONTENT")]
    pub explicit_track_content: InternalDeezerExplicitTrackContent,
    #[serde(rename = "GENRE_ID")]
    pub genre_id: String,
    #[serde(rename = "ISRC")]
    pub isrc: String,
    #[serde(rename = "LYRICS_ID")]
    pub lyrics_id: i64,
    #[serde(rename = "PHYSICAL_RELEASE_DATE")]
    pub physical_release_date: String,
    #[serde(rename = "PROVIDER_ID")]
    pub provider_id: String,
    #[serde(rename = "RANK")]
    pub rank: String,
    #[serde(rename = "SMARTRADIO")]
    pub smartradio: i64,
    #[serde(rename = "SNG_CONTRIBUTORS")]
    pub sng_contributors: InternalDeezerSongContributors,
    #[serde(rename = "SNG_ID")]
    pub sng_id: String,
    #[serde(rename = "SNG_TITLE")]
    pub sng_title: String,
    #[serde(rename = "STATUS")]
    pub status: i64,
    #[serde(rename = "TRACK_NUMBER")]
    pub track_number: String,
    #[serde(rename = "USER_ID")]
    pub user_id: i64,
    #[serde(rename = "VERSION")]
    pub version: Option<String>,
    #[serde(rename = "MD5_ORIGIN")]
    pub md5_origin: Option<String>,
    #[serde(rename = "FILESIZE_AAC_64")]
    pub filesize_aac_64: String,
    #[serde(rename = "FILESIZE_MP3_64")]
    pub filesize_mp3_64: String,
    #[serde(rename = "FILESIZE_MP3_128")]
    pub filesize_mp3_128: String,
    #[serde(rename = "FILESIZE_MP3_256")]
    pub filesize_mp3_256: String,
    #[serde(rename = "FILESIZE_MP3_320")]
    pub filesize_mp3_320: String,
    #[serde(rename = "FILESIZE_MP4_RA1")]
    pub filesize_mp4_ra1: String,
    #[serde(rename = "FILESIZE_MP4_RA2")]
    pub filesize_mp4_ra2: String,
    #[serde(rename = "FILESIZE_MP4_RA3")]
    pub filesize_mp4_ra3: String,
    #[serde(rename = "FILESIZE_FLAC")]
    pub filesize_flac: String,
    #[serde(rename = "FILESIZE")]
    pub filesize: String,
    #[serde(rename = "GAIN")]
    pub gain: Option<String>,
    #[serde(rename = "MEDIA_VERSION")]
    pub media_version: String,
    #[serde(rename = "TRACK_TOKEN")]
    pub track_token: String,
    #[serde(rename = "TRACK_TOKEN_EXPIRE")]
    pub track_token_expire: i64,
    #[serde(rename = "MEDIA")]
    pub media: Vec<InteralDeezerMedia>,
}

#[derive(Debug, Deserialize)]
pub struct InternalDeezerSongArtist {
    #[serde(rename = "ART_ID")]
    pub art_id: String,
    #[serde(rename = "ROLE_ID")]
    pub role_id: String,
    #[serde(rename = "ARTISTS_SONGS_ORDER")]
    pub artists_songs_order: String,
    #[serde(rename = "ART_NAME")]
    pub art_name: String,
    #[serde(rename = "ARTIST_IS_DUMMY")]
    pub artist_is_dummy: bool,
    #[serde(rename = "ART_PICTURE")]
    pub art_picture: String,
    #[serde(rename = "RANK")]
    pub rank: String,
}

#[derive(Debug, Deserialize)]
pub struct InternalDeezerExplicitTrackContent {
    #[serde(rename = "EXPLICIT_LYRICS_STATUS")]
    pub explicit_lyrics_status: i64,
    #[serde(rename = "EXPLICIT_COVER_STATUS")]
    pub explicit_cover_status: i64,
}

#[derive(Debug, Deserialize)]
pub struct InternalDeezerSongContributors {
    #[serde(rename = "main_artist")]
    pub main_artist: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct InteralDeezerMedia {
    #[serde(rename = "TYPE")]
    pub type_field: String,
    #[serde(rename = "HREF")]
    pub href: String,
}

#[derive(Debug, Deserialize)]
pub struct InternalDeezerGetUserData {
    #[serde(rename = "USER")]
    pub user: InternalDeezerUser,
    #[serde(rename = "SETTING_LANG", deserialize_with = "string_or_number")]
    pub setting_lang: String,
    #[serde(rename = "SETTING_LOCALE", deserialize_with = "string_or_number")]
    pub setting_locale: String,
    #[serde(rename = "DIRECTION", deserialize_with = "string_or_number")]
    pub direction: String,
    #[serde(rename = "SESSION_ID", deserialize_with = "string_or_number")]
    pub session_id: String,
    #[serde(rename = "USER_TOKEN", deserialize_with = "string_or_number")]
    pub user_token: String,
    #[serde(rename = "PLAYLIST_WELCOME_ID", deserialize_with = "string_or_number")]
    pub playlist_welcome_id: String,
    #[serde(rename = "COUNTRY", deserialize_with = "string_or_number")]
    pub country: String,
    #[serde(rename = "COUNTRY_CATEGORY", deserialize_with = "string_or_number")]
    pub country_category: String,
    #[serde(rename = "SERVER_TIMESTAMP")]
    pub server_timestamp: i64,
    #[serde(rename = "PLAYER_TOKEN", deserialize_with = "string_or_number")]
    pub player_token: String,
    #[serde(rename = "checkForm", deserialize_with = "string_or_number")]
    pub check_form: String,
    #[serde(rename = "FROM_ONBOARDING", deserialize_with = "string_or_number")]
    pub from_onboarding: String,
    #[serde(rename = "CUSTO", deserialize_with = "string_or_number")]
    pub custo: String,
    #[serde(rename = "SETTING_REFERER_UPLOAD", deserialize_with = "string_or_number")]
    pub setting_referer_upload: String,
    #[serde(rename = "URL_MEDIA", deserialize_with = "string_or_number")]
    pub url_media: String,
}

#[derive(Debug, Deserialize)]
pub struct InternalDeezerUser {
    #[serde(rename = "USER_ID")]
    pub user_id: i64,
    #[serde(rename = "INSCRIPTION_DATE", deserialize_with = "string_or_number")]
    pub inscription_date: String,
    #[serde(rename = "OPTIONS")]
    pub options: InternalDeezerOptions,
    #[serde(rename = "EXPLICIT_CONTENT_LEVEL", default, deserialize_with = "optional_string_or_number")]
    pub explicit_content_level: Option<String>,
    #[serde(rename = "EXPLICIT_CONTENT_LEVELS_AVAILABLE", default)]
    pub explicit_content_levels_available: Option<Vec<String>>,
    #[serde(rename = "HAS_UPNEXT", default)]
    pub has_upnext: bool,
    #[serde(rename = "LOVEDTRACKS_ID", deserialize_with = "string_or_number")]
    pub lovedtracks_id: String,
}

#[derive(Debug, Deserialize)]
pub struct InternalDeezerOptions {
    #[serde(default)]
    pub mobile_radio: bool,
    #[serde(default)]
    pub mobile_offline: bool,
    #[serde(default)]
    pub mobile_sound_quality: InternalDeezerOptionSoundQuality,
    #[serde(default)]
    pub mobile_hq: bool,
    #[serde(default)]
    pub mobile_lossless: bool,
    #[serde(default)]
    pub tablet_sound_quality: InternalDeezerOptionSoundQuality,
    #[serde(default)]
    pub web_hq: bool,
    #[serde(default)]
    pub web_lossless: bool,
    #[serde(default)]
    pub web_sound_quality: InternalDeezerOptionSoundQuality,
    #[serde(default)]
    pub license_token: String,
    #[serde(default)]
    pub expiration_timestamp: i64,
    #[serde(default, deserialize_with = "string_or_number")]
    pub license_country: String,
    #[serde(default)]
    pub timestamp: i64,
    #[serde(default)]
    pub audio_qualities: InternalDeezerOptionAudioQualities,
    #[serde(default)]
    pub hq: bool,
    #[serde(default)]
    pub lossless: bool,
    #[serde(default)]
    pub offline: bool,
    #[serde(default)]
    pub preview: bool,
    #[serde(default)]
    pub radio: bool,
    #[serde(default)]
    pub sound_quality: InternalDeezerOptionSoundQuality,
}

#[derive(Debug, Default, Deserialize)]
pub struct InternalDeezerOptionSoundQuality {
    #[serde(default)]
    pub low: Option<bool>,
    #[serde(default)]
    pub standard: bool,
    #[serde(default)]
    pub high: bool,
    #[serde(default)]
    pub lossless: bool,
    #[serde(default)]
    pub reality: bool,
}

#[derive(Debug, Default, Deserialize)]
pub struct InternalDeezerOptionAudioQualities {
    #[serde(default)]
    pub mobile_download: Vec<String>,
    #[serde(default)]
    pub mobile_streaming: Vec<String>,
    #[serde(default)]
    pub wifi_download: Vec<String>,
    #[serde(default)]
    pub wifi_streaming: Vec<String>,
}

#[derive(Deserialize, Debug)]
pub struct InternalDeezerResponseError {
    #[serde(rename = "type")]
    pub cause: String,
    pub code: u16,
    pub message: String,
}

#[derive(Deserialize, Debug)]
pub struct InternalDeezerResponse<T> {
    #[serde(default)]
    pub error: Value,
    pub results: T,
}

#[derive(Deserialize, Debug)]
pub struct DeezerApiArtist {
    pub id: u32,
    pub name: String,
    pub link: String,
    #[serde(rename = "picture_medium")]
    pub thumbnail: String,
    #[serde(rename = "tracklist")]
    pub tracks: String,
}

#[derive(Deserialize, Debug)]
pub struct DeezerApiAlbum {
    pub id: u32,
    pub title: String,
    #[serde(rename = "cover_medium")]
    pub thumbnail: String,
    #[serde(rename = "tracklist")]
    pub tracks: String,
}

#[derive(Deserialize, Debug)]
pub struct DeezerApiTrack {
    pub id: u32,
    pub readable: bool,
    pub title: String,
    pub link: String,
    pub duration: u16,
    pub isrc: Option<String>,
    pub artist: DeezerApiArtist,
    pub album: DeezerApiAlbum,
}

#[derive(Deserialize, Debug)]
pub struct DeezerData<T> {
    pub data: T,
}

#[derive(Clone, Debug)]
pub struct Tokens {
    pub session_id: String,
    pub unique_id: String,
    pub check_form: String,
    pub license_token: String,
    pub expire_at: Instant,
}

impl Tokens {
    pub fn create_cookie(&self) -> String {
        format!(
            "arl={}; {}; {}",
            CONFIG
                .deezer_config
                .as_ref()
                .expect("Unexpected Nullish Config")
                .arl
                .to_owned(),
            self.session_id,
            self.unique_id
        )
    }
}

impl DeezerQualityFormat {
    pub fn new(data: &InternalDeezerSongData, quality: Option<DeezerQuality>) -> Self {
        let mut quality = {
            match quality.unwrap_or(DeezerQuality::Mp3_128) {
                DeezerQuality::Flac => "FLAC",
                DeezerQuality::Mp3_320 => "MP3_320",
                DeezerQuality::Mp3_256 => "MP3_256",
                DeezerQuality::Mp3_128 => "MP3_128",
            }
        };

        if quality == "FLAC" && data.filesize_flac == "0" {
            quality = "MP3_128";
        }

        if quality == "MP3_320" && data.filesize_mp3_320 == "0" {
            quality = "MP3_128";
        }

        if quality == "MP3_256" && data.filesize_mp3_256 == "0" {
            quality = "MP3_128";
        }

        Self {
            format: quality.to_string(),
            cipher: String::from("BF_CBC_STRIPE"),
        }
    }
}
