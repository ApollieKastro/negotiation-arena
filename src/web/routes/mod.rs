//! Сборка роутера приложения.

pub mod health;

use axum::Router;

use crate::web::state::AppState;

pub fn build(state: AppState) -> Router {
    Router::new()
        .merge(api_v1_routes())
        .nest_service("/static", tower_http::services::ServeDir::new("static"))
        .fallback(health::not_found)
        .with_state(state)
}

/// Корневая группа API. Этапы 3–4 добавляют сюда admin/user маршруты.
fn api_v1_routes() -> Router<AppState> {
    Router::new().route("/health", axum::routing::get(health::health))
}
