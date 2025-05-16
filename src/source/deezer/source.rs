use std::time::Duration;
use std::{str::FromStr, sync::Arc};

use regex::Regex;
use reqwest::{Body, Client, Url};
use songbird::input::{Compose, HttpRequest, Input, LiveInput};
use songbird::tracks::Track;
use tokio::sync::Mutex;
use tokio::time::Instant;

use crate::util::encoder::encode_base64;
use crate::{
    models::{ApiTrack, ApiTrackInfo, ApiTrackResult},
    util::{errors::ResolverError, source::Source},
};

use super::model::{
    DeezerApiTrack, DeezerData, DeezerGetMedia, DeezerGetUrlBody, DeezerGetUrlMedia,
    DeezerMakePlayableBody, DeezerQuality, DeezerQualityFormat, InternalDeezerGetUserData,
    InternalDeezerResponse, InternalDeezerSongData,
};
use super::stream::DeezerHttpStream;
use super::{ARL, MEDIA_BASE, PUBLIC_API_BASE};
use super::{PRIVATE_API_BASE, SECRET_KEY, model::Tokens};

pub struct Deezer {
    client: Client,
    tokens: Arc<Mutex<Option<Tokens>>>,
    regex: Regex,
    search_prefixes: (&'static str, &'static str, &'static str),
}

impl Source for Deezer {
    fn new(client: Option<Client>) -> Self {
        Self {
            client: client.unwrap_or_default(),
            tokens: Arc::new(Mutex::new(None)),
            regex: Regex::new("(https?://)?(www\\.)?deezer\\.com/(?<countrycode>[a-zA-Z]{2}/)?(?<type>track|album|playlist|artist)/(?<identifier>[0-9]+)").expect("Failed to init RegEx"),
            search_prefixes: ("dzsearch:", "dzisrc:", "dzrec:"),
        }
    }

    fn get_name(&self) -> &'static str {
        "deezer"
    }

    fn get_client(&self) -> Client {
        self.client.clone()
    }

    async fn valid_url(&self, url: &str) -> bool {
        Url::from_str(url).ok().is_some() && self.regex.captures(url).is_some()
    }

    async fn try_search(&self, query: &str) -> bool {
        !query.starts_with(self.search_prefixes.0)
            && !query.starts_with(self.search_prefixes.1)
            && !query.starts_with(self.search_prefixes.2)
    }

    async fn search(&self, query: &str) -> Result<ApiTrackResult, ResolverError> {
        let mut data: Option<Vec<DeezerApiTrack>> = None;

        if query.starts_with(self.search_prefixes.0) {
            let term = query.split_at(self.search_prefixes.0.len()).1;

            let query = [("q", term)];

            let request = self
                .client
                .get(format!("{PUBLIC_API_BASE}/search"))
                .query(&query)
                .build()?;

            let response = self.client.execute(request).await?;

            if !response.status().is_success() {
                return Ok(ApiTrackResult::Empty(None));
            }

            let tracks = response.json::<DeezerData<Vec<DeezerApiTrack>>>().await?;

            let _ = data.insert(tracks.data);
        } else if query.starts_with(self.search_prefixes.1) {
            let isrc = query.split_at(self.search_prefixes.1.len()).1;

            let request = self
                .client
                .get(format!("{PUBLIC_API_BASE}/track/isrc:{isrc}"))
                .query(&query)
                .build()?;

            let response = self.client.execute(request).await?;

            if !response.status().is_success() {
                return Ok(ApiTrackResult::Empty(None));
            }

            let _ = data.insert(vec![response.json::<DeezerApiTrack>().await?]);
        }

        let Some(api_tracks) = data else {
            return Ok(ApiTrackResult::Empty(None));
        };

        let tracks = api_tracks
            .iter()
            .filter(|deezer_api_track| deezer_api_track.readable)
            .map(|deezer_api_track| {
                let info = ApiTrackInfo {
                    identifier: deezer_api_track.id.to_string(),
                    is_seekable: true,
                    author: deezer_api_track.artist.name.clone(),
                    length: (deezer_api_track.duration * 1000) as u64,
                    is_stream: false,
                    position: 0,
                    title: deezer_api_track.title.clone(),
                    uri: Some(deezer_api_track.link.clone()),
                    artwork_url: Some(deezer_api_track.album.thumbnail.clone()),
                    isrc: deezer_api_track.isrc.clone(),
                    source_name: self.get_name().to_string(),
                };

                ApiTrack {
                    encoded: encode_base64(&info).unwrap(),
                    info,
                    plugin_info: crate::models::Empty,
                }
            })
            .collect::<Vec<ApiTrack>>();

        Ok(ApiTrackResult::Search(tracks))
    }

    async fn resolve(&self, _url: &str) -> Result<ApiTrackResult, ResolverError> {
        todo!()
    }

