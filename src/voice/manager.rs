use super::player::{Player, PlayerOptions};
use crate::models::ApiVoiceData;
use crate::ws::client::WebSocketClient;
use dashmap::DashMap;
use dashmap::mapref::one::Ref;
use kameo::actor::{ActorId, ActorRef, Spawn, WeakActorRef};
use songbird::Config;
use songbird::id::{GuildId, UserId};
use std::sync::Arc;

pub struct CreatePlayerOptions {
    pub guild_id: GuildId,
    pub server_update: ApiVoiceData,
    pub config: Option<Config>,
}

/// Possible race condition here, still thinking for a fix
pub struct PlayerManager {
    pub user_id: UserId,
    pub players: Arc<DashMap<GuildId, ActorRef<Player>>>,
    websocket: WeakActorRef<WebSocketClient>,
}

impl PlayerManager {
    pub fn new(websocket: WeakActorRef<WebSocketClient>, user_id: UserId) -> Self {
        Self {
            user_id,
            websocket,
            players: Arc::new(DashMap::new()),
        }
    }

    pub fn get_player(&self, guild_id: &GuildId) -> Option<Ref<'_, GuildId, ActorRef<Player>>> {
        self.players.get(guild_id)
    }

    pub fn create_player(&self, options: CreatePlayerOptions) -> (ActorRef<Player>, bool) {
        if let Some(player) = self.get_player(&options.guild_id) {
            return (player.clone(), false);
        }

        let options = PlayerOptions {
            websocket: self.websocket.clone(),
            config: options.config,
            user_id: self.user_id,
            guild_id: options.guild_id.clone(),
            server_update: options.server_update,
            players: self.players.clone(),
        };

        let guild_id = options.guild_id;
        let player = Player::spawn(options);

        self.players.insert(guild_id, player.clone());

        (player, true)
    }

    pub fn remove_player(&self, guild_id: &GuildId, id: ActorId) {
        self.players
            .remove_if(guild_id, |_, player| player.id() == id);
    }

    pub async fn destroy_player(&self, guild_id: &GuildId) {
        self.players.remove(guild_id);
    }

    pub fn destroy_all(&self) {
        self.players.clear();
    }
}

impl Drop for PlayerManager {
    fn drop(&mut self) {
        self.destroy_all();
        tracing::info!("PlayerManager with [UserId: {}] dropped!", self.user_id);
    }
}
