use crate::CLIENTS;
use crate::models::{ApiNodeMessage, ApiReady};
use crate::util::errors::PlayerManagerError;
use crate::voice::manager::{CreatePlayerOptions, PlayerManager};
use crate::voice::player::Player;
use crate::ws::receiver::{ReceiverActor, ReceiverActorArgs};
use crate::ws::sender::{SendToWebsocket, SenderActor};
use axum::Error;
use axum::extract::ConnectInfo;
use axum::extract::ws::{CloseFrame, Message as WsMessage, WebSocket};
use futures::StreamExt;
use kameo::actor::{ActorRef, Spawn};
use kameo::message::Context;
use kameo::{Actor, Reply, messages};
use songbird::id::{GuildId, UserId};
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::option::Option;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use uuid::Uuid;

#[derive(Clone)]
pub struct WebsocketRequestData {
    pub user_agent: String,
    pub user_id: UserId,
    pub session_id: Option<u128>,
}

#[derive(Reply, Clone, Debug)]
pub struct WebSocketClientData {
    pub session_id: u128,
    pub resume: bool,
    pub timeout: u32,
}

pub struct WebSocketClient {
    user_id: UserId,
    session_id: u128,
    sender: Option<ActorRef<SenderActor>>,
    receiver: Option<ActorRef<ReceiverActor>>,
    player_manager: PlayerManager,
    message_queue: VecDeque<WsMessage>,
    resume: Arc<AtomicBool>,
    timeout: Arc<AtomicU32>,
    dropped: Arc<AtomicBool>,
}

impl From<&WebSocketClient> for WebSocketClientData {
    fn from(value: &WebSocketClient) -> Self {
        Self {
            session_id: value.session_id,
            resume: value.resume.load(Ordering::Acquire),
            timeout: value.timeout.load(Ordering::Acquire),
        }
    }
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
            message_queue: VecDeque::new(),
            resume: Arc::new(AtomicBool::new(false)),
            timeout: Arc::new(AtomicU32::new(30)),
            // todo!() im not sure if i want this or i can just link the receiver actor to this actor so if that dies, so is this
            dropped: Arc::new(AtomicBool::new(false)),
        })
    }
}

#[messages]
impl WebSocketClient {
    #[message]
    pub fn get_websocket_info(&self) -> WebSocketClientData {
        self.into()
    }

    #[message(ctx)]
    pub async fn establish_connection(
        &mut self,
        ctx: &mut Context<Self, bool>,
        socket: WebSocket,
        session_id: Option<u128>,
    ) -> bool {
        self.cleanup();

        let (sink, stream) = socket.split();

        let resumed = self.resume.load(Ordering::Acquire)
            && session_id.filter(|id| *id == self.session_id).is_some();

        if resumed {
            tracing::info!(
                "WebSocket connection resumed [SessionId: {}], replaying {} messages",
                self.session_id,
                self.message_queue.len()
            );
        } else {
            let queue_length = self.message_queue.len();
            self.message_queue.clear();
            self.session_id = Uuid::new_v4().as_u128();

            // todo!() disconnect_all and clear refactor soon for clearer code
            self.player_manager.destroy_all();

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
            for buffered_msg in self.message_queue.drain(..) {
                sender_actor.tell(SendToWebsocket(buffered_msg)).await.ok();
            }
        }

        let receiver_actor = ReceiverActor::spawn(ReceiverActorArgs {
            stream,
            client_ref: ctx.actor_ref().clone(),
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

    #[message]
    pub async fn cleanup_connection(&mut self) {
        if let Some(sender) = &self.sender {
            let close_msg = WsMessage::Close(Some(CloseFrame {
                code: 1000,
                reason: "Invoked Disconnect".into(),
            }));
            sender.ask(SendToWebsocket(close_msg)).await.ok();
        }
        self.cleanup();
        self.terminate();
    }

    #[message]
    pub async fn send_connection_message(&mut self, message: WsMessage) {
        if let Some(sender) = &self.sender {
            let Err(error) = sender.tell(SendToWebsocket(message)).await else {
                return;
            };
            tracing::warn!("Failed to send to sender task due to {:?}", error);
        } else {
            self.message_queue.push_back(message);
            tracing::debug!("Sender task is disconnected, buffering...");
        }
    }

    #[message]
    pub fn update_websocket(&mut self, resuming: bool, timeout: u32) -> WebSocketClientData {
        self.resume.store(resuming, Ordering::Release);
        self.timeout.store(timeout, Ordering::Release);
        (self as &WebSocketClient).into()
    }

    #[message]
    pub async fn create_player(
        &self,
        options: CreatePlayerOptions,
    ) -> Result<(), PlayerManagerError> {
        self.player_manager.create_player(options).await.map(|_| ())
    }

    #[message]
    pub async fn get_player(&self, guild_id: GuildId) -> Option<ActorRef<Player>> {
        self.player_manager.get_player(&guild_id).map(|p| p.clone())
    }

    #[message]
    pub async fn destroy_player(&self, guild_id: GuildId) {
        self.player_manager.destroy_player(&guild_id).await;
    }

    #[message]
    pub async fn destroy_all_players(&self) {
        self.player_manager.destroy_all();
    }

    fn cleanup(&mut self) {
        if let Some(sender) = self.sender.take() {
            sender.kill();
        }
        if let Some(receiver) = self.receiver.take() {
            receiver.kill();
        }
    }

    fn terminate(&mut self) {
        self.dropped.store(false, Ordering::Release);
        self.player_manager.destroy_all();
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
        .ask(EstablishConnection {
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
