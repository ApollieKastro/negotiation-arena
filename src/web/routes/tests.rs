//! Интеграционные тесты HTTP API: роутер, auth, RBAC, валидация.
//!
//! Поднимают in-memory БД с миграциями, сидами и полным [`AppState`],
//! затем выполняют запросы через `tower::ServiceExt::oneshot` без сокета.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::Router;
use serde_json::{json, Value};
use tower::ServiceExt; // `oneshot`

use super::build;
use crate::application::Services;
use crate::config::{AppConfig, SecurityConfig, ServerConfig, StorageConfig};
use crate::domain::entities::user::UserRole;
use crate::domain::ports::UserRepository;
use crate::infrastructure::crypto::SecretCipher;
use crate::infrastructure::db::repos::SqliteRepos;
use crate::infrastructure::db::seeds;
use crate::infrastructure::db::Database;
use crate::web::state::AppState;

const ADMIN_LOGIN: &str = "admin";
const ADMIN_PASSWORD: &str = "admin123";
const USER_LOGIN: &str = "alice";
const USER_PASSWORD: &str = "secret1";

/// Полный стенд: роутер + известные креды админа и обычного пользователя.
struct TestApp {
    app: Router,
}

fn test_config() -> AppConfig {
    AppConfig {
        server: ServerConfig {
            host: "127.0.0.1".into(),
            port: 0,
        },
        security: SecurityConfig {
            jwt_secret: "test-jwt-secret-16ch".into(),
            jwt_ttl_seconds: 3600,
            admin_password: ADMIN_PASSWORD.into(),
            encryption_secret: "test-encryption-secret".into(),
        },
        storage: StorageConfig {
            db_path: std::path::PathBuf::from(":memory:"),
            models_dir: std::path::PathBuf::from("models"),
        },
    }
}

impl TestApp {
    /// Собирает чистый стенд: миграции → admin с известным паролем → сиды → роутер.
    ///
    /// Admin создаётся **до** `seeds::run`, иначе сид создаст админа из env
    /// и пароль в тесте будет неизвестен.
    fn new() -> Self {
        let db = Arc::new(Database::open_in_memory().expect("in-memory БД"));
        db.run_migrations().expect("миграции");

        // Фиксированный admin до сидов (сиды пропустят admin, если уже есть).
        let repos = Arc::new(SqliteRepos::new(db.clone()));
        let hash = crate::infrastructure::crypto::password::hash_password(ADMIN_PASSWORD);
        repos
            .users
            .create(ADMIN_LOGIN, &hash, UserRole::Admin, Some("Администратор"))
            .expect("create admin");

        seeds::run(&db).expect("сиды");

        // Обычный пользователь для проверки RBAC.
        let user_hash = crate::infrastructure::crypto::password::hash_password(USER_PASSWORD);
        repos
            .users
            .create(USER_LOGIN, &user_hash, UserRole::User, Some("Алиса"))
            .expect("create user");

        let config = test_config();
        let cipher = Arc::new(
            SecretCipher::from_secret(&config.security.encryption_secret).expect("cipher"),
        );
        let services = Arc::new(Services::new(&config, repos, cipher.clone()).expect("services"));

        let state = AppState {
            config: Arc::new(config),
            db,
            cipher,
            services,
        };

        Self { app: build(state) }
    }

    async fn call(
        &self,
        method: &str,
        uri: &str,
        body: Option<Value>,
        token: Option<&str>,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(t) = token {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        let req = match body {
            Some(v) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(v.to_string()))
                .expect("request"),
            None => builder.body(Body::empty()).expect("request"),
        };

        let resp = self.app.clone().oneshot(req).await.expect("oneshot");
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024)
            .await
            .expect("body");
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(Value::Null)
        };
        (status, value)
    }

    async fn login(&self, login: &str, password: &str) -> String {
        let (status, body) = self
            .call(
                "POST",
                "/api/v1/auth/login",
                Some(json!({ "login": login, "password": password })),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "login failed: {body}");
        body["token"].as_str().expect("token").to_string()
    }

    async fn admin_token(&self) -> String {
        self.login(ADMIN_LOGIN, ADMIN_PASSWORD).await
    }

    async fn user_token(&self) -> String {
        self.login(USER_LOGIN, USER_PASSWORD).await
    }
}

