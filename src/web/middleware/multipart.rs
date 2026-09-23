//! Extractor multipart-тела с маппингом отказа в единый [`AppError`].

use async_trait::async_trait;
use axum::extract::{FromRequest, Multipart, Request};

use crate::error::AppError;

/// multipart/form-data тело; ошибка разбора — `400` в формате приложения
/// `{"error": "..."}` (стандартный `Multipart` отвечает своим форматом).
pub struct AppMultipart(pub Multipart);

#[async_trait]
impl<S> FromRequest<S> for AppMultipart
where
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Multipart::from_request(req, state).await {
            Ok(multipart) => Ok(AppMultipart(multipart)),
            Err(err) => Err(AppError::BadRequest(format!(
                "некорректное multipart-тело: {err}"
            ))),
        }
    }
}
