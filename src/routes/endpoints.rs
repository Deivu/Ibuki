use super::DecodeQueryString;
use super::EncodeQueryString;
use super::PlayerMethodsPath;
use super::PlayerUpdateQuery;
use super::SessionMethodsPath;
use crate::CLIENTS;
use crate::SOURCES;
use crate::models::{
    ApiPlayerOptions, ApiSessionBody, ApiSessionInfo, ApiTrack, ApiTrackResult, Empty,
};
use crate::util::converter::numbers::FromU64;
use crate::util::decoder::decode_base64;
use crate::util::errors::EndpointError;
use crate::util::source::Source;
use crate::util::source::Sources;
use crate::voice::manager::CreatePlayerOptions;
use crate::ws::client::{
    CreatePlayer, DisconnectPlayer, GetPlayer, GetWebsocketInfo, UpdateWebsocket, WebSocketClient,
};
use axum::Json;
use axum::body::Body;
use axum::extract::Path;
use axum::extract::Query;
use axum::response::Response;
use dashmap::mapref::multiple::RefMulti;
use kameo::actor::ActorRef;
use serde_json::Value;
use songbird::id::{GuildId, UserId};
use std::sync::atomic::Ordering;

async fn get_client(
    session_id: u128,
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
) -> Result<Response<Body>, EndpointError> {
    let client = get_client(session_id)
        .await
        .ok_or(EndpointError::NotFound)?;

    let player = client
        .ask(GetPlayer {
            guild_id: GuildId::from_u64(guild_id),
        })
        .await?
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
    let client = get_client(session_id)
        .await
        .ok_or(EndpointError::NotFound)?;

    let option_player = client
        .ask(GetPlayer {
            guild_id: GuildId::from_u64(guild_id),
        })
        .await?;

    if option_player.is_none() && update_player.voice.is_none() {
        return Err(EndpointError::NotFound);
    }

    if let Some(server_update) = update_player.voice {
        let options = CreatePlayerOptions {
            guild_id: GuildId::from_u64(guild_id),
            server_update,
            config: None,
        };

        client.ask(CreatePlayer { options }).await?;
    }

    let player = client
        .ask(GetPlayer {
            guild_id: GuildId::from_u64(guild_id),
        })
        .await?
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
    let client = get_client(session_id)
        .await
        .ok_or(EndpointError::NotFound)?;

    client
        .ask(DisconnectPlayer {
            guild_id: GuildId::from_u64(guild_id),
        })
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

    let data = client
        .ask(UpdateWebsocket {
            resuming: update_session.resuming,
            timeout: update_session.timeout,
        })
        .await?;

    let info = ApiSessionInfo {
        resuming_key: data.session_id,
        timeout: data.timeout as u16,
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
