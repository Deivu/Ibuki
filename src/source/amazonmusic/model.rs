use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmazonConfig {
    #[serde(rename = "accessToken")]
    pub access_token: Option<String>,
    pub csrf: CsrfToken,
    #[serde(rename = "deviceId")]
    pub device_id: Option<String>,
    #[serde(rename = "sessionId")]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsrfToken {
    pub token: String,
    #[serde(rename = "ts")]
    pub timestamp: String,
    #[serde(rename = "rnd")]
    pub nonce: String,
}

#[derive(Debug, Clone)]
pub struct CachedConfig {
    pub access_token: String,
    pub csrf: CsrfToken,
    pub device_id: String,
    pub session_id: String,
    pub cached_at: Instant,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JsonLdData {
    #[serde(rename = "@type")]
    pub data_type: String,
    pub name: Option<String>,
    pub url: Option<String>,
    #[serde(rename = "byArtist")]
    pub by_artist: Option<JsonLdArtist>,
    pub author: Option<JsonLdArtist>,
    pub image: Option<String>,
    pub duration: Option<String>,
    #[serde(rename = "isrcCode")]
    pub isrc_code: Option<String>,
    pub track: Option<Vec<JsonLdTrack>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum JsonLdArtist {
    Single(JsonLdArtistData),
    Multiple(Vec<JsonLdArtistData>),
}

impl JsonLdArtist {
    pub fn name(&self) -> Option<&str> {
        match self {
            JsonLdArtist::Single(artist) => artist.name.as_deref(),
            JsonLdArtist::Multiple(artists) => artists.first().and_then(|a| a.name.as_deref()),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct JsonLdArtistData {
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JsonLdTrack {
    pub name: String,
    pub url: Option<String>,
    #[serde(rename = "@id")]
    pub id: Option<String>,
    #[serde(rename = "byArtist")]
    pub by_artist: Option<JsonLdArtist>,
    pub author: Option<JsonLdArtist>,
    pub duration: Option<String>,
    #[serde(rename = "isrcCode")]
    pub isrc_code: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SearchResponse {
    pub methods: Option<Vec<SearchMethod>>,
}

#[derive(Debug, Deserialize)]
pub struct SearchMethod {
    pub template: Option<SearchTemplate>,
}

#[derive(Debug, Deserialize)]
pub struct SearchTemplate {
    pub widgets: Option<Vec<SearchWidget>>,
    #[serde(rename = "headerTertiaryText")]
    pub header_tertiary_text: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SearchWidget {
    pub items: Option<Vec<SearchItem>>,
}

#[derive(Debug, Deserialize)]
pub struct SearchItem {
    pub label: Option<String>,
    pub interface: Option<String>,
    #[serde(rename = "primaryText")]
    pub primary_text: Option<ItemText>,
    #[serde(rename = "secondaryText")]
    pub secondary_text: Option<ItemText>,
    #[serde(rename = "primaryLink")]
    pub primary_link: Option<PrimaryLink>,
    pub image: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ItemText {
    String(String),
    Object { text: String },
}

impl ItemText {
    pub fn as_str(&self) -> &str {
        match self {
            ItemText::String(s) => s,
            ItemText::Object { text } => text,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct PrimaryLink {
    pub deeplink: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TrackDurationResponse {
    pub methods: Option<Vec<TrackDurationMethod>>,
}

#[derive(Debug, Deserialize)]
pub struct TrackDurationMethod {
    pub template: Option<TrackDurationTemplate>,
}

#[derive(Debug, Deserialize)]
pub struct TrackDurationTemplate {
    #[serde(rename = "headerTertiaryText")]
    pub header_tertiary_text: Option<String>,
}
