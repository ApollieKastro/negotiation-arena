//! Единый тип ошибок приложения и его маппинг в HTTP.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    BadRequest(String),

    #[error("{0}")]
    Unauthorized(String),

    #[error("{0}")]
    Forbidden(String),

    #[error("{0}")]
    NotFound(String),

    #[error("{0}")]
    Conflict(String),

    #[error("config: {0}")]
    Config(String),

    #[error("{provider}: {message}")]
    Upstream { provider: String, message: String },

    #[error("{0}")]
    Internal(#[from] anyhow::Error),
}

impl AppError {
    pub fn upstream(provider: impl Into<String>, message: impl std::fmt::Display) -> Self {
        Self::Upstream {
            provider: provider.into(),
            message: message.to_string(),
        }
    }

    pub fn internal(message: impl std::fmt::Display) -> Self {
        Self::Internal(anyhow::anyhow!("{message}"))
    }

    fn status(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::Config(_) | Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Upstream { .. } => StatusCode::BAD_GATEWAY,
        }
    }

    /// Сообщение, безопасное для выдачи клиенту (внутренности не раскрываем).
    fn public_message(&self) -> String {
        match self {
            Self::Internal(err) => {
                tracing::error!(error = %err, "внутренняя ошибка");
                "Внутренняя ошибка сервера".to_string()
            }
            Self::Config(msg) => {
                tracing::error!(error = %msg, "ошибка конфигурации");
                "Внутренняя ошибка конфигурации".to_string()
            }
            Self::Upstream { provider, message } => {
                // Тело ответа провайдера может содержать внутренние URL и детали —
                // клиентам отдаём общее сообщение, детали только в лог.
                tracing::warn!(provider, message, "ошибка внешнего провайдера");
                "Внешний сервис недоступен".to_string()
            }
            other => other.to_string(),
        }
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(err: rusqlite::Error) -> Self {
        match &err {
            rusqlite::Error::QueryReturnedNoRows => Self::NotFound("Запись не найдена".into()),
            rusqlite::Error::SqliteFailure(f, _)
                if f.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                Self::Conflict("Нарушение ограничения целостности".into())
            }
            _ => Self::Internal(anyhow::anyhow!("sqlite: {err}")),
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        Self::Internal(anyhow::anyhow!("io: {err}"))
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        Self::Internal(anyhow::anyhow!("serde: {err}"))
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let body = json!({ "error": self.public_message() });
        (self.status(), Json(body)).into_response()
    }
}
