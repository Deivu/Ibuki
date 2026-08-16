use super::DecodeQueryString;
use super::EncodeQueryString;
use super::PlayerMethodsPath;
use super::PlayerUpdateQuery;
use super::SessionMethodsPath;
use super::{ApiError, ApiResult};
use crate::CLIENTS;
use crate::SOURCES;
use crate::models::{ApiPlayerOptions, ApiSessionBody, ApiSessionInfo};
use crate::util::converter::numbers::FromU64;
use crate::util::decoder::decode_base64;
use crate::voice::manager::CreatePlayerOptions;
use crate::voice::player::{GetApiPlayerInfo, IsActive, Pause, Play, Seek, SetVolume, Stop};
use crate::ws::client::{
    CreatePlayer, DestroyPlayer, GetPlayer, GetWebsocketInfo, UpdateWebsocket, WebSocketClient,
};
use axum::Json;
use axum::body::Body;
use axum::extract::Path;
use axum::extract::Query;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use dashmap::mapref::multiple::RefMulti;
use impero_source::api::{ApiTrack, ApiTrackResult, ResolveOptions};
use kameo::actor::ActorRef;
use serde_json::Value;
use songbird::id::{GuildId, UserId};

// todo: clean this up

async fn get_client(
    session_id: String,
) -> Option<RefMulti<'static, UserId, ActorRef<WebSocketClient>>> {
    for client in CLIENTS.iter() {
        let Some(data) = client.ask(GetWebsocketInfo).await.ok() else {
            continue;
        };
        if session_id == data.session_id {
            return Some(client);
        }
    }
    None
}

pub async fn get_player(
    Path(PlayerMethodsPath {
        session_id,
        guild_id,
    }): Path<PlayerMethodsPath>,
) -> ApiResult<Response> {
    let Some(client) = get_client(session_id.clone()).await else {
        tracing::debug!(
            "Failed to find websocket client for session id: {} and guild id: {}",
            session_id,
            guild_id
        );

        return Ok(StatusCode::NOT_FOUND.into_response());
    };

    let Some(player) = client
        .ask(GetPlayer {
            guild_id: GuildId::from_u64(guild_id),
        })
        .await
        .map_err(ApiError::new)?
    else {
        tracing::debug!("No player found for {}/{}", session_id, guild_id);

        return Ok(StatusCode::NOT_FOUND.into_response());
    };

    let data = player.ask(GetApiPlayerInfo).await.map_err(ApiError::new)?;

    let string = serde_json::to_string_pretty(&data).map_err(ApiError::new)?;

    Ok(Response::new(Body::from(string)))
}

