use serde::Deserialize;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct CachedToken {
    pub token: String,
    pub origin: Option<String>,
    pub expiry: Option<Instant>,
    pub cached_at: Instant,
}

#[derive(Debug, Deserialize)]
pub struct TokenPayload {
    pub root_https_origin: Option<Vec<String>>,
    pub exp: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ApiResponse<T> {
    pub data: Option<Vec<T>>,
    pub results: Option<SearchResults>,
}

#[derive(Debug, Deserialize)]
pub struct SearchResults {
    pub songs: Option<SongsResult>,
}

#[derive(Debug, Deserialize)]
pub struct SongsResult {
    pub data: Option<Vec<Song>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Song {
    pub id: String,
    pub attributes: Option<SongAttributes>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SongAttributes {
    pub name: String,
    pub artist_name: Option<String>,
    pub duration_in_millis: Option<u64>,
    pub url: Option<String>,
    pub artwork: Option<Artwork>,
    pub isrc: Option<String>,
    pub content_rating: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Artwork {
    pub url: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct Album {
    pub id: String,
    pub attributes: Option<AlbumAttributes>,
    pub relationships: Option<AlbumRelationships>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumAttributes {
    pub name: String,
    pub artwork: Option<Artwork>,
}

#[derive(Debug, Deserialize)]
pub struct AlbumRelationships {
    pub tracks: Option<TracksRelation>,
}

#[derive(Debug, Deserialize)]
pub struct TracksRelation {
    pub data: Option<Vec<Song>>,
    pub meta: Option<RelationMeta>,
}

#[derive(Debug, Deserialize)]
pub struct RelationMeta {
    pub total: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct Playlist {
    pub id: String,
    pub attributes: Option<PlaylistAttributes>,
    pub relationships: Option<PlaylistRelationships>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistAttributes {
    pub name: String,
    pub artwork: Option<Artwork>,
}

#[derive(Debug, Deserialize)]
pub struct PlaylistRelationships {
    pub tracks: Option<TracksRelation>,
}

#[derive(Debug, Deserialize)]
pub struct Artist {
    pub id: String,
    pub attributes: Option<ArtistAttributes>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistAttributes {
    pub name: String,
    pub artwork: Option<Artwork>,
}

