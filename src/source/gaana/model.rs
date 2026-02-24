#![allow(dead_code, unused)]
use serde::de::Error;
use serde::{Deserialize, Serialize};

fn deserialize_duration<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value: serde_json::Value = serde::Deserialize::deserialize(deserializer)?;
    
    match value {
        serde_json::Value::Number(n) => Ok(Some(n.as_f64().unwrap_or(0.0))),
        serde_json::Value::String(s) => s.parse::<f64>().ok().map(Some).ok_or_else(|| {
            Error::custom(format!("Failed to parse duration string: {}", s))
        }),
        serde_json::Value::Null => Ok(None),
        _ => Err(Error::custom("Invalid duration type")),
    }
}

#[derive(Debug, Deserialize)]
pub struct GaanaSearchResponse {
    pub success: Option<bool>,
    pub data: Option<Vec<GaanaTrack>>,
}

#[derive(Debug, Deserialize)]
pub struct GaanaTrackResponse {
    pub success: Option<bool>,
    pub data: Option<GaanaTrack>,
    pub track_id: Option<serde_json::Value>,
    pub title: Option<String>,
    pub name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_duration")]
    pub duration: Option<f64>,
    pub artists: Option<serde_json::Value>,
    pub seokey: Option<String>,
    pub artwork: Option<String>,
    #[serde(rename = "artworkUrl")]
    pub artwork_url: Option<String>,
    pub song_url: Option<String>,
    pub isrc: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct GaanaTrack {
    pub track_id: Option<serde_json::Value>,
    pub title: Option<String>,
    pub name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_duration")]
    pub duration: Option<f64>,
    pub artists: Option<serde_json::Value>,
    pub seokey: Option<String>,
    pub artwork: Option<String>,
    #[serde(rename = "artworkUrl")]
    pub artwork_url: Option<String>,
    pub song_url: Option<String>,
    pub isrc: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GaanaStreamResponse {
    pub success: Option<bool>,
    pub data: Option<GaanaStreamData>,
    pub url: Option<String>,
    #[serde(rename = "hlsUrl")]
    pub hls_url: Option<String>,
    #[serde(rename = "hls_url")]
    pub hls_url_alt: Option<String>,
    pub format: Option<String>,
    #[serde(rename = "initUrl")]
    pub init_url: Option<String>,
    #[serde(rename = "init_url")]
    pub init_url_alt: Option<String>,
    pub segments: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Deserialize)]
pub struct GaanaStreamData {
    pub url: Option<String>,
    #[serde(rename = "hlsUrl")]
    pub hls_url: Option<String>,
    #[serde(rename = "hls_url")]
    pub hls_url_alt: Option<String>,
    pub format: Option<String>,
    #[serde(rename = "initUrl")]
    pub init_url: Option<String>,
    #[serde(rename = "init_url")]
    pub init_url_alt: Option<String>,
    pub segments: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Deserialize)]
pub struct GaanaAlbumResponse {
    pub success: Option<bool>,
    pub data: Option<GaanaAlbum>,
    pub title: Option<String>,
    pub name: Option<String>,
    pub seokey: Option<String>,
    pub tracks: Option<Vec<GaanaTrack>>,
    pub songs: Option<Vec<GaanaTrack>>,
    pub artwork: Option<String>,
    #[serde(rename = "artworkUrl")]
    pub artwork_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GaanaAlbum {
    pub title: Option<String>,
    pub name: Option<String>,
    pub seokey: Option<String>,
    pub tracks: Option<Vec<GaanaTrack>>,
    pub songs: Option<Vec<GaanaTrack>>,
    pub artwork: Option<String>,
    #[serde(rename = "artworkUrl")]
    pub artwork_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GaanaPlaylistResponse {
    pub success: Option<bool>,
    pub data: Option<GaanaPlaylist>,
    pub playlist: Option<GaanaPlaylist>,
    pub title: Option<String>,
    pub name: Option<String>,
    pub playlist_name: Option<String>,
    pub seokey: Option<String>,
    pub playlist_id: Option<String>,
    pub tracks: Option<Vec<GaanaTrack>>,
    pub songs: Option<Vec<GaanaTrack>>,
    pub artwork: Option<String>,
    #[serde(rename = "artworkUrl")]
    pub artwork_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GaanaPlaylist {
    pub title: Option<String>,
    pub name: Option<String>,
    pub playlist_name: Option<String>,
    pub seokey: Option<String>,
    pub playlist_id: Option<String>,
    pub tracks: Option<Vec<GaanaTrack>>,
    pub songs: Option<Vec<GaanaTrack>>,
    pub artwork: Option<String>,
    #[serde(rename = "artworkUrl")]
    pub artwork_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GaanaArtistResponse {
    pub success: Option<bool>,
    pub data: Option<GaanaArtist>,
    pub title: Option<String>,
    pub name: Option<String>,
    pub seokey: Option<String>,
    pub artist_id: Option<String>,
    pub top_tracks: Option<Vec<GaanaTrack>>,
    pub tracks: Option<Vec<GaanaTrack>>,
    pub songs: Option<Vec<GaanaTrack>>,
    pub artwork: Option<String>,
    #[serde(rename = "artworkUrl")]
    pub artwork_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GaanaArtist {
    pub title: Option<String>,
    pub name: Option<String>,
    pub seokey: Option<String>,
    pub artist_id: Option<String>,
    pub top_tracks: Option<Vec<GaanaTrack>>,
    pub tracks: Option<Vec<GaanaTrack>>,
    pub songs: Option<Vec<GaanaTrack>>,
    pub artwork: Option<String>,
    #[serde(rename = "artworkUrl")]
    pub artwork_url: Option<String>,
}

pub fn format_artists(artists: &serde_json::Value) -> String {
    match artists {
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|a| {
                a.get("name")
                    .and_then(|n| n.as_str())
                    .or_else(|| a.as_str())
            })
            .collect::<Vec<&str>>()
            .join(", "),
        serde_json::Value::String(s) => s.clone(),
        _ => String::from("Unknown"),
    }
}

#[derive(Debug)]
pub struct GaanaStreamInfo {
    pub url: String,
    pub protocol: String,
    pub format: String,
    pub init_url: Option<String>,
    pub segments: Vec<String>,
}
