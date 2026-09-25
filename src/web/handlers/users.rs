//! Управление пользователями (RBAC: `ManageUsers`).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use crate::domain::entities::user::{User, UserRole};
use crate::error::AppResult;
use crate::web::middleware::{AppJson, AuthUser};
use crate::web::state::AppState;

#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    pub login: String,
    pub password: String,
    pub role: UserRole,
    #[serde(default)]
    pub display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RoleRequest {
    pub role: UserRole,
}

#[derive(Debug, Deserialize)]
pub struct ActiveRequest {
    pub is_active: bool,
}

/// `GET /api/v1/users` — список пользователей.
pub async fn list(State(state): State<AppState>, user: AuthUser) -> AppResult<Json<Vec<User>>> {
    let users = state.services.auth.list_users(user.context())?;
    Ok(Json(users))
}

/// `POST /api/v1/users` — создание пользователя администратором.
///
/// Argon2 синхронный и дорогой: уводим в `spawn_blocking`.
pub async fn create(
    State(state): State<AppState>,
    actor: AuthUser,
    AppJson(req): AppJson<CreateUserRequest>,
) -> AppResult<(StatusCode, Json<User>)> {
    let auth = state.services.auth.clone();
    let ctx = actor.context().clone();
    let login = req.login;
    let password = req.password;
    let role = req.role;
    let display_name = req.display_name;
    let user = tokio::task::spawn_blocking(move || {
        auth.create_user(&ctx, &login, &password, role, display_name.as_deref())
    })
    .await
    .map_err(|e| {
        crate::error::AppError::internal(format!("создание пользователя прервано: {e}"))
    })??;
    Ok((StatusCode::CREATED, Json(user)))
}

/// `GET /api/v1/users/:id` — профиль по id.
pub async fn get(
    State(state): State<AppState>,
    actor: AuthUser,
    Path(id): Path<String>,
) -> AppResult<Json<User>> {
    let user = state.services.auth.get_user(actor.context(), &id)?;
    Ok(Json(user))
}

/// `GET /api/v1/users/:id/avatar` — байты аватара (любой вошедший).
///
/// Ответ — бинарный; 404, если аватара нет. Реализация в `profile`.
pub async fn avatar(
    State(state): State<AppState>,
    actor: AuthUser,
    Path(id): Path<String>,
) -> AppResult<axum::response::Response> {
    super::profile::avatar_response(State(state), actor, id).await
}

/// `PATCH /api/v1/users/:id/role` — смена роли.
pub async fn set_role(
    State(state): State<AppState>,
    actor: AuthUser,
    Path(id): Path<String>,
    AppJson(req): AppJson<RoleRequest>,
) -> AppResult<Json<serde_json::Value>> {
    state
        .services
        .auth
        .set_role(actor.context(), &id, req.role)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `PATCH /api/v1/users/:id/active` — включение/отключение учётной записи.
pub async fn set_active(
    State(state): State<AppState>,
    actor: AuthUser,
    Path(id): Path<String>,
    AppJson(req): AppJson<ActiveRequest>,
) -> AppResult<Json<serde_json::Value>> {
    state
        .services
        .auth
        .set_active(actor.context(), &id, req.is_active)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `DELETE /api/v1/users/:id` — удаление пользователя.
pub async fn remove(
    State(state): State<AppState>,
    actor: AuthUser,
    Path(id): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    state.services.auth.delete_user(actor.context(), &id)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}
