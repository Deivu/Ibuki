use crate::CLIENTS;
use crate::models::{ApiNodeMessage, ApiReady};
use crate::util::errors::PlayerManagerError;
use crate::voice::manager::{CreatePlayerOptions, PlayerManager};
use crate::voice::player::Player;
use crate::ws::receiver::{ReceiverActor, ReceiverActorArgs};
use crate::ws::sender::{SendToWebsocket, SenderActor};
use axum::Error;
use axum::extract::ConnectInfo;
use axum::extract::ws::{Message as WsMessage, WebSocket};
use futures::StreamExt;
use kameo::Actor;
use kameo::actor::ActorRef;
use kameo::message::{Context, Message as KameoMessage};
use songbird::id::{GuildId, UserId};
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::option::Option;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use uuid::Uuid;

pub struct ConnectWebsocket {
    socket: WebSocket,
    session_id: Option<u128>,
}

pub struct DisconnectWebsocket;

pub struct DestroyWebsocket;

pub struct SendMessageWebsocket(pub WsMessage);

pub struct GetSessionId;

pub struct CreatePlayerFromWebsocket(pub CreatePlayerOptions);

pub struct GetPlayerFromWebsocket(pub GuildId);

pub struct DisconnectPlayerFromWebsocket(pub GuildId);

pub struct DisconnectAllPlayersFromWebsocket;

pub struct UpdateResumeAndTimeout(pub bool, pub u32);

#[derive(Clone)]
pub struct WebsocketRequestData {
    pub user_agent: String,
    pub user_id: UserId,
    pub session_id: Option<u128>,
}

pub struct WebSocketClient {
    user_id: UserId,
    session_id: u128,
    sender: Option<ActorRef<SenderActor>>,
    receiver: Option<ActorRef<ReceiverActor>>,
    player_manager: PlayerManager,
    message_buffer: VecDeque<WsMessage>,
    resume: Arc<AtomicBool>,
    timeout: Arc<AtomicU32>,
    dropped: Arc<AtomicBool>,
}

impl Actor for WebSocketClient {
    type Args = UserId;
    type Error = ();

    async fn on_start(
        user_id: Self::Args,
        actor_ref: ActorRef<Self>,
    ) -> Result<WebSocketClient, Self::Error> {
        Ok(Self {
            user_id,
            session_id: Uuid::new_v4().as_u128(),
            sender: None,
            receiver: None,
            player_manager: PlayerManager::new(actor_ref.clone(), user_id),
            message_buffer: VecDeque::new(),
            resume: Arc::new(AtomicBool::new(false)),
            timeout: Arc::new(AtomicU32::new(30)),
            // todo!() im not sure if i want this or i can just link the receiver actor to this actor so if that dies, so is this
            dropped: Arc::new(AtomicBool::new(false)),
        })
    }
}

impl KameoMessage<ConnectWebsocket> for WebSocketClient {
    type Reply = bool;

    async fn handle(
        &mut self,
        msg: ConnectWebsocket,
        ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        if let Some(sender) = self.sender.take() {
            sender.kill();
        }
        if let Some(receiver) = self.receiver.take() {
            receiver.kill();
        }

        let (sink, stream) = msg.socket.split();

        let resumed = self.resume.load(Ordering::Acquire)
            && msg.session_id.filter(|id| *id == self.session_id).is_some();

        if resumed {
            tracing::info!(
                "WebSocket connection resumed [SessionId: {}], replaying {} messages",
                self.session_id,
                self.message_buffer.len()
            );
        } else {
            let queue_length = self.message_buffer.len();
            self.message_buffer.clear();
            self.session_id = Uuid::new_v4().as_u128();

            // todo!() disconnect_all and clear refactor soon for clearer code
            self.player_manager.disconnect_all();

            tracing::info!(
                "WebSocket connection identified [SessionId: {}], dropped {} messages",
                self.session_id,
                queue_length
            );
        }

        let sender_actor = SenderActor::spawn(SenderActor {
            sink,
            dropped: self.dropped.clone(),
        });

        ctx.actor_ref().link(&sender_actor).await;

        self.sender = Some(sender_actor.clone());

        if resumed {
            for buffered_msg in self.message_buffer.drain(..) {
                sender_actor.tell(SendToWebsocket(buffered_msg)).await.ok();
            }
        }

        let client_ref = ctx.actor_ref();
        let receiver_actor = ReceiverActor::spawn(ReceiverActorArgs {
            stream,
            client_ref: client_ref.clone(),
            dropped: self.dropped.clone(),
            user_id: self.user_id.clone(),
            players: self.player_manager.players.clone(),
            resume: self.resume.clone(),
            timeout: self.timeout.clone(),
        });

        ctx.actor_ref().link(&receiver_actor).await;

        self.receiver = Some(receiver_actor);

        let event = ApiReady {
            resumed,
            session_id: self.session_id.to_string(),
        };

        // Normally, this should never happen, but we ignore it if it does happen and log it
        let Ok(serialized) = serde_json::to_string(&ApiNodeMessage::Ready(Box::new(event))) else {
            tracing::warn!("Failed to encode ready op, this should not happen usually");
            return resumed;
        };

        sender_actor
            .tell(SendToWebsocket(WsMessage::Text(serialized.into())))
            .await
            .ok();

        resumed
    }
}

