#![recursion_limit = "256"]
use crate::models::{ApiCpu, ApiMemory, ApiNodeMessage, ApiStats};
use crate::source::amazonmusic::source::AmazonMusic;
use crate::source::applemusic::source::AppleMusic;
use crate::source::deezer::source::Deezer;
// use crate::source::gaana::source::Gaana;
use crate::source::http::Http;
use crate::source::jiosaavn::source::JioSaavn;
use crate::source::soundcloud::source::SoundCloud;
use crate::source::spotify::source::Spotify;
use crate::source::songlink::source::Songlink;
use crate::source::youtube::source::Youtube;
use crate::util::config::Config;
use crate::util::routeplanner::RoutePlanner;
use crate::util::headers::generate_headers;
use crate::util::source::{FixAsyncTraitSource, Source};
use crate::util::task::{AddTask, TasksManager};
use crate::ws::client::{SendConnectionMessage, WebSocketClient};
use axum::Router;
use axum::middleware::from_fn;
use axum::routing;
use axum::serve;
use bytesize::ByteSize;
use cap::Cap;
use dashmap::DashMap;
use dotenv::dotenv;
use kameo::actor::ActorRef;
use mimalloc::MiMalloc;
use reqwest::{Client, ClientBuilder};
use songbird::driver::Scheduler;
use songbird::id::UserId;
use std::env::set_var;
use std::net::{IpAddr, SocketAddr};
use std::sync::LazyLock;
use moka::sync::Cache;
use tokio::main;
use tokio::net;
use tokio::task::JoinSet;
use tokio::time::{Duration, Instant};
use tower::ServiceBuilder;
use tracing::Level;
use tracing_subscriber::fmt;

mod constants;
mod middlewares;
mod filters;
mod models;
mod playback;
mod routes;
mod source;
mod util;
mod voice;
mod ws;

#[global_allocator]
static ALLOCATOR: Cap<MiMalloc> = Cap::new(MiMalloc, usize::MAX);
static CONFIG: LazyLock<Config> = LazyLock::new(Config::new);
static SCHEDULER: LazyLock<Scheduler> = LazyLock::new(Scheduler::default);
static CLIENTS: LazyLock<DashMap<UserId, ActorRef<WebSocketClient>>> = LazyLock::new(DashMap::new);
static SOURCES: LazyLock<DashMap<String, FixAsyncTraitSource>> = LazyLock::new(DashMap::new);
static TASKS: LazyLock<TasksManager<String>> = LazyLock::new(TasksManager::default);
static START: LazyLock<Instant> = LazyLock::new(Instant::now);
pub static ROUTE_PLANNER: LazyLock<Option<RoutePlanner>> = LazyLock::new(|| {
    CONFIG.route_planner.as_ref().and_then(|config| {
        match RoutePlanner::new(config) {
            Ok(planner) => Some(planner),
            Err(e) => {
                tracing::error!("Failed to initialize RoutePlanner: {}", e);
                None
            }
        }
    })
});
static REQWEST: LazyLock<Client> = LazyLock::new(|| {
    create_reqwest_client(None)
});

static CLIENT_POOL: LazyLock<Cache<IpAddr, Client>> = LazyLock::new(|| {
    Cache::builder()
        .max_capacity(100)
        .time_to_idle(Duration::from_secs(600))
        .build()
});

fn create_reqwest_client(local_address: Option<IpAddr>) -> Client {
    let mut builder = ClientBuilder::new().default_headers(generate_headers().unwrap());
    if let Some(addr) = local_address {
        builder = builder.local_address(addr);
    }
    builder.build().expect("Failed to create reqwest client")
}

pub fn get_client() -> (Client, Option<IpAddr>) {
    if let Some(planner) = &*ROUTE_PLANNER {
        if let Some(ip) = planner.get_next_ip() {
            if let Some(client) = CLIENT_POOL.get(&ip) {
                return (client, Some(ip));
            }
            let client = create_reqwest_client(Some(ip));
            CLIENT_POOL.insert(ip, client.clone());
            return (client, Some(ip));
        }
    }
    (REQWEST.clone(), None)
}
pub static SYSTEM: LazyLock<tokio::sync::Mutex<sysinfo::System>> = LazyLock::new(|| {
    tokio::sync::Mutex::new(sysinfo::System::new())
});

