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
use crate::util::decoder::{decode_base64, decode_track};
use crate::util::errors::EndpointError;
use crate::voice::manager::CreatePlayerOptions;
use crate::voice::player::{
    GetApiPlayerInfo, IsActive, Pause, Play, Seek, SetFilters, SetVolume, Stop,
};
use crate::ws::client::{
    CreatePlayer, DestroyPlayer, GetPlayer, GetWebsocketInfo, UpdateWebsocket, WebSocketClient,
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
) -> Result<Response<Body>, EndpointError> {
    let client = get_client(session_id)
        .await
        .ok_or(EndpointError::NoWebsocketClientFound)?;

    let player = client
        .ask(GetPlayer {
            guild_id: GuildId::from_u64(guild_id),
        })
        .await?
        .ok_or(EndpointError::NoPlayerFound)?;

    let data = player.ask(GetApiPlayerInfo).await?;

    let string = serde_json::to_string_pretty(&data)?;

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
        .ok_or(EndpointError::NoWebsocketClientFound)?;

    let option_player = client
        .ask(GetPlayer {
            guild_id: GuildId::from_u64(guild_id),
        })
        .await?;

    if option_player.is_none() {
        let options = CreatePlayerOptions {
            guild_id: GuildId::from_u64(guild_id),
            server_update: update_player.voice.clone(),
            config: None,
        };

        client.ask(CreatePlayer { options }).await?;
    } else if let Some(server_update) = update_player.voice {
        let options = CreatePlayerOptions {
            guild_id: GuildId::from_u64(guild_id),
            server_update: Some(server_update),
            config: None,
        };

        client.ask(CreatePlayer { options }).await?;
    }

    let player = client
        .ask(GetPlayer {
            guild_id: GuildId::from_u64(guild_id),
        })
        .await?
        .ok_or(EndpointError::NoPlayerFound)?;

    let mut stopped = false;

    let is_active = player.ask(IsActive).await?;

    if let Some(encoded) = update_player
        .track
        .as_ref()
        .map(|track| track.encoded.clone())
    {
        if !is_active || !query.no_replace.unwrap_or(false) {
            match encoded {
                Value::String(encoded) => {
                    player
                        .ask(Play {
                            encoded,
                            user_data: update_player
                                .track
                                .as_ref()
                                .and_then(|t| t.user_data.clone()),
                        })
                        .await?;
                }
                _ => {
                    player.ask(Stop).await?;
                    stopped = true;
                }
            }
        }
    }

    if !stopped {
        if let Some(pause) = update_player.paused {
            player.ask(Pause { pause }).await?;
        }

        if let Some(position) = update_player.position {
            player.ask(Seek { position }).await?;
        }

        if let Some(volume) = update_player.volume {
            player
                .ask(SetVolume {
                    volume: volume as f32,
                })
                .await?;
        }

        if let Some(filters) = update_player.filters {
            player.ask(SetFilters { filters }).await?;
        }
    }

    let data = player.ask(GetApiPlayerInfo).await?;

    let string = serde_json::to_string_pretty(&data)?;

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
        .ok_or(EndpointError::NoWebsocketClientFound)?;

    client
        .ask(DestroyPlayer {
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
        .ok_or(EndpointError::NoWebsocketClientFound)?;

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
    let track = decode_track(&query.track).or_else(|_| decode_base64(&query.track))?;

    let track = ApiTrack {
        encoded: query.track.clone(),
        info: track,
        plugin_info: Empty,
        user_data: None,
    };

    let string = serde_json::to_string_pretty(&track)?;

    Ok(Response::new(Body::from(string)))
}

#[tracing::instrument]
pub async fn encode(query: Query<EncodeQueryString>) -> Result<Response<Body>, EndpointError> {
    let mut track = ApiTrackResult::Empty(None);

    for source in SOURCES.iter() {
        let Some(data) = source.to_inner_ref().parse_query(&query.identifier) else {
            continue;
        };

        tracing::info!("Trying source: {}", source.to_inner_ref().get_name());

        track = source
            .to_inner_ref()
            .resolve(data)
            .await?
            .unwrap_or(ApiTrackResult::Empty(None));

        if !matches!(track, ApiTrackResult::Empty(_)) {
            tracing::info!(
                "Track found by source: {}",
                source.to_inner_ref().get_name()
            );
            break;
        }
    }

    let string = serde_json::to_string_pretty(&track)?;

    Ok(Response::new(Body::from(string)))
}

pub async fn node_info() -> Result<Response<Body>, EndpointError> {
    let sources: Vec<String> = SOURCES.iter().map(|entry| entry.key().clone()).collect();

    let info = serde_json::json!({
        "version": {
            "semver": "4.0.0",
            "major": 4,
            "minor": 0,
            "patch": 0,
            "preRelease": null,
            "build": null
        },
        "buildTime": 0,
        "git": {
            "branch": "main",
            "commit": "unknown",
            "commitTime": 0
        },
        "sourceManagers": sources,
        "filters": [
            "volume",
            "equalizer",
            "timescale",
            "tremolo",
            "vibrato",
            "rotation",
            "distortion",
            "channelMix",
            "lowPass",
            "karaoke"
        ],
    });

    let string = serde_json::to_string_pretty(&info)?;

    Ok(Response::builder()
        .header("Content-Type", "application/json")
        .body(Body::from(string))
        .unwrap())
}

pub async fn version() -> Response<Body> {
    Response::builder()
        .header("Content-Type", "text/plain")
        .body(Body::from("4.0.0"))
        .unwrap()
}

pub async fn get_stats() -> Result<Response<Body>, EndpointError> {
    let mut sys = crate::SYSTEM.lock().await;
    sys.refresh_cpu_usage();
    let cpus = sys.cpus();
    let global_cpu: f32 = if cpus.is_empty() {
        0.0
    } else {
        cpus.iter().map(|cpu| cpu.cpu_usage()).sum::<f32>() / cpus.len() as f32
    };

    let cores = perf_monitor::cpu::processor_numbers().unwrap_or(1);
    let process_cpu = if let Ok(mut stat) = perf_monitor::cpu::ProcessStat::cur() {
        stat.cpu().unwrap_or(0.0) / cores as f64
    } else {
        0.0
    };

    let used = crate::ALLOCATOR.allocated() as u64;
    let free = crate::ALLOCATOR.remaining() as u64;

    let process_memory_info = perf_monitor::mem::get_process_memory_info()
        .map_err(|e| EndpointError::FailedMessage(format!("Failed to get memory info: {}", e)))?;

    let stats = crate::models::ApiStats {
        players: crate::SCHEDULER.total_tasks() as u32,
        playing_players: crate::SCHEDULER.live_tasks() as u32,
        uptime: crate::START.elapsed().as_millis() as u64,
        memory: crate::models::ApiMemory {
            free,
            used,
            allocated: process_memory_info.resident_set_size,
            reservable: process_memory_info.virtual_memory_size,
        },
        cpu: crate::models::ApiCpu {
            cores: cores as u32,
            system_load: global_cpu as f64,
            lavalink_load: process_cpu,
        },
        frame_stats: None,
    };

    let string = serde_json::to_string_pretty(&stats)?;
    Ok(Response::builder()
        .header("Content-Type", "application/json")
        .body(Body::from(string))
        .unwrap())
}

pub async fn get_all_players(
    Path(SessionMethodsPath { session_id }): Path<SessionMethodsPath>,
) -> Result<Response<Body>, EndpointError> {
    let client = get_client(session_id)
        .await
        .ok_or(EndpointError::NoWebsocketClientFound)?;

    let players = client.ask(crate::ws::client::GetAllPlayers).await?;

    let mut player_list = Vec::new();
    for (_guild_id, player_ref) in players {
        match player_ref.ask(GetApiPlayerInfo).await {
            Ok(data) => player_list.push(data),
            Err(e) => tracing::error!(
                "Failed to GetApiPlayerInfo for guild {}: {:?}",
                _guild_id,
                e
            ),
        }
    }

    let string = serde_json::to_string_pretty(&player_list)?;
    Ok(Response::builder()
        .header("Content-Type", "application/json")
        .body(Body::from(string))
        .unwrap())
}
