use super::J_SECRET_KEY;
use super::model::{
    JioSaavnAlbumResponse, JioSaavnArtistResponse, JioSaavnPlaylistResponse,
    JioSaavnRecommendationsResponse, JioSaavnSearchResponse, JioSaavnTrack,
    JioSaavnTrackResponse,
};
use crate::models::{ApiTrack, ApiTrackInfo, ApiTrackResult};
use crate::util::encoder::encode_track;
use crate::util::errors::ResolverError;
use crate::util::source::{Query, Source};
use crate::util::url::is_url;
use async_trait::async_trait;
use base64::prelude::*;
use regex::Regex;
use reqwest::Client;
use songbird::input::{HttpRequest, Input};

pub struct JioSaavn {
    client: Client,
    regex: Regex,
    search_prefix: &'static str,
    recommendation_prefix: &'static str,
    api_url: String,
    secret_key: String,
    playlist_track_limit: u32,
    recommendations_track_limit: u32,
}

#[async_trait]
impl Source for JioSaavn {
    fn get_name(&self) -> &'static str {
        "jiosaavn"
    }

    fn get_client(&self) -> Client {
        self.client.clone()
    }

    fn parse_query(&self, query: &str) -> Option<Query> {
        if !is_url(query) {
            if query.starts_with(self.search_prefix) || query.starts_with(self.recommendation_prefix)
            {
                return Some(Query::Search(query.to_string()));
            } else {
                return None;
            }
        }

        self.regex.captures(query)?;

        Some(Query::Url(query.to_string()))
    }

    async fn init(&self) -> Result<(), ResolverError> {
        Ok(())
    }

    async fn resolve(&self, query: Query) -> Result<Option<ApiTrackResult>, ResolverError> {
        match query {
            Query::Url(url) => {
                let captures = self
                    .regex
                    .captures(&url)
                    .ok_or(ResolverError::InvalidUrl)?;
                let url_type = captures
                    .name("type")
                    .map(|m| m.as_str())
                    .ok_or(ResolverError::InvalidUrl)?;

                match url_type {
                    "song" => self.resolve_track(&url).await,
                    "album" => self.resolve_album(&url).await,
                    "playlist" | "featured" => self.resolve_playlist(&url).await,
                    "artist" => self.resolve_artist(&url).await,
                    _ => Ok(None),
                }
            }
            Query::Search(input) => {
                if input.starts_with(self.search_prefix) {
                    let term = input.split_at(self.search_prefix.len()).1;
                    self.search(term).await
                } else if input.starts_with(self.recommendation_prefix) {
                    let identifier = input.split_at(self.recommendation_prefix.len()).1;
                    self.get_recommendations(identifier).await
                } else {
                    Ok(None)
                }
            }
        }
    }

    async fn make_playable(&self, track: ApiTrack) -> Result<Input, ResolverError> {
        let url = self.get_stream_url(&track.info.identifier).await?;
        Ok(Input::from(HttpRequest::new(self.get_client(), url)))
    }
}

impl JioSaavn {
    pub fn new(client: Option<Client>) -> Self {
        use crate::CONFIG;
        
        let config = CONFIG
            .jiosaavn_config
            .as_ref()
            .expect("JioSaavn config is required");
        
        Self {
            client: client.unwrap_or_default(),
            regex: Regex::new(
                r"(?:https?://)?(?:www\.)?jiosaavn\.com/(?<type>song|album|featured|s/playlist|artist)/([^/]+)/(?<identifier>[A-Za-z0-9_,-]+)"
            )
            .expect("Failed to init JioSaavn RegEx"),
            search_prefix: "jssearch:",
            recommendation_prefix: "jsrec:",
            api_url: config.api_url.clone(),
            secret_key: config.secret_key.clone().unwrap_or_else(|| J_SECRET_KEY.to_string()),
            playlist_track_limit: config.playlist_track_limit.unwrap_or(50),
            recommendations_track_limit: config.recommendations_track_limit.unwrap_or(10),
        }
    }