#[main(flavor = "multi_thread")]
async fn main() {
    unsafe { set_var("RUST_BACKTRACE", "1") };

    dotenv().ok();

    let subscriber = fmt()
        .pretty()
        .with_file(true)
        .with_line_number(true)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_target(true)
        .with_max_level(Level::DEBUG)
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("Failed to set global logger");

    LazyLock::force(&CONFIG);
    LazyLock::force(&ROUTE_PLANNER);
    LazyLock::force(&CLIENTS);
    LazyLock::force(&SOURCES);
    LazyLock::force(&TASKS);
    LazyLock::force(&START);
    LazyLock::force(&REQWEST);

    if CONFIG.youtube_config.is_some() {
        register_source!(Youtube, Some(REQWEST.clone()));
    }
    if CONFIG.deezer_config.is_some() {
        register_source!(Deezer, Some(REQWEST.clone()));
    }
    if CONFIG.jiosaavn_config.is_some() {
        register_source!(JioSaavn, Some(REQWEST.clone()));
    }
    // if CONFIG.gaana_config.is_some() {
    //     register_source!(Gaana, Some(REQWEST.clone()));
    // }
    if CONFIG.http_config.is_some() {
        register_source!(Http, Some(REQWEST.clone()));
    }
    if CONFIG.spotify_config.is_some() {
        register_source!(Spotify, Some(REQWEST.clone()));
    }
    if CONFIG.songlink_config.is_some() {
        register_source!(Songlink, Some(REQWEST.clone()));
    }
    if CONFIG.amazonmusic_config.is_some() {
        register_source!(AmazonMusic, Some(REQWEST.clone()));
    }
    if CONFIG.applemusic_config.is_some() {
        register_source!(AppleMusic, Some(REQWEST.clone()), CONFIG.applemusic_config.as_ref());
    }
    if CONFIG.soundcloud_config.is_some() {
        register_source!(SoundCloud, Some(REQWEST.clone()), CONFIG.soundcloud_config.as_ref());
    }

    create_tasks().await;

    let app = Router::new()
        .route("/v{version}/websocket", routing::any(routes::global::ws))
        .route(
            "/v{version}/info",
            routing::get(routes::endpoints::node_info),
        )
        .route(
            "/v{version}/decodetrack",
            routing::get(routes::endpoints::decode),
        )
        .route(
            "/v{version}/loadtracks",
            routing::get(routes::endpoints::encode),
        )
        .route(
            "/v{version}/sessions/{session_id}/players/{guild_id}",
            routing::get(routes::endpoints::get_player),
        )
        .route(
            "/v{version}/sessions/{session_id}/players/{guild_id}",
            routing::patch(routes::endpoints::update_player),
        )
        .route(
            "/v{version}/sessions/{session_id}/players/{guild_id}",
            routing::delete(routes::endpoints::destroy_player),
        )
        .route(
            "/v{version}/sessions/{session_id}",
            routing::patch(routes::endpoints::update_session),
        )
        .route(
            "/v{version}/sessions/{session_id}/players",
            routing::get(routes::endpoints::get_all_players),
        )
        .route(
            "/v{version}/stats",
            routing::get(routes::endpoints::get_stats),
        )
        .route_layer(
            ServiceBuilder::new()
                .layer(from_fn(middlewares::version::check))
                .layer(from_fn(middlewares::auth::authenticate))
                .layer(from_fn(middlewares::log::request)),
        )
        .route("/version", routing::get(routes::endpoints::version))
        .route("/", routing::get(routes::global::landing))
        .fallback(|request: axum::extract::Request| async move {
            tracing::warn!(
                "Unmatched request: [Method: {}] [URI: {}]",
                request.method(),
                request.uri()
            );
            (
                axum::http::StatusCode::NOT_FOUND,
                format!("Not Found: {} {}", request.method(), request.uri()),
            )
        });

    let listener = net::TcpListener::bind(format!("{}:{}", CONFIG.address, CONFIG.port))
        .await
        .unwrap();

    tracing::info!("Server is bound to {}", listener.local_addr().unwrap());

    serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .ok();
}

async fn create_tasks() {
    let task = AddTask {
        key: "status_interval".to_lowercase(),
        duration: Duration::from_secs(CONFIG.status_update_secs.unwrap_or(30) as u64),
        handler: || async move {
            let global_cpu: f32 = {
                let mut sys = SYSTEM.lock().await;
                sys.refresh_cpu_usage();
                let cpus = sys.cpus();
                if cpus.is_empty() {
                    0.0
                } else {
                    cpus.iter().map(|cpu| cpu.cpu_usage()).sum::<f32>() / cpus.len() as f32
                }
            };

            let Ok(mut stat) = perf_monitor::cpu::ProcessStat::cur() else { return };
            let cores = perf_monitor::cpu::processor_numbers().unwrap_or(1);

            let Ok(process_memory_info) = perf_monitor::mem::get_process_memory_info() else {
                return;
            };

            let Ok(usage) = stat.cpu() else {
                return;
            };
            let process_cpu = usage / cores as f64;

            let used = ALLOCATOR.allocated() as u64;
            let free = ALLOCATOR.remaining() as u64;
            let limit = ALLOCATOR.limit() as u64;

            tracing::debug!(
                "Memory Usage: (Heap => [Used: {:.2}] [Free: {:.2}] [Limit: {:.2}]) (RSS => [{:.2}]) (VM => [{:.2}])",
                ByteSize::b(used).display().si(),
                ByteSize::b(free).display().si(),
                ByteSize::b(limit).display().si(),
                ByteSize::b(process_memory_info.resident_set_size)
                    .display()
                    .si(),
                ByteSize::b(process_memory_info.virtual_memory_size)
                    .display()
                    .si(),
            );

            let stats = ApiStats {
                players: SCHEDULER.total_tasks() as u32,
                playing_players: SCHEDULER.live_tasks() as u32,
                uptime: START.elapsed().as_millis() as u64,
                // todo: api memory is wip
                memory: ApiMemory {
                    free,
                    used,
                    allocated: process_memory_info.resident_set_size,
                    reservable: process_memory_info.virtual_memory_size,
                },
                cpu: ApiCpu {
                    cores: cores as u32,
                    system_load: global_cpu as f64,
                    lavalink_load: process_cpu,
                },
                frame_stats: None,
            };

            let serialized =
                serde_json::to_string(&ApiNodeMessage::Stats(Box::new(stats))).unwrap();

            let set = CLIENTS
                .iter()
                .map(|client| {
                    let message = serialized.clone();
                    async move {
                        let _ = client
                            .tell(SendConnectionMessage {
                                message: message.into(),
                            })
                            .await;
                    }
                })
                .collect::<JoinSet<()>>();

            set.join_all().await;
        },
    };
    TASKS.add(task);
}

