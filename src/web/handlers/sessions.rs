//! Сессии прохождения: старт, ход, финиш, история, отчёт.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use crate::domain::entities::session::{Session, SessionMessage, SessionMode};
use crate::error::AppResult;
use crate::web::middleware::{AppJson, AppQuery, AuthUser};
use crate::web::state::AppState;

fn default_history_limit() -> u32 {
    50
}

fn default_history_offset() -> u32 {
    0
}

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    #[serde(default = "default_history_limit")]
    pub limit: u32,
    #[serde(default = "default_history_offset")]
    pub offset: u32,
}

#[derive(Debug, Deserialize)]
pub struct MessagesQuery {
    /// Ветка диалога; без параметра — текущая ветка сессии.
    pub branch_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StartRequest {
    pub scenario_id: String,
    #[serde(default)]
    pub mode: Option<SessionMode>,
}

#[derive(Debug, Deserialize)]
pub struct TurnRequest {
    pub text: String,
}

#[derive(Debug, Deserialize)]
pub struct BranchRequest {
    /// Реплика собеседника, **после** которой создаётся ветка.
    pub after_message_id: String,
}

/// `GET /api/v1/sessions?limit=&offset=` — история сессий текущего пользователя.
///
/// Ответ: `{ items, total, limit, offset }` — offset-пагинация.
pub async fn history(
    State(state): State<AppState>,
    user: AuthUser,
    AppQuery(q): AppQuery<HistoryQuery>,
) -> AppResult<Json<serde_json::Value>> {
    let (items, total) = state
        .services
        .sessions
        .history(user.context(), q.limit, q.offset)?;
    Ok(Json(serde_json::json!({
        "items": items,
        "total": total,
        "limit": q.limit,
        "offset": q.offset,
    })))
}

/// `POST /api/v1/sessions` — старт сессии по сценарию.
pub async fn start(
    State(state): State<AppState>,
    user: AuthUser,
    AppJson(req): AppJson<StartRequest>,
) -> AppResult<(
    StatusCode,
    Json<crate::application::session::SessionStarted>,
)> {
    let mode = req.mode.unwrap_or(SessionMode::Text);
    let started = state
        .services
        .sessions
        .start(user.context(), &req.scenario_id, mode)?;
    Ok((StatusCode::CREATED, Json(started)))
}

/// `GET /api/v1/sessions/:id` — сессия (владелец или админ).
pub async fn get(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> AppResult<Json<Session>> {
    let session = state.services.sessions.get(user.context(), &id)?;
    Ok(Json(session))
}

/// `GET /api/v1/sessions/:id/messages?branch_id=` — реплики диалога.
///
/// Без `branch_id` — текущая ветка; с параметром — указанная (404, если
/// ветка не принадлежит сессии).
pub async fn messages(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    AppQuery(q): AppQuery<MessagesQuery>,
) -> AppResult<Json<Vec<SessionMessage>>> {
    let messages = state
        .services
        .sessions
        .messages(user.context(), &id, q.branch_id.as_deref())?;
    Ok(Json(messages))
}

/// `GET /api/v1/sessions/:id/branches` — дерево веток сессии.
pub async fn branches(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> AppResult<Json<Vec<crate::domain::entities::session::SessionBranch>>> {
    let branches = state.services.sessions.branches(user.context(), &id)?;
    Ok(Json(branches))
}

/// `POST /api/v1/sessions/:id/branches` — форк после реплики собеседника.
///
/// Ответ 201: `{ branch, session }` — сессия откатнута к точке ветвления.
pub async fn create_branch(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    AppJson(req): AppJson<BranchRequest>,
) -> AppResult<(StatusCode, Json<crate::application::session::BranchCreated>)> {
    let created =
        state
            .services
            .sessions
            .create_branch(user.context(), &id, &req.after_message_id)?;
    Ok((StatusCode::CREATED, Json(created)))
}

/// `PUT /api/v1/sessions/:id/branches/:branch_id` — переключение ветки.
///
/// Ответ: `{ branch, session }` — состояние сессии из снапшота ветки.
pub async fn switch_branch(
    State(state): State<AppState>,
    user: AuthUser,
    Path((id, branch_id)): Path<(String, String)>,
) -> AppResult<Json<serde_json::Value>> {
    let (session, branch) =
        state
            .services
            .sessions
            .switch_branch(user.context(), &id, &branch_id)?;
    Ok(Json(serde_json::json!({
        "session": session,
        "branch": branch,
    })))
}

/// `POST /api/v1/sessions/:id/turn` — ход игрока (LLM + скоринг).
pub async fn turn(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    AppJson(req): AppJson<TurnRequest>,
) -> AppResult<Json<crate::application::session::TurnOutcome>> {
    let outcome = state
        .services
        .sessions
        .submit_turn(user.context(), &id, &req.text)
        .await?;
    Ok(Json(outcome))
}

/// `POST /api/v1/sessions/:id/finish` — финализация с отчётом.
pub async fn finish(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    let (session, report) = state.services.sessions.finish(user.context(), &id)?;
    Ok(Json(serde_json::json!({
        "session": session,
        "report": report,
    })))
}

/// `POST /api/v1/sessions/:id/abandon` — прерывание без отчёта.
pub async fn abandon(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> AppResult<Json<Session>> {
    let session = state.services.sessions.abandon(user.context(), &id)?;
    Ok(Json(session))
}

/// `GET /api/v1/sessions/:id/report` — пересборка отчёта для UI.
pub async fn report(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> AppResult<Json<crate::domain::services::scoring::SessionReport>> {
    let report = state.services.sessions.report(user.context(), &id)?;
    Ok(Json(report))
}
