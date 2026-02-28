use super::events::PlayerEvent;
use crate::CONFIG;
use crate::SCHEDULER;
use crate::filters::processor::FilterChain;
use crate::filters::source::{FilteredCompose, FilteredSource};
use crate::models::{ApiPlayer, ApiPlayerState, ApiTrack, ApiVoiceData, Empty, LavalinkFilters};
use crate::util::decoder::{decode_base64, decode_track};
use crate::util::errors::PlayerError;
use crate::util::frame_counter::FrameCounter;
use crate::ws::client::{SendConnectionMessage, WebSocketClient};
use axum::extract::ws::Message;
use dashmap::DashMap;
use kameo::actor::{ActorRef, WeakActorRef};
use kameo::error::ActorStopReason;
use kameo::message::Context;
use kameo::{Actor, messages};
use serde_json::Value;
use songbird::Config as SongbirdConfig;
use songbird::ConnectionInfo;
use songbird::CoreEvent;
use songbird::Driver;
use songbird::Event;
use songbird::TrackEvent;
use songbird::driver::Bitrate;
use songbird::id::{GuildId, UserId};
use songbird::input::{AudioStream, File, Input, LiveInput};
use songbird::tracks::{Track, TrackHandle};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug)]
pub enum PlayerUpdate {
    GuildId(GuildId),
    Track(Option<ApiTrack>),
    Volume(u32),
    Position(u32),
    Connected(bool),
    Paused(bool),
    Active(bool),
}

#[derive(Debug)]
struct PlayerInternal {
    pub actor_ref: WeakActorRef<Player>,
    pub user_id: UserId,
    pub active: bool,
    pub websocket: WeakActorRef<WebSocketClient>,
    pub driver: Option<Driver>,
    pub handle: Option<TrackHandle>,
    pub end_time_task: Option<tokio::task::JoinHandle<()>>,
    pub players: Arc<DashMap<GuildId, ActorRef<Player>>>,
}

pub struct PlayerOptions {
    pub websocket: WeakActorRef<WebSocketClient>,
    pub config: Option<SongbirdConfig>,
    pub user_id: UserId,
    pub guild_id: GuildId,
    pub server_update: Option<ApiVoiceData>,
    pub players: Arc<DashMap<GuildId, ActorRef<Player>>>,
}
pub struct Player {
    pub guild_id: GuildId,
    pub track: Option<ApiTrack>,
    pub volume: u32,
    pub paused: bool,
    pub state: ApiPlayerState,
    pub voice: ApiVoiceData,
    pub filters: LavalinkFilters,
    pub filter_chain: Arc<Mutex<FilterChain>>,
    pub frame_counter: Arc<FrameCounter>,
    internal: PlayerInternal,
}

impl Actor for Player {
    type Args = PlayerOptions;
    type Error = PlayerError;

    async fn on_start(args: Self::Args, actor_ref: ActorRef<Self>) -> Result<Self, Self::Error> {
        let player = Player::new(args, actor_ref.downgrade()).await?;
        tracing::debug!("New Player Task spawned for GuildId: [{}]", player.guild_id);
        Ok(player)
    }

    async fn on_stop(
        &mut self,
        _: WeakActorRef<Self>,
        reason: ActorStopReason,
    ) -> Result<(), Self::Error> {
        if let Some(driver) = self.internal.driver.take().as_mut() {
            driver.stop();
            driver.leave();
        }
        self.state.connected = false;
        self.internal.active = false;
        self.internal.players.remove(&self.guild_id);
        tracing::debug!(
            "Stopped and cleaned up the Player Task for GuildId: [{}] Reason: [{}]",
            self.guild_id,
            reason
        );
        Ok(())
    }
}

impl From<&Player> for ApiPlayer {
    fn from(player: &Player) -> Self {
        Self {
            guild_id: player.guild_id.0.get(),
            track: player.track.clone(),
            volume: player.volume,
            paused: player.paused,
            state: player.state.clone(),
            voice: player.voice.clone(),
            filters: player.filters.clone(),
        }
    }
}

