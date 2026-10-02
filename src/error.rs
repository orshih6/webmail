use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("not signed in")]
    Unauthorized,
    #[error("invalid or missing CSRF token")]
    Csrf,
    #[error("wrong email or password")]
    BadLogin,
    #[error("too many attempts, try again later")]
    RateLimited,
    #[error("{0}")]
    BadRequest(String),
    #[error("not found")]
    NotFound,
    /// The mail server answered, but refused or failed the operation.
    #[error("mail server: {0}")]
    Mail(String),
    /// The mail server could not be reached or did not answer in time. Transient: the UI
    /// shows a "reconnecting" banner instead of an error.
    #[error("{0}")]
    Unavailable(String),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

#[derive(Serialize)]
struct Body {
    error: String,
    /// Machine-readable kind, for errors the UI handles specially.
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<&'static str>,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match &self {
            Self::Unauthorized | Self::BadLogin => StatusCode::UNAUTHORIZED,
            Self::Csrf => StatusCode::FORBIDDEN,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Mail(_) => StatusCode::BAD_GATEWAY,
            Self::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let error = match &self {
            Self::Internal(e) => {
                tracing::error!(error = ?e, "internal error");
                "internal error".to_owned()
            }
            Self::Mail(e) => {
                tracing::warn!(error = %e, "mail server error");
                self.to_string()
            }
            Self::Unavailable(e) => {
                tracing::warn!(error = %e, "mail server unavailable");
                self.to_string()
            }
            _ => self.to_string(),
        };
        let code = match &self {
            Self::Unavailable(_) => Some("mail_unavailable"),
            Self::Unauthorized => Some("unauthorized"),
            _ => None,
        };
        (status, Json(Body { error, code })).into_response()
    }
}

impl From<async_imap::error::Error> for AppError {
    fn from(e: async_imap::error::Error) -> Self {
        use async_imap::error::Error;
        match e {
            // The connection itself failed: transient, not a refusal.
            Error::Io(_) | Error::ConnectionLost => Self::Unavailable(UNREACHABLE.into()),
            e => Self::Mail(e.to_string()),
        }
    }
}

pub const UNREACHABLE: &str = "can't reach the mail server";

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        Self::Internal(e.into())
    }
}
