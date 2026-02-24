use axum::body::Body;
use axum::http::{self, StatusCode};
use axum::response::{IntoResponse, Response};
use kameo::error::HookError;
use kameo::prelude::SendError;
use std::sync::Arc;
use thiserror::Error;

macro_rules! impl_send_error_for {
    ($($ty:ty),+) => {
        $(impl<M, E> From<SendError<M, E>> for $ty
        where
            E: std::fmt::Debug,
        {
            fn from(error: SendError<M, E>) -> Self {
                Self::FailedMessage(format!("{:?}", error))
            }
        })+
    }
}

macro_rules! impl_arc_error_for {
    ($enum:ty { $($variant:ident => $err:ty),+ $(,)? }) => {
        $(
            impl From<$err> for $enum {
                fn from(err: $err) -> Self {
                    Self::$variant(std::sync::Arc::new(err))
                }
            }
        )+
    };
}

#[derive(Error, Debug)]
pub enum ConverterError {
    #[error("Tried to convert {0} to NonZero64 but failed")]
    NonZeroU64(u64),
}

#[derive(Error, Debug)]
pub enum SeekableInitError {
    #[error("Request was not sent due to [{0}]")]
    FailedGet(String),
    #[error("Response received is not ok [{0}]")]
    FailedStatusCode(String),
    #[error("Invalid retry header received [{0}]")]
    InvalidRetryHeader(String),
    #[error("Retry again after [{0}s]")]
    RetryIn(u64),
}

