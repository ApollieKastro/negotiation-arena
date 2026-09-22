//! Эндпоинт живости `/health`: проверяет БД и версию.

use axum::{extract::State, http::StatusCode, Json};
use serde_json::json;

use crate::web::state::AppState;

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub async fn health(State(state): State<AppState>) -> (StatusCode, Json<serde_json::Value>) {
    let db_ok = state.db.ping();
    let body = json!({
        "status": if db_ok { "ok" } else { "degraded" },
        "version": VERSION,
        "database": if db_ok { "up" } else { "down" },
    });

    let status = if db_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (status, Json(body))
}

pub async fn not_found() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": "Маршрут не найден" })),
    )
}
