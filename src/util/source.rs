use super::errors::ResolverError;
use crate::SOURCES;
use crate::models::{ApiTrack, ApiTrackResult};
use async_trait::async_trait;
use reqwest::Client;
use songbird::tracks::Track;

#[macro_export]
macro_rules! register_source {
    ($source_type:ty) => {{
        let instance = Box::new(<$source_type>::new(None));
        instance.init().await.expect("Failed to initialize source");
        let name = instance.get_name().to_string();
        SOURCES.insert(name.clone(), FixAsyncTraitSource(instance));
        tracing::info!("Registered Source: [{}]", name);
    }};
    ($source_type:ty, $($arg:expr),+) => {{
        let instance = Box::new(<$source_type>::new($($arg),+));
        instance.init().await.expect("Failed to initialize source");
        let name = instance.get_name().to_string();
        SOURCES.insert(name.clone(), FixAsyncTraitSource(instance));
        tracing::info!("Registered Source: [{}]", name);
    }};
}

pub enum Query {
    Url(String),
    Search(String),
}

#[async_trait]
pub trait Source: Send + Sync + 'static {
    fn get_name(&self) -> &'static str;
    fn get_client(&self) -> Client;
    fn parse_query(&self, url: &str) -> Option<Query>;
    async fn init(&self) -> Result<(), ResolverError>;
    async fn resolve(&self, query: Query) -> Result<Option<ApiTrackResult>, ResolverError>;
    async fn make_playable(&self, track: ApiTrack) -> Result<Track, ResolverError>;
}

/// http://github.com/dtolnay/async-trait/issues/141
pub struct FixAsyncTraitSource(pub Box<dyn Source + Send + Sync + 'static>);

impl FixAsyncTraitSource {
    pub fn to_inner_ref(&self) -> &Box<dyn Source + Send + Sync> {
        &self.0
    }
}

impl ApiTrack {
    pub async fn make_playable(self) -> Result<Track, ResolverError> {
        let Some(client) = SOURCES.get(&self.info.source_name) else {
            return Err(ResolverError::InvalidSource(self.info.source_name));
        };
        client.to_inner_ref().make_playable(self).await
    }
}
