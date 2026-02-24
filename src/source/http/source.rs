use crate::models::{ApiTrack, ApiTrackInfo, ApiTrackResult, Empty};
use crate::util::encoder::encode_track;
use crate::util::errors::ResolverError;
use crate::util::seek::SeekableSource;
use crate::util::source::Query;
use crate::util::source::Source;
use crate::util::url::is_url;
use async_trait::async_trait;
use reqwest::Client;
use songbird::input::{AuxMetadata, Compose, HttpRequest, Input, LiveInput};
use songbird::tracks::Track;
use std::sync::Arc;
use std::time::Duration;

pub struct Http {
    client: Client,
}

#[async_trait]
impl Source for Http {
    fn get_name(&self) -> &'static str {
        "http"
    }

    fn get_client(&self) -> Client {
        self.client.clone()
    }

    fn parse_query(&self, query: &str) -> Option<Query> {
        if !is_url(query) {
            return None;
        }

        if query.contains("spotify.com") || query.contains("spotify.app.link") || query.contains("soundcloud.com") {
            return None;
        }

        Some(Query::Url(query.to_string()))
    }

    async fn init(&self) -> Result<(), ResolverError> {
        Ok(())
    }

    async fn resolve(&self, query: Query) -> Result<Option<ApiTrackResult>, ResolverError> {
        let url = match query {
            Query::Url(url) => url,
            Query::Search(_) => return Ok(None),
        };

        let client = self.get_client();
        let response = client.get(url.as_str()).send().await?;

        let Some(content) = response.headers().get("Content-Type") else {
            return Ok(None);
        };

        if !content.to_str()?.contains("audio") {
            return Ok(None);
        }

        let mut request = HttpRequest::new(self.get_client(), url.to_owned());

        let mut metadata = request
            .aux_metadata()
            .await
            .unwrap_or(AuxMetadata::default());

        if metadata.source_url.is_none() {
            let _ = metadata.source_url.insert(url);
        }

        let info = self.make_track(metadata);

        let track = ApiTrack {
            encoded: encode_track(&info)?,
            info,
            plugin_info: Empty,
        };

        Ok(Some(ApiTrackResult::Track(track)))
    }

    async fn make_playable(&self, track: ApiTrack) -> Result<Input, ResolverError> {
        let url = track.info.uri.clone().ok_or(ResolverError::MissingRequiredData("HTTP URI"))?;
        let mut request = HttpRequest::new(self.get_client(), url);
        let stream = request.create_async().await?;

        let seekable = SeekableSource::new_default(stream.input);

        Ok(Input::Live(
            songbird::input::LiveInput::Raw(seekable.into_audio_stream(stream.hint)),
            None,
        ))
    }
}

impl Http {
    pub fn new(client: Option<Client>) -> Self {
        Self {
            client: client.unwrap_or_default(),
        }
    }

    fn make_track(&self, metadata: AuxMetadata) -> ApiTrackInfo {
        let identifier = metadata
            .source_url
            .clone()
            .unwrap_or(String::from("Unknown"));

        let is_seekable = metadata.duration.is_some();
        let author = metadata.artist.unwrap_or(String::from("Unknown"));
        let length = metadata.duration.unwrap_or(Duration::from_millis(u64::MAX));
        let is_stream = length.as_millis() == Duration::from_millis(u64::MAX).as_millis();
        let title = metadata.title.unwrap_or(String::from("Unknown"));

        ApiTrackInfo {
            identifier,
            is_seekable,
            author,
            length: length.as_millis() as u64,
            is_stream,
            position: 0,
            title,
            uri: metadata.source_url,
            artwork_url: metadata.thumbnail,
            isrc: None,
            source_name: self.get_name().into(),
        }
    }
}