    async fn make_playable(&self, track: ApiTrack) -> Result<Track, ResolverError> {
        let tokens = self.get_token().await?;

        let response = {
            let query = [
                ("method", "song.getData"),
                ("input", "3"),
                ("api_version", "1.0"),
                ("api_token", tokens.check_form.as_str()),
            ];

            let body = DeezerMakePlayableBody {
                sng_id: track.info.identifier.clone(),
            };

            let request = self
                .client
                .post(PRIVATE_API_BASE)
                .header("Cookie", tokens.create_cookie())
                .body(Body::from(serde_json::to_string(&body)?))
                .query(&query)
                .build()?;

            self.client.execute(request).await?
        };

        if !response.status().is_success() {
            return Err(ResolverError::FailedStatusCode(
                response.status().to_string(),
            ));
        }

        let response = {
            let data = response.json::<InternalDeezerSongData>().await?;

            let format = DeezerQualityFormat::new(&data, Some(DeezerQuality::Flac));

            let body = DeezerGetUrlBody {
                license_token: tokens.license_token.clone(),
                media: vec![DeezerGetUrlMedia {
                    media_type: String::from("FULL"),
                    formats: vec![format],
                }],
                track_tokens: vec![data.track_token],
            };

            let request = self
                .client
                .post(format!("{MEDIA_BASE}/get_url"))
                .header("Cookie", tokens.create_cookie())
                .body(Body::from(serde_json::to_string(&body)?))
                .build()?;

            self.client.execute(request).await?
        };

        if !response.status().is_success() {
            return Err(ResolverError::FailedStatusCode(
                response.status().to_string(),
            ));
        }

        let json = response.json::<DeezerGetMedia>().await?;

        let data = json
            .data
            .ok_or(ResolverError::MissingRequiredData("media.data"))?;

        let media = data
            .first()
            .ok_or(ResolverError::MissingRequiredData("media.data.first()"))?
            .media
            .first()
            .ok_or(ResolverError::MissingRequiredData(
                "media.data.first().media.first()",
            ))?
            .sources
            .first()
            .ok_or(ResolverError::MissingRequiredData(
                "media.data.first().media.first().sources.first()",
            ))?;

        let mut stream = DeezerHttpStream::new(
            HttpRequest::new(self.get_client(), media.url.clone()),
            self.get_track_key(track.info.identifier.clone()),
        );

        let input = Input::Live(LiveInput::Raw(stream.create_async().await?), None);

        Ok(Track::new_with_data(input, Arc::new(track)))
    }
}

impl Deezer {
    pub async fn init(&self) {
        self.get_token().await.unwrap();
    }

    fn get_track_key(&self, id: String) -> [u8; 16] {
        let md5 = hex::encode(md5::compute(id).0);
        let hash = md5.as_bytes();

        let mut key: [u8; 16] = [0; 16];

        for i in 0..16 {
            key[i] = hash[i] ^ hash[i + 16] ^ SECRET_KEY[i];
        }

        key
    }

    async fn get_token(&self) -> Result<Tokens, ResolverError> {
        let mut guard = self.tokens.lock().await;

        if let Some(token) = guard.as_ref() {
            if Instant::now().duration_since(token.expire_at).as_secs() > 3600 {
                return Ok(token.clone());
            }
        }

        let query = [
            ("method", "deezer.getUserData"),
            ("input", "3"),
            ("api_version", "1.0"),
            ("api_token", ""),
        ];

        let request = self
            .client
            .post(PRIVATE_API_BASE)
            .header("Content-Length", "0")
            .header("Cookie", format!("arl={ARL}"))
            .query(&query)
            .build()?;

        let response = self.client.execute(request).await?;

        if !response.status().is_success() {
            return Err(ResolverError::FailedStatusCode(
                response.status().to_string(),
            ));
        }

        let headers = response
            .headers()
            .get_all("Set-Cookie")
            .iter()
            .map(|header| header.to_str())
            .filter(|header| header.is_ok())
            .map(|header| header.unwrap().to_string())
            .collect::<Vec<String>>();

        let session_id = headers.iter().find(|str| str.starts_with("sid=")).ok_or(
            ResolverError::MissingRequiredData("Missing Deezer Session Id"),
        )?;

        let unique_id = headers
            .iter()
            .find(|str| str.starts_with("dzr_uniq_id="))
            .ok_or(ResolverError::MissingRequiredData(
                "Missing Deezer Unique Id",
            ))?;

        let data = response
            .json::<InternalDeezerResponse<InternalDeezerGetUserData>>()
            .await?;

        let tokens = Tokens {
            session_id: (*session_id).to_string(),
            unique_id: (*unique_id).to_string(),
            check_form: data.results.check_form,
            license_token: data.results.user.options.license_token,
            expire_at: Instant::now()
                .checked_add(Duration::from_secs(3600))
                .ok_or(ResolverError::MissingRequiredData("Invalid Expire At"))?,
        };

        let _ = guard.insert(tokens.clone());

        Ok(tokens)
    }
}
