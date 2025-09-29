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
use crate::{SOURCES, CLIENTS};
use axum::Json;
use axum::extract::Path;
use axum::{body::Body, extract::Query, response::Response};
use serde_json::Value;
use songbird::id::GuildId;
use std::num::NonZeroU64;
use std::sync::atomic::Ordering;

pub async fn get_player(
    Path(PlayerMethodsPath {
        session_id,
        guild_id,
    }): Path<PlayerMethodsPath>,
) -> Result<Response<Body>, EndpointError> {
    let client = CLIENTS
        .iter()
        .find(|client| client.session_id == session_id)
        .ok_or(EndpointError::NotFound)?;

    let id = GuildId::from(NonZeroU64::try_from(IbukiGuildId(guild_id))?);

    let player = client
        .player_manager
        .get_player(&id)
        .ok_or(EndpointError::NotFound)?;

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
    let client = CLIENTS
        .iter()
        .find(|client| client.session_id == session_id)
        .ok_or(EndpointError::NotFound)?;

    let id = GuildId::from(NonZeroU64::try_from(IbukiGuildId(guild_id))?);

    if client.player_manager.get_player(&id).is_none() && update_player.voice.is_none() {
        return Err(EndpointError::NotFound);
    }

    if let Some(update_voice) = update_player.voice {
        client
            .player_manager
            .create_player(id, update_voice, None)
            .await?;
    }

    let player = client
        .player_manager
        .get_player(&id)
        .ok_or(EndpointError::NotFound)?;

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
    let client = CLIENTS
        .iter()
        .find(|client| client.session_id == session_id)
        .ok_or(EndpointError::NotFound)?;

    let id = GuildId::from(NonZeroU64::try_from(IbukiGuildId(guild_id))?);

    client.player_manager.disconnect_player(&id).await;

    Ok(Response::new(Body::from(())))
}

#[tracing::instrument]
pub async fn update_session(
    Path(SessionMethodsPath { session_id }): Path<SessionMethodsPath>,
    Json(update_session): Json<ApiSessionBody>,
) -> Result<Response<Body>, EndpointError> {
    let mut client = CLIENTS
        .iter_mut()
        .find(|client| client.session_id == session_id)
        .ok_or(EndpointError::NotFound)?;

    client.resume = update_session.resuming;
    client.timeout = update_session.timeout as u16;

    let info = ApiSessionInfo {
        resuming_key: client.session_id,
        timeout: client.timeout,
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
        let mut result = ApiTrackResult::Empty(None);

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

            if result != ApiTrackResult::Empty(None) {
                break;
            }
        }

        result
    };

    let string = serde_json::to_string_pretty(&track)?;

    Ok(Response::new(Body::from(string)))
}
