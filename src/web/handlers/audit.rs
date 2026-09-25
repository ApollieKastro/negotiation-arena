//! Аудит-лог: чтение для админ-панели (RBAC: `ViewAuditLog`).

use axum::extract::State;
use axum::Json;
use serde::Deserialize;

use crate::application::audit::DEFAULT_AUDIT_LIMIT;
use crate::domain::entities::audit::AuditEntry;
use crate::error::AppResult;
use crate::web::middleware::{AppQuery, AuthUser};
use crate::web::state::AppState;

#[derive(Debug, Deserialize)]
pub struct AuditQuery {
    /// Сколько последних записей; default 50, clamp 1..=200.
    #[serde(default = "default_limit")]
    pub limit: u32,
    /// Фильтр по исполнителю (`user_id`).
    #[serde(default)]
    pub user_id: Option<String>,
    /// Фильтр по коду события (точное совпадение), например `auth.login`.
    #[serde(default)]
    pub action: Option<String>,
}

fn default_limit() -> u32 {
    DEFAULT_AUDIT_LIMIT
}

/// `GET /api/v1/audit?limit=&user_id=&action=` — журнал действий.
pub async fn list(
    State(state): State<AppState>,
    actor: AuthUser,
    AppQuery(q): AppQuery<AuditQuery>,
) -> AppResult<Json<Vec<AuditEntry>>> {
    let entries = state.services.audit.list(
        actor.context(),
        q.limit,
        q.user_id.as_deref(),
        q.action.as_deref(),
    )?;
    Ok(Json(entries))
}
