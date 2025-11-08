use crate::constants::VERSION;
use crate::util::errors::EndpointError;
use axum::body::Body;
use axum::extract::Path;
use axum::extract::Request;
use axum::http::Response;
use axum::middleware::Next;
use std::collections::HashMap;

pub async fn check(
    Path(params): Path<HashMap<String, String>>,
    request: Request,
    next: Next,
) -> Result<Response<Body>, EndpointError> {
    if params
        .get("version")
        .ok_or(EndpointError::UnprocessableEntity("Unsupported version"))?
        .as_str()
        != VERSION.to_string().as_str()
    {
        return Err(EndpointError::UnprocessableEntity("Unsupported version"));
    }

    Ok(next.run(request).await)
}
