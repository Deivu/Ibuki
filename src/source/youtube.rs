use crate::models::{
    ApiPlaylistInfo, ApiTrack, ApiTrackInfo, ApiTrackPlaylist, ApiTrackResult, Empty,
};
use crate::util::encoder::encode_base64;
use crate::util::errors::ResolverError;
use crate::util::seek::SeekableSource;
use crate::util::source::Query;
use crate::util::source::Source;
use async_trait::async_trait;
use bytesize::ByteSize;
use regex::Regex;
use reqwest::Client;
use rustypipe::client::{ClientType, RustyPipe};
use rustypipe::model::{UrlTarget, VideoPlayer, YouTubeItem};
use rustypipe::param::search_filter::{ItemType, SearchFilter};
use songbird::input::{Compose, HttpRequest, Input, LiveInput};
use songbird::tracks::Track;
use std::fs;
use std::sync::Arc;

static PROTOCOL_REGEX: &str = "(?:http://|https://|)";
static DOMAIN_REGEX: &str = "(?:www\\.|m\\.|music\\.|)youtube\\.com";
static SHORT_DOMAIN_REGEX: &str = "(?:www\\.|)youtu\\.be";

struct RegexList {
    pub main_domain: Regex,
    pub short_hand_domain: Regex,
}

impl RegexList {
    pub fn new() -> Self {
        Self {
            main_domain: Regex::new(format!("^{PROTOCOL_REGEX}{DOMAIN_REGEX}/.*").as_str())
                .expect("Failed to init main domain regex"),
            short_hand_domain: Regex::new(
                format!(
                    "^{PROTOCOL_REGEX}(?:{DOMAIN_REGEX}/(?:live|embed|shorts)|{SHORT_DOMAIN_REGEX})/(?<videoId>.*)"
                )
                .as_str(),
            )
            .expect("Failed to init short hand domain regex"),
        }
    }
}

pub struct Youtube {
    client: Client,
    rusty_pipe: RustyPipe,
    client_types: Vec<ClientType>,
    video_itags: Vec<u32>,
    audio_itags: Vec<u32>,
    regex_list: RegexList,
}

