use crate::util::converter::numbers::FromU64;
use crate::util::errors::EndpointError;
use crate::ws::client::{
    WebsocketRequestData, handle_websocket_upgrade_error, handle_websocket_upgrade_request,
};
use axum::body::Body;
use axum::extract::{ConnectInfo, WebSocketUpgrade};
use axum::http::{HeaderMap, Response};
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
) -> Result<Response<Body>, EndpointError> {
    let user_agent = headers
        .get("User-Agent")
        .ok_or(EndpointError::MissingOption("User-Agent"))?
        .to_str()?;

    let user_id = headers
        .get("User-Id")
        .ok_or(EndpointError::MissingOption("User-Id"))?
        .to_str()?
        .parse::<u64>()?;

    let request = WebsocketRequestData {
        user_agent: user_agent.into(),
        user_id: UserId::from_u64(user_id),
        session_id: headers
            .get("Session-Id")
            .and_then(|data| data.to_str().map_or(None, |data| data.parse::<u128>().ok())),
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

    Ok(response)
}
