use crate::CONFIG;
use crate::util::errors::EndpointError;
use axum::{body::Body, extract::Request, http::Response, middleware::Next};

pub async fn authenticate(request: Request, next: Next) -> Result<Response<Body>, EndpointError> {
    let authorization = request
        .headers()
        .get("Authorization")
        .ok_or(EndpointError::MissingOption("Authorization"))?
        .to_str()?;

    if authorization != CONFIG.authorization {
        return Err(EndpointError::Unauthorized);
    }

    Ok(next.run(request).await)
}
