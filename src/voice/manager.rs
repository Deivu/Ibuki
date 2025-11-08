use super::player::Player;
use crate::models::ApiVoiceData;
use crate::util::errors::PlayerManagerError;
use crate::ws::client::WebSocketClient;
use dashmap::DashMap;
use dashmap::mapref::one::Ref;
use flume::{Sender, unbounded};
use kameo::actor::ActorRef;
use songbird::Config;
use songbird::id::{GuildId, UserId};
use std::sync::Arc;

pub enum CleanerSender {
    GuildId(GuildId),
    Destroy,
}

pub struct CreatePlayerOptions {
    pub guild_id: GuildId,
    pub server_update: ApiVoiceData,
    pub config: Option<Config>,
}

pub struct PlayerManager {
    pub user_id: UserId,
    pub players: Arc<DashMap<GuildId, Player>>,
    cleaner: Sender<CleanerSender>,
    websocket: ActorRef<WebSocketClient>,
}

impl PlayerManager {
    pub fn new(websocket: ActorRef<WebSocketClient>, user_id: UserId) -> Self {
        let (cleaner, listener) = unbounded::<CleanerSender>();

        let manager = Self {
            user_id,
            cleaner,
            websocket,
            players: Arc::new(DashMap::new()),
        };

        let players = manager.players.clone();

        tokio::spawn(async move {
            while let Ok(data) = listener.recv_async().await {
                if let CleanerSender::GuildId(guild_id) = data {
                    players.remove(&guild_id);
                    continue;
                }
                break;
            }
        });

        manager
    }

    pub fn get_player(&self, guild_id: &GuildId) -> Option<Ref<'_, GuildId, Player>> {
        self.players.get(guild_id)
    }

    pub async fn create_player(
        &self,
        options: CreatePlayerOptions,
    ) -> Result<Ref<'_, GuildId, Player>, PlayerManagerError> {
        let Some(player) = self.players.get(&options.guild_id) else {
            let player = Player::new(
                self.websocket.clone(),
                self.cleaner.downgrade(),
                options.config,
                self.user_id,
                options.guild_id,
                options.server_update.clone(),
            )
            .await?;

            self.players.insert(options.guild_id, player);

            return self
                .players
                .get(&options.guild_id)
                .ok_or(PlayerManagerError::MissingPlayer);
        };

        player
            .connect(&options.server_update, options.config)
            .await?;

        Ok(player)
    }

    pub async fn disconnect_player(&self, guild_id: &GuildId) {
        let Some(player) = self.get_player(guild_id) else {
            return;
        };

        player.disconnect().await;
    }

    pub fn disconnect_all(&self) {
        self.players.clear();
    }
}

impl Drop for PlayerManager {
    fn drop(&mut self) {
        self.cleaner.send(CleanerSender::Destroy).ok();

        self.players.clear();

        tracing::info!("PlayerManager with [UserId: {}] dropped!", self.user_id);
    }
}