#[messages]
impl Player {
    pub async fn new(
        options: PlayerOptions,
        actor_ref: WeakActorRef<Player>,
    ) -> Result<Self, PlayerError> {
        let mut player = Player {
            guild_id: options.guild_id,
            track: None,
            volume: 80,
            paused: false,
            state: Default::default(),
            voice: options.server_update.clone().unwrap_or_default(),
            filters: Default::default(),
            filter_chain: Arc::new(Mutex::new(FilterChain::new(48000))),
            frame_counter: Arc::new(FrameCounter::new()),
            internal: PlayerInternal {
                actor_ref,
                user_id: options.user_id,
                active: false,
                websocket: options.websocket,
                driver: Default::default(),
                handle: None,
                end_time_task: None,
                players: options.players,
            },
        };

        if let Some(server_update) = options.server_update {
            player.connect(server_update, options.config).await?;
        }

        Ok(player)
    }

    #[message]
    pub fn is_active(&self) -> bool {
        self.internal.active
    }

    #[message]
    pub fn get_frame_counter(&self) -> Arc<FrameCounter> {
        self.frame_counter.clone()
    }

    #[message]
    pub fn get_api_player_info(&self) -> ApiPlayer {
        self.into()
    }

    #[message]
    pub fn set_end_time_task(&mut self, task: Option<tokio::task::JoinHandle<()>>) {
        if let Some(old_task) = self.internal.end_time_task.take() {
            old_task.abort();
        }
        self.internal.end_time_task = task;
    }

    #[message]
    pub async fn get_track_handle(&self) -> Option<TrackHandle> {
        self.internal.handle.clone()
    }

    #[message]
    pub async fn get_driver(&self) -> Option<Driver> {
        self.internal.driver.clone()
    }

    #[message]
    pub async fn connect(
        &mut self,
        server_update: ApiVoiceData,
        config: Option<SongbirdConfig>,
    ) -> Result<(), PlayerError> {
        let connection = ConnectionInfo {
            channel_id: None,
            endpoint: server_update.endpoint.to_owned(),
            guild_id: self.guild_id,
            session_id: server_update.session_id.to_owned(),
            token: server_update.token.to_owned(),
            user_id: self.internal.user_id,
        };

        let Some(driver) = self.internal.driver.as_mut() else {
            let config = config
                .unwrap_or_default()
                .scheduler(SCHEDULER.to_owned())
                .use_softclip(false);

            let mut driver = Driver::new(config.clone());

            driver.set_bitrate(Bitrate::Max);

            driver.add_global_event(
                Event::Core(CoreEvent::DriverDisconnect),
                PlayerEvent::new(
                    Event::Core(CoreEvent::DriverDisconnect),
                    self.guild_id,
                    self.internal.user_id,
                    self.internal.actor_ref.clone(),
                ),
            );

            driver.add_global_event(
                Event::Periodic(
                    Duration::from_secs(CONFIG.player_update_secs.unwrap_or(5) as u64),
                    None,
                ),
                PlayerEvent::new(
                    Event::Periodic(Duration::from_secs(10), None),
                    self.guild_id,
                    self.internal.user_id,
                    self.internal.actor_ref.clone(),
                ),
            );

            let _ = self.internal.driver.insert(driver);

            return Box::pin(self.connect(server_update, Some(config))).await;
        };

        driver.connect(connection).await?;

        self.state.connected = true;
        self.voice = server_update.clone();

        if let Some(api_track) = self.track.clone() {
            tracing::debug!(
                "Playing queued track after connection for GuildId: [{}]",
                self.guild_id
            );

            let track_data = Arc::new(api_track.clone());
            let input = api_track.make_playable().await?;
            let input = Self::apply_filters(&self.filter_chain, self.guild_id, input);

            let volume_f32 = self.volume as f32 / 100.0;
            let track = Track::new_with_data(input, track_data).volume(volume_f32);

            let track_handle = driver.play_only(track);

            track_handle.add_event(
                Event::Track(TrackEvent::Play),
                PlayerEvent::new(
                    Event::Track(TrackEvent::Play),
                    self.guild_id,
                    self.internal.user_id,
                    self.internal.actor_ref.clone(),
                ),
            )?;

            track_handle.add_event(
                Event::Track(TrackEvent::Pause),
                PlayerEvent::new(
                    Event::Track(TrackEvent::Pause),
                    self.guild_id,
                    self.internal.user_id,
                    self.internal.actor_ref.clone(),
                ),
            )?;

            track_handle.add_event(
                Event::Track(TrackEvent::Playable),
                PlayerEvent::new(
                    Event::Track(TrackEvent::Playable),
                    self.guild_id,
                    self.internal.user_id,
                    self.internal.actor_ref.clone(),
                ),
            )?;

            track_handle.add_event(
                Event::Track(TrackEvent::End),
                PlayerEvent::new(
                    Event::Track(TrackEvent::End),
                    self.guild_id,
                    self.internal.user_id,
                    self.internal.actor_ref.clone(),
                ),
            )?;

            let _ = self.internal.handle.insert(track_handle);
        }

        Ok(())
    }

