use crate::constants::VERSION;
use axum::extract::Path;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::collections::HashMap;

pub async fn check(
    Path(params): Path<HashMap<String, String>>,
    request: Request,
    next: Next,
) -> Response {
    let Some(version) = params.get("version").and_then(|s| s.parse::<u8>().ok()) else {
        tracing::debug!("No version provided",);

        return StatusCode::NOT_FOUND.into_response();
    };

    if version != VERSION {
        tracing::debug!("Invalid version provided");

        return StatusCode::NOT_FOUND.into_response();
    }

    next.run(request).await
}
