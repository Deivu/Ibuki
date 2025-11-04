use crate::{
    models::{ApiTrack, ApiTrackInfo, ApiTrackResult, Empty},
    util::{
        encoder::encode_base64,
        errors::ResolverError,
        seek::SeekableSource,
        source::{Query, Source},
        url::is_url,
    },
};
use reqwest::Client;
use songbird::{
    input::{AuxMetadata, Compose, HttpRequest, Input, LiveInput},
    tracks::Track,
};
use std::{sync::Arc, time::Duration};

pub struct Http {
    client: Client,
}

impl Source for Http {
    fn new(client: Option<Client>) -> Self {
        Self {
            client: client.unwrap_or_default(),
        }
    }

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

        Some(Query::Url(query.to_string()))
    }

    async fn resolve(&self, query: Query) -> Result<ApiTrackResult, ResolverError> {
        let url = match query {
            Query::Url(url) => url,
            Query::Search(_) => return Err(ResolverError::InputNotSupported),
        };

        let client = self.get_client();
        let response = client.get(url.as_str()).send().await?;

        let content = response
            .headers()
            .get("Content-Type")
            .ok_or(ResolverError::InputNotSupported)?;

        if !content.to_str()?.contains("audio") {
            return Err(ResolverError::InputNotSupported);
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
            encoded: encode_base64(&info)?,
            info,
            plugin_info: Empty,
        };

        Ok(ApiTrackResult::Track(track))
    }

    async fn make_playable(&self, track: ApiTrack) -> Result<Track, ResolverError> {
        let mut request = HttpRequest::new(
            self.get_client(),
            track
                .info
                .uri
                .clone()
                .ok_or(ResolverError::MissingRequiredData("uri"))?,
        );

        let stream = request.create_async().await?;

        let seekable = SeekableSource::new_default(stream.input);

        let input = Input::Live(
            LiveInput::Raw(seekable.into_audio_stream(stream.hint)),
            None,
        );

        Ok(Track::new_with_data(input, Arc::new(track)))
    }
}

impl Http {
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
