use super::DecodeQueryString;
use super::EncodeQueryString;
use super::PlayerMethodsPath;
use super::PlayerUpdateQuery;
use super::SessionMethodsPath;
use super::{ApiError, ApiResult};
use crate::SESSIONS;
use crate::filter::chain::check_filters;
use crate::models::{ApiPlayerOptions, ApiSessionBody, ApiSessionInfo};
use crate::source::library::SourceLibrary;
use crate::util::converter::numbers::FromU64;
use crate::voice::manager::CreatePlayerOptions;
use crate::voice::player::{
    Connect, GetApiPlayerInfo, IsActive, Pause, Play, Seek, SetFilters, SetVolume, Stop,
};
use crate::ws::client::{
    CreatePlayer, DestroyPlayer, GetPlayer, RemovePlayer, UpdateWebsocket, WebSocketClient,
};
use axum::Json;
use axum::body::Body;
use axum::extract::Path;
use axum::extract::Query;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use impero_source::api::{ApiTrack, ApiTrackResult, ResolveOptions};
use impero_source::util::decode_track;
use kameo::actor::ActorRef;
use serde_json::Value;
use songbird::id::GuildId;

// todo: clean this up

fn get_client(session_id: &str) -> Option<ActorRef<WebSocketClient>> {
    SESSIONS.get(session_id).map(|client| client.clone())
}

pub async fn get_player(
    Path(PlayerMethodsPath {
        session_id,
        guild_id,
    }): Path<PlayerMethodsPath>,
) -> ApiResult<Response> {
    let Some(client) = get_client(&session_id) else {
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
    if let Some(filters) = &update_player.filters
        && let Err(message) = check_filters(filters)
    {
        tracing::debug!(
            "Filters rejected for {}/{}: {}",
            session_id,
            guild_id,
            message
        );

        return Ok((StatusCode::BAD_REQUEST, message).into_response());
    }

    let Some(client) = get_client(&session_id) else {
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
            server_update: server_update.clone(),
            config: None,
        };

        let (player, created) = client
            .ask(CreatePlayer { options })
            .await
            .map_err(ApiError::new)?;

        if let Err(error) = player.wait_for_startup_result().await {
            client
                .ask(RemovePlayer {
                    guild_id: GuildId::from_u64(guild_id),
                    id: player.id(),
                })
                .await
                .map_err(ApiError::new)?;

            tracing::error!("Failed to start player: {:?}", error);

            return Ok(StatusCode::INTERNAL_SERVER_ERROR.into_response());
        }

        if !created {
            player
                .ask(Connect {
                    server_update,
                    config: None,
                })
                .await
                .map_err(ApiError::new)?;
        }
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

    if let Some(filters) = update_player.filters {
        player
            .ask(SetFilters { filters })
            .await
            .map_err(ApiError::new)?;
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
    let Some(client) = get_client(&session_id) else {
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
    let Some(client) = get_client(&session_id) else {
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
    let info = decode_track(&query.track).map_err(ApiError::new)?;

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
    for plugin in SourceLibrary::sources() {
        track = plugin
            .resolve(ResolveOptions {
                identifier: query.identifier.clone(),
                ctx: None,
            })
            .await
            .map_err(ApiError::new)?;

        if !matches!(track, ApiTrackResult::Empty(_)) {
            break;
        }
    }

    let string = serde_json::to_string_pretty(&track).map_err(ApiError::new)?;

    Ok(Response::new(Body::from(string)))
}