#[async_trait]
impl Source for Youtube {
    fn get_name(&self) -> &'static str {
        "youtube"
    }

    fn get_client(&self) -> Client {
        self.client.clone()
    }

    fn parse_query(&self, query: &str) -> Option<Query> {
        if !self.regex_list.main_domain.is_match(query)
            && !self.regex_list.short_hand_domain.is_match(query)
        {
            if query.starts_with("ytsearch") || query.starts_with("ytmsearch") {
                return Some(Query::Search(query.to_string()));
            } else {
                return None;
            }
        }

        Some(Query::Url(query.to_string()))
    }

    async fn init(&self) -> Result<(), ResolverError> {
        Ok(())
    }

    async fn resolve(&self, query: Query) -> Result<Option<ApiTrackResult>, ResolverError> {
        match query {
            Query::Url(url) => {
                let request = self.rusty_pipe.query().resolve_url(url, true).await?;

                let request_url = request.to_url();

                match request {
                    UrlTarget::Video { id, .. } => {
                        let player = self.fetch_video(&id).await?;

                        let metadata = player.details;

                        let info = ApiTrackInfo {
                            identifier: id.to_owned(),
                            is_seekable: !metadata.is_live,
                            author: metadata.channel_name.unwrap_or(String::from("Unknown")),
                            length: (metadata.duration * 1000) as u64,
                            is_stream: metadata.is_live,
                            position: 0,
                            title: metadata.name.unwrap_or(String::from("Unknown")),
                            uri: Some(request_url),
                            artwork_url: metadata.thumbnail.first().map(|data| data.url.to_owned()),
                            isrc: None,
                            source_name: self.get_name().into(),
                        };

                        let track = ApiTrack {
                            encoded: encode_base64(&info)?,
                            info,
                            plugin_info: Empty,
                        };

                        Ok(Some(ApiTrackResult::Track(track)))
                    }
                    UrlTarget::Channel { .. } => Ok(Some(ApiTrackResult::Empty(None))),
                    UrlTarget::Playlist { id } => {
                        let mut metadata = self.rusty_pipe.query().playlist(&id).await?;

                        let mut playlist = ApiTrackPlaylist {
                            info: ApiPlaylistInfo {
                                name: metadata.name,
                                selected_track: 0,
                            },
                            plugin_info: Empty,
                            tracks: Vec::new(),
                        };

                        metadata
                            .videos
                            .extend_pages(self.rusty_pipe.query(), usize::MAX)
                            .await?;

                        for video in metadata.videos.items {
                            let url = self
                                .rusty_pipe
                                .query()
                                .resolve_string(&video.id, true)
                                .await?;

                            let info = ApiTrackInfo {
                                identifier: video.id,
                                is_seekable: !video.is_live,
                                author: video
                                    .channel
                                    .map(|channel| channel.name)
                                    .unwrap_or(String::from("Unknown")),
                                length: video.duration.unwrap_or(u32::MAX) as u64,
                                is_stream: video.is_live,
                                position: 0,
                                title: video.name,
                                uri: Some(url.to_url()),
                                artwork_url: video
                                    .thumbnail
                                    .first()
                                    .map(|data| data.url.to_owned()),
                                isrc: None,
                                source_name: self.get_name().into(),
                            };

                            let track = ApiTrack {
                                encoded: encode_base64(&info)?,
                                info,
                                plugin_info: Empty,
                            };

                            playlist.tracks.push(track);
                        }

                        Ok(Some(ApiTrackResult::Playlist(playlist)))
                    }
                    UrlTarget::Album { .. } => Ok(Some(ApiTrackResult::Empty(None))),
                }
            }
            Query::Search(input) => {
                let Some(term) = input
                    .strip_prefix("ytsearch")
                    .or(input.strip_prefix("ytmsearch"))
                else {
                    return Ok(None);
                };

                let (prefix, _) = input.split_at(term.len() - input.len());

                match prefix {
                    "ytsearch" => {
                        let filter = SearchFilter::new().item_type(ItemType::Video);

                        let results = self
                            .rusty_pipe
                            .query()
                            .search_filter::<YouTubeItem, _>(term, &filter)
                            .await?;

                        let mut tracks = Vec::new();

                        for result in results.items.items {
                            match result {
                                YouTubeItem::Video(video) => {
                                    let info = ApiTrackInfo {
                                        identifier: video.id.to_owned(),
                                        is_seekable: !video.is_live,
                                        author: video
                                            .channel
                                            .map(|channel| channel.name)
                                            .unwrap_or(String::from("Unknown")),
                                        length: video.duration.unwrap_or(u32::MAX) as u64,
                                        is_stream: video.is_live,
                                        position: 0,
                                        title: video.name,
                                        uri: Some(format!(
                                            "https://www.youtube.com/watch?{}",
                                            video.id
                                        )),
                                        artwork_url: video
                                            .thumbnail
                                            .first()
                                            .map(|data| data.url.to_owned()),
                                        isrc: None,
                                        source_name: self.get_name().into(),
                                    };

                                    let track = ApiTrack {
                                        encoded: encode_base64(&info)?,
                                        info,
                                        plugin_info: Empty,
                                    };

                                    tracks.push(track);
                                }
                                _ => return Err(ResolverError::MissingRequiredData("Video Item")),
                            }
                        }

                        Ok(Some(ApiTrackResult::Search(tracks)))
                    }
                    "ytmsearch" => {
                        let results = self.rusty_pipe.query().music_search_videos(term).await?;

                        let mut tracks = Vec::new();

                        for result in results.items.items {
                            let info = ApiTrackInfo {
                                identifier: result.id.to_owned(),
                                is_seekable: true,
                                author: result
                                    .artists
                                    .first()
                                    .map(|artist| artist.name.to_owned())
                                    .unwrap_or(String::from("Unknown")),
                                length: result.duration.unwrap_or(0) as u64,
                                is_stream: result.duration.map(|_| true).unwrap_or(false),
                                position: 0,
                                title: result.name,
                                uri: Some(format!(
                                    "https://music.youtube.com/watch?v={}",
                                    result.id
                                )),
                                artwork_url: result.cover.first().map(|data| data.url.to_owned()),
                                isrc: None,
                                source_name: self.get_name().into(),
                            };

                            let track = ApiTrack {
                                encoded: encode_base64(&info)?,
                                info,
                                plugin_info: Empty,
                            };

                            tracks.push(track);
                        }

                        Ok(Some(ApiTrackResult::Search(tracks)))
                    }
                    _ => Ok(None),
                }
            }
        }
    }

    async fn make_playable(&self, track: ApiTrack) -> Result<Track, ResolverError> {
        let player = self.fetch_video(&track.info.identifier).await?;

        let audio = player
            .audio_streams
            .iter()
            .filter(|stream| self.audio_itags.contains(&stream.itag))
            .reduce(|prev, current| {
                if prev.bitrate > current.bitrate {
                    prev
                } else {
                    current
                }
            });

        if let Some(stream) = audio {
            tracing::info!(
                "Picked [{}] [{} ({}/s)] for the playback",
                stream.itag,
                stream.mime,
                ByteSize::b(stream.bitrate as u64).display().iec_short()
            );
        }

        let video = player
            .video_streams
            .iter()
            .filter(|stream| self.video_itags.contains(&stream.itag))
            .reduce(|prev, current| {
                if prev.bitrate > current.bitrate {
                    prev
                } else {
                    current
                }
            });

        if let Some(stream) = audio
            && audio.is_none()
        {
            tracing::info!(
                "Picked [{}] [{} ({}/s)] for the playback",
                stream.itag,
                stream.mime,
                ByteSize::b(stream.bitrate as u64).display().iec_short()
            );
        }

        let mut url = audio.map(|stream| &stream.url);

        if url.is_none() {
            url = video.map(|stream| &stream.url);
        }

        let mut request = HttpRequest::new(
            self.get_client(),
            url.ok_or(ResolverError::MissingRequiredData("Stream to Play"))?
                .clone(),
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

impl Youtube {
    pub fn new(client: Option<Client>) -> Self {
        let mut rusty_pipe = RustyPipe::builder().n_http_retries(0);

        if !fs::exists("./rustypipe").unwrap() {
            fs::create_dir("./rustypipe").unwrap();
        }

        if !fs::exists("./rustypipe/botguard").unwrap() {
            fs::create_dir("./rustypipe/botguard").unwrap();
        }

        if fs::exists("./rustypipe/botguard/bin").unwrap() {
            rusty_pipe = rusty_pipe
                .po_token_cache()
                .botguard_bin("./rustypipe/botguard/bin")
                .botguard_snapshot_file("./rustypipe/botguard");

            tracing::info!("Youtube rustypipe-botguard (po_token) is set");
        } else {
            rusty_pipe = rusty_pipe.no_botguard();

            tracing::warn!(
                "The po_token feature was not enabled. The rustypipe-botguard was not found from './rustypipe/botguard' folder but po_cache setting was enabled.
                Please download one that is built for your system from 'https://codeberg.org/ThetaDev/rustypipe-botguard/releases', put it at ./rustypipe/botguard then rename it to 'bin'"
            );
        }

        rusty_pipe = rusty_pipe.storage_dir("./rustypipe/");

        Self {
            client: client.unwrap_or_default(),
            rusty_pipe: rusty_pipe.build().unwrap(),
            client_types: vec![
                ClientType::Mobile,
                ClientType::Tv,
                ClientType::Android,
                ClientType::Ios,
            ],
            video_itags: vec![18, 22, 37, 44, 45, 46],
            audio_itags: vec![140, 141, 171, 250, 251],
            regex_list: RegexList::new(),
        }
    }

    async fn fetch_video(&self, id: &str) -> Result<VideoPlayer, ResolverError> {
        let mut result: Option<VideoPlayer> = None;

        for client in &self.client_types {
            let video = self
                .rusty_pipe
                .query()
                .player_from_client(id, *client)
                .await;

            let client_name = format!("Client [{}]", self.readable_client_type(client));

            tracing::debug!("Trying to resolve [{}] with: ", client_name);

            match video {
                Ok(video) => {
                    if video.audio_streams.is_empty() && video.video_streams.is_empty() {
                        tracing::warn!(
                            "{} failed to get results due to: No streams available",
                            client_name,
                        );
                        continue;
                    }

                    tracing::info!(
                        "{} got results! Formats Count => [Audio: {}]  [Video: {}]",
                        client_name,
                        video.audio_streams.len(),
                        video.video_streams.len()
                    );

                    let _ = result.insert(video);

                    break;
                }
                Err(err) => {
                    tracing::warn!("{} failed to get results due to: {:?}", client_name, err);
                }
            }
        }

        if result.is_none() {
            return Err(ResolverError::MissingRequiredData("No data available"));
        }

        Ok(result.unwrap())
    }

    pub fn readable_client_type(&self, client: &ClientType) -> &'static str {
        match client {
            ClientType::Desktop => "Desktop",
            ClientType::DesktopMusic => "Desktop Music",
            ClientType::Mobile => "Mobile",
            ClientType::Tv => "TV",
            ClientType::Android => "Android",
            ClientType::Ios => "IOS",
            _ => "Unknown",
        }
    }
}
