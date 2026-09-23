//! Пользовательские предпочтения моделей (llm / stt / tts).
//!
//! Отсутствие записи = глобальное назначение роли. Выбор — только среди
//! включённых моделей matching-роли («разрешённые админом» = `is_enabled`).

use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;

use crate::domain::entities::model::ModelRole;
use crate::domain::entities::provider::{ModelRecord, UserModelPreference};
use crate::error::{AppError, AppResult};
use crate::web::middleware::{AppJson, AppQuery, AuthUser};
use crate::web::state::AppState;

#[derive(Debug, Deserialize)]
pub struct PreferenceBody {
    pub model_id: String,
}

#[derive(Debug, Deserialize)]
pub struct OptionsQuery {
    pub role: ModelRole,
}

fn parse_role(raw: &str) -> AppResult<ModelRole> {
    ModelRole::from_slug(raw)
        .ok_or_else(|| AppError::BadRequest(format!("неизвестная роль модели: {raw}")))
}

/// `GET /api/v1/model-preferences/me` — свои предпочтения.
pub async fn list_me(
    State(state): State<AppState>,
    user: AuthUser,
) -> AppResult<Json<Vec<UserModelPreference>>> {
    let uid = user.user_id().to_string();
    let prefs = state
        .services
        .providers
        .user_preferences(user.context(), &uid)?;
    Ok(Json(prefs))
}

/// `GET /api/v1/model-preferences/me/:role` — одно предпочтение или `null`.
pub async fn get_me(
    State(state): State<AppState>,
    user: AuthUser,
    Path(role): Path<String>,
) -> AppResult<Json<Option<UserModelPreference>>> {
    let role = parse_role(&role)?;
    let uid = user.user_id().to_string();
    let pref = state
        .services
        .providers
        .user_preference(user.context(), &uid, role)?;
    Ok(Json(pref))
}

/// `PUT /api/v1/model-preferences/me/:role` `{model_id}` — выбрать модель.
pub async fn set_me(
    State(state): State<AppState>,
    user: AuthUser,
    Path(role): Path<String>,
    AppJson(req): AppJson<PreferenceBody>,
) -> AppResult<Json<serde_json::Value>> {
    let role = parse_role(&role)?;
    let uid = user.user_id().to_string();
    state
        .services
        .providers
        .set_user_preference(user.context(), &uid, role, &req.model_id)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `DELETE /api/v1/model-preferences/me/:role` — вернуться к глобальной.
pub async fn remove_me(
    State(state): State<AppState>,
    user: AuthUser,
    Path(role): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    let role = parse_role(&role)?;
    let uid = user.user_id().to_string();
    state
        .services
        .providers
        .delete_user_preference(user.context(), &uid, role)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `GET /api/v1/model-preferences/options?role=llm` — кандидаты для выбора
/// (включённые модели роли; любой авторизованный — нужен для UI-пикера).
pub async fn options(
    State(state): State<AppState>,
    user: AuthUser,
    AppQuery(q): AppQuery<OptionsQuery>,
) -> AppResult<Json<Vec<ModelRecord>>> {
    let models = state
        .services
        .providers
        .model_options(user.context(), q.role)?;
    Ok(Json(models))
}

/// `GET /api/v1/model-preferences/users/:user_id` — чужие (admin).
pub async fn list_user(
    State(state): State<AppState>,
    actor: AuthUser,
    Path(user_id): Path<String>,
) -> AppResult<Json<Vec<UserModelPreference>>> {
    let prefs = state
        .services
        .providers
        .user_preferences(actor.context(), &user_id)?;
    Ok(Json(prefs))
}

/// `PUT /api/v1/model-preferences/users/:user_id/:role` — выбор за пользователя (admin).
pub async fn set_user(
    State(state): State<AppState>,
    actor: AuthUser,
    Path((user_id, role)): Path<(String, String)>,
    AppJson(req): AppJson<PreferenceBody>,
) -> AppResult<Json<serde_json::Value>> {
    let role = parse_role(&role)?;
    state
        .services
        .providers
        .set_user_preference(actor.context(), &user_id, role, &req.model_id)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `DELETE /api/v1/model-preferences/users/:user_id/:role` — очистить чужое (admin).
pub async fn remove_user(
    State(state): State<AppState>,
    actor: AuthUser,
    Path((user_id, role)): Path<(String, String)>,
) -> AppResult<Json<serde_json::Value>> {
    let role = parse_role(&role)?;
    state
        .services
        .providers
        .delete_user_preference(actor.context(), &user_id, role)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}
