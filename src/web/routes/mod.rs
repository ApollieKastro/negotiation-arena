//! Сборка роутера приложения: `/api/v1` + статика + health.

pub mod health;
#[cfg(test)]
mod tests;

use axum::http::{header, Method};
use axum::Router;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::web::handlers;
use crate::web::state::AppState;

pub fn build(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE, header::ACCEPT]);

    Router::new()
        .route("/health", axum::routing::get(health::health))
        .nest("/api/v1", api_v1_routes())
        .nest_service("/static", tower_http::services::ServeDir::new("static"))
        .fallback(health::not_found)
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state)
}

/// Группа API v1. Хендлеры — тонкие обёртки над `AppState.services`.
fn api_v1_routes() -> Router<AppState> {
    use axum::routing::{delete, get, patch, post, put};

    Router::new()
        // ── Health ──
        .route("/health", get(health::health))
        // ── Auth (публичные + me) ──
        .route("/auth/register", post(handlers::auth::register))
        .route("/auth/login", post(handlers::auth::login))
        .route("/auth/refresh", post(handlers::auth::refresh))
        .route("/auth/me", get(handlers::auth::me))
        // ── Пользователи (admin) ──
        .route(
            "/users",
            get(handlers::users::list).post(handlers::users::create),
        )
        .route(
            "/users/:id",
            get(handlers::users::get).delete(handlers::users::remove),
        )
        .route("/users/:id/role", patch(handlers::users::set_role))
        .route("/users/:id/active", patch(handlers::users::set_active))
        // ── Сценарии ──
        .route(
            "/scenarios",
            get(handlers::scenarios::list).post(handlers::scenarios::create),
        )
        .route("/scenarios/import", post(handlers::scenarios::import))
        .route("/scenarios/generate", post(handlers::scenarios::generate))
        .route(
            "/scenarios/:id",
            get(handlers::scenarios::get)
                .put(handlers::scenarios::update)
                .delete(handlers::scenarios::remove),
        )
        .route("/scenarios/:id/export", get(handlers::scenarios::export))
        .route(
            "/scenarios/:id/active",
            patch(handlers::scenarios::set_active),
        )
        // ── Сессии ──
        .route(
            "/sessions",
            get(handlers::sessions::history).post(handlers::sessions::start),
        )
        .route("/sessions/:id", get(handlers::sessions::get))
        .route("/sessions/:id/messages", get(handlers::sessions::messages))
        .route("/sessions/:id/turn", post(handlers::sessions::turn))
        .route("/sessions/:id/finish", post(handlers::sessions::finish))
        .route("/sessions/:id/abandon", post(handlers::sessions::abandon))
        .route("/sessions/:id/report", get(handlers::sessions::report))
        // ── Статистика ──
        .route("/stats/me", get(handlers::stats::me))
        .route("/stats/users/:id", get(handlers::stats::user))
        .route("/stats/leaderboard", get(handlers::stats::leaderboard))
        .route("/stats/overview", get(handlers::stats::overview))
        .route("/stats/activity", get(handlers::stats::activity))
        // ── Настройки ──
        .route(
            "/settings/global",
            get(handlers::settings::list_global).put(handlers::settings::set_global),
        )
        .route("/settings/me", get(handlers::settings::me))
        .route(
            "/settings/me/:key",
            get(handlers::settings::get_me).put(handlers::settings::set_me),
        )
        .route(
            "/settings/users/:user_id",
            get(handlers::settings::list_user),
        )
        .route(
            "/settings/users/:user_id/:key",
            get(handlers::settings::get_user).put(handlers::settings::set_user),
        )
        // ── Провайдеры и модели (admin) ──
        .route(
            "/providers",
            get(handlers::providers::list).post(handlers::providers::upsert),
        )
        .route(
            "/providers/:id",
            get(handlers::providers::get).delete(handlers::providers::remove),
        )
        .route("/providers/:id/ping", post(handlers::providers::ping))
        .route(
            "/providers/:id/discover",
            get(handlers::providers::discover),
        )
        .route(
            "/models",
            get(handlers::providers::list_models).post(handlers::providers::upsert_model),
        )
        .route("/models/:id", delete(handlers::providers::remove_model))
        .route("/model-assignments", get(handlers::providers::assignments))
        .route(
            "/model-assignments/:role",
            put(handlers::providers::assign_role),
        )
}
