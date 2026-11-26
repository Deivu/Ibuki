use axum::extract::ws::{Message as WebsocketMessage, WebSocket};
use futures::SinkExt;
use futures::stream::SplitSink;
use kameo::Actor;
use kameo::message::Context;
use kameo::message::Message as KameoMessage;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Notify;
use tokio::time::{Duration, timeout};

const SEND_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Actor)]
pub struct SenderActor {
    pub sink: SplitSink<WebSocket, WebsocketMessage>,
    pub dropped: Arc<AtomicBool>,
    pub send_failed: Arc<Notify>,
}
pub struct SendToWebsocket(pub WebsocketMessage);

impl KameoMessage<SendToWebsocket> for SenderActor {
    type Reply = ();

    async fn handle(
        &mut self,
        msg: SendToWebsocket,
        ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        if self.dropped.load(Ordering::Acquire) {
            tracing::debug!("Sender will be dropped because dropped is truthy");
            ctx.actor_ref().kill();
            return;
        }

        tracing::debug!("Sending message to WebSocket {:?}", msg.0);

        match timeout(SEND_TIMEOUT, self.sink.send(msg.0)).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                tracing::error!("Error sending message to WebSocket {:?}", error);
                self.send_failed.notify_one();
            }
            Err(_) => {
                tracing::error!(
                    "Sending message to WebSocket timed out after {:?}",
                    SEND_TIMEOUT
                );
                self.send_failed.notify_one();
            }
        }
    }
}