    async fn search(&self, query: &str) -> Result<Option<ApiTrackResult>, ResolverError> {
        let url = format!("{}/search?q={}", self.api_url, urlencoding::encode(query));

        let request = self.client.get(&url).build()?;
        let response = self.client.execute(request).await?;

        if !response.status().is_success() {
            return Ok(None);
        }

        let search_response: JioSaavnSearchResponse = response.json().await?;

        let tracks = match search_response.results {
            Some(results) => results
                .into_iter()
                .map(|track| self.convert_to_api_track(track))
                .collect::<Result<Vec<ApiTrack>, ResolverError>>()?,
            None => return Ok(Some(ApiTrackResult::Empty(None))),
        };

        if tracks.is_empty() {
            return Ok(Some(ApiTrackResult::Empty(None)));
        }

        Ok(Some(ApiTrackResult::Search(tracks)))
    }

    async fn get_recommendations(
        &self,
        identifier: &str,
    ) -> Result<Option<ApiTrackResult>, ResolverError> {
        let url = format!("{}/recommendations?id={}&limit={}", self.api_url, identifier, self.recommendations_track_limit);

        let request = self.client.get(&url).build()?;
        let response = self.client.execute(request).await?;

        if !response.status().is_success() {
            return Ok(None);
        }

        let rec_response: JioSaavnRecommendationsResponse = response.json().await?;

        let tracks = match rec_response.tracks {
            Some(tracks) => tracks
                .into_iter()
                .map(|track| self.convert_to_api_track(track))
                .collect::<Result<Vec<ApiTrack>, ResolverError>>()?,
            None => return Ok(Some(ApiTrackResult::Empty(None))),
        };

        if tracks.is_empty() {
            return Ok(Some(ApiTrackResult::Empty(None)));
        }

        Ok(Some(ApiTrackResult::Search(tracks)))
    }

    async fn resolve_track(&self, url: &str) -> Result<Option<ApiTrackResult>, ResolverError> {
        let api_url = format!("{}/track?url={}", self.api_url, urlencoding::encode(url));

        let request = self.client.get(&api_url).build()?;
        let response = self.client.execute(request).await?;

        if !response.status().is_success() {
            return Ok(None);
        }

        let track_response: JioSaavnTrackResponse = response.json().await?;

        match track_response.track {
            Some(track) => {
                let api_track = self.convert_to_api_track(track)?;
                Ok(Some(ApiTrackResult::Track(api_track)))
            }
            None => Ok(Some(ApiTrackResult::Empty(None))),
        }
    }

    async fn resolve_album(&self, url: &str) -> Result<Option<ApiTrackResult>, ResolverError> {
        let api_url = format!("{}/album?url={}", self.api_url, urlencoding::encode(url));

        let request = self.client.get(&api_url).build()?;
        let response = self.client.execute(request).await?;

        if !response.status().is_success() {
            return Ok(None);
        }

        let album_response: JioSaavnAlbumResponse = response.json().await?;

        match album_response.album {
            Some(album) => {
                let tracks = album
                    .tracks
                    .into_iter()
                    .map(|track| self.convert_to_api_track(track))
                    .collect::<Result<Vec<ApiTrack>, ResolverError>>()?;

                if tracks.is_empty() {
                    return Ok(Some(ApiTrackResult::Empty(None)));
                }

                Ok(Some(ApiTrackResult::Playlist(
                    crate::models::ApiTrackPlaylist {
                        info: crate::models::ApiPlaylistInfo {
                            name: album.name,
                            selected_track: 0,
                        },
                        plugin_info: crate::models::Empty,
                        tracks,
                    },
                )))
            }
            None => Ok(Some(ApiTrackResult::Empty(None))),
        }
    }

    async fn resolve_playlist(&self, url: &str) -> Result<Option<ApiTrackResult>, ResolverError> {
        let api_url = format!("{}/playlist?url={}&limit={}", self.api_url, urlencoding::encode(url), self.playlist_track_limit);

        let request = self.client.get(&api_url).build()?;
        let response = self.client.execute(request).await?;

        if !response.status().is_success() {
            return Ok(None);
        }

        let playlist_response: JioSaavnPlaylistResponse = response.json().await?;

        match playlist_response.playlist {
            Some(playlist) => {
                let tracks = playlist
                    .tracks
                    .into_iter()
                    .map(|track| self.convert_to_api_track(track))
                    .collect::<Result<Vec<ApiTrack>, ResolverError>>()?;

                if tracks.is_empty() {
                    return Ok(Some(ApiTrackResult::Empty(None)));
                }

                Ok(Some(ApiTrackResult::Playlist(
                    crate::models::ApiTrackPlaylist {
                        info: crate::models::ApiPlaylistInfo {
                            name: playlist.title,
                            selected_track: 0,
                        },
                        plugin_info: crate::models::Empty,
                        tracks,
                    },
                )))
            }
            None => Ok(Some(ApiTrackResult::Empty(None))),
        }
    }

