#![allow(dead_code, unused)]
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct JioSaavnSearchResponse {
    pub results: Option<Vec<JioSaavnTrack>>,
}

#[derive(Debug, Deserialize)]
pub struct JioSaavnTrackResponse {
    pub track: Option<JioSaavnTrack>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JioSaavnTrack {
    pub identifier: String,
    pub title: String,
    pub author: String,
    #[serde(rename = "length")]
    pub duration: u64,
    pub uri: String,
    #[serde(rename = "artworkUrl")]
    pub artwork_url: Option<String>,
    pub encrypted_media_url: Option<String>,
    #[serde(rename = "albumUrl")]
    pub album_url: Option<String>,
    #[serde(rename = "albumName")]
    pub album_name: Option<String>,
    #[serde(rename = "previewUrl")]
    pub preview_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct JioSaavnAlbumResponse {
    pub album: Option<JioSaavnAlbum>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JioSaavnAlbum {
    pub name: String,
    pub url: String,
    pub artwork_url: Option<String>,
    pub total_songs: Option<u32>,
    pub tracks: Vec<JioSaavnTrack>,
}

#[derive(Debug, Deserialize)]
pub struct JioSaavnPlaylistResponse {
    pub playlist: Option<JioSaavnPlaylist>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JioSaavnPlaylist {
    pub title: String,
    pub uri: String,
    pub artwork_url: Option<String>,
    pub total_songs: Option<u32>,
    pub tracks: Vec<JioSaavnTrack>,
}

#[derive(Debug, Deserialize)]
pub struct JioSaavnArtistResponse {
    pub artist: Option<JioSaavnArtist>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JioSaavnArtist {
    pub name: String,
    pub artwork_url: Option<String>,
    pub tracks: Vec<JioSaavnTrack>,
}

#[derive(Debug, Deserialize)]
pub struct JioSaavnRecommendationsResponse {
    pub tracks: Option<Vec<JioSaavnTrack>>,
}

fn clean_html_entities(text: &str) -> String {
    text.replace("&quot;", "\"")
        .replace("&amp;", "&")
        .replace("&#039;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}
