use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use axum::extract::ws::{Message as WebsocketMessage, WebSocket};
use futures::SinkExt;
use futures::stream::SplitSink;
use kameo::Actor;
use kameo::message::Message as KameoMessage;
use kameo::message::Context;

#[derive(Actor)]
pub struct SenderActor {
	pub sink: SplitSink<WebSocket, WebsocketMessage>,
	pub dropped: Arc<AtomicBool>,
}
pub struct SendToWebsocket(pub WebsocketMessage);

impl KameoMessage<SendToWebsocket> for SenderActor {
	type Reply = ();

	async fn handle(&mut self, msg: SendToWebsocket, ctx: &mut Context<Self, Self::Reply>) -> Self::Reply {
		if self.dropped.load(Ordering::Acquire) {
			tracing::debug!("Sender will be dropped because dropped is truthy");
			ctx.actor_ref().kill();
			return;
		}

		tracing::debug!("Sending message to WebSocket {:?}", msg.0);

		if let Err(error) = self.sink.send(msg.0).await {
			tracing::error!("Error sending message to WebSocket {:?}", error);
		}
	}
}