    async fn resolve_artist(&self, url: &str) -> Result<Option<ApiTrackResult>, ResolverError> {
        let api_url = format!("{}/artist?url={}", self.api_url, urlencoding::encode(url));

        let request = self.client.get(&api_url).build()?;
        let response = self.client.execute(request).await?;

        if !response.status().is_success() {
            return Ok(None);
        }

        let artist_response: JioSaavnArtistResponse = response.json().await?;

        match artist_response.artist {
            Some(artist) => {
                let tracks = artist
                    .tracks
                    .into_iter()
                    .map(|track| self.convert_to_api_track(track))
                    .collect::<Result<Vec<ApiTrack>, ResolverError>>()?;

                if tracks.is_empty() {
                    return Ok(Some(ApiTrackResult::Empty(None)));
                }

                Ok(Some(ApiTrackResult::Playlist(
                    crate::models::ApiTrackPlaylist {
                        info: crate::models::ApiPlaylistInfo {
                            name: format!("{} - Top Tracks", artist.name),
                            selected_track: 0,
                        },
                        plugin_info: crate::models::Empty,
                        tracks,
                    },
                )))
            }
            None => Ok(Some(ApiTrackResult::Empty(None))),
        }
    }

    async fn get_stream_url(&self, identifier: &str) -> Result<String, ResolverError> {
        let url = format!("{}/track?id={}", self.api_url, identifier);

        let request = self.client.get(&url).build()?;
        let response = self.client.execute(request).await?;

        if !response.status().is_success() {
            return Err(ResolverError::FailedStatusCode(
                response.status().to_string(),
            ));
        }

        let track_response: JioSaavnTrackResponse = response.json().await?;

        let track = track_response
            .track
            .ok_or(ResolverError::MissingRequiredData("track data"))?;
        if let Some(encrypted_url) = track.encrypted_media_url {
            let mut decrypted_url = self.decrypt_url(&encrypted_url)?;
            if decrypted_url.ends_with("_96.mp4") {
                decrypted_url = decrypted_url.replace("_96.mp4", "_320.mp4");
            }
            
            Ok(decrypted_url)
        } else {
            Err(ResolverError::MissingRequiredData("media URL"))
        }
    }

    fn decrypt_url(&self, encrypted_url: &str) -> Result<String, ResolverError> {
        use des::cipher::{BlockDecryptMut, KeyInit};
        use block_padding::Pkcs7;
        type DesEcb = ecb::Decryptor<des::Des>;

        let encrypted_bytes = BASE64_STANDARD
            .decode(encrypted_url)
            .map_err(|e| ResolverError::DecryptionError(e.to_string()))?;

        let key = self.secret_key.as_bytes();
        let cipher = DesEcb::new(key.into());

        let mut buffer = encrypted_bytes.clone();
        let decrypted = cipher
            .decrypt_padded_mut::<Pkcs7>(&mut buffer)
            .map_err(|e| ResolverError::DecryptionError(format!("Decryption failed: {:?}", e)))?;

        String::from_utf8(decrypted.to_vec())
            .map_err(|e| ResolverError::DecryptionError(e.to_string()))
    }

    fn convert_to_api_track(&self, track: JioSaavnTrack) -> Result<ApiTrack, ResolverError> {
        let info = ApiTrackInfo {
            identifier: track.identifier,
            is_seekable: true,
            author: track.author,
            length: track.duration,
            is_stream: false,
            position: 0,
            title: track.title,
            uri: Some(track.uri),
            artwork_url: track.artwork_url,
            isrc: None,
            source_name: self.get_name().to_string(),
        };

        Ok(ApiTrack {
            encoded: encode_track(&info)?,
            info,
            plugin_info: crate::models::Empty,
        })
    }
}

