//! Статистика: своя, чужая (admin), лидерборд, обзор, активность.

use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;

use crate::application::stats::{ActivityPoint, LeaderboardEntry, PlatformOverview, UserStats};
use crate::error::AppResult;
use crate::web::middleware::{AppQuery, AuthUser};
use crate::web::state::AppState;

fn default_leaderboard_limit() -> u32 {
    10
}

fn default_activity_days() -> u32 {
    30
}

#[derive(Debug, Deserialize)]
pub struct LeaderboardQuery {
    #[serde(default = "default_leaderboard_limit")]
    pub limit: u32,
}

#[derive(Debug, Deserialize)]
pub struct ActivityQuery {
    #[serde(default = "default_activity_days")]
    pub days: u32,
}

/// `GET /api/v1/stats/me` — своя сводка.
pub async fn me(State(state): State<AppState>, user: AuthUser) -> AppResult<Json<UserStats>> {
    let stats = state.services.stats.my_stats(user.context())?;
    Ok(Json(stats))
}

/// `GET /api/v1/stats/users/:id` — чужая сводка (admin) или своя.
pub async fn user(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> AppResult<Json<UserStats>> {
    let stats = state.services.stats.user_stats(user.context(), &id)?;
    Ok(Json(stats))
}

/// `GET /api/v1/stats/leaderboard` — лидерборд (admin).
pub async fn leaderboard(
    State(state): State<AppState>,
    user: AuthUser,
    AppQuery(q): AppQuery<LeaderboardQuery>,
) -> AppResult<Json<Vec<LeaderboardEntry>>> {
    let board = state.services.stats.leaderboard(user.context(), q.limit)?;
    Ok(Json(board))
}

/// `GET /api/v1/stats/overview` — обзор платформы (admin).
pub async fn overview(
    State(state): State<AppState>,
    user: AuthUser,
) -> AppResult<Json<PlatformOverview>> {
    let overview = state.services.stats.overview(user.context())?;
    Ok(Json(overview))
}

/// `GET /api/v1/stats/activity` — сессии по дням (admin).
pub async fn activity(
    State(state): State<AppState>,
    user: AuthUser,
    AppQuery(q): AppQuery<ActivityQuery>,
) -> AppResult<Json<Vec<ActivityPoint>>> {
    let points = state.services.stats.activity(user.context(), q.days)?;
    Ok(Json(points))
}
