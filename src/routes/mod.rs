use anyhow::Error;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

pub mod endpoints;
pub mod global;

pub type ApiResult<T> = Result<T, ApiError>;

pub struct ApiError(Error);

impl ApiError {
    pub fn new<E>(error: E) -> Self
    where
        E: Into<Error>,
    {
        Self(error.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        tracing::error!(error = ?self.0, "Request failed");
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    }
}

#[derive(Deserialize, Debug)]
pub struct PlayerMethodsPath {
    pub session_id: String,
    pub guild_id: u64,
}

#[derive(Deserialize, Debug)]
pub struct SessionMethodsPath {
    pub session_id: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PlayerUpdateQuery {
    pub no_replace: Option<bool>,
}

#[derive(Deserialize, Debug)]
pub struct DecodeQueryString {
    pub track: String,
}

#[derive(Deserialize, Debug)]
pub struct EncodeQueryString {
    pub identifier: String,
}
