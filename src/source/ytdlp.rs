use crate::models::{ApiTrack, ApiTrackResult};
use crate::util::errors::ResolverError;
use crate::util::source::{Query, Source};
use async_trait::async_trait;
use reqwest::Client;
use songbird::tracks::Track;

pub struct Ytdlp;

#[async_trait]
impl Source for Ytdlp {
    fn get_name(&self) -> &'static str {
        todo!()
    }

    fn get_client(&self) -> Client {
        todo!()
    }

    fn parse_query(&self, url: &str) -> Option<Query> {
        todo!()
    }

    async fn init(&self) -> Result<(), ResolverError> {
        todo!()
    }

    async fn resolve(&self, query: Query) -> Result<Option<ApiTrackResult>, ResolverError> {
        todo!()
    }

    async fn make_playable(&self, track: ApiTrack) -> Result<Track, ResolverError> {
        todo!()
    }
}

impl Ytdlp {
    fn new(_: Option<Client>) -> Self {
        todo!()
    }
}
