use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RelayError {
    #[error("invalid config: {0}")]
    ConfigInvalid(String),
    #[error("invalid request: {0}")]
    BadRequest(String),
    #[error("captcha rejected")]
    CaptchaRejected,
    #[error("captcha verification request failed: {0}")]
    CaptchaRequest(reqwest::Error),
    #[error("captcha response decode failed: {0}")]
    CaptchaDecode(reqwest::Error),
    #[error("banned")]
    Banned,
    #[error("not found: {0}")]
    NotFound(String),
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error("upstream rejected: {0}")]
    Upstream(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl RelayError {
    pub fn status(&self) -> StatusCode {
        match self {
            RelayError::ConfigInvalid(_) => StatusCode::INTERNAL_SERVER_ERROR,
            RelayError::BadRequest(_) => StatusCode::BAD_REQUEST,
            RelayError::CaptchaRejected => StatusCode::UNAUTHORIZED,
            RelayError::CaptchaRequest(_) | RelayError::CaptchaDecode(_) => StatusCode::BAD_GATEWAY,
            RelayError::Banned => StatusCode::FORBIDDEN,
            RelayError::NotFound(_) => StatusCode::NOT_FOUND,
            RelayError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            RelayError::Upstream(_) => StatusCode::BAD_GATEWAY,
            RelayError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl From<ech_db_client::Error> for RelayError {
    fn from(error: ech_db_client::Error) -> Self {
        RelayError::Upstream(error.to_string())
    }
}

impl From<ech_db_entity::Error> for RelayError {
    fn from(error: ech_db_entity::Error) -> Self {
        RelayError::Internal(error.to_string())
    }
}

impl From<ech_db_entity::ReadError> for RelayError {
    fn from(error: ech_db_entity::ReadError) -> Self {
        match error {
            ech_db_entity::ReadError::Client(error) => error.into(),
            ech_db_entity::ReadError::Entity(error) => error.into(),
            ech_db_entity::ReadError::Protocol => {
                RelayError::Internal("protocol violation".into())
            }
            ech_db_entity::ReadError::PageLimit => RelayError::Internal("page limit".into()),
        }
    }
}

impl IntoResponse for RelayError {
    fn into_response(self) -> Response {
        let status = self.status();
        (status, Json(serde_json::json!({ "error": self.to_string() }))).into_response()
    }
}
