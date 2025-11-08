use super::{
    DecodeQueryString, EncodeQueryString, PlayerMethodsPath, PlayerUpdateQuery, SessionMethodsPath,
};
use crate::models::{
    ApiPlayerOptions, ApiSessionBody, ApiSessionInfo, ApiTrack, ApiTrackResult, Empty,
};
use crate::util::converter::numbers::IbukiGuildId;
use crate::util::decoder::decode_base64;
use crate::util::errors::EndpointError;
use crate::util::source::{Source, Sources};
use crate::voice::manager::CreatePlayerOptions;
use crate::voice::player::Player;
use crate::ws::client::{
    CreatePlayerFromWebsocket, DisconnectPlayerFromWebsocket, GetPlayerFromWebsocket, GetSessionId,
    UpdateResumeAndTimeout, WebSocketClient,
};
use crate::{CLIENTS, SOURCES};
use axum::Json;
use axum::extract::Path;
use axum::{body::Body, extract::Query, response::Response};
use dashmap::mapref::multiple::RefMulti;
use kameo::actor::ActorRef;
use serde_json::Value;
use songbird::id::{GuildId, UserId};
use std::num::NonZeroU64;
use std::sync::atomic::Ordering;

async fn get_client(
    session_id: u128,
) -> Option<RefMulti<'static, UserId, ActorRef<WebSocketClient>>> {
    for client in CLIENTS.iter() {
        let Some(client_session_id) = client.ask(GetSessionId).await.ok() else {
            continue;
        };
        if session_id == client_session_id {
            return Some(client);
        }
    }
    None
}

async fn get_player_internal(
    client_ref: &RefMulti<'static, UserId, ActorRef<WebSocketClient>>,
    guild_id: u64,
) -> Result<Player, EndpointError> {
    let id = GuildId::from(NonZeroU64::try_from(IbukiGuildId(guild_id))?);
    client_ref
        .ask(GetPlayerFromWebsocket(id))
        .await
        .map_err(|_| EndpointError::NotFound)?
        .ok_or(EndpointError::NotFound)
}

pub async fn get_player(
    Path(PlayerMethodsPath {
        session_id,
        guild_id,
    }): Path<PlayerMethodsPath>,
) -> Result<Response<Body>, EndpointError> {
    let client = get_client(session_id)
        .await
        .ok_or(EndpointError::NotFound)?;

    let player = get_player_internal(&client, guild_id).await?;

    let string = serde_json::to_string_pretty(&*player.data.lock().await)?;

    Ok(Response::new(Body::from(string)))
}

pub async fn update_player(
    query: Query<PlayerUpdateQuery>,
    Path(PlayerMethodsPath {
        session_id,
        guild_id,
    }): Path<PlayerMethodsPath>,
    Json(update_player): Json<ApiPlayerOptions>,
) -> Result<Response<Body>, EndpointError> {
    let client = get_client(session_id)
        .await
        .ok_or(EndpointError::NotFound)?;

    if get_player_internal(&client, guild_id).await.is_err() && update_player.voice.is_none() {
        return Err(EndpointError::NotFound);
    }

    if let Some(server_update) = update_player.voice {
        let guild_id = GuildId::from(NonZeroU64::try_from(IbukiGuildId(guild_id))?);
        let options = CreatePlayerOptions {
            guild_id,
            server_update,
            config: None,
        };

        client.ask(CreatePlayerFromWebsocket(options)).await?;
    }

    let player = get_player_internal(&client, guild_id).await?;

    let mut stopped = false;

    if let Some(encoded) = update_player.track.map(|track| track.encoded) {
        if !player.active.load(Ordering::Relaxed) || !query.no_replace.unwrap_or(false) {
            match encoded {
                Value::String(encoded) => {
                    player.play(encoded).await?;
                }
                _ => {
                    player.stop().await;
                    stopped = true;
                }
            }
        }
    }

    if !stopped {
        if let Some(pause) = update_player.paused {
            player.pause(pause).await;
        }

        if let Some(position) = update_player.position {
            player.seek(position).await;
        }

        if let Some(volume) = update_player.volume {
            player.set_volume(volume as f32).await;
        }
    }

    let string = serde_json::to_string_pretty(&*player.data.lock().await)?;

    Ok(Response::new(Body::from(string)))
}

#[tracing::instrument]
pub async fn destroy_player(
    Path(PlayerMethodsPath {
        session_id,
        guild_id,
    }): Path<PlayerMethodsPath>,
) -> Result<Response<Body>, EndpointError> {
    let client = get_client(session_id)
        .await
        .ok_or(EndpointError::NotFound)?;

    client
        .ask(DisconnectPlayerFromWebsocket(GuildId::from(
            NonZeroU64::try_from(IbukiGuildId(guild_id))?,
        )))
        .await?;

    Ok(Response::new(Body::from(())))
}

#[tracing::instrument]
pub async fn update_session(
    Path(SessionMethodsPath { session_id }): Path<SessionMethodsPath>,
    Json(update_session): Json<ApiSessionBody>,
) -> Result<Response<Body>, EndpointError> {
    let client = get_client(session_id)
        .await
        .ok_or(EndpointError::NotFound)?;

    client
        .ask(UpdateResumeAndTimeout(
            update_session.resuming,
            update_session.timeout,
        ))
        .await?;

    let info = ApiSessionInfo {
        resuming_key: client.ask(GetSessionId).await?,
        timeout: update_session.timeout as u16,
    };

    let string = serde_json::to_string_pretty(&info)?;

    Ok(Response::new(Body::from(string)))
}

pub async fn decode(query: Query<DecodeQueryString>) -> Result<Response<Body>, EndpointError> {
    let track = decode_base64(&query.track)?;

    let track = ApiTrack {
        encoded: query.track.clone(),
        info: track,
        plugin_info: Empty,
    };

    let string = serde_json::to_string_pretty(&track)?;

    Ok(Response::new(Body::from(string)))
}

#[tracing::instrument]
pub async fn encode(query: Query<EncodeQueryString>) -> Result<Response<Body>, EndpointError> {
    let track: ApiTrackResult = {
        let mut result: Option<ApiTrackResult> = None;

        for source in SOURCES.iter() {
            match source.value() {
                Sources::Youtube(src) => {
                    let option = src.parse_query(&query.identifier);

                    if let Some(query) = option {
                        result = src.resolve(query).await?;
                    }
                }
                Sources::Deezer(src) => {
                    let option = src.parse_query(&query.identifier);

                    if let Some(query) = option {
                        result = src.resolve(query).await?;
                    }
                }
                Sources::Http(src) => {
                    let option = src.parse_query(&query.identifier);

                    if let Some(query) = option {
                        result = src.resolve(query).await?;
                    }
                }
            }

            if result.is_some() {
                break;
            }
        }

        result.unwrap_or(ApiTrackResult::Empty(None))
    };

    let string = serde_json::to_string_pretty(&track)?;

    Ok(Response::new(Body::from(string)))
}
