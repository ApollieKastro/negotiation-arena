//! Extractor'ы тела и query с маппингом отказов в единый [`AppError`].
//!
//! Стандартные `Json`/`Query` axum отвечают своими форматами ошибок;
//! здесь отказы приводятся к общему JSON `{"error": "..."}`.

use async_trait::async_trait;
use axum::extract::{FromRequest, Query, Request};
use serde::de::DeserializeOwned;

use crate::error::AppError;

/// JSON-тело запроса; ошибка десериализации — `400` в формате приложения.
pub struct AppJson<T>(pub T);

#[async_trait]
impl<S, T> FromRequest<S> for AppJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match axum::extract::Json::<T>::from_request(req, state).await {
            Ok(axum::extract::Json(value)) => Ok(AppJson(value)),
            Err(err) => Err(AppError::BadRequest(format!(
                "некорректное тело запроса: {err}"
            ))),
        }
    }
}

/// Query-параметры; ошибка разбора — `400` в формате приложения.
pub struct AppQuery<T>(pub T);

#[async_trait]
impl<S, T> FromRequest<S> for AppQuery<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        // Query извлекает только из URI — тело не нужно.
        let (parts, _body) = req.into_parts();
        let uri = parts.uri.clone();
        let _ = state;
        match Query::<T>::try_from_uri(&uri) {
            Ok(Query(value)) => Ok(AppQuery(value)),
            Err(err) => Err(AppError::BadRequest(format!(
                "некорректные query-параметры: {err}"
            ))),
        }
    }
}
