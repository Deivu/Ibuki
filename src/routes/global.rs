use crate::util::converter::numbers::FromU64;
use crate::ws::client::{
    WebsocketRequestData, handle_websocket_upgrade_error, handle_websocket_upgrade_request,
};
use axum::extract::{ConnectInfo, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use songbird::id::UserId;
use std::net::SocketAddr;

pub async fn landing() -> String {
    String::from("Hello World")
}

#[tracing::instrument]
pub async fn ws(
    websocket_upgrade: WebSocketUpgrade,
    headers: HeaderMap,
    connection: ConnectInfo<SocketAddr>,
) -> Response {
    let Some(user_agent) = headers.get("User-Agent").and_then(|v| v.to_str().ok()) else {
        tracing::warn!("No User-Agent header");

        return StatusCode::NOT_FOUND.into_response();
    };

    let Some(user_id) = headers
        .get("User-Id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
    else {
        tracing::warn!("No User-Id header");

        return StatusCode::NOT_FOUND.into_response();
    };

    let request = WebsocketRequestData {
        user_agent: user_agent.to_string(),
        user_id: UserId::from_u64(user_id),
        session_id: headers
            .get("Session-Id")
            .and_then(|data| data.to_str().ok().map(String::from)),
    };

    tracing::info!(
        "Received a connection request from {}({})",
        user_id,
        user_agent
    );

    // now stop complaining compiler
    let on_error_request = request.clone();
    let on_upgrade_request = request.clone();

    let response = websocket_upgrade
        .on_failed_upgrade(move |error| {
            handle_websocket_upgrade_error(&error, on_error_request, connection)
        })
        .on_upgrade(move |socket| {
            handle_websocket_upgrade_request(socket, on_upgrade_request, connection)
        });

    response
}
