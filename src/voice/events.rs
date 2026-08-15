use super::player::{
    Destroy, GetApiPlayerInfo, GetDriver, GetTrackHandle, Player, PlayerUpdate,
    SendToPlayerWebsocket, Stop, UpdateFromInternalEvent,
};
use crate::models::{
    ApiNodeMessage, ApiPlayerEvents, ApiPlayerUpdate, ApiTrack, ApiTrackEnd, ApiTrackStart,
    ApiWebSocketClosed,
};
use async_trait::async_trait;
use axum::extract::ws::{Message, Utf8Bytes};
use kameo::actor::{ActorRef, WeakActorRef};
use songbird::CoreEvent;
use songbird::Driver;
use songbird::Event;
use songbird::EventContext;
use songbird::EventHandler;
use songbird::TrackEvent;
use songbird::events::context_data::DisconnectReason;
use songbird::id::{GuildId, UserId};
use songbird::model::CloseCode;
use songbird::tracks::{TrackHandle, TrackState};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

enum DataResult {
    // probably usable in future
    #[allow(dead_code)]
    Track(TrackState, Arc<ApiTrack>),
    Disconnect(i32, String),
    Empty,
}

#[derive(Clone)]
pub struct PlayerEvent {
    pub user_id: UserId,
    pub guild_id: GuildId,
    pub event: Event,
    pub player_ref: WeakActorRef<Player>,
    pub fired: Arc<AtomicBool>,
}

impl PlayerEvent {
    pub fn new(
        event: Event,
        guild_id: GuildId,
        user_id: UserId,
        player_ref: WeakActorRef<Player>,
    ) -> Self {
        Self {
            user_id,
            guild_id,
            event,
            player_ref,
            fired: Arc::new(AtomicBool::new(false)),
        }
    }
    pub async fn get_driver(&self) -> Option<Driver> {
        self.get_actor_ref()?.ask(GetDriver).await.ok()?
    }

    pub async fn get_track_handle(&self) -> Option<TrackHandle> {
        self.get_actor_ref()?.ask(GetTrackHandle).await.ok()?
    }

    pub fn get_actor_ref(&self) -> Option<ActorRef<Player>> {
        self.player_ref.upgrade().clone()
    }

    pub async fn stop(&self) -> Option<()> {
        self.get_actor_ref()?.ask(Stop).await.ok()
    }

    pub async fn destroy(&self) -> Option<()> {
        self.get_actor_ref()?.ask(Destroy).await.ok()
    }

    pub async fn send_to_websocket(&self, message: Message) -> Option<()> {
        self.get_actor_ref()?
            .ask(SendToPlayerWebsocket { message })
            .await
            .ok()
    }
}

#[async_trait]
impl EventHandler for PlayerEvent {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        let mut data_result = DataResult::Empty;

        match ctx {
            EventContext::Track([(state, handle)]) => {
                let state = state.to_owned().clone();

                let track = handle.data::<ApiTrack>();

                data_result = DataResult::Track(state, track);
            }
            EventContext::DriverDisconnect(info) => {
                let (code, message) = {
                    // todo: make this have the enum as reason
                    if let Some(DisconnectReason::WsClosed(Some(code))) = info.reason {
                        match code {
                            CloseCode::UnknownOpcode => (4001, "Unknown Op Code"),
                            CloseCode::InvalidPayload => (4003, "Invalid Payload"),
                            CloseCode::NotAuthenticated => (4004, "Not Authenticated"),
                            CloseCode::AuthenticationFailed => (4005, "Authentication Failed"),
                            CloseCode::AlreadyAuthenticated => (4006, "Already Authenticated"),
                            CloseCode::SessionInvalid => (4009, "Session Invalid"),
                            CloseCode::SessionTimeout => (4011, "Session Timeout"),
                            CloseCode::ServerNotFound => (4012, "Server Not Found"),
                            CloseCode::UnknownProtocol => (4012, "Unknown Protocol"),
                            CloseCode::Disconnected => (4013, "Disconnected"),
                            CloseCode::VoiceServerCrash => (4015, "Voice Server Crash"),
                            CloseCode::UnknownEncryptionMode => (4016, "Unknown Encryption Mode"),
                        }
                    } else {
                        (1000, "Graceful close")
                    }
                };

                data_result = DataResult::Disconnect(code, message.to_string());
            }
            _ => {}
        };

        let player_event = self.clone();

        tokio::spawn(async move {
            handle_player_event(player_event, data_result).await;
        });

        None
    }
}