pub async fn update_player(
    query: Query<PlayerUpdateQuery>,
    Path(PlayerMethodsPath {
        session_id,
        guild_id,
    }): Path<PlayerMethodsPath>,
    Json(update_player): Json<ApiPlayerOptions>,
) -> ApiResult<Response> {
    let Some(client) = get_client(session_id.clone()).await else {
        tracing::debug!(
            "Failed to find websocket client for session id: {} and guild id: {}",
            session_id,
            guild_id
        );

        return Ok(StatusCode::NOT_FOUND.into_response());
    };

    let option_player = client
        .ask(GetPlayer {
            guild_id: GuildId::from_u64(guild_id),
        })
        .await
        .map_err(ApiError::new)?;

    if option_player.is_none() && update_player.voice.is_none() {
        tracing::debug!("No player found for {}/{}", session_id, guild_id);

        return Ok(StatusCode::NOT_FOUND.into_response());
    }

    if let Some(server_update) = update_player.voice {
        let options = CreatePlayerOptions {
            guild_id: GuildId::from_u64(guild_id),
            server_update,
            config: None,
        };

        client
            .ask(CreatePlayer { options })
            .await
            .map_err(ApiError::new)?;
    }

    let Some(player) = client
        .ask(GetPlayer {
            guild_id: GuildId::from_u64(guild_id),
        })
        .await
        .map_err(ApiError::new)?
    else {
        tracing::debug!("No player found for {}/{}", session_id, guild_id);

        return Ok(StatusCode::NOT_FOUND.into_response());
    };

    let mut stopped = false;

    let is_active = player.ask(IsActive).await.map_err(ApiError::new)?;
    let should_update_track = !is_active || !query.no_replace.unwrap_or_default();

    if let Some(track) = update_player.track
        && should_update_track
    {
        match track.encoded {
            Value::String(encoded) => {
                player.ask(Play { encoded }).await.map_err(ApiError::new)?;
            }
            _ => {
                player.ask(Stop).await.map_err(ApiError::new)?;
                stopped = true;
            }
        }
    }

    if !stopped {
        if let Some(pause) = update_player.paused {
            player.ask(Pause { pause }).await.map_err(ApiError::new)?;
        }

        if let Some(position) = update_player.position {
            player.ask(Seek { position }).await.map_err(ApiError::new)?;
        }

        if let Some(volume) = update_player.volume {
            player
                .ask(SetVolume {
                    volume: volume as f32,
                })
                .await
                .map_err(ApiError::new)?;
        }
    }

    let data = player.ask(GetApiPlayerInfo).await.map_err(ApiError::new)?;

    let string = serde_json::to_string_pretty(&data).map_err(ApiError::new)?;

    Ok(Response::new(Body::from(string)))
}

#[tracing::instrument]
pub async fn destroy_player(
    Path(PlayerMethodsPath {
        session_id,
        guild_id,
    }): Path<PlayerMethodsPath>,
) -> ApiResult<Response> {
    let Some(client) = get_client(session_id.clone()).await else {
        tracing::debug!(
            "Failed to find websocket client for session id: {} and guild id: {}",
            session_id,
            guild_id
        );

        return Ok(StatusCode::NOT_FOUND.into_response());
    };

    client
        .ask(DestroyPlayer {
            guild_id: GuildId::from_u64(guild_id),
        })
        .await
        .map_err(ApiError::new)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

#[tracing::instrument]
pub async fn update_session(
    Path(SessionMethodsPath { session_id }): Path<SessionMethodsPath>,
    Json(update_session): Json<ApiSessionBody>,
) -> ApiResult<Response> {
    let Some(client) = get_client(session_id.clone()).await else {
        tracing::debug!(
            "Failed to find websocket client for session id: {}",
            session_id,
        );

        return Ok(StatusCode::NOT_FOUND.into_response());
    };

    let data = client
        .ask(UpdateWebsocket {
            resuming: update_session.resuming,
            timeout: update_session.timeout,
        })
        .await
        .map_err(ApiError::new)?;

    let info = ApiSessionInfo {
        resuming_key: data.session_id,
        timeout: data.timeout as u16,
    };

    let string = serde_json::to_string_pretty(&info).map_err(ApiError::new)?;

    Ok(Response::new(Body::from(string)))
}

pub async fn decode(query: Query<DecodeQueryString>) -> ApiResult<Response> {
    let info = decode_base64(&query.track).map_err(ApiError::new)?;

    let track = ApiTrack {
        encoded: query.track.clone(),
        info,
        plugin_info: None,
        user_data: None,
    };

    let string = serde_json::to_string_pretty(&track).map_err(ApiError::new)?;

    Ok(Response::new(Body::from(string)))
}

#[tracing::instrument]
pub async fn encode(query: Query<EncodeQueryString>) -> ApiResult<Response> {
    let mut track = ApiTrackResult::Empty(Value::Array(Vec::new()));

    for source in SOURCES.iter() {
        let plugin = source.value();

        track = plugin
            .resolve(ResolveOptions {
                identifier: query.identifier.clone(),
                ctx: None,
            })
            .await
            .map_err(ApiError::new)?;
    }

    let string = serde_json::to_string_pretty(&track).map_err(ApiError::new)?;

    Ok(Response::new(Body::from(string)))
}
