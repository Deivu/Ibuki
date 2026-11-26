use crate::CONFIG;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

pub async fn censor(mut request: Request, next: Next) -> Response {
    if let Some(value) = request.headers_mut().get_mut("Authorization") {
        value.set_sensitive(true);
    }

    next.run(request).await
}

#[tracing::instrument]
pub async fn authenticate(request: Request, next: Next) -> Response {
    let Some(auth) = request
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
    else {
        tracing::warn!("Missing auth header");

        return StatusCode::UNAUTHORIZED.into_response();
    };

    if auth != CONFIG.authorization {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    next.run(request).await
}