impl KameoMessage<SendMessageWebsocket> for WebSocketClient {
    type Reply = ();

    async fn handle(
        &mut self,
        msg: SendMessageWebsocket,
        _: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        if let Some(sender) = &self.sender {
            let Err(error) = sender.tell(SendToWebsocket(msg.0)).await else {
                return;
            };
            tracing::warn!("Failed to send to sender task due to {:?}", error);
        } else {
            self.message_buffer.push_back(msg.0);
            tracing::debug!("Sender task is disconnected, buffering...");
        }
    }
}

impl KameoMessage<DisconnectWebsocket> for WebSocketClient {
    type Reply = ();

    async fn handle(
        &mut self,
        _: DisconnectWebsocket,
        _: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        if let Some(sender) = &self.sender {
            let close_msg = WsMessage::Close(Some(axum::extract::ws::CloseFrame {
                code: 1000,
                reason: "Invoked Disconnect".into(),
            }));
            sender.tell(SendToWebsocket(close_msg)).await.ok();
        }

        if let Some(sender) = self.sender.take() {
            sender.kill();
        }
        if let Some(receiver) = self.receiver.take() {
            receiver.kill();
        }
    }
}

impl KameoMessage<DestroyWebsocket> for WebSocketClient {
    type Reply = ();

    async fn handle(
        &mut self,
        _: DestroyWebsocket,
        _: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        if let Some(sender) = self.sender.take() {
            sender.kill();
        }
        if let Some(receiver) = self.receiver.take() {
            receiver.kill();
        }

        // todo!() disconnect_all and clear refactor soon for clearer code
        self.player_manager.disconnect_all();
    }
}

impl KameoMessage<GetSessionId> for WebSocketClient {
    type Reply = u128;

    async fn handle(&mut self, _: GetSessionId, _: &mut Context<Self, Self::Reply>) -> Self::Reply {
        self.session_id
    }
}

impl KameoMessage<CreatePlayerFromWebsocket> for WebSocketClient {
    type Reply = Result<(), PlayerManagerError>;

    async fn handle(
        &mut self,
        msg: CreatePlayerFromWebsocket,
        _: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.player_manager.create_player(msg.0).await.map(|_| ())
    }
}

impl KameoMessage<GetPlayerFromWebsocket> for WebSocketClient {
    type Reply = Option<Player>;

    async fn handle(
        &mut self,
        msg: GetPlayerFromWebsocket,
        _: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.player_manager
            .get_player(&msg.0)
            .map(|player| player.clone())
    }
}

impl KameoMessage<DisconnectPlayerFromWebsocket> for WebSocketClient {
    type Reply = ();

    async fn handle(
        &mut self,
        msg: DisconnectPlayerFromWebsocket,
        _: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.player_manager.disconnect_player(&msg.0).await;
    }
}

impl KameoMessage<DisconnectAllPlayersFromWebsocket> for WebSocketClient {
    type Reply = ();

    async fn handle(
        &mut self,
        _: DisconnectAllPlayersFromWebsocket,
        _: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.player_manager.disconnect_all();
    }
}

impl KameoMessage<UpdateResumeAndTimeout> for WebSocketClient {
    type Reply = ();
    async fn handle(
        &mut self,
        msg: UpdateResumeAndTimeout,
        _: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.resume.swap(msg.0, Ordering::Release);
        self.timeout.swap(msg.1, Ordering::Release);
    }
}

pub async fn handle_websocket_upgrade_request(
    socket: WebSocket,
    data: WebsocketRequestData,
    addr: ConnectInfo<SocketAddr>,
) {
    let Some(client) = CLIENTS.get_mut(&data.user_id) else {
        let client = WebSocketClient::spawn(data.user_id);

        CLIENTS.insert(data.user_id, client);

        return Box::pin(handle_websocket_upgrade_request(socket, data, addr)).await;
    };

    match client
        .ask(ConnectWebsocket {
            socket,
            session_id: data.session_id,
        })
        .await
    {
        Ok(resumed) => {
            tracing::info!(
                "Handled connection from: {}. [SessionId: {:?}] [UserId: {}] [UserAgent: {}] [Resume: {}]",
                addr.ip(),
                data.session_id,
                data.user_id,
                data.user_agent,
                resumed
            );
        }
        Err(error) => {
            // todo: probably remove the client here?
            tracing::warn!(
                "Connection was not handled properly from: {}. [SessionId: {:?}] [UserId: {}] [UserAgent: {}] [Error: {:?}]",
                addr.ip(),
                data.session_id,
                data.user_id,
                data.user_agent,
                error
            );
        }
    };
}

pub fn handle_websocket_upgrade_error(
    error: &Error,
    data: WebsocketRequestData,
    addr: ConnectInfo<SocketAddr>,
) {
    let session_id = data
        .session_id
        .map(|id| id.to_string())
        .unwrap_or("None".to_owned());

    tracing::warn!(
        "Websocket Upgrade errored from: {}. [SessionId: {}] [UserId: {}] [UserAgent: {}] [Error: {:?}]",
        addr.ip(),
        session_id,
        data.user_id,
        data.user_agent,
        error
    );
}
