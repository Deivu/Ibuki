#![recursion_limit = "256"]
use crate::filter::library::FilterLibrary;
use crate::models::{ApiCpu, ApiMemory, ApiNodeMessage, ApiStats};
use crate::source::library::SourceLibrary;
use crate::util::config::Config;
use crate::util::headers::generate_headers;
use crate::util::task::{AddTask, TasksManager};
use crate::ws::client::{SendConnectionMessage, WebSocketClient};
use axum::Router;
use axum::middleware::from_fn;
use axum::routing;
use axum::serve;
use cap::Cap;
use dashmap::DashMap;
use dotenv::dotenv;
use kameo::actor::ActorRef;
use mimalloc::MiMalloc;
use reqwest::{Client, ClientBuilder};
use songbird::driver::Scheduler;
use songbird::id::UserId;
use std::env::set_var;
use std::net::SocketAddr;
use std::sync::LazyLock;
use tokio::main;
use tokio::net;
use tokio::task::JoinSet;
use tokio::time::{Duration, Instant};
use tower::ServiceBuilder;
use tracing::Level;
use tracing_subscriber::fmt;

mod constants;
mod filter;
mod middlewares;
mod models;
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
static SESSIONS: LazyLock<DashMap<String, ActorRef<WebSocketClient>>> = LazyLock::new(DashMap::new);
static TASKS: LazyLock<TasksManager<String>> = LazyLock::new(TasksManager::default);
static START: LazyLock<Instant> = LazyLock::new(Instant::now);
static REQWEST: LazyLock<Client> = LazyLock::new(|| {
    let builder = ClientBuilder::new().default_headers(generate_headers().unwrap());
    builder.build().expect("Failed to create reqwest client")
});

#[main(flavor = "multi_thread")]
async fn main() {
    unsafe { set_var("RUST_BACKTRACE", "1") };

    dotenv().ok();

    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

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
    LazyLock::force(&CLIENTS);
    LazyLock::force(&SESSIONS);
    LazyLock::force(&TASKS);
    LazyLock::force(&START);
    LazyLock::force(&REQWEST);

    SourceLibrary::load_sources().await;
    FilterLibrary::load_filters();

    create_tasks().await;

    let app = Router::new()
        .route("/v{version}/websocket", routing::any(routes::global::ws))
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
        .route_layer(
            ServiceBuilder::new()
                .layer(from_fn(middlewares::auth::censor))
                .layer(from_fn(middlewares::version::check))
                .layer(from_fn(middlewares::auth::authenticate))
                .layer(from_fn(middlewares::log::request)),
        )
        .route("/", routing::get(routes::global::landing));

    let listener = net::TcpListener::bind(format!("{}:{}", CONFIG.address, CONFIG.port))
        .await
        .unwrap();

    let local_addr = listener.local_addr().unwrap();

    tracing::info!("Server is bound to {}", local_addr);

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
            let mut stat = perf_monitor::cpu::ProcessStat::cur().unwrap();
            let cores = perf_monitor::cpu::processor_numbers().unwrap();

            let Ok(process_memory_info) = perf_monitor::mem::get_process_memory_info() else {
                return;
            };

            let Ok(usage) = stat.cpu() else {
                return;
            };

            let used = ALLOCATOR.allocated() as u64;
            let free = ALLOCATOR.remaining() as u64;

            let stats = ApiStats {
                players: SCHEDULER.total_tasks() as u32,
                playing_players: SCHEDULER.live_tasks() as u32,
                uptime: START.elapsed().as_millis() as u64,
                memory: ApiMemory {
                    free,
                    used,
                    allocated: process_memory_info.resident_set_size,
                    reservable: process_memory_info.virtual_memory_size,
                },
                cpu: ApiCpu {
                    cores: cores as u32,
                    system_load: usage,
                    lavalink_load: usage,
                },
                // todo: to be implemented
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
