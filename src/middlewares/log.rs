use axum::extract::Path;
use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use std::collections::HashMap;

#[tracing::instrument]
pub async fn request(
    Path(params): Path<HashMap<String, String>>,
    request: Request,
    next: Next,
) -> Response {
    tracing::info!(
        "Received a request: [Method: {}] [Endpoint: {}]",
        request.method(),
        request.uri()
    );
    next.run(request).await
}
