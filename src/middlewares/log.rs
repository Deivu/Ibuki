use crate::util::errors::EndpointError;
use axum::body::Body;
use axum::extract::Path;
use axum::extract::Request;
use axum::http::Response;
use axum::middleware::Next;
use std::collections::HashMap;

#[tracing::instrument]
pub async fn request(
    Path(params): Path<HashMap<String, String>>,
    request: Request,
    next: Next,
) -> Result<Response<Body>, EndpointError> {
    tracing::info!("Received a request! [Endpoint: {}]", request.uri());

    Ok(next.run(request).await)
}
