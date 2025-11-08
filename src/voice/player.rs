use super::events::PlayerEvent;
use super::manager::CleanerSender;
use crate::CONFIG;
use crate::SCHEDULER;
use crate::models::{ApiPlayer, ApiPlayerState, ApiTrack, ApiVoiceData, Empty};
use crate::util::decoder::decode_base64;
use crate::util::errors::PlayerError;
use crate::ws::client::WebSocketClient;
use flume::WeakSender;
use kameo::actor::ActorRef;
use songbird::Config as SongbirdConfig;
use songbird::ConnectionInfo;
use songbird::CoreEvent;
use songbird::Driver;
use songbird::Event;
use songbird::TrackEvent;
use songbird::driver::Bitrate;
use songbird::id::{GuildId, UserId};
use songbird::tracks::{TrackHandle, TrackState};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio::task;

#[derive(Clone)]
pub struct Player {
    pub user_id: UserId,
    pub guild_id: GuildId,
    pub active: Arc<AtomicBool>,
    pub data: Arc<Mutex<ApiPlayer>>,
    pub websocket: ActorRef<WebSocketClient>,
    pub cleaner: WeakSender<CleanerSender>,
    pub driver: Arc<Mutex<Option<Driver>>>,
    pub handle: Arc<Mutex<Option<TrackHandle>>>,
}

impl Player {
    pub async fn new(
        websocket: ActorRef<WebSocketClient>,
        cleaner: WeakSender<CleanerSender>,
        config: Option<SongbirdConfig>,
        user_id: UserId,
        guild_id: GuildId,
        server_update: ApiVoiceData,
    ) -> Result<Self, PlayerError> {
        let data = ApiPlayer {
            guild_id: guild_id.0.get(),
            track: None,
            volume: 1,
            paused: false,
            state: ApiPlayerState {
                // todo: fix this
                time: Instant::now().elapsed().as_secs(),
                position: 0,
                connected: false,
                ping: None,
            },
            voice: server_update.clone(),
            filters: Empty,
        };

        let active = Arc::new(AtomicBool::new(false));
        let data = Arc::new(Mutex::new(data));

        let player = Player {
            user_id,
            guild_id,
            active,
            data,
            websocket,
            cleaner,
            driver: Arc::new(Mutex::new(None)),
            handle: Arc::new(Mutex::new(None)),
        };

        player.connect(&server_update, config).await?;

        Ok(player)
    }

    pub async fn get_raw_state(&self) -> Option<TrackState> {
        let lock = self.handle.lock().await;

        let handle = lock.clone()?;

        drop(lock);

        let state = handle.get_info().await.ok()?;

        Some(state.clone())
    }

    pub async fn connect(
        &self,
        server_update: &ApiVoiceData,
        config: Option<SongbirdConfig>,
    ) -> Result<(), PlayerError> {
        let connection = ConnectionInfo {
            channel_id: None,
            endpoint: server_update.endpoint.to_owned(),
            guild_id: self.guild_id,
            session_id: server_update.session_id.to_owned(),
            token: server_update.token.to_owned(),
            user_id: self.user_id,
        };

        let mut guard = self.driver.lock().await;

        if guard.is_none() {
            let config = config.unwrap_or_default().scheduler(SCHEDULER.to_owned());

            let mut driver = Driver::new(config.clone());

            driver.set_bitrate(Bitrate::Max);

            driver.add_global_event(
                Event::Core(CoreEvent::DriverDisconnect),
                PlayerEvent::new(Event::Core(CoreEvent::DriverDisconnect), self),
            );

            driver.add_global_event(
                Event::Periodic(
                    Duration::from_secs(CONFIG.player_update_secs.unwrap_or(5) as u64),
                    None,
                ),
                PlayerEvent::new(Event::Periodic(Duration::from_secs(10), None), self),
            );

            let _ = guard.insert(driver);

            drop(guard);

            return Box::pin(self.connect(server_update, Some(config))).await;
        }

        let driver = guard.as_mut().ok_or(PlayerError::MissingDriver)?;

        driver.connect(connection).await?;

        drop(guard);

        let mut guard = self.data.lock().await;

        guard.state.connected = true;
        guard.voice = server_update.clone();

        Ok(())
    }

