//! Публичная аутентификация: регистрация, вход, refresh, профиль.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use crate::application::auth::AuthSession;
use crate::domain::entities::user::User;
use crate::error::{AppError, AppResult};
use crate::web::middleware::{AppJson, AuthUser};
use crate::web::state::AppState;

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub login: String,
    pub password: String,
    #[serde(default)]
    pub display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub login: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct RefreshRequest {
    pub token: String,
}

/// `POST /api/v1/auth/register` — публичная регистрация с ролью `user`.
///
/// Argon2 синхронный и дорогой: уводим в `spawn_blocking`, чтобы не
/// блокировать токен-рантайм axum на время хеширования.
pub async fn register(
    State(state): State<AppState>,
    AppJson(req): AppJson<RegisterRequest>,
) -> AppResult<(StatusCode, Json<User>)> {
    let auth = state.services.auth.clone();
    let login = req.login;
    let password = req.password;
    let display_name = req.display_name;
    let user = tokio::task::spawn_blocking(move || {
        auth.register(&login, &password, display_name.as_deref())
    })
    .await
    .map_err(|e| AppError::internal(format!("регистрация прервана: {e}")))??;
    Ok((StatusCode::CREATED, Json(user)))
}

/// `POST /api/v1/auth/login` — выдаёт JWT.
///
/// Как и регистрация: Argon2 в `spawn_blocking`.
pub async fn login(
    State(state): State<AppState>,
    AppJson(req): AppJson<LoginRequest>,
) -> AppResult<Json<AuthSession>> {
    let auth = state.services.auth.clone();
    let login = req.login;
    let password = req.password;
    let session = tokio::task::spawn_blocking(move || auth.login(&login, &password))
        .await
        .map_err(|e| AppError::internal(format!("вход прерван: {e}")))??;
    Ok(Json(session))
}

/// `POST /api/v1/auth/refresh` — продлевает валидный токен.
pub async fn refresh(
    State(state): State<AppState>,
    AppJson(req): AppJson<RefreshRequest>,
) -> AppResult<Json<AuthSession>> {
    let session = state.services.auth.refresh(&req.token)?;
    Ok(Json(session))
}

/// `GET /api/v1/auth/me` — текущий аутентифицированный пользователь.
pub async fn me(user: AuthUser) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "user_id": user.user_id(),
        "login": user.context().login,
        "role": user.context().role,
    }))
}
