#![recursion_limit = "256"]
use crate::models::{ApiCpu, ApiMemory, ApiNodeMessage, ApiStats};
use crate::source::deezer::source::Deezer;
use crate::source::http::Http;
use crate::source::youtube::Youtube;
use crate::util::config::Config;
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
static SOURCES: LazyLock<DashMap<String, FixAsyncTraitSource>> = LazyLock::new(DashMap::new);
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
    if CONFIG.http_config.is_some() {
        register_source!(Http, Some(REQWEST.clone()));
    }

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
                .layer(from_fn(middlewares::version::check))
                .layer(from_fn(middlewares::auth::authenticate))
                .layer(from_fn(middlewares::log::request)),
        )
        .route("/", routing::get(routes::global::landing));

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
                // todo: get actual system load later
                cpu: ApiCpu {
                    cores: cores as u32,
                    system_load: usage,
                    lavalink_load: usage,
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
