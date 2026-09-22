//! Настройки: глобальные (запись — admin, чтение — любой авторизованный)
//! и пользовательские (свои/чужие).

use std::collections::BTreeMap;

use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;

use crate::error::AppResult;
use crate::web::middleware::{AppJson, AuthUser};
use crate::web::state::AppState;

#[derive(Debug, Deserialize)]
pub struct GlobalSetRequest {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Deserialize)]
pub struct ValueRequest {
    pub value: String,
}

/// `GET /api/v1/settings/global` — все глобальные настройки.
pub async fn list_global(
    State(state): State<AppState>,
    _user: AuthUser,
) -> AppResult<Json<BTreeMap<String, String>>> {
    let map = state
        .services
        .settings
        .all_global()?
        .into_iter()
        .collect::<BTreeMap<_, _>>();
    Ok(Json(map))
}

/// `PUT /api/v1/settings/global` — запись глобальной настройки (admin).
pub async fn set_global(
    State(state): State<AppState>,
    actor: AuthUser,
    AppJson(req): AppJson<GlobalSetRequest>,
) -> AppResult<Json<serde_json::Value>> {
    state
        .services
        .settings
        .set_global(actor.context(), &req.key, &req.value)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `GET /api/v1/settings/me` — пакет своих UI-настроек.
pub async fn me(
    State(state): State<AppState>,
    user: AuthUser,
) -> AppResult<Json<BTreeMap<String, String>>> {
    let uid = user.user_id().to_string();
    let map = state
        .services
        .settings
        .user_settings(user.context(), &uid)?;
    Ok(Json(map))
}

/// `GET /api/v1/settings/me/:key` — своя настройка.
pub async fn get_me(
    State(state): State<AppState>,
    user: AuthUser,
    Path(key): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    let uid = user.user_id().to_string();
    let value = state
        .services
        .settings
        .get_user(user.context(), &uid, &key)?;
    Ok(Json(serde_json::json!({ "key": key, "value": value })))
}

/// `PUT /api/v1/settings/me/:key` — запись своей настройки.
pub async fn set_me(
    State(state): State<AppState>,
    user: AuthUser,
    Path(key): Path<String>,
    AppJson(req): AppJson<ValueRequest>,
) -> AppResult<Json<serde_json::Value>> {
    let uid = user.user_id().to_string();
    state
        .services
        .settings
        .set_user(user.context(), &uid, &key, &req.value)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `GET /api/v1/settings/users/:user_id` — настройки пользователя.
pub async fn list_user(
    State(state): State<AppState>,
    actor: AuthUser,
    Path(user_id): Path<String>,
) -> AppResult<Json<BTreeMap<String, String>>> {
    let map = state
        .services
        .settings
        .user_settings(actor.context(), &user_id)?;
    Ok(Json(map))
}

/// `GET /api/v1/settings/users/:user_id/:key` — чужая настройка (admin).
pub async fn get_user(
    State(state): State<AppState>,
    actor: AuthUser,
    Path((user_id, key)): Path<(String, String)>,
) -> AppResult<Json<serde_json::Value>> {
    let value = state
        .services
        .settings
        .get_user(actor.context(), &user_id, &key)?;
    Ok(Json(serde_json::json!({ "key": key, "value": value })))
}

/// `PUT /api/v1/settings/users/:user_id/:key` — запись чужой настройки (admin).
pub async fn set_user(
    State(state): State<AppState>,
    actor: AuthUser,
    Path((user_id, key)): Path<(String, String)>,
    AppJson(req): AppJson<ValueRequest>,
) -> AppResult<Json<serde_json::Value>> {
    state
        .services
        .settings
        .set_user(actor.context(), &user_id, &key, &req.value)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}
