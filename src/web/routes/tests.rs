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
use crate::config::{AppConfig, LockoutConfig, SecurityConfig, ServerConfig, StorageConfig};
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
            allowed_origins: Vec::new(),
            jwt_refresh_max_age_secs: 7200,
        },
        storage: StorageConfig {
            db_path: std::path::PathBuf::from(":memory:"),
            models_dir: std::path::PathBuf::from("models"),
        },
        rate_limit: crate::config::RateLimitConfig {
            // В тестах много login — лимит выключен.
            auth_max: 0,
            auth_window_secs: 60,
        },
        // В тестах много login — lockout выключен.
        lockout: LockoutConfig {
            max_failures: 0,
            window_secs: 900,
            lockout_secs: 900,
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

    /// Запрос с произвольным content-type и сырым телом: статус, заголовки, байты.
    async fn call_bytes(
        &self,
        method: &str,
        uri: &str,
        content_type: Option<&str>,
        body: Option<Vec<u8>>,
        token: Option<&str>,
    ) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(ct) = content_type {
            builder = builder.header(header::CONTENT_TYPE, ct);
        }
        if let Some(t) = token {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        let req = builder
            .body(Body::from(body.unwrap_or_default()))
            .expect("request");

        let resp = self.app.clone().oneshot(req).await.expect("oneshot");
        let status = resp.status();
        let headers = resp.headers().clone();
        let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024)
            .await
            .expect("body");
        (status, headers, bytes.to_vec())
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
async fn unknown_route_returns_json_404_or_spa_index() {
    let app = TestApp::new();
    let (status, body) = app.call("GET", "/no/such/route", None, None).await;
    if std::path::Path::new("dist/index.html").exists() {
        // Собранный UI: SPA-fallback отдаёт index.html.
        assert_eq!(status, StatusCode::OK);
    } else {
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(body["error"].is_string(), "ожидали JSON с error: {body}");
    }
}

#[tokio::test]
async fn unknown_api_route_returns_json_404_not_spa() {
    let app = TestApp::new();
    // Даже при наличии index.html API-пути не уходят в SPA.
    let (status, body) = app
        .call("GET", "/api/v1/definitely-missing", None, None)
        .await;
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

    // Refresh продлевает (single-use: первый вызов — ок).
    let (status, body) = app
        .call(
            "POST",
            "/api/v1/auth/refresh",
            Some(json!({ "token": token })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let rotated = body["token"].as_str().expect("token").to_string();

    // Повторный refresh с тем же токеном — reuse → 401.
    let (status, body) = app
        .call(
            "POST",
            "/api/v1/auth/refresh",
            Some(json!({ "token": token })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");

    // Новый токен после ротации валиден для /auth/me.
    let (status, body) = app
        .call("GET", "/api/v1/auth/me", None, Some(&rotated))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["login"], "bob");
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

// ── Lockout неудачных входов ───────────────────────────────────

/// Стенд с lockout: `max` неудач → блокировка на `lockout_secs`.
impl TestApp {
    fn with_login_lockout(max_failures: u32) -> Self {
        let mut config = test_config();
        config.lockout.max_failures = max_failures;
        config.lockout.window_secs = 900;
        config.lockout.lockout_secs = 900;

        let db = Arc::new(Database::open_in_memory().expect("in-memory БД"));
        db.run_migrations().expect("миграции");
        let repos = Arc::new(SqliteRepos::new(db.clone()));
        let hash = crate::infrastructure::crypto::password::hash_password(ADMIN_PASSWORD);
        repos
            .users
            .create(ADMIN_LOGIN, &hash, UserRole::Admin, Some("Администратор"))
            .expect("create admin");
        seeds::run(&db).expect("сиды");

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
}

#[tokio::test]
async fn login_lockout_after_max_failures_returns_429() {
    let app = TestApp::with_login_lockout(3);
    let wrong = json!({ "login": ADMIN_LOGIN, "password": "wrong-password" });

    // Две неудачи — 401 (lockout ещё не сработал).
    let (status, _) = app
        .call("POST", "/api/v1/auth/login", Some(wrong.clone()), None)
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = app
        .call("POST", "/api/v1/auth/login", Some(wrong.clone()), None)
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Третья неудача фиксирует блокировку (сама — ещё 401).
    let (status, _) = app
        .call("POST", "/api/v1/auth/login", Some(wrong.clone()), None)
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Четвёртая попытка — 429, даже с правильным паролем.
    let (status, body) = app
        .call(
            "POST",
            "/api/v1/auth/login",
            Some(json!({ "login": ADMIN_LOGIN, "password": ADMIN_PASSWORD })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|m| m.contains("заблокирована")),
        "{body}"
    );
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

// ── Audit log (admin) ───────────────────────────────────────────

#[tokio::test]
async fn audit_requires_admin_and_returns_entries() {
    let app = TestApp::new();

    // Без токена → 401.
    let (status, _) = app.call("GET", "/api/v1/audit", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Обычный пользователь → 403.
    let user = app.user_token().await;
    let (status, body) = app.call("GET", "/api/v1/audit", None, Some(&user)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    // Логины уже записали auth.login в аудит; админ видит журнал.
    let admin = app.admin_token().await;
    let (status, body) = app.call("GET", "/api/v1/audit", None, Some(&admin)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let entries = body.as_array().expect("array");
    assert!(
        entries.iter().any(|e| e["action"] == "auth.login"),
        "ожидали auth.login в аудите: {body}"
    );

    // Фильтр по action.
    let (status, body) = app
        .call("GET", "/api/v1/audit?action=auth.login", None, Some(&admin))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let filtered = body.as_array().expect("array");
    assert!(!filtered.is_empty());
    assert!(filtered.iter().all(|e| e["action"] == "auth.login"));
}

// ── Rate-limit auth + CORS ──────────────────────────────────────

/// Стенд с включённым rate-limit: `max` login-попыток на ключ (XFF) в окне.
impl TestApp {
    fn with_auth_rate_limit(max: u32) -> Self {
        let mut config = test_config();
        config.rate_limit.auth_max = max;
        config.rate_limit.auth_window_secs = 60;

        let db = Arc::new(Database::open_in_memory().expect("in-memory БД"));
        db.run_migrations().expect("миграции");
        let repos = Arc::new(SqliteRepos::new(db.clone()));
        let hash = crate::infrastructure::crypto::password::hash_password(ADMIN_PASSWORD);
        repos
            .users
            .create(ADMIN_LOGIN, &hash, UserRole::Admin, Some("Администратор"))
            .expect("create admin");
        seeds::run(&db).expect("сиды");

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

    /// POST с заголовком X-Forwarded-For (ключ rate-limit в тестах).
    async fn call_xff(
        &self,
        uri: &str,
        body: Value,
        xff: &str,
    ) -> (StatusCode, Value, axum::http::HeaderMap) {
        let req = Request::builder()
            .method("POST")
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .header("x-forwarded-for", xff)
            .body(Body::from(body.to_string()))
            .expect("request");
        let resp = self.app.clone().oneshot(req).await.expect("oneshot");
        let status = resp.status();
        let headers = resp.headers().clone();
        let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024)
            .await
            .expect("body");
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(Value::Null)
        };
        (status, value, headers)
    }
}

#[tokio::test]
async fn auth_login_rate_limited_returns_429_with_retry_after() {
    let app = TestApp::with_auth_rate_limit(2);
    let body = json!({ "login": ADMIN_LOGIN, "password": "wrong-password" });
    let ip = "203.0.113.10";

    // Две попытки в пределах лимита — не 429 (пароль неверный → 401).
    let (status, _, _) = app.call_xff("/api/v1/auth/login", body.clone(), ip).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _, _) = app.call_xff("/api/v1/auth/login", body.clone(), ip).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Третья — 429 + Retry-After.
    let (status, body, headers) = app.call_xff("/api/v1/auth/login", body, ip).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert!(
        headers.get(header::RETRY_AFTER).is_some(),
        "должен быть Retry-After: {headers:?}"
    );
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|m| m.contains("Слишком много попыток")),
        "{body}"
    );

    // Другой IP не заблокирован.
    let (status, _, _) = app
        .call_xff(
            "/api/v1/auth/login",
            json!({ "login": ADMIN_LOGIN, "password": "wrong" }),
            "203.0.113.99",
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn cors_allowlist_only_echoes_allowed_origin() {
    // allowlist: один origin.
    let mut config = test_config();
    config.security.allowed_origins = vec!["https://allowed.example".into()];

    let db = Arc::new(Database::open_in_memory().expect("db"));
    db.run_migrations().expect("миграции");
    let repos = Arc::new(SqliteRepos::new(db.clone()));
    let hash = crate::infrastructure::crypto::password::hash_password(ADMIN_PASSWORD);
    repos
        .users
        .create(ADMIN_LOGIN, &hash, UserRole::Admin, None)
        .expect("admin");
    seeds::run(&db).expect("сиды");
    let cipher =
        Arc::new(SecretCipher::from_secret(&config.security.encryption_secret).expect("cipher"));
    let services = Arc::new(Services::new(&config, repos, cipher.clone()).expect("services"));
    let state = AppState {
        config: Arc::new(config),
        db,
        cipher,
        services,
    };
    let app = build(state);

    // Разрешённый origin → эхо в ответе.
    let req = Request::builder()
        .method("OPTIONS")
        .uri("/api/v1/auth/login")
        .header(header::ORIGIN, "https://allowed.example")
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
        .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "content-type")
        .body(Body::empty())
        .expect("preflight");
    let resp = app.clone().oneshot(req).await.expect("oneshot");
    assert!(
        resp.status().is_success() || resp.status() == StatusCode::NO_CONTENT,
        "preflight: {}",
        resp.status()
    );
    let acao = resp
        .headers()
        .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    assert_eq!(
        acao.as_deref(),
        Some("https://allowed.example"),
        "allowed origin должен эхом: {acao:?}"
    );

    // Чужой origin → без ACAO (или не тот).
    let req = Request::builder()
        .method("OPTIONS")
        .uri("/api/v1/auth/login")
        .header(header::ORIGIN, "https://evil.example")
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
        .body(Body::empty())
        .expect("preflight");
    let resp = app.clone().oneshot(req).await.expect("oneshot");
    let acao = resp
        .headers()
        .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    assert!(
        acao.as_deref() != Some("https://evil.example"),
        "чужой origin не должен получать ACAO: {acao:?}"
    );
}

// ── Model preferences ───────────────────────────────────────────

/// Создаёт local-провайдер + LLM-модель через admin API; возвращает id модели.
async fn seed_llm_model(app: &TestApp, admin: &str) -> String {
    let (status, body) = app
        .call(
            "POST",
            "/api/v1/providers",
            Some(json!({
                "id": "",
                "name": "Локальный",
                "kind": "local",
                "is_enabled": true,
                "created_at": ""
            })),
            Some(admin),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "provider upsert: {body}");
    let provider_id = body["id"].as_str().expect("provider.id").to_string();

    let (status, body) = app
        .call(
            "POST",
            "/api/v1/models",
            Some(json!({
                "id": "",
                "provider_id": provider_id,
                "role": "llm",
                "model_key": "test-llm",
                "display_name": "Test LLM",
                "is_enabled": true,
                "metadata": {},
                "created_at": ""
            })),
            Some(admin),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "model upsert: {body}");
    body["id"].as_str().expect("model.id").to_string()
}

#[tokio::test]
async fn model_preference_set_get_clear_flow_for_user() {
    let app = TestApp::new();
    let admin = app.admin_token().await;
    let model_id = seed_llm_model(&app, &admin).await;

    let user = app.user_token().await;

    // Пусто до выбора.
    let (status, body) = app
        .call("GET", "/api/v1/model-preferences/me", None, Some(&user))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body.as_array().map(Vec::len), Some(0));

    // Варианты для выбора.
    let (status, body) = app
        .call(
            "GET",
            "/api/v1/model-preferences/options?role=llm",
            None,
            Some(&user),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let options = body.as_array().expect("array");
    assert_eq!(options.len(), 1);
    assert_eq!(options[0]["id"], model_id);

    // Выбор.
    let (status, body) = app
        .call(
            "PUT",
            "/api/v1/model-preferences/me/llm",
            Some(json!({ "model_id": model_id })),
            Some(&user),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // Чтение одного.
    let (status, body) = app
        .call("GET", "/api/v1/model-preferences/me/llm", None, Some(&user))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["model_id"], model_id);
    assert_eq!(body["role"], "llm");
    assert_eq!(body["model_display_name"], "Test LLM");

    // Очистка → null.
    let (status, _) = app
        .call(
            "DELETE",
            "/api/v1/model-preferences/me/llm",
            None,
            Some(&user),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = app
        .call("GET", "/api/v1/model-preferences/me/llm", None, Some(&user))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_null(), "после clear предпочтение = null: {body}");
}

#[tokio::test]
async fn user_cannot_set_foreign_preference_but_admin_can() {
    let app = TestApp::new();
    let admin = app.admin_token().await;
    let model_id = seed_llm_model(&app, &admin).await;

    // Целевой user id через /users (admin).
    let (status, body) = app.call("GET", "/api/v1/users", None, Some(&admin)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let users = body.as_array().expect("users");
    let alice = users
        .iter()
        .find(|u| u["login"] == USER_LOGIN)
        .expect("alice")
        .clone();
    let alice_id = alice["id"].as_str().expect("alice.id").to_string();

    // Чужой обычный пользователь → 403 (нужен admin-контекст для чужого id).
    let other = app
        .call(
            "POST",
            "/api/v1/auth/register",
            Some(json!({ "login": "carol", "password": "secret1" })),
            None,
        )
        .await;
    assert_eq!(other.0, StatusCode::CREATED, "{:?}", other.1);
    let carol = app.login("carol", "secret1").await;
    let (status, body) = app
        .call(
            "PUT",
            &format!("/api/v1/model-preferences/users/{alice_id}/llm"),
            Some(json!({ "model_id": model_id })),
            Some(&carol),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    // Админ может назначить за alice и прочитать.
    let (status, body) = app
        .call(
            "PUT",
            &format!("/api/v1/model-preferences/users/{alice_id}/llm"),
            Some(json!({ "model_id": model_id })),
            Some(&admin),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = app
        .call(
            "GET",
            &format!("/api/v1/model-preferences/users/{alice_id}"),
            None,
            Some(&admin),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body.as_array().map(Vec::len), Some(1));

    // Обычный пользователь не читает чужие.
    let (status, _) = app
        .call(
            "GET",
            &format!("/api/v1/model-preferences/users/{alice_id}"),
            None,
            Some(&carol),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn preference_rejects_unknown_model_and_bad_role() {
    let app = TestApp::new();
    let user = app.user_token().await;

    // Неизвестная модель → 404.
    let (status, body) = app
        .call(
            "PUT",
            "/api/v1/model-preferences/me/llm",
            Some(json!({ "model_id": "does-not-exist" })),
            Some(&user),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // Плохая роль в пути → 400.
    let (status, body) = app
        .call(
            "PUT",
            "/api/v1/model-preferences/me/voice",
            Some(json!({ "model_id": "x" })),
            Some(&user),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    // Неизвестная роль в query options → 400.
    let (status, _) = app
        .call(
            "GET",
            "/api/v1/model-preferences/options?role=nope",
            None,
            Some(&user),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ── Model assignments (глобальные роли) ─────────────────────────

#[tokio::test]
async fn model_assignment_unassign_flow() {
    let app = TestApp::new();
    let admin = app.admin_token().await;
    let model_id = seed_llm_model(&app, &admin).await;

    // Назначение.
    let (status, body) = app
        .call(
            "PUT",
            "/api/v1/model-assignments/llm",
            Some(json!({ "model_id": model_id })),
            Some(&admin),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "assign: {body}");
    let (status, body) = app
        .call("GET", "/api/v1/model-assignments", None, Some(&admin))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body.as_array()
            .is_some_and(|a| a.iter().any(|x| x["role"] == "llm")),
        "после assign роль llm в списке: {body}"
    );

    // Снятие назначения.
    let (status, body) = app
        .call(
            "DELETE",
            "/api/v1/model-assignments/llm",
            None,
            Some(&admin),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "unassign: {body}");
    let (status, body) = app
        .call("GET", "/api/v1/model-assignments", None, Some(&admin))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body.as_array()
            .is_some_and(|a| a.iter().all(|x| x["role"] != "llm")),
        "после unassign роль llm отсутствует: {body}"
    );

    // Повторный assign после снятия работает.
    let (status, body) = app
        .call(
            "PUT",
            "/api/v1/model-assignments/llm",
            Some(json!({ "model_id": model_id })),
            Some(&admin),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "re-assign: {body}");

    // Обычный пользователь → 403.
    let user = app.user_token().await;
    let (status, body) = app
        .call("DELETE", "/api/v1/model-assignments/llm", None, Some(&user))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    // Некорректная роль → 400.
    let (status, body) = app
        .call(
            "DELETE",
            "/api/v1/model-assignments/voice",
            None,
            Some(&admin),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn scenario_generate_without_llm_returns_503() {
    let app = TestApp::new();
    let admin = app.admin_token().await;

    let (status, body) = app
        .call(
            "POST",
            "/api/v1/scenarios/generate",
            Some(json!({ "brief": "Переговоры о зарплате" })),
            Some(&admin),
        )
        .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    let msg = body["error"].as_str().expect("error-сообщение");
    assert!(msg.contains("не настроен"), "{msg}");
    assert!(msg.contains("llm"), "{msg}");
}

// ── Voice: TTS / STT ──────────────────────────────────────────

/// Сеедит OpenAI-совместимого провайдера, указывающего на `mock_base`,
/// и назначает модели на роли `tts` / `stt`.
async fn seed_voice_models(app: &TestApp, admin: &str, mock_base: &str) {
    let (status, body) = app
        .call(
            "POST",
            "/api/v1/providers",
            Some(json!({
                "id": "",
                "name": "MockVoice",
                "kind": "openai_compatible",
                "base_url": mock_base,
                "is_enabled": true,
                "created_at": "",
                "api_key": "test-key"
            })),
            Some(admin),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "provider upsert: {body}");
    let provider_id = body["id"].as_str().expect("provider.id").to_string();

    for (role, model_key) in [("tts", "tts-1"), ("stt", "whisper-1")] {
        let (status, body) = app
            .call(
                "POST",
                "/api/v1/models",
                Some(json!({
                    "id": "",
                    "provider_id": provider_id,
                    "role": role,
                    "model_key": model_key,
                    "display_name": model_key,
                    "is_enabled": true,
                    "metadata": {},
                    "created_at": ""
                })),
                Some(admin),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "model upsert ({role}): {body}");
        let model_id = body["id"].as_str().expect("model.id").to_string();

        let (status, body) = app
            .call(
                "PUT",
                &format!("/api/v1/model-assignments/{role}"),
                Some(json!({ "model_id": model_id })),
                Some(admin),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "assign {role}: {body}");
    }
}

#[tokio::test]
async fn voice_endpoints_require_auth() {
    let app = TestApp::new();

    // TTS без токена → 401 (AuthUser до разбора тела).
    let (status, _, bytes) = app
        .call_bytes(
            "POST",
            "/api/v1/voice/tts",
            Some("application/json"),
            Some(r#"{"text":"Привет"}"#.as_bytes().to_vec()),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let body: Value = serde_json::from_slice(&bytes).expect("json 401");
    assert!(body["error"]
        .as_str()
        .is_some_and(|m| m.contains("Authorization")));

    // STT без токена → 401 (даже с битым multipart — auth раньше extractors тела).
    let (status, _, bytes) = app
        .call_bytes(
            "POST",
            "/api/v1/voice/stt",
            Some("multipart/form-data; boundary=x"),
            Some(b"garbage".to_vec()),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let body: Value = serde_json::from_slice(&bytes).expect("json 401");
    assert!(body["error"].as_str().is_some());
}

#[tokio::test]
async fn tts_without_configured_tts_returns_503() {
    let app = TestApp::new();
    let token = app.user_token().await;
    let (status, body) = app
        .call(
            "POST",
            "/api/v1/voice/tts",
            Some(json!({ "text": "Привет" })),
            Some(&token),
        )
        .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    let msg = body["error"].as_str().expect("error-сообщение");
    assert!(msg.contains("TTS"), "сообщение должно упоминать TTS: {msg}");
    assert!(msg.contains("не настроен"), "{msg}");
}

#[tokio::test]
async fn tts_rejects_overlong_text() {
    let app = TestApp::new();
    let token = app.user_token().await;
    let long = "ы".repeat(2001);
    let (status, body) = app
        .call(
            "POST",
            "/api/v1/voice/tts",
            Some(json!({ "text": long })),
            Some(&token),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["error"].as_str().is_some_and(|m| m.contains("2000")),
        "{body}"
    );

    // Пустой текст — тоже 400.
    let (status, _) = app
        .call(
            "POST",
            "/api/v1/voice/tts",
            Some(json!({ "text": "   " })),
            Some(&token),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn stt_broken_multipart_returns_400() {
    let app = TestApp::new();
    let token = app.user_token().await;

    // multipart/form-data без корректного boundary/тела.
    let (status, _, bytes) = app
        .call_bytes(
            "POST",
            "/api/v1/voice/stt",
            Some("multipart/form-data"),
            Some(b"this is not multipart".to_vec()),
            Some(&token),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let body: Value = serde_json::from_slice(&bytes).expect("json 400");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|m| m.contains("multipart")),
        "ожидали сообщение про multipart: {body}"
    );

    // Корректный multipart без поля file.
    let boundary = "----negBroken";
    let mut body_bytes = Vec::new();
    body_bytes.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"payload\"\r\n\r\nnope\r\n--{boundary}--\r\n"
        )
        .as_bytes(),
    );
    let (status, _, bytes) = app
        .call_bytes(
            "POST",
            "/api/v1/voice/stt",
            Some(&format!("multipart/form-data; boundary={boundary}")),
            Some(body_bytes),
            Some(&token),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let body: Value = serde_json::from_slice(&bytes).expect("json 400");
    assert!(
        body["error"].as_str().is_some_and(|m| m.contains("file")),
        "ожидали сообщение про поле file: {body}"
    );
}

#[tokio::test]
async fn tts_happy_path_returns_audio_binary() {
    use axum::routing::post as route_post;
    use axum::{Json as AxumJson, Router};

    async fn speech(
        AxumJson(body): AxumJson<Value>,
    ) -> ([(axum::http::header::HeaderName, &'static str); 1], Vec<u8>) {
        assert_eq!(body["model"], "tts-1");
        assert_eq!(body["input"], "Здравствуйте");
        (
            [(axum::http::header::CONTENT_TYPE, "audio/mpeg")],
            b"ID3fake-audio".to_vec(),
        )
    }

    let mock = crate::infrastructure::providers::testkit::spawn(
        Router::new().route("/v1/audio/speech", route_post(speech)),
    )
    .await;

    let app = TestApp::new();
    let admin = app.admin_token().await;
    seed_voice_models(&app, &admin, &format!("{mock}/v1")).await;
    let user = app.user_token().await;

    let (status, headers, bytes) = app
        .call_bytes(
            "POST",
            "/api/v1/voice/tts",
            Some("application/json"),
            Some(r#"{"text":"Здравствуйте"}"#.as_bytes().to_vec()),
            Some(&user),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    assert_eq!(
        headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("audio/mpeg")
    );
    assert_eq!(bytes, b"ID3fake-audio");
}

#[tokio::test]
async fn stt_happy_path_returns_text() {
    use axum::extract::Multipart;
    use axum::routing::post as route_post;
    use axum::{Json as AxumJson, Router};

    async fn transcribe(mut multipart: Multipart) -> AxumJson<Value> {
        let mut saw_file = false;
        while let Some(field) = multipart.next_field().await.expect("field") {
            if field.name() == Some("file") {
                saw_file = true;
            }
        }
        assert!(saw_file, "адаптер не отправил поле file");
        AxumJson(json!({ "text": "привет из мока" }))
    }

    let mock = crate::infrastructure::providers::testkit::spawn(
        Router::new().route("/v1/audio/transcriptions", route_post(transcribe)),
    )
    .await;

    let app = TestApp::new();
    let admin = app.admin_token().await;
    seed_voice_models(&app, &admin, &format!("{mock}/v1")).await;
    let user = app.user_token().await;

    let boundary = "----negotiationBoundary";
    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"clip.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(b"RIFFdata");
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let (status, _, bytes) = app
        .call_bytes(
            "POST",
            "/api/v1/voice/stt",
            Some(&format!("multipart/form-data; boundary={boundary}")),
            Some(body),
            Some(&user),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    let value: Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(value["text"], "привет из мока");
}
