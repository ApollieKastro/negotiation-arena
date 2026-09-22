//! Сценарии: чтение для всех, запись — admin (`ManageScenarios`).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use crate::domain::entities::scenario::{Difficulty, Scenario};
use crate::error::{AppError, AppResult};
use crate::web::middleware::{AppJson, AppQuery, AuthUser};
use crate::web::state::AppState;

#[derive(Debug, Default, Deserialize)]
pub struct ListQuery {
    /// Только активные (для не-админа включается всегда на стороне сервиса).
    #[serde(default)]
    pub active_only: bool,
}

#[derive(Debug, Deserialize)]
pub struct GenerateRequest {
    pub brief: String,
    #[serde(default)]
    pub difficulty: Option<Difficulty>,
    #[serde(default)]
    pub sphere: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ActiveRequest {
    pub is_active: bool,
}

/// `GET /api/v1/scenarios` — список сценариев.
pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    AppQuery(q): AppQuery<ListQuery>,
) -> AppResult<Json<Vec<Scenario>>> {
    let scenarios = state
        .services
        .scenarios
        .list(user.context(), q.active_only)?;
    Ok(Json(scenarios))
}

/// `GET /api/v1/scenarios/:id` — один сценарий.
pub async fn get(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> AppResult<Json<Scenario>> {
    let scenario = state.services.scenarios.get(user.context(), &id)?;
    Ok(Json(scenario))
}

/// `POST /api/v1/scenarios` — создание (admin).
pub async fn create(
    State(state): State<AppState>,
    actor: AuthUser,
    AppJson(scenario): AppJson<Scenario>,
) -> AppResult<(StatusCode, Json<Scenario>)> {
    let created = state.services.scenarios.create(actor.context(), scenario)?;
    Ok((StatusCode::CREATED, Json(created)))
}

/// `PUT /api/v1/scenarios/:id` — обновление (admin).
pub async fn update(
    State(state): State<AppState>,
    actor: AuthUser,
    Path(id): Path<String>,
    AppJson(mut scenario): AppJson<Scenario>,
) -> AppResult<Json<Scenario>> {
    // id берём из пути, чтобы PUT /scenarios/A с телом id=B не уходил в B.
    scenario.id = id;
    let updated = state.services.scenarios.update(actor.context(), scenario)?;
    Ok(Json(updated))
}

/// `DELETE /api/v1/scenarios/:id` — удаление (admin).
pub async fn remove(
    State(state): State<AppState>,
    actor: AuthUser,
    Path(id): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    state.services.scenarios.delete(actor.context(), &id)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `PATCH /api/v1/scenarios/:id/active` — публикация/снятие (admin).
pub async fn set_active(
    State(state): State<AppState>,
    actor: AuthUser,
    Path(id): Path<String>,
    AppJson(req): AppJson<ActiveRequest>,
) -> AppResult<Json<Scenario>> {
    let scenario = state
        .services
        .scenarios
        .set_active(actor.context(), &id, req.is_active)?;
    Ok(Json(scenario))
}

/// `GET /api/v1/scenarios/:id/export` — экспорт в JSON.
pub async fn export(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    let raw = state.services.scenarios.export_json(user.context(), &id)?;
    let value: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| AppError::internal(format!("сериализация экспорта: {e}")))?;
    Ok(Json(value))
}

/// `POST /api/v1/scenarios/import` — импорт сценария (admin).
pub async fn import(
    State(state): State<AppState>,
    actor: AuthUser,
    AppJson(body): AppJson<serde_json::Value>,
) -> AppResult<(StatusCode, Json<Scenario>)> {
    let raw = body.to_string();
    let scenario = state
        .services
        .scenarios
        .import_json(actor.context(), &raw)?;
    Ok((StatusCode::CREATED, Json(scenario)))
}

/// `POST /api/v1/scenarios/generate` — ИИ-генератор по брифу (admin).
pub async fn generate(
    State(state): State<AppState>,
    actor: AuthUser,
    AppJson(req): AppJson<GenerateRequest>,
) -> AppResult<(StatusCode, Json<Scenario>)> {
    let difficulty = req.difficulty.unwrap_or(Difficulty::Medium);
    let scenario = state
        .services
        .scenarios
        .generate(
            actor.context(),
            &req.brief,
            difficulty,
            req.sphere.as_deref(),
        )
        .await?;
    Ok((StatusCode::CREATED, Json(scenario)))
}