// ── Health / fallback ──────────────────────────────────────────

#[tokio::test]
async fn health_endpoints_return_ok() {
    let app = TestApp::new();

    let (status, body) = app.call("GET", "/health", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
    assert_eq!(body["database"], "up");

    let (status, body) = app.call("GET", "/api/v1/health", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn unknown_route_returns_json_404() {
    let app = TestApp::new();
    let (status, body) = app.call("GET", "/no/such/route", None, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body["error"].is_string(), "ожидали JSON с error: {body}");
}

// ── Auth: register / login / me / refresh ─────────────────────

#[tokio::test]
async fn register_login_me_refresh_flow() {
    let app = TestApp::new();

    // Регистрация → 201.
    let (status, body) = app
        .call(
            "POST",
            "/api/v1/auth/register",
            Some(json!({
                "login": "bob",
                "password": "secret1",
                "display_name": "Боб"
            })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["login"], "bob");
    assert_eq!(body["role"], "user");

    // Дубликат → 409.
    let (status, _) = app
        .call(
            "POST",
            "/api/v1/auth/register",
            Some(json!({ "login": "bob", "password": "secret1" })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);

    // Login → токен.
    let token = app.login("bob", "secret1").await;

    // /auth/me → профиль.
    let (status, body) = app.call("GET", "/api/v1/auth/me", None, Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["login"], "bob");
    assert_eq!(body["role"], "user");

    // Refresh продлевает.
    let (status, body) = app
        .call(
            "POST",
            "/api/v1/auth/refresh",
            Some(json!({ "token": token })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["token"].as_str().is_some_and(|t| !t.is_empty()));
}

#[tokio::test]
async fn login_with_wrong_password_is_unauthorized() {
    let app = TestApp::new();
    let (status, _) = app
        .call(
            "POST",
            "/api/v1/auth/login",
            Some(json!({ "login": ADMIN_LOGIN, "password": "wrong" })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

// ── Auth middleware / RBAC ─────────────────────────────────────

#[tokio::test]
async fn protected_route_without_token_is_401() {
    let app = TestApp::new();
    let (status, body) = app.call("GET", "/api/v1/users", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert!(body["error"]
        .as_str()
        .is_some_and(|m| m.contains("Authorization")));
}

#[tokio::test]
async fn user_cannot_access_admin_users_list() {
    let app = TestApp::new();
    let token = app.user_token().await;
    let (status, body) = app.call("GET", "/api/v1/users", None, Some(&token)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
}

#[tokio::test]
async fn admin_can_list_users() {
    let app = TestApp::new();
    let token = app.admin_token().await;
    let (status, body) = app.call("GET", "/api/v1/users", None, Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let users = body.as_array().expect("array");
    // admin + alice из setup + admin может быть только один.
    assert!(users.len() >= 2, "ожидали минимум 2 пользователей: {body}");
}

// ── Валидация ─────────────────────────────────────────────────

#[tokio::test]
async fn malformed_json_body_is_400_app_format() {
    let app = TestApp::new();
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{not json"))
        .expect("request");
    let resp = app.app.clone().oneshot(req).await.expect("oneshot");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&bytes).expect("json error body");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|m| m.contains("некорректное тело")),
        "ожидали сообщение AppJson: {body}"
    );
}

#[tokio::test]
async fn short_password_register_is_400() {
    let app = TestApp::new();
    let (status, _) = app
        .call(
            "POST",
            "/api/v1/auth/register",
            Some(json!({ "login": "shorty", "password": "12345" })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ── Scenarios (seeded) ────────────────────────────────────────

#[tokio::test]
async fn scenario_list_returns_seeded_active_for_user() {
    let app = TestApp::new();
    let token = app.user_token().await;
    let (status, body) = app
        .call("GET", "/api/v1/scenarios", None, Some(&token))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let list = body.as_array().expect("array");
    assert_eq!(list.len(), 6, "сида дают 6 активных сценариев: {body}");
    assert!(list.iter().all(|s| s["is_active"] == true));
}

#[tokio::test]
async fn user_getting_unknown_scenario_is_404() {
    let app = TestApp::new();
    let token = app.user_token().await;
    let (status, _) = app
        .call("GET", "/api/v1/scenarios/nope", None, Some(&token))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ── Settings RBAC ─────────────────────────────────────────────

#[tokio::test]
async fn user_cannot_write_global_settings() {
    let app = TestApp::new();
    let token = app.user_token().await;
    let (status, body) = app
        .call(
            "PUT",
            "/api/v1/settings/global",
            Some(json!({ "key": "platform.site_name", "value": "Хак" })),
            Some(&token),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
}

#[tokio::test]
async fn admin_can_write_global_settings_and_user_can_read() {
    let app = TestApp::new();

    let admin = app.admin_token().await;
    let (status, body) = app
        .call(
            "PUT",
            "/api/v1/settings/global",
            Some(json!({ "key": "platform.site_name", "value": "Тестовая арена" })),
            Some(&admin),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let user = app.user_token().await;
    let (status, body) = app
        .call("GET", "/api/v1/settings/global", None, Some(&user))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["platform.site_name"], "Тестовая арена");
}

// ── Sessions: start + history ─────────────────────────────────

#[tokio::test]
async fn session_start_and_history_flow() {
    let app = TestApp::new();
    let token = app.user_token().await;

    // Пустая история.
    let (status, body) = app
        .call("GET", "/api/v1/sessions", None, Some(&token))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body.as_array().map(Vec::len), Some(0));

    // Старт по сид-сценарию.
    let (status, body) = app
        .call(
            "POST",
            "/api/v1/sessions",
            Some(json!({ "scenario_id": "sales_easy" })),
            Some(&token),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let session_id = body["session"]["id"]
        .as_str()
        .expect("session.id")
        .to_string();
    assert_eq!(body["session"]["status"], "active");
    assert!(body["opening"].as_str().is_some_and(|s| !s.is_empty()));

    // История содержит одну сессию.
    let (status, body) = app
        .call("GET", "/api/v1/sessions", None, Some(&token))
        .await;
    assert_eq!(status, StatusCode::OK);
    let list = body.as_array().expect("array");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"], session_id);

    // GET /sessions/:id владелец.
    let (status, _) = app
        .call(
            "GET",
            &format!("/api/v1/sessions/{session_id}"),
            None,
            Some(&token),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    // Сообщения: opening-реплика партнёра.
    let (status, body) = app
        .call(
            "GET",
            &format!("/api/v1/sessions/{session_id}/messages"),
            None,
            Some(&token),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let msgs = body.as_array().expect("messages");
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0]["role"], "partner");
}

#[tokio::test]
async fn session_start_unknown_scenario_is_404() {
    let app = TestApp::new();
    let token = app.user_token().await;
    let (status, _) = app
        .call(
            "POST",
            "/api/v1/sessions",
            Some(json!({ "scenario_id": "does-not-exist" })),
            Some(&token),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ── Stats (RBAC) ──────────────────────────────────────────────

#[tokio::test]
async fn user_can_read_own_stats_but_not_overview() {
    let app = TestApp::new();
    let token = app.user_token().await;

    let (status, body) = app
        .call("GET", "/api/v1/stats/me", None, Some(&token))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["login"], USER_LOGIN);

    let (status, _) = app
        .call("GET", "/api/v1/stats/overview", None, Some(&token))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn admin_can_read_overview_and_leaderboard() {
    let app = TestApp::new();
    let token = app.admin_token().await;

    let (status, body) = app
        .call("GET", "/api/v1/stats/overview", None, Some(&token))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["users"].as_u64().is_some());

    let (status, body) = app
        .call("GET", "/api/v1/stats/leaderboard", None, Some(&token))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.is_array());
}
