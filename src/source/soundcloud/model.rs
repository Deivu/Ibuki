use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Track {
    pub id: u64,
    pub title: String,
    pub duration: Option<u64>,
    pub permalink_url: Option<String>,
    pub artwork_url: Option<String>,
    pub user: Option<User>,
    pub media: Option<Media>,
    pub publisher_metadata: Option<PublisherMetadata>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct User {
    pub id: u64,
    pub username: String,
    pub permalink_url: Option<String>,
    pub avatar_url: Option<String>,
    pub followers_count: Option<u32>,
    pub track_count: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Playlist {
    pub id: u64,
    pub title: String,
    pub permalink_url: Option<String>,
    pub artwork_url: Option<String>,
    pub user: Option<User>,
    pub tracks: Option<Vec<TrackOrStub>>,
    pub track_count: Option<u32>,
    pub is_album: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum TrackOrStub {
    Track(Track),
    Stub(TrackStub),
}

#[derive(Debug, Clone, Deserialize)]
pub struct TrackStub {
    pub id: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Media {
    pub transcodings: Vec<Transcoding>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Transcoding {
    pub url: String,
    pub format: TranscodingFormat,
    pub quality: Option<String>,
    pub preset: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TranscodingFormat {
    pub protocol: String,
    pub mime_type: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PublisherMetadata {
    pub isrc: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ResolveResponse {
    pub kind: String,
    #[serde(flatten)]
    pub data: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct SearchResponse {
    pub collection: Vec<serde_json::Value>,
    pub total_results: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct StreamAuthResponse {
    pub url: String,
}