    #[message]
    pub async fn disconnect(&mut self) {
        if let Some(driver) = self.internal.driver.take().as_mut() {
            driver.stop();
            driver.leave();
        }
        self.state.connected = false;
        self.internal.active = false;
    }

    #[message(ctx)]
    pub async fn destroy(&mut self, ctx: &mut Context<Self, ()>) {
        ctx.stop();
    }

    #[message]
    pub async fn play(
        &mut self,
        encoded: String,
        user_data: Option<Value>,
    ) -> Result<(), PlayerError> {
        let info = decode_track(&encoded).or_else(|_| decode_base64(&encoded))?;

        let api_track = ApiTrack {
            encoded,
            info,
            plugin_info: Empty,
            user_data,
        };
        self.track = Some(api_track.clone());

        // If no driver yet (disconnected player), just queue the track
        let Some(driver) = self.internal.driver.as_mut() else {
            tracing::debug!(
                "No driver yet, track queued for GuildId: [{}]",
                self.guild_id
            );
            return Ok(());
        };

        // We have a driver, play the track
        let track_data = Arc::new(api_track.clone());
        let input = api_track.make_playable().await?;
        let input = Self::apply_filters(&self.filter_chain, self.guild_id, input);

        let volume_f32 = self.volume as f32 / 100.0;
        let track = Track::new_with_data(input, track_data).volume(volume_f32);

        // todo: before sending the new track, we may want to send a replaced notification from here before playing the new track

        let track_handle = driver.play_only(track);

        self.frame_counter.on_track_start();

        track_handle.add_event(
            Event::Track(TrackEvent::Play),
            PlayerEvent::new(
                Event::Track(TrackEvent::Play),
                self.guild_id,
                self.internal.user_id,
                self.internal.actor_ref.clone(),
            ),
        )?;

        track_handle.add_event(
            Event::Track(TrackEvent::Pause),
            PlayerEvent::new(
                Event::Track(TrackEvent::Pause),
                self.guild_id,
                self.internal.user_id,
                self.internal.actor_ref.clone(),
            ),
        )?;

        track_handle.add_event(
            Event::Track(TrackEvent::Error),
            PlayerEvent::new(
                Event::Track(TrackEvent::Error),
                self.guild_id,
                self.internal.user_id,
                self.internal.actor_ref.clone(),
            ),
        )?;

        track_handle.add_event(
            Event::Track(TrackEvent::Playable),
            PlayerEvent::new(
                Event::Track(TrackEvent::Playable),
                self.guild_id,
                self.internal.user_id,
                self.internal.actor_ref.clone(),
            ),
        )?;

        track_handle.add_event(
            Event::Track(TrackEvent::End),
            PlayerEvent::new(
                Event::Track(TrackEvent::End),
                self.guild_id,
                self.internal.user_id,
                self.internal.actor_ref.clone(),
            ),
        )?;

        let _ = self.internal.handle.insert(track_handle);

        Ok(())
    }

    #[message]
    pub async fn stop(&self) {
        let Some(handle) = self.internal.handle.as_ref() else {
            return;
        };
        handle.stop().ok();
    }

    #[message]
    pub async fn seek(&mut self, position: u32) {
        if self.state.position == position
            || self
                .track
                .as_ref()
                .is_some_and(|track| track.info.length < position as u64)
        {
            return;
        }

        let Some(handle) = self.internal.handle.as_ref() else {
            return;
        };

        let result = handle
            .seek_async(Duration::from_millis(position as u64))
            .await;

        if result.is_ok() {
            self.state.position = position;
        }
    }

    #[message]
    pub async fn pause(&mut self, pause: bool) {
        let paused = self.paused;

        if paused == pause {
            return;
        }

        let Some(handle) = self.internal.handle.as_ref() else {
            return;
        };

        let result = match paused {
            true => handle.play(),
            false => handle.pause(),
        };

        if !result.is_ok() {
            return;
        }

        self.paused = pause;
    }

