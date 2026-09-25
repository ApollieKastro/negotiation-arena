//! Сборка роутера приложения: `/api/v1` + статика + SPA-fallback + health.
//!
//! Fallback-цепочка для неизвестных путей:
//! 1. `/api/v1/*` без совпадения роута → JSON-404 (fallback внутри nest);
//! 2. файл из `dist/` (assets, если собран фронтенд);
//! 3. `dist/index.html` (SPA-навигация);
//! 4. JSON-404 `{"error": "Маршрут не найден"}` (если UI ещё не собран).
//!
//! Также отдаётся `docs/` под `/docs` (MODELS_GUIDE.md и т.п. — ссылки из UI).

pub mod health;
#[cfg(test)]
mod tests;

use std::convert::Infallible;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use tower::service_fn;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

use crate::web::handlers;
use crate::web::middleware::rate_limit::{rate_limit_auth, RateLimiter};
use crate::web::state::AppState;

pub fn build(state: AppState) -> Router {
    let allowed = &state.config.security.allowed_origins;
    let cors = if allowed.is_empty() {
        // Dev-фоллбэк: любой origin. В проде задайте ALLOWED_ORIGINS.
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(auth_cors_methods())
            .allow_headers(auth_cors_headers())
    } else if allowed.iter().any(|o| o == "*") {
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(auth_cors_methods())
            .allow_headers(auth_cors_headers())
    } else {
        let origins: Vec<header::HeaderValue> =
            allowed.iter().filter_map(|o| o.parse().ok()).collect();
        CorsLayer::new()
            .allow_origin(origins)
            .allow_methods(auth_cors_methods())
            .allow_headers(auth_cors_headers())
    };

    let limiter = RateLimiter::new(&state.config.rate_limit);

    Router::new()
        .route("/health", axum::routing::get(health::health))
        .nest("/api/v1", api_v1_routes(limiter))
        .nest_service("/static", ServeDir::new("static"))
        // Документация (гайды): /docs/MODELS_GUIDE.md и др.
        .nest_service("/docs", ServeDir::new("docs"))
        // Собранный UI в dist/: файлы as-is, при промахе — index.html / JSON-404.
        .fallback_service(ServeDir::new("dist").fallback(service_fn(
            |req: Request<Body>| async move { Ok::<_, Infallible>(spa_fallback(req).await) },
        )))
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state)
}

fn auth_cors_methods() -> [Method; 6] {
    [
        Method::GET,
        Method::POST,
        Method::PUT,
        Method::PATCH,
        Method::DELETE,
        Method::OPTIONS,
    ]
}

fn auth_cors_headers() -> [header::HeaderName; 3] {
    [header::AUTHORIZATION, header::CONTENT_TYPE, header::ACCEPT]
}

/// Поведение после промаха ServeDir по `dist/`: SPA-index или JSON-404.
async fn spa_fallback(req: Request<Body>) -> Response {
    let path = req.uri().path();
    // API-пути не должны уходить в index.html — JSON-404 (страховка на случай,
    // если nest как-то пробросит запрос наружу).
    if path == "/api" || path.starts_with("/api/") {
        return health::not_found_response().into_response();
    }

    if req.method() == Method::GET || req.method() == Method::HEAD {
        match tokio::fs::read("dist/index.html").await {
            Ok(bytes) => {
                return (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                    bytes,
                )
                    .into_response();
            }
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
                tracing::warn!(error = %err, "не удалось прочитать dist/index.html");
            }
            Err(_) => {}
        }
    }

    health::not_found_response().into_response()
}

/// Группа API v1. Хендлеры — тонкие обёртки над `AppState.services`.
///
/// `limiter` навешивается только на публичные auth-роуты (login/register/refresh).
fn api_v1_routes(limiter: RateLimiter) -> Router<AppState> {
    use axum::middleware;
    use axum::routing::{delete, get, patch, post, put};

    // Публичная аутентификация — за rate-limit по IP.
    let public_auth = Router::new()
        .route("/register", post(handlers::auth::register))
        .route("/login", post(handlers::auth::login))
        .route("/refresh", post(handlers::auth::refresh))
        .layer(middleware::from_fn_with_state(limiter, rate_limit_auth));

    Router::new()
        // ── Health ──
        .route("/health", get(health::health))
        // ── Auth (публичные + me) ──
        .nest("/auth", public_auth)
        .route("/auth/me", get(handlers::auth::me))
        // ── Профиль (свой: логин/имя, пароль, аватар) ──
        .route(
            "/profile",
            get(handlers::profile::get).patch(handlers::profile::update),
        )
        .route("/profile/password", put(handlers::profile::change_password))
        .route(
            "/profile/avatar",
            put(handlers::profile::upload_avatar)
                .delete(handlers::profile::remove_avatar)
                .layer(axum::extract::DefaultBodyLimit::max(
                    handlers::profile::AVATAR_BODY_LIMIT,
                )),
        )
        // ── Пользователи (admin) ──
        .route(
            "/users",
            get(handlers::users::list).post(handlers::users::create),
        )
        .route(
            "/users/:id",
            get(handlers::users::get).delete(handlers::users::remove),
        )
        // Аватар — ниже `/users/:id`: любой вошедший (шапка, лидерборд).
        .route("/users/:id/avatar", get(handlers::users::avatar))
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
        // ── Ветвление диалога ──
        .route(
            "/sessions/:id/branches",
            get(handlers::sessions::branches).post(handlers::sessions::create_branch),
        )
        .route(
            "/sessions/:id/branches/:branch_id",
            put(handlers::sessions::switch_branch),
        )
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
            put(handlers::providers::assign_role).delete(handlers::providers::unassign_role),
        )
        // ── Локальные файлы моделей (MODELS_DIR): список/скачивание/удаление ──
        .route(
            "/local-models",
            get(handlers::providers::list_local_models)
                .delete(handlers::providers::delete_local_model),
        )
        .route(
            "/local-models/download",
            post(handlers::providers::download_local_model),
        )
        // ── Пользовательские предпочтения моделей ──
        .route("/model-preferences/me", get(handlers::model_prefs::list_me))
        .route(
            "/model-preferences/options",
            get(handlers::model_prefs::options),
        )
        .route(
            "/model-preferences/me/:role",
            get(handlers::model_prefs::get_me)
                .put(handlers::model_prefs::set_me)
                .delete(handlers::model_prefs::remove_me),
        )
        .route(
            "/model-preferences/users/:user_id",
            get(handlers::model_prefs::list_user),
        )
        .route(
            "/model-preferences/users/:user_id/:role",
            put(handlers::model_prefs::set_user).delete(handlers::model_prefs::remove_user),
        )
        // ── Голос: синтез и распознавание речи (любой авторизованный) ──
        .route("/voice/tts", post(handlers::voice::tts))
        .route(
            "/voice/stt",
            // Лимит тела выше дефолтного axum (2 МБ): STT принимает до 12 МБ аудио.
            post(handlers::voice::stt)
                .layer(axum::extract::DefaultBodyLimit::max(14 * 1024 * 1024)),
        )
        // ── Аудит-лог (admin) ──
        .route("/audit", get(handlers::audit::list))
        // Неизвестные `/api/v1/*` → JSON-404, не SPA.
        .fallback(health::not_found)
}
