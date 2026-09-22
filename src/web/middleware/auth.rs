//! Извлечение аутентифицированного пользователя из `Authorization: Bearer`.

use async_trait::async_trait;
use axum::extract::FromRequestParts;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;

use crate::application::auth::{AuthContext, Permission};
use crate::error::{AppError, AppResult};
use crate::web::state::AppState;

/// Аутентифицированный контекст запроса.
///
/// Читает `Authorization: Bearer <jwt>`, проверяет подпись и активность
/// пользователя через [`AuthService::verify`](crate::application::auth::AuthService::verify).
/// Роль берётся из БД — разжалование действует без переиздания токена.
#[derive(Debug, Clone)]
pub struct AuthUser(pub AuthContext);

impl AuthUser {
    pub fn context(&self) -> &AuthContext {
        &self.0
    }

    /// Проверяет право текущего пользователя (RBAC).
    pub fn require(&self, permission: Permission) -> AppResult<()> {
        self.0.require(permission)
    }

    pub fn user_id(&self) -> &str {
        &self.0.user_id
    }

    pub fn is_admin(&self) -> bool {
        self.0.is_admin()
    }
}

#[async_trait]
impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get(AUTHORIZATION)
            .ok_or_else(|| AppError::Unauthorized("отсутствует заголовок Authorization".into()))?
            .to_str()
            .map_err(|_| AppError::Unauthorized("некорректный заголовок Authorization".into()))?;

        let ctx = state.services.auth.verify(header)?;
        Ok(AuthUser(ctx))
    }
}