    #[message]
    pub async fn set_volume(&mut self, volume: f32) {
        let Some(handle) = self.internal.handle.as_ref() else {
            tracing::debug!(
                "Cannot set volume for GuildId [{}]: no active track handle",
                self.guild_id
            );
            return;
        };

        let volume_f32 = volume / 100.0;
        match handle.set_volume(volume_f32) {
            Ok(_) => {
                self.volume = volume as u32;
                tracing::debug!(
                    "Volume set to {} (raw {}) for GuildId: [{}]",
                    volume_f32,
                    volume,
                    self.guild_id
                );
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to set volume for GuildId [{}]: {:?}",
                    self.guild_id,
                    e
                );
            }
        }
    }

    fn apply_filters(
        filter_chain: &Arc<Mutex<FilterChain>>,
        guild_id: GuildId,
        input: Input,
    ) -> Input {
        match input {
            Input::Lazy(compose) => {
                tracing::debug!(
                    "Wrapping lazy input with FilteredCompose for GuildId [{guild_id}]"
                );
                Input::Lazy(Box::new(FilteredCompose::new(
                    compose,
                    filter_chain.clone(),
                    48000,
                    2,
                )))
            }
            Input::Live(LiveInput::Raw(stream), data) => {
                tracing::debug!(
                    "Live raw input detected for GuildId [{guild_id}]. Attempting filter wrap..."
                );

                let hint = stream.hint.unwrap_or_default();
                match FilteredSource::new(stream.input, hint, filter_chain.clone(), 48000, 2) {
                    Ok(filtered) => {
                        let out = AudioStream {
                            input: Box::new(filtered) as Box<dyn symphonia::core::io::MediaSource>,
                            hint: Some({
                                let mut h = symphonia::core::probe::Hint::new();
                                h.with_extension("wav");
                                h
                            }),
                        };
                        Input::Live(LiveInput::Raw(out), data)
                    }
                    Err(e) => {
                        tracing::error!(
                            "FilteredSource creation failed for GuildId [{guild_id}]: {e}. \
                             Track will not play."
                        );
                        Input::Lazy(Box::new(File::new("__filter_error_unsupported_codec__")))
                    }
                }
            }
            other => {
                tracing::debug!(
                    "Input type cannot be filtered for GuildId [{guild_id}], playing unfiltered"
                );
                other
            }
        }
    }

    #[message]
    pub async fn set_filters(&mut self, filters: LavalinkFilters) -> Result<(), PlayerError> {
        {
            let mut chain = self
                .filter_chain
                .lock()
                .map_err(|e| PlayerError::FailedMessage(format!("Filter lock error: {}", e)))?;
            chain
                .update_from_config(&filters)
                .map_err(|e| PlayerError::FailedMessage(format!("Filter error: {}", e)))?;
        }
        self.filters = filters;

        tracing::debug!(
            "Filters updated for GuildId: [{}], active: {}",
            self.guild_id,
            self.filter_chain
                .lock()
                .map(|c| c.has_active_filters())
                .unwrap_or(false)
        );

        Ok(())
    }

    #[message]
    pub fn update_from_internal_event(&mut self, updates: Vec<PlayerUpdate>) {
        for update in updates {
            match update {
                PlayerUpdate::GuildId(id) => self.guild_id = id,
                PlayerUpdate::Track(track) => self.track = track,
                PlayerUpdate::Volume(vol) => self.volume = vol,
                PlayerUpdate::Position(pos) => self.state.position = pos,
                PlayerUpdate::Connected(connected) => self.state.connected = connected,
                PlayerUpdate::Paused(paused) => self.paused = paused,
                PlayerUpdate::Active(active) => self.internal.active = active,
            }
        }
    }

    #[message]
    pub async fn send_to_player_websocket(&self, message: Message) {
        let Some(actor_ref) = self.internal.websocket.upgrade() else {
            return;
        };
        let Err(error) = actor_ref.ask(SendConnectionMessage { message }).await else {
            return;
        };
        tracing::warn!(
            "Player with GuildId [{}] failed to send message to websocket ({})",
            self.guild_id,
            error
        );
    }
}