async fn handle_player_event(player_event: PlayerEvent, data_result: DataResult) -> Option<()> {
    let actor_ref = player_event.get_actor_ref()?;

    match player_event.event {
        Event::Periodic(_, _) => {
            let state = player_event
                .get_track_handle()
                .await?
                .get_info()
                .await
                .ok()?;

            let updates: Vec<PlayerUpdate> = vec![
                PlayerUpdate::Volume(state.volume as u32),
                PlayerUpdate::Position(state.position.as_millis() as u32),
            ];

            actor_ref
                .ask(UpdateFromInternalEvent { updates })
                .await
                .ok()?;

            let api_player = actor_ref.ask(GetApiPlayerInfo).await.ok()?;

            let data = ApiPlayerUpdate {
                guild_id: api_player.guild_id,
                state: api_player.state,
            };

            let serialized =
                serde_json::to_string(&ApiNodeMessage::PlayerUpdate(Box::new(data))).ok()?;

            player_event
                .send_to_websocket(Message::Text(Utf8Bytes::from(serialized)))
                .await;

            Some(())
        }
        Event::Track(event) => {
            let DataResult::Track(_, track) = data_result else {
                tracing::warn!("Expected DataResult::Track but got a different thing");
                return None;
            };

            match event {
                TrackEvent::Pause => {
                    let updates = vec![PlayerUpdate::Paused(true)];
                    actor_ref
                        .ask(UpdateFromInternalEvent { updates })
                        .await
                        .ok()?;
                    Some(())
                }
                TrackEvent::Play => {
                    let updates = vec![PlayerUpdate::Paused(false)];
                    actor_ref
                        .ask(UpdateFromInternalEvent { updates })
                        .await
                        .ok()?;
                    Some(())
                }
                TrackEvent::End => {
                    let updates = vec![
                        PlayerUpdate::Active(false),
                        PlayerUpdate::Track(None),
                        PlayerUpdate::Position(0),
                    ];
                    actor_ref
                        .ask(UpdateFromInternalEvent { updates })
                        .await
                        .ok()?;
                    actor_ref.ask(Stop).await.ok()?;

                    let event = ApiTrackEnd {
                        guild_id: player_event.guild_id.0.get(),
                        track: track.as_ref().clone(),
                        // todo: reflect reason for this end
                        reason: String::from("finished"),
                    };

                    let serialized = serde_json::to_string(&ApiNodeMessage::Event(Box::new(
                        ApiPlayerEvents::TrackEndEvent(event),
                    )))
                    .ok()?;

                    player_event
                        .send_to_websocket(Message::Text(Utf8Bytes::from(serialized)))
                        .await;

                    Some(())
                }
                TrackEvent::Playable => {
                    actor_ref
                        .ask(UpdateFromInternalEvent {
                            updates: vec![PlayerUpdate::Active(true)],
                        })
                        .await
                        .ok()?;
                    // ensures playable is only sent to client once
                    if player_event.fired.load(Ordering::Acquire) {
                        return None;
                    }
                    player_event.fired.swap(true, Ordering::Release);

                    actor_ref
                        .ask(UpdateFromInternalEvent {
                            updates: vec![PlayerUpdate::Track(Some(track.as_ref().clone()))],
                        })
                        .await
                        .ok()?;

                    let event = ApiTrackStart {
                        guild_id: player_event.guild_id.0.get(),
                        track: track.as_ref().clone(),
                    };

                    let serialized = serde_json::to_string(&ApiNodeMessage::Event(Box::new(
                        ApiPlayerEvents::TrackStartEvent(event),
                    )))
                    .ok()?;

                    player_event
                        .send_to_websocket(Message::Text(Utf8Bytes::from(serialized)))
                        .await;

                    Some(())
                }
                _ => None,
            }
        }
        Event::Core(CoreEvent::DriverDisconnect) => {
            actor_ref
                .ask(UpdateFromInternalEvent {
                    updates: vec![PlayerUpdate::Active(false)],
                })
                .await
                .ok()?;

            player_event.stop().await;
            player_event.destroy().await;

            let DataResult::Disconnect(code, reason) = data_result else {
                tracing::warn!("Expected DataResult::Disconnect but got a different thing");
                return None;
            };

            let event = ApiWebSocketClosed {
                guild_id: player_event.guild_id.0.get(),
                code: code as usize,
                reason,
                by_remote: code != 1000,
            };

            let serialized = serde_json::to_string(&ApiNodeMessage::Event(Box::new(
                ApiPlayerEvents::WebSocketClosedEvent(event),
            )))
            .ok()?;

            player_event
                .send_to_websocket(Message::Text(Utf8Bytes::from(serialized)))
                .await;

            Some(())
        }
        _ => None,
    }
}