#[derive(Error, Debug)]
pub enum ResolverError {
    #[error("Important data missing: {0}")]
    MissingRequiredData(&'static str),
    #[error("Response received is not ok [{0}]")]
    FailedStatusCode(String),
    #[error("Source {0} is not supported")]
    InvalidSource(String),
    #[error("Invalid URL provided")]
    InvalidUrl,
    #[error("Decryption error: {0}")]
    DecryptionError(String),
    #[error("{0}")]
    Custom(String),
    #[error(transparent)]
    SeekableInit(#[from] SeekableInitError),
    #[error(transparent)]
    Base64Encode(#[from] Base64EncodeError),
    #[error(transparent)]
    SerdeJson(#[from] serde_json::Error),
    #[error(transparent)]
    AudioStream(#[from] songbird::input::AudioStreamError),
    #[error(transparent)]

    Reqwest(#[from] reqwest::Error),
    #[error(transparent)]
    ToStr(#[from] reqwest::header::ToStrError),
}

#[derive(Error, Debug)]
pub enum PlayerManagerError {
    #[error(transparent)]
    Player(#[from] PlayerError),
    #[error(transparent)]
    Connection(#[from] songbird::error::ConnectionError),
    #[error(transparent)]
    Control(#[from] songbird::error::ControlError),
    #[error("Failed to send a message to a task: {0}")]
    FailedMessage(String),
    #[error("Expected a player but got none")]
    MissingPlayer,
    #[error("A connection is required to execute this action")]
    MissingConnection,
}

use kameo::Reply;

#[derive(Error, Clone, Debug, Reply)]
pub enum PlayerError {
    #[error("A driver is required to execute this action")]
    MissingDriver,
    #[error("A connection is required to execute this action")]
    MissingConnection,
    #[error("Failed to send a message to a task: {0}")]
    FailedMessage(String),
    #[error(transparent)]
    Base64Decode(#[from] Arc<Base64DecodeError>),
    #[error(transparent)]
    Connection(#[from] Arc<songbird::error::ConnectionError>),
    #[error(transparent)]
    Resolver(#[from] Arc<ResolverError>),
    #[error(transparent)]
    Control(#[from] Arc<songbird::error::ControlError>),
}

#[derive(Error, Debug)]
pub enum Base64DecodeError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Utf8(#[from] std::string::FromUtf8Error),
    #[error(transparent)]
    Base64Decode(#[from] base64::DecodeError),
    #[error("Unknown version detected. Got {0}")]
    UnknownVersion(u8),
    #[error("{0}")]
    Custom(String),
}

#[derive(Error, Debug)]
pub enum Base64EncodeError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Error, Debug)]
pub enum EndpointError {
    #[error("Unauthorized")]
    Unauthorized,
    #[error("No websocket client found for the session id provided")]
    NoWebsocketClientFound,
    #[error("No player found for the guild id provided")]
    NoPlayerFound,
    #[error(
        "No player found for the guild id provided, and the supplied data is missing a voice update data"
    )]
    NoPlayerAndVoiceUpdateFound,
    #[error("Required option {0} missing in headers")]
    MissingOption(&'static str),
    #[error("Unprocessable Entity due to: {0}")]
    UnprocessableEntity(&'static str),
    #[error("Invalid IP address: {0}")]
    InvalidIpAddress(String),
    #[error("Failed to send a message to a task: {0}")]
    FailedMessage(String),
    #[error(transparent)]
    JsonError(#[from] serde_json::Error),
    #[error(transparent)]
    Base64Decode(#[from] Base64DecodeError),
    #[error(transparent)]
    Base64Encode(#[from] Base64EncodeError),
    #[error(transparent)]
    ToStr(#[from] http::header::ToStrError),
    #[error(transparent)]
    ParseInt(#[from] std::num::ParseIntError),
    #[error(transparent)]
    Resolver(#[from] ResolverError),
    #[error(transparent)]
    Converter(#[from] ConverterError),
    #[error(transparent)]
    PlayerManager(#[from] PlayerManagerError),
    #[error(transparent)]
    PlayerError(#[from] PlayerError),
}

impl_send_error_for!(EndpointError, PlayerError, PlayerManagerError);
impl_arc_error_for!(PlayerError {
    Base64Decode => Base64DecodeError,
    Connection   => songbird::error::ConnectionError,
    Resolver     => ResolverError,
    Control      => songbird::error::ControlError,
});

impl From<HookError<PlayerError>> for PlayerManagerError {
    fn from(error: HookError<PlayerError>) -> Self {
        match error {
            HookError::Panicked(panic) => Self::FailedMessage(panic.to_string()),
            HookError::Error(error) => Self::Player(error),
        }
    }
}

impl IntoResponse for EndpointError {
    #[tracing::instrument]
    fn into_response(self) -> Response<Body> {
        tracing::warn!(
            "Something Happened when processing this endpoint: {:?}",
            self
        );

        let tuple = match self {
            EndpointError::MissingOption(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            EndpointError::UnprocessableEntity(_) => {
                (StatusCode::UNPROCESSABLE_ENTITY, self.to_string())
            }
            EndpointError::Base64Decode(base64_decode_error) => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                base64_decode_error.to_string(),
            ),
            EndpointError::Base64Encode(base64_encode_error) => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                base64_encode_error.to_string(),
            ),
            EndpointError::ToStr(to_str_error) => {
                (StatusCode::UNPROCESSABLE_ENTITY, to_str_error.to_string())
            }
            EndpointError::ParseInt(parse_int_error) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                parse_int_error.to_string(),
            ),
            EndpointError::JsonError(error) => {
                (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
            }
            EndpointError::Resolver(resolver_error) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                resolver_error.to_string(),
            ),
            EndpointError::NoWebsocketClientFound => (StatusCode::NOT_FOUND, self.to_string()),
            EndpointError::NoPlayerFound => (StatusCode::NOT_FOUND, self.to_string()),
            EndpointError::NoPlayerAndVoiceUpdateFound => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            EndpointError::Converter(converter_error) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                converter_error.to_string(),
            ),
            EndpointError::PlayerManager(player_manager_error) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                player_manager_error.to_string(),
            ),
            EndpointError::PlayerError(player_error) => {
                (StatusCode::INTERNAL_SERVER_ERROR, player_error.to_string())
            }
            EndpointError::FailedMessage(actor_error) => {
                (StatusCode::INTERNAL_SERVER_ERROR, actor_error.to_string())
            }
            EndpointError::InvalidIpAddress(_) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            EndpointError::Unauthorized => (StatusCode::FORBIDDEN, self.to_string()),
        };

        tuple.into_response()
    }
}
