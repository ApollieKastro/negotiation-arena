//! Провайдеры, модели и назначения по ролям (admin: `ManageProviders`).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use crate::domain::entities::model::{ModelDescriptor, ModelRole};
use crate::domain::entities::provider::{ModelRecord, Provider, RoleAssignment};
use crate::error::{AppError, AppResult};
use crate::web::middleware::{AppJson, AppQuery, AuthUser};
use crate::web::state::AppState;

#[derive(Debug, Deserialize)]
pub struct UpsertProviderRequest {
    #[serde(flatten)]
    pub provider: Provider,
    /// `None` — не менять; `Some("")` — очистить; `Some(value)` — сохранить.
    #[serde(default)]
    pub api_key: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DiscoverQuery {
    pub role: ModelRole,
}

#[derive(Debug, Deserialize)]
pub struct ModelsQuery {
    #[serde(default)]
    pub provider_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AssignRequest {
    pub model_id: String,
}

fn parse_role(raw: &str) -> AppResult<ModelRole> {
    ModelRole::from_slug(raw)
        .ok_or_else(|| AppError::BadRequest(format!("неизвестная роль модели: {raw}")))
}

/// `GET /api/v1/providers` — список провайдеров.
pub async fn list(
    State(state): State<AppState>,
    actor: AuthUser,
) -> AppResult<Json<Vec<Provider>>> {
    let providers = state.services.providers.list(actor.context())?;
    Ok(Json(providers))
}

/// `POST /api/v1/providers` — создание/обновление провайдера.
pub async fn upsert(
    State(state): State<AppState>,
    actor: AuthUser,
    AppJson(req): AppJson<UpsertProviderRequest>,
) -> AppResult<(StatusCode, Json<Provider>)> {
    let provider =
        state
            .services
            .providers
            .upsert(actor.context(), req.provider, req.api_key.as_deref())?;
    Ok((StatusCode::OK, Json(provider)))
}

/// `GET /api/v1/providers/:id` — один провайдер.
pub async fn get(
    State(state): State<AppState>,
    actor: AuthUser,
    Path(id): Path<String>,
) -> AppResult<Json<Provider>> {
    let provider = state.services.providers.get(actor.context(), &id)?;
    Ok(Json(provider))
}

/// `DELETE /api/v1/providers/:id` — удаление провайдера.
pub async fn remove(
    State(state): State<AppState>,
    actor: AuthUser,
    Path(id): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    state.services.providers.delete(actor.context(), &id)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `POST /api/v1/providers/:id/ping` — проверка соединения.
pub async fn ping(
    State(state): State<AppState>,
    actor: AuthUser,
    Path(id): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    state.services.providers.ping(actor.context(), &id).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `GET /api/v1/providers/:id/discover?role=llm` — discovery моделей.
pub async fn discover(
    State(state): State<AppState>,
    actor: AuthUser,
    Path(id): Path<String>,
    AppQuery(q): AppQuery<DiscoverQuery>,
) -> AppResult<Json<Vec<ModelDescriptor>>> {
    let models = state
        .services
        .providers
        .discover_models(actor.context(), &id, q.role)
        .await?;
    Ok(Json(models))
}

/// `GET /api/v1/models?provider_id=` — список сохранённых моделей.
pub async fn list_models(
    State(state): State<AppState>,
    actor: AuthUser,
    AppQuery(q): AppQuery<ModelsQuery>,
) -> AppResult<Json<Vec<ModelRecord>>> {
    let models = state
        .services
        .providers
        .list_models(actor.context(), q.provider_id.as_deref())?;
    Ok(Json(models))
}

/// `POST /api/v1/models` — создание/обновление модели.
pub async fn upsert_model(
    State(state): State<AppState>,
    actor: AuthUser,
    AppJson(model): AppJson<ModelRecord>,
) -> AppResult<(StatusCode, Json<ModelRecord>)> {
    let model = state
        .services
        .providers
        .upsert_model(actor.context(), model)?;
    Ok((StatusCode::OK, Json(model)))
}

/// `DELETE /api/v1/models/:id` — удаление модели.
pub async fn remove_model(
    State(state): State<AppState>,
    actor: AuthUser,
    Path(id): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    state
        .services
        .providers
        .delete_model(actor.context(), &id)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `GET /api/v1/model-assignments` — текущие назначения ролей.
pub async fn assignments(
    State(state): State<AppState>,
    actor: AuthUser,
) -> AppResult<Json<Vec<RoleAssignment>>> {
    let assignments = state.services.providers.role_assignments(actor.context())?;
    Ok(Json(assignments))
}

/// `PUT /api/v1/model-assignments/:role` — назначить модель на роль.
pub async fn assign_role(
    State(state): State<AppState>,
    actor: AuthUser,
    Path(role): Path<String>,
    AppJson(req): AppJson<AssignRequest>,
) -> AppResult<Json<serde_json::Value>> {
    let role = parse_role(&role)?;
    state
        .services
        .providers
        .assign_role(actor.context(), role, &req.model_id)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}
