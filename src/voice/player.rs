use super::events::PlayerEvent;
use crate::CONFIG;
use crate::SCHEDULER;
use crate::models::{ApiPlayer, ApiPlayerState, ApiTrack, ApiVoiceData, Empty, LavalinkFilters};
use crate::util::decoder::decode_base64;
use crate::util::errors::PlayerError;
use crate::ws::client::{SendConnectionMessage, WebSocketClient};
use axum::extract::ws::Message;
use dashmap::DashMap;
use kameo::actor::{ActorRef, WeakActorRef};
use kameo::error::ActorStopReason;
use kameo::message::Context;
use kameo::{Actor, messages};
use songbird::Config as SongbirdConfig;
use songbird::ConnectionInfo;
use songbird::CoreEvent;
use songbird::Driver;
use songbird::Event;
use songbird::TrackEvent;
use songbird::driver::Bitrate;
use songbird::id::{GuildId, UserId};
use songbird::tracks::TrackHandle;
use std::sync::Arc;
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
    pub players: Arc<DashMap<GuildId, ActorRef<Player>>>,
}

pub struct PlayerOptions {
    pub websocket: WeakActorRef<WebSocketClient>,
    pub config: Option<SongbirdConfig>,
    pub user_id: UserId,
    pub guild_id: GuildId,
    pub server_update: ApiVoiceData,
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
    internal: PlayerInternal,
}

impl Actor for Player {
    type Args = PlayerOptions;
    type Error = PlayerError;

    async fn on_start(args: Self::Args, actor_ref: ActorRef<Self>) -> Result<Self, Self::Error> {
        let player = Player::new(args, actor_ref.downgrade()).await?;
        player.internal.players.insert(player.guild_id, actor_ref);
        tracing::debug!("New player task spawned for guild [{}]", player.guild_id);
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
            "Stopped player task for guild [{}]({})",
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
            volume: 1,
            paused: false,
            state: Default::default(),
            voice: options.server_update.clone(),
            filters: Default::default(),
            internal: PlayerInternal {
                actor_ref,
                user_id: options.user_id,
                active: false,
                websocket: options.websocket,
                driver: Default::default(),
                handle: None,
                players: options.players,
            },
        };

        player
            .connect(options.server_update, options.config)
            .await?;

        Ok(player)
    }

    #[message]
    pub fn is_active(&self) -> bool {
        self.internal.active
    }

    #[message]
    pub fn get_api_player_info(&self) -> ApiPlayer {
        self.into()
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
            let config = config.unwrap_or_default().scheduler(SCHEDULER.to_owned());

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
    pub async fn play(&mut self, encoded: String) -> Result<(), PlayerError> {
        let info = decode_base64(&encoded)?;

        let api_track = ApiTrack {
            encoded,
            info,
            plugin_info: Empty,
        };

        let mut track = api_track.make_playable().await?;

        if self.volume as f32 != track.volume {
            track = track.volume(self.volume as f32);
        }

        // todo: before sending the new track, we may want to send a replaced notification from here before playing the new track

        let driver = self
            .internal
            .driver
            .as_mut()
            .ok_or(PlayerError::MissingDriver)?;

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

        self.paused = paused;
    }

    #[message]
    pub async fn set_volume(&mut self, volume: f32) {
        let Some(handle) = self.internal.handle.as_ref() else {
            return;
        };

        if handle.set_volume(volume).is_ok() {
            self.volume = volume as u32;
        }
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
