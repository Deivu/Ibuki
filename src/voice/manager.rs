use super::player::{Connect, Player, PlayerOptions};
use crate::models::ApiVoiceData;
use crate::util::errors::PlayerManagerError;
use crate::ws::client::WebSocketClient;
use dashmap::DashMap;
use dashmap::mapref::one::Ref;
use kameo::actor::{ActorRef, Spawn, WeakActorRef};
use songbird::Config;
use songbird::id::{GuildId, UserId};
use std::sync::Arc;

pub struct CreatePlayerOptions {
    pub guild_id: GuildId,
    pub server_update: Option<ApiVoiceData>,
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

    pub async fn create_player(
        &self,
        options: CreatePlayerOptions,
    ) -> Result<(), PlayerManagerError> {
        if let Some(player) = self.get_player(&options.guild_id) {
            player.wait_for_startup_result().await?;
            if let Some(server_update) = options.server_update {
                player
                    .ask(Connect {
                        server_update,
                        config: options.config,
                    })
                    .await?;
            }
            return Ok(());
        }
        let guild_id = options.guild_id.clone();
        let player_options = PlayerOptions {
            websocket: self.websocket.clone(),
            config: options.config,
            user_id: self.user_id,
            guild_id: guild_id.clone(),
            server_update: options.server_update,
            players: self.players.clone(),
        };

        let player_ref = Player::spawn(player_options);
        self.players.insert(guild_id.clone(), player_ref.clone());
        match player_ref.wait_for_startup_result().await {
            Ok(_) => Ok(()),
            Err(e) => {
                self.players.remove(&guild_id);
                Err(e.into())
            }
        }
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
