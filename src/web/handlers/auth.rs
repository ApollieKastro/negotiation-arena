//! Публичная аутентификация: регистрация, вход, refresh, профиль.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use crate::application::auth::AuthSession;
use crate::domain::entities::user::User;
use crate::error::AppResult;
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
pub async fn register(
    State(state): State<AppState>,
    AppJson(req): AppJson<RegisterRequest>,
) -> AppResult<(StatusCode, Json<User>)> {
    let user =
        state
            .services
            .auth
            .register(&req.login, &req.password, req.display_name.as_deref())?;
    Ok((StatusCode::CREATED, Json(user)))
}

/// `POST /api/v1/auth/login` — выдаёт JWT.
pub async fn login(
    State(state): State<AppState>,
    AppJson(req): AppJson<LoginRequest>,
) -> AppResult<Json<AuthSession>> {
    let session = state.services.auth.login(&req.login, &req.password)?;
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
