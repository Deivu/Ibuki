use super::errors::ResolverError;
use crate::{
    AvailableSources,
    models::{ApiTrack, ApiTrackResult},
    source::{deezer::source::Deezer, http::Http, youtube::Youtube},
};
use reqwest::Client;
use songbird::tracks::Track;

pub enum Query {
    Url(String),
    Search(String),
}

pub enum Sources {
    Youtube(Youtube),
    Deezer(Deezer),
    Http(Http),
}

pub trait Source {
    fn new(client: Option<Client>) -> Self;

    fn get_name(&self) -> &'static str;

    fn get_client(&self) -> Client;

    fn parse_query(&self, url: &str) -> Option<Query>;

    async fn resolve(&self, query: Query) -> Result<ApiTrackResult, ResolverError>;

    async fn make_playable(&self, track: ApiTrack) -> Result<Track, ResolverError>;
}

impl ApiTrack {
    pub async fn make_playable(self) -> Result<Track, ResolverError> {
        let Some(client) = AvailableSources.get(&self.info.source_name) else {
            return Err(ResolverError::InputNotSupported);
        };

        match &*client {
            Sources::Youtube(src) => src.make_playable(self).await,
            Sources::Deezer(src) => src.make_playable(self).await,
            Sources::Http(src) => src.make_playable(self).await,
        }
    }
}
