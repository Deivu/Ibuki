use crate::voice::player::Player;
use crate::ws::client::WebSocketClient;
use axum::extract::ws::{Message, WebSocket};
use dashmap::DashMap;
use futures::StreamExt;
use futures::stream::SplitStream;
use kameo::Actor;
use kameo::actor::{ActorRef, WeakActorRef};
use kameo::error::{ActorStopReason, Infallible};
use songbird::id::{GuildId, UserId};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use tokio::task::JoinHandle;

pub struct ReceiverActorArgs {
    pub stream: SplitStream<WebSocket>,
    pub dropped: Arc<AtomicBool>,
    pub user_id: UserId,
    pub players: Arc<DashMap<GuildId, ActorRef<Player>>>,
    pub resume: Arc<AtomicBool>,
    pub timeout: Arc<AtomicU32>,
}

pub struct ReceiverActor {
    handle: JoinHandle<()>,
}

impl Actor for ReceiverActor {
    type Args = ReceiverActorArgs;
    type Error = Infallible;

    async fn on_start(
        mut args: Self::Args,
        actor_ref: ActorRef<Self>,
    ) -> Result<ReceiverActor, Self::Error> {
        let handle = tokio::spawn(async move {
            while let Some(Ok(message)) = args.stream.next().await {
                match message {
                    Message::Close(close_frame) => {
                        tracing::info!("WebSocket closed: {:?}", close_frame);
                        break;
                    }
                    Message::Text(data) => {
                        tracing::warn!(
                            "Received unexpected text message from client for user {}: {}. Incoming WebSocket messages are not supported in v4; please use the REST API instead.",
                            args.user_id,
                            data
                        );
                    }
                    Message::Ping(_) | Message::Pong(_) => {}
                    _ => {}
                }
            }

            args.dropped.store(true, Ordering::Release);

            let timeout = args.timeout.load(Ordering::Acquire);

            if args.resume.load(Ordering::Acquire) && timeout > 0 {
                tracing::info!("Connection can be resumed within {} seconds", timeout);
                tokio::time::sleep(tokio::time::Duration::from_secs(timeout as u64)).await;
            }
            for ref_entry in args.players.iter() {
                ref_entry.value().kill();
            }
            args.players.clear();

            tracing::info!("Cleaned up WebSocket client for user {}", args.user_id);

            actor_ref.kill();
        });

        Ok(ReceiverActor { handle })
    }

    async fn on_stop(
        &mut self,
        _: WeakActorRef<Self>,
        reason: ActorStopReason,
    ) -> Result<(), Self::Error> {
        tracing::debug!("A receiver actor stopped due to {:?}", reason);
        self.handle.abort();
        Ok(())
    }
}
