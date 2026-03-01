use axum::{
    Json,
    extract::Path,
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};

use crate::source::youtube::YOUTUBE_MANAGER;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct YoutubeConfigResponse {
    pub refresh_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct YoutubeConfigUpdateRequest {
    #[serde(default = "default_refresh_token")]
    pub refresh_token: String,
    #[serde(default = "default_skip_init")]
    pub skip_initialization: bool,
    pub po_token: Option<String>,
    pub visitor_data: Option<String>,
}

fn default_refresh_token() -> String {
    "x".to_string()
}

fn default_skip_init() -> bool {
    true
}

pub async fn get_youtube_config() -> impl IntoResponse {
    let Some(manager) = YOUTUBE_MANAGER.get() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "message": "YouTube source is not initialised" })),
        )
            .into_response();
    };

    let refresh_token = manager.get_refresh_token().await;
    Json(YoutubeConfigResponse { refresh_token }).into_response()
}

pub async fn update_youtube_config(
    Json(config): Json<YoutubeConfigUpdateRequest>,
) -> impl IntoResponse {
    let Some(manager) = YOUTUBE_MANAGER.get() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "message": "YouTube source is not initialised" })),
        )
            .into_response();
    };

    if config.refresh_token != "x" {
        manager
            .set_oauth_refresh_token(config.refresh_token.clone(), config.skip_initialization)
            .await;
        tracing::debug!(
            "YouTube: OAuth refresh token updated via REST (refresh = {:?})",
            config.refresh_token
        );
    }

    let pt = config.po_token.as_deref();
    let vd = config.visitor_data.as_deref();
    let should_update = pt.is_none()
        || vd.is_none()
        || (pt.is_some_and(|p| !p.is_empty()) && vd.is_some_and(|v| !v.is_empty()));

    if should_update {
        manager
            .update_po_token_and_visitor_data(config.po_token, config.visitor_data)
            .await;
        tracing::debug!("YouTube: PoToken/VisitorData updated via REST");
    }

    StatusCode::NO_CONTENT.into_response()
}

pub async fn get_youtube_oauth_token(
    Path(refresh_token): Path<String>,
) -> impl IntoResponse {
    let Some(manager) = YOUTUBE_MANAGER.get() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "message": "YouTube source is not initialised" })),
        )
            .into_response();
    };

    match manager.create_access_token_from_refresh(&refresh_token).await {
        Ok(token) => Json(serde_json::json!({
            "accessToken":  token.access_token,
            "expiresAt":    token.expires_at,
            "tokenType":    token.token_type,
            "refreshToken": token.refresh_token,
        }))
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "message": format!("Failed to create access token: {:?}", e)
            })),
        )
            .into_response(),
    }
}