    pub async fn disconnect(&self) {
        let mut guard = self.driver.lock().await;

        if let Some(driver) = guard.take().as_mut() {
            driver.stop();
            driver.leave();
        }

        drop(guard);

        let mut guard = self.data.lock().await;

        guard.state.connected = false;
    }

    pub async fn play(&self, encoded: String) -> Result<(), PlayerError> {
        let info = decode_base64(&encoded)?;

        let api_track = ApiTrack {
            encoded,
            info,
            plugin_info: Empty,
        };

        let mut track = api_track.make_playable().await?;

        let guard = self.data.lock().await;

        if guard.volume as f32 != track.volume {
            track = track.volume(guard.volume as f32);
        }

        drop(guard);

        // todo: before sending the new track, we may want to send a replaced notification from here before playing the new track

        let mut guard = self.driver.lock().await;

        let driver = guard.as_mut().ok_or(PlayerError::MissingDriver)?;

        let track_handle = driver.play_only(track);

        drop(guard);

        track_handle.add_event(
            Event::Track(TrackEvent::Play),
            PlayerEvent::new(Event::Track(TrackEvent::Play), self),
        )?;

        track_handle.add_event(
            Event::Track(TrackEvent::Pause),
            PlayerEvent::new(Event::Track(TrackEvent::Pause), self),
        )?;

        track_handle.add_event(
            Event::Track(TrackEvent::Playable),
            PlayerEvent::new(Event::Track(TrackEvent::Playable), self),
        )?;

        track_handle.add_event(
            Event::Track(TrackEvent::End),
            PlayerEvent::new(Event::Track(TrackEvent::End), self),
        )?;

        let mut handle = self.handle.lock().await;

        let _ = handle.insert(track_handle);

        Ok(())
    }

    pub async fn stop(&self) {
        let guard = self.handle.lock().await;

        let Some(handle) = guard.as_ref() else {
            return;
        };

        handle.stop().ok();
    }

    pub async fn seek(&self, position: u32) {
        let guard = self.data.lock().await;

        if guard.state.position == position
            || guard
                .track
                .as_ref()
                .is_some_and(|track| track.info.length < position as u64)
        {
            return;
        }

        drop(guard);

        let guard = self.handle.lock().await;

        let Some(handle) = guard.as_ref() else {
            return;
        };

        let result = handle
            .seek_async(Duration::from_millis(position as u64))
            .await;

        drop(guard);

        if result.is_ok() {
            let mut guard = self.data.lock().await;

            guard.state.position = position;
        }
    }

    pub async fn pause(&self, pause: bool) {
        let guard = self.data.lock().await;

        let paused = guard.paused;

        if paused == pause {
            return;
        }

        drop(guard);

        let guard = self.handle.lock().await;

        let Some(handle) = guard.as_ref() else {
            return;
        };

        let is_ok = {
            if !paused {
                handle.pause().is_ok()
            } else {
                handle.play().is_ok()
            }
        };

        if !is_ok {
            return;
        }

        drop(guard);

        let mut guard = self.data.lock().await;

        guard.paused = pause;
    }

    pub async fn set_volume(&self, volume: f32) {
        let guard = self.handle.lock().await;

        let Some(handle) = guard.as_ref() else {
            return;
        };

        if handle.set_volume(volume).is_ok() {
            drop(guard);

            let mut guard = self.data.lock().await;

            guard.volume = volume as u32;
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let arc_driver = self.driver.clone();

        task::block_in_place(move || {
            if let Some(driver) = arc_driver.blocking_lock().take() {
                drop(driver);
            }
        });

        let arc_handle = self.handle.clone();

        task::block_in_place(move || {
            if let Some(handle) = arc_handle.blocking_lock().take() {
                drop(handle);
            }
        });

        tracing::info!(
            "Player with [GuildId: {}] [UserId: {}] dropped!",
            self.guild_id,
            self.user_id
        );
    }
}
