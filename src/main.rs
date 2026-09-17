use tower_http::services::ServeDir;

mod db;
mod engine;
mod models;
mod tts;

use axum::{
    extract::{Path, State},
    http::{header, StatusCode, Request},
    middleware::{self, Next},
    response::{Html, Response},
    routing::{get, post, put, delete},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tera::Tera;
use tokio::sync::RwLock;

use crate::db::Store;
use crate::engine::*;
use crate::models::*;

// ─────────────────────────────────────────────────────────────
// Состояние приложения
// ─────────────────────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
    tera: Arc<Tera>,
    store: Arc<Store>,
    sessions: Arc<RwLock<HashMap<String, NegotiationSession>>>,
}

// ─────────────────────────────────────────────────────────────
// Middleware: JWT авторизация
// ─────────────────────────────────────────────────────────────

async fn auth_middleware(
    State(_state): State<AppState>,
    mut request: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let path = request.uri().path().to_string();

    // Публичные маршруты (HTML страницы и нечувствительные API)
    // HTML страницы проверяют auth на клиенте и редиректят на /auth
    if path == "/"
        || path == "/auth"
        || path == "/dashboard"
        || path == "/admin"
        || path == "/admin/scenarios"
        || path == "/admin/users"
        || path == "/admin/settings"
        || path.starts_with("/dialogue/")
        || path.starts_with("/result/")
        || path.starts_with("/static/")
        || path == "/api/login"
        || path == "/api/register"
        || path == "/api/scenarios"
        || path == "/api/generate-scenario"
        || path == "/api/start-generated"
        || path == "/api/config"
        || path.starts_with("/api/dialogue/")
        || path == "/api/history"
    {
        return Ok(next.run(request).await);
    }

    // Защищённые API маршруты — проверяем токен
    let token = request.headers()
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|cookies| {
            cookies.split(';')
                .find(|c| c.trim().starts_with("token="))
                .map(|c| c.trim().trim_start_matches("token=").to_string())
        })
        .or_else(|| {
            request.headers()
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
                .map(|v| v.to_string())
        });

    match token {
        Some(t) => {
            if let Some((user_id, role)) = db::verify_token(&t) {
                // Проверка прав для admin маршрутов
                if path.starts_with("/admin") && role != "admin" {
                    return Err(StatusCode::FORBIDDEN);
                }
                request.extensions_mut().insert((user_id, role));
                Ok(next.run(request).await)
            } else {
                Err(StatusCode::UNAUTHORIZED)
            }
        }
        None => Err(StatusCode::UNAUTHORIZED),
    }
}

// ─────────────────────────────────────────────────────────────
// Точка входа
// ─────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    // Загрузка .env файла
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt::init();

    // Инициализация БД
    let store = Store::new("negotiation_arena.db")
        .expect("Failed to initialize database");

    let tera = Arc::new(
        Tera::new("templates/**/*").expect("Failed to initialize Tera templates")
    );

    let state = AppState {
        tera,
        store,
        sessions: Arc::new(RwLock::new(HashMap::new())),
    };

    // ──────────── Маршруты ────────────

    let public_routes = Router::new()
        // UI
        .route("/", get(start_screen))
        .route("/auth", get(auth_screen))
        .route("/dashboard", get(dashboard_screen))
        .route("/dialogue/:session_id", get(dialogue_screen))
        .route("/result/:session_id", get(result_screen))
        // API (публичные)
        .route("/api/login", post(api_login))
        .route("/api/register", post(api_register))
        .route("/api/config", post(start_negotiation))
        .route("/api/dialogue/:session_id/respond", post(handle_response))
        .route("/api/dialogue/:session_id/tts", post(handle_tts))
        .route("/api/dialogue/:session_id/stt", post(handle_stt));

    let admin_routes = Router::new()
        // Admin UI
        .route("/admin", get(admin_screen))
        .route("/admin/scenarios", get(admin_scenarios_screen))
        .route("/admin/users", get(admin_users_screen))
        .route("/admin/settings", get(admin_settings_screen))
        // Admin API
        .route("/api/admin/scenarios", get(admin_list_scenarios))
        .route("/api/admin/scenarios", post(admin_upsert_scenario))
        .route("/api/admin/scenarios/:id", delete(admin_delete_scenario))
        .route("/api/admin/users", get(admin_list_users))
        .route("/api/admin/users/:id/role", put(admin_update_user_role))
        .route("/api/admin/users/:id", delete(admin_delete_user))
        .route("/api/admin/roles", get(admin_list_roles))
        .route("/api/admin/roles", post(admin_upsert_role))
        .route("/api/admin/roles/:id", delete(admin_delete_role))
        .route("/api/admin/settings", get(admin_list_settings))
        .route("/api/admin/settings", post(admin_update_setting))
        .route("/api/admin/opponents", get(admin_list_opponents))
        .route("/api/admin/opponents", post(admin_upsert_opponent))
        .route("/api/admin/opponents/:id", delete(admin_delete_opponent))
        .route("/api/admin/stats", get(admin_stats))
        .route("/api/scenarios", get(list_scenarios))
        // AI Scenario Generation
        .route("/api/generate-scenario", post(generate_scenario))
        .route("/api/start-generated", post(start_generated_scenario))
        // History
        .route("/api/history", get(get_user_history));

    let app = Router::new()
        .merge(public_routes)
        .merge(admin_routes)
        .nest_service("/static", ServeDir::new("static"))
        .layer(middleware::from_fn_with_state(state.clone(), auth_middleware))
        .fallback(get(not_found))
        .with_state(state);

    let addr = "0.0.0.0:3001";
    println!("🚀 Server running at http://{}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// ─────────────────────────────────────────────────────────────
// UI Handlers
// ─────────────────────────────────────────────────────────────

async fn start_screen() -> Html<&'static str> {
    Html(include_str!("../static/auth.html"))
}

async fn auth_screen() -> Html<&'static str> {
    Html(include_str!("../static/auth.html"))
}

async fn dashboard_screen() -> Html<&'static str> {
    Html(include_str!("../static/dashboard.html"))
}

async fn not_found() -> (StatusCode, &'static str) {
    (StatusCode::NOT_FOUND, "Not Found")
}

// ─────────────────────────────────────────────────────────────
// Auth API
// ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct LoginRequest {
    login: String,
    password: String,
}

#[derive(Serialize)]
struct AuthResponse {
    success: bool,
    token: Option<String>,
    user: Option<db::User>,
    message: Option<String>,
}

async fn api_login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<AuthResponse>, StatusCode> {
    let (user_id, password_hash, role) = match state.store
        .get_user_by_login(&req.login)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        Some((id, pw, role)) => (id, pw, role),
        None => {
            return Ok(Json(AuthResponse {
                success: false,
                token: None,
                user: None,
                message: Some("Пользователь не найден".to_string()),
            }));
        }
    };

    if !db::verify_password(&req.password, &password_hash) {
        return Ok(Json(AuthResponse {
            success: false,
            token: None,
            user: None,
            message: Some("Неверный пароль".to_string()),
        }));
    }

    let token = db::create_token(&user_id, &role);
    let user = db::User {
        id: user_id,
        login: req.login,
        role,
        created_at: String::new(),
    };

    Ok(Json(AuthResponse {
        success: true,
        token: Some(token),
        user: Some(user),
        message: None,
    }))
}

async fn api_register(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<AuthResponse>, StatusCode> {
    if req.login.len() < 3 || req.password.len() < 6 {
        return Ok(Json(AuthResponse {
            success: false,
            token: None,
            user: None,
            message: Some("Логин минимум 3 символа, пароль — 6".to_string()),
        }));
    }

    let password_hash = db::hash_password(&req.password);
    let user_id = state.store
        .create_user(&req.login, &password_hash, "user")
        .map_err(|_| StatusCode::CONFLICT)?;

    let token = db::create_token(&user_id, "user");
    let user = db::User {
        id: user_id,
        login: req.login,
        role: "user".to_string(),
        created_at: String::new(),
    };

    Ok(Json(AuthResponse {
        success: true,
        token: Some(token),
        user: Some(user),
        message: None,
    }))
}

// ─────────────────────────────────────────────────────────────
// Admin UI Screens
// ─────────────────────────────────────────────────────────────

async fn admin_screen(
    State(state): State<AppState>,
) -> Result<Html<String>, StatusCode> {
    let users = state.store.list_users().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let scenarios = state.store.list_scenarios().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut context = tera::Context::new();
    context.insert("users_count", &users.len());
    context.insert("scenarios_count", &scenarios.len());
    context.insert("active_scenarios", &scenarios.iter().filter(|s| s.is_active).count());

    state.tera.render("admin/index.html", &context)
        .map_err(|e| { eprintln!("Template error: {}", e); StatusCode::INTERNAL_SERVER_ERROR })
        .map(Html)
}

async fn admin_scenarios_screen(
    State(state): State<AppState>,
) -> Result<Html<String>, StatusCode> {
    let scenarios = state.store.list_scenarios().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut context = tera::Context::new();
    context.insert("scenarios", &scenarios);

    state.tera.render("admin/scenarios.html", &context)
        .map_err(|e| { eprintln!("Template error: {}", e); StatusCode::INTERNAL_SERVER_ERROR })
        .map(Html)
}

async fn admin_users_screen(
    State(state): State<AppState>,
) -> Result<Html<String>, StatusCode> {
    let users = state.store.list_users().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let roles = state.store.list_roles().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut context = tera::Context::new();
    context.insert("users", &users);
    context.insert("roles", &roles);

    state.tera.render("admin/users.html", &context)
        .map_err(|e| { eprintln!("Template error: {}", e); StatusCode::INTERNAL_SERVER_ERROR })
        .map(Html)
}

async fn admin_settings_screen(
    State(state): State<AppState>,
) -> Result<Html<String>, StatusCode> {
    let settings = state.store.list_settings().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let opponents = state.store.list_opponents().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let roles = state.store.list_roles().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut context = tera::Context::new();
    context.insert("settings", &settings);
    context.insert("opponents", &opponents);
    context.insert("roles", &roles);

    state.tera.render("admin/settings.html", &context)
        .map_err(|e| { eprintln!("Template error: {}", e); StatusCode::INTERNAL_SERVER_ERROR })
        .map(Html)
}

// ─────────────────────────────────────────────────────────────
// Admin API: Scenarios
// ─────────────────────────────────────────────────────────────

async fn admin_list_scenarios(
    State(state): State<AppState>,
) -> Result<Json<Vec<db::ScenarioRow>>, StatusCode> {
    state.store.list_scenarios()
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(Deserialize)]
struct ScenarioInput {
    id: Option<String>,
    title: String,
    description: String,
    sphere: String,
    difficulty: String,
    partner_name: String,
    partner_role: String,
    partner_goals: String,
    initial_context: String,
    dialogue_tree: String,
    endings: String,
    partner_batna: String,
    player_batna: String,
    is_active: bool,
}

async fn admin_upsert_scenario(
    State(state): State<AppState>,
    Json(input): Json<ScenarioInput>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let id = input.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let now = chrono::Utc::now().to_rfc3339();

    let scenario = db::ScenarioRow {
        id: id.clone(),
        title: input.title,
        description: input.description,
        sphere: input.sphere,
        difficulty: input.difficulty,
        partner_name: input.partner_name,
        partner_role: input.partner_role,
        partner_goals: input.partner_goals,
        initial_context: input.initial_context,
        dialogue_tree: input.dialogue_tree,
        endings: input.endings,
        partner_batna: input.partner_batna,
        player_batna: input.player_batna,
        is_active: input.is_active,
        created_at: now,
    };

    state.store.upsert_scenario(&scenario)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({"success": true, "id": id})))
}

async fn admin_delete_scenario(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    state.store.delete_scenario(&id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({"success": true})))
}

// ─────────────────────────────────────────────────────────────
// Admin API: Users
// ─────────────────────────────────────────────────────────────

async fn admin_list_users(
    State(state): State<AppState>,
) -> Result<Json<Vec<db::User>>, StatusCode> {
    state.store.list_users()
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn admin_update_user_role(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let role = body["role"].as_str().unwrap_or("user");
    state.store.update_user_role(&id, role)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({"success": true})))
}

async fn admin_delete_user(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    state.store.delete_user(&id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({"success": true})))
}

// ─────────────────────────────────────────────────────────────
// Admin API: Roles
// ─────────────────────────────────────────────────────────────

async fn admin_list_roles(
    State(state): State<AppState>,
) -> Result<Json<Vec<db::RoleRow>>, StatusCode> {
    state.store.list_roles()
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn admin_upsert_role(
    State(state): State<AppState>,
    Json(input): Json<db::RoleRow>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    state.store.upsert_role(&input)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({"success": true})))
}

async fn admin_delete_role(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    state.store.delete_role(&id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({"success": true})))
}

// ─────────────────────────────────────────────────────────────
// Admin API: Settings & Opponents
// ─────────────────────────────────────────────────────────────

async fn admin_list_settings(
    State(state): State<AppState>,
) -> Result<Json<Vec<db::SettingRow>>, StatusCode> {
    state.store.list_settings()
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn admin_update_setting(
    State(state): State<AppState>,
    Json(input): Json<db::SettingRow>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    state.store.set_setting(&input.key, &input.value)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({"success": true})))
}

async fn admin_list_opponents(
    State(state): State<AppState>,
) -> Result<Json<Vec<db::OpponentProfile>>, StatusCode> {
    state.store.list_opponents()
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn admin_upsert_opponent(
    State(state): State<AppState>,
    Json(input): Json<db::OpponentProfile>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    state.store.upsert_opponent(&input)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({"success": true})))
}

async fn admin_delete_opponent(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    state.store.delete_opponent(&id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({"success": true})))
}

async fn admin_stats(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let stats = get_stats(&state).await.unwrap_or_default();
    Ok(Json(stats))
}

// ─────────────────────────────────────────────────────────────
// Вспомогательные функции
// ─────────────────────────────────────────────────────────────

async fn get_stats(state: &AppState) -> Option<serde_json::Value> {
    let users = state.store.list_users().ok()?;
    let scenarios = state.store.list_scenarios().ok()?;
    Some(serde_json::json!({
        "users_count": users.len(),
        "scenarios_count": scenarios.len(),
        "active_scenarios": scenarios.iter().filter(|s| s.is_active).count(),
    }))
}

// ─────────────────────────────────────────────────────────────
// Dialogue API (с существующей логикой)
// ─────────────────────────────────────────────────────────────

async fn start_negotiation(
    State(state): State<AppState>,
    Json(config): Json<NegotiationConfig>,
) -> Result<Json<StartResponse>, StatusCode> {
    let all_scenarios = load_scenarios();
    let scenario = all_scenarios.get(&config.scenario_id)
        .ok_or(StatusCode::NOT_FOUND)?;

    let session_id = uuid::Uuid::new_v4().to_string();
    let mode = config.mode.clone();
    let session = NegotiationSession::new(session_id.clone(), scenario.clone(), config, mode);

    let start_node = session.get_current_node()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let initial = start_node.clone();
    let partner_name = session.scenario.partner_name.clone();
    let partner_role = session.scenario.partner_role.clone();

    let mut sessions = state.sessions.write().await;
    sessions.insert(session_id.clone(), session);

    Ok(Json(StartResponse {
        session_id,
        partner_name,
        partner_role,
        initial_node: initial,
    }))
}

async fn dialogue_screen(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Html<String>, StatusCode> {
    let sessions = state.sessions.read().await;
    let session = sessions.get(&session_id)
        .ok_or(StatusCode::NOT_FOUND)?;

    let current_node = session.get_current_node()
        .ok_or(StatusCode::NOT_FOUND)?;

    let mut context = tera::Context::new();
    context.insert("session_id", &session.id);
    context.insert("partner_name", &session.scenario.partner_name);
    context.insert("partner_role", &session.scenario.partner_role);
    context.insert("initial_context", &session.scenario.initial_context);
    context.insert("start_text", &current_node.text);
    context.insert("current_score", &session.current_score);
    context.insert("partner_avatar", "/static/avatar.svg");
    context.insert("mode", &session.mode);
    context.insert("player_batna", &session.scenario.player_batna);
    context.insert("partner_batna", &session.scenario.partner_batna);

    let responses_json = serde_json::to_string(&current_node.responses)
        .unwrap_or_else(|_| "[]".to_string());
    context.insert("initial_responses_json", &responses_json);

    let html = state.tera.render("dialogue.html", &context)
        .map_err(|e| { eprintln!("Template error: {}", e); StatusCode::INTERNAL_SERVER_ERROR })?;
    Ok(Html(html))
}

async fn handle_response(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(response): Json<PlayerResponse>,
) -> Result<Json<DialogueResponse>, StatusCode> {
    // Сначала получаем данные, потом отпускаем lock
    let (partner_name, partner_role, scenario_context, history_snapshot, spin_type, current_node) = {
        let sessions = state.sessions.read().await;
        let session = sessions.get(&session_id)
            .ok_or(StatusCode::NOT_FOUND)?;

        let current_node = session.get_current_node()
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?
            .clone();

        // Собираем snapshot истории
        let history_snapshot: Vec<(String, String)> = session.history.iter()
            .map(|h| (h.partner_text.clone(), h.player_text.clone()))
            .collect();

        let selected = current_node.responses.iter().find(|r| r.id == response.response_id);
        let spin_type = selected.map(|r| r.spin_type.as_str()).unwrap_or("").to_string();

        (
            session.scenario.partner_name.clone(),
            session.scenario.partner_role.clone(),
            session.scenario.initial_context.clone(),
            history_snapshot,
            spin_type,
            current_node,
        )
    };
    // write lock отпущен здесь

    // AI-генерация ответа (без удержания lock)
    let selected = current_node.responses.iter().find(|r| r.id == response.response_id);
    let player_text = selected.map(|r| r.text.as_str()).unwrap_or("");

    let partner_text = engine::generate_partner_response_ai(
        &partner_name,
        &partner_role,
        &scenario_context,
        player_text,
        &history_snapshot,
        &spin_type,
    ).await
    .unwrap_or_else(|e| {
        eprintln!("AI response error: {}", e);
        // Fallback на хардкод
        match selected.map(|r| r.strategy.as_str()) {
            Some("Сотрудничество") => "Понимаю вашу позицию. Давайте найдём решение, которое устроит обе стороны.".to_string(),
            Some("Компромисс") => "Хорошо, я готов обсудить компромиссные варианты. Что вы предлагаете?".to_string(),
            Some("Конфронтация") => "Это интересный подход, но давайте рассмотрим это подробнее.".to_string(),
            _ => "Продолжайте, пожалуйста.".to_string(),
        }
    });

    // Теперь обновляем сессию
    let mut sessions = state.sessions.write().await;
    let session = sessions.get_mut(&session_id)
        .ok_or(StatusCode::NOT_FOUND)?;

    let next_node = session.advance(&response, &current_node);
    let is_final = next_node.map(|n| n.is_ending).unwrap_or(true);
    let current_score = session.current_score;
    let score_delta = session.history.last().map(|h| h.score_delta).unwrap_or(0);

    let history_entry = HistoryEntry {
        partner_text: partner_text.clone(),
        player_text: player_text.to_string(),
        strategy: response.selected_strategy.clone(),
        score_delta,
    };
    session.history.push(history_entry);

    let feedback = if is_final {
        let ending = determine_ending(&session.scenario, session.current_score);
        let fb = generate_feedback(session, &ending);

        // Сохраняем в историю БД
        let scenario_id = session.scenario.id.clone();
        let ending_title = ending.title.clone();
        let total_score = session.current_score;
        let session_id_clone = session.id.clone();

        drop(sessions); // отпускаем write lock перед вставкой в БД
        if let Err(e) = state.store.save_session(&db::SessionHistory {
            id: session_id_clone,
            user_id: "anonymous".to_string(),
            scenario_id,
            score: total_score,
            ending: ending_title,
            feedback: fb.clone(),
            created_at: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        }) {
            eprintln!("Failed to save session history: {}", e);
        }

        // Очищаем от управляющих символов
        let clean = |s: &str| -> String {
            s.chars().filter(|c| !c.is_control())
                .collect::<String>()
                .split_whitespace().collect::<Vec<&str>>().join(" ")
        };

        let clean_partner = clean(&partner_text);
        let clean_fb = clean(&fb);

        return Ok(Json(DialogueResponse {
            partner_text: clean_partner,
            score_delta,
            is_final: true,
            current_score,
            next_node: None,
            feedback: Some(clean_fb),
        }));
    } else {
        None
    };

    // Очищаем от управляющих символов перед JSON-сериализацией
    let clean_text = |s: &str| -> String {
        s.chars()
            .filter(|c| !c.is_control())
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<&str>>()
            .join(" ")
    };

    // Очищаем feedback от markdown — заменяем переносы на пробелы
    let clean_feedback = feedback.map(|fb: String| {
        let cleaned: String = fb.chars()
            .filter(|c| !c.is_control())
            .collect();
        cleaned.split_whitespace().collect::<Vec<&str>>().join(" ")
    });

    let mut next = session.get_current_node().cloned();
    if let Some(ref mut node) = next {
        node.text = clean_text(&node.text);
        for resp in &mut node.responses {
            resp.text = clean_text(&resp.text);
        }
    }

    Ok(Json(DialogueResponse {
        partner_text: clean_text(&partner_text),
        score_delta,
        is_final,
        current_score,
        next_node: if is_final { None } else { next },
        feedback: clean_feedback,
    }))
}

async fn result_screen(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Html<String>, StatusCode> {
    let sessions = state.sessions.read().await;
    let session = sessions.get(&session_id)
        .ok_or(StatusCode::NOT_FOUND)?;

    let ending = determine_ending(&session.scenario, session.current_score);
    let feedback = generate_feedback(session, &ending);
    let tech = session.technique_summary();

    let mut context = tera::Context::new();
    context.insert("session_id", &session.id);
    context.insert("total_score", &session.current_score);
    context.insert("strategy_score", &session.strategy_score);
    context.insert("argument_score", &session.argument_score);
    context.insert("tone_score", &session.tone_score);
    context.insert("feedback", &feedback);
    context.insert("ending_title", &ending.title);
    context.insert("ending_text", &ending.text);
    context.insert("partner_name", &session.scenario.partner_name);
    context.insert("history", &session.history);
    context.insert("turn_count", &session.turn_count);
    context.insert("partner_batna", &session.scenario.partner_batna);
    context.insert("player_batna", &session.scenario.player_batna);
    context.insert("spin_counts", &tech.spin_counts);
    context.insert("interest_focused", &tech.interest_focused);
    context.insert("objective_criteria_used", &tech.objective_criteria_used);
    context.insert("collaboration_count", &tech.collaboration_count);
    context.insert("compromise_count", &tech.compromise_count);
    context.insert("confrontation_count", &tech.confrontation_count);

    let html = state.tera.render("result.html", &context)
        .map_err(|e| { eprintln!("Template error: {}", e); StatusCode::INTERNAL_SERVER_ERROR })?;
    Ok(Html(html))
}

async fn get_user_history(
    State(state): State<AppState>,
) -> Result<Json<Vec<db::SessionHistory>>, StatusCode> {
    let history = state.store.get_user_history("anonymous")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(history))
}

async fn _list_scenarios_api() -> Json<Vec<Scenario>> {
    Json(load_scenarios().into_values().collect())
}

async fn handle_tts(
    State(_state): State<AppState>,
    Json(body): Json<tts::TtsRequest>,
) -> Result<Json<tts::TtsResponse>, StatusCode> {
    let speaker = body.speaker.unwrap_or_else(|| "aidar".to_string());
    tts::text_to_speech(&body.text, &speaker)
        .await
        .map(|audio_base64| Json(tts::TtsResponse { audio_base64 }))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn handle_stt(
    mut multipart: axum::extract::Multipart,
) -> Result<Json<tts::SttResponse>, StatusCode> {
    let mut audio_data = Vec::new();
    let mut filename = "recording.webm".to_string();

    while let Some(field) = multipart.next_field().await.map_err(|_| StatusCode::BAD_REQUEST)? {
        filename = field.file_name().unwrap_or("recording.webm").to_string();
        let data = field.bytes().await.map_err(|_| StatusCode::BAD_REQUEST)?;
        audio_data.extend_from_slice(&data);
    }

    if audio_data.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    tts::speech_to_text(audio_data, &filename).await
        .map(|text| Json(tts::SttResponse { text }))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn list_scenarios() -> Json<Vec<Scenario>> {
    Json(load_scenarios().into_values().collect())
}

// ─────────────────────────────────────────────────────────────
// AI Scenario Generation
// ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct GenerateScenarioRequest {
    topic: String,
    difficulty: String,
    sphere: String,
    opponent_style: String,
    details: Option<String>,
}

#[derive(Deserialize, Serialize)]
struct GeneratedScenario {
    title: String,
    description: String,
    partner_name: String,
    partner_role: String,
    player_goal: String,
    partner_goal: String,
    player_batna: String,
    partner_batna: String,
    initial_context: String,
    #[serde(default)]
    difficulty: Option<String>,
    #[serde(default)]
    sphere: Option<String>,
}

#[derive(Serialize)]
struct GenerateScenarioResponse {
    success: bool,
    scenario: Option<GeneratedScenario>,
    message: Option<String>,
}

async fn generate_scenario(
    Json(req): Json<GenerateScenarioRequest>,
) -> Result<Json<GenerateScenarioResponse>, StatusCode> {
    let prompt = format!(
        r#"Создай детальный сценарий переговоров на тему: "{}"

Параметры:
- Сфера: {}
- Сложность: {}
- Стиль оппонента: {}
{}

Формат ответа (строго JSON без markdown):
{{
  "title": "Краткое название сценария",
  "description": "Подробное описание ситуации (2-3 предложения)",
  "partner_name": "Имя оппонента",
  "partner_role": "Должность и роль оппонента",
  "player_goal": "Что должен достичь игрок (конкретная цель)",
  "partner_goal": "Чего хочет оппонент (конкретная цель)",
  "player_batna": "Лучшая альтернатива игрока (BATNA)",
  "partner_batna": "Лучшая альтернатива оппонента (BATNA)",
  "initial_context": "Начальный контекст для диалога (1-2 предложения)"
}}"#,
        req.topic,
        req.sphere,
        req.difficulty,
        req.opponent_style,
        req.details.map(|d| format!("- Дополнительно: {}", d)).unwrap_or_default()
    );

    // Вызов Groq API
    let api_key = std::env::var("GROQ_API_KEY")
        .expect("GROQ_API_KEY must be set in .env");
    let client = reqwest::Client::new();

    let body = serde_json::json!({
        "model": "qwen/qwen3.8-27b",
        "messages": [
            {"role": "system", "content": "Ты - профессиональный создатель сценариев для тренировки переговоров. Создавай реалистичные, детализированные сценарии. Отвечай строго в формате JSON, без markdown-обертки."},
            {"role": "user", "content": prompt}
        ],
        "temperature": 0.7,
        "max_tokens": 1000
    });

    let resp = client.post("https://api.groq.com/openai/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&body)
        .send()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let groq_resp: serde_json::Value = resp.json()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let content = groq_resp["choices"][0]["message"]["content"]
        .as_str()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    // Парсинг JSON (убираем markdown-обертку если есть)
    let json_str = content
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    let scenario: GeneratedScenario = serde_json::from_str(json_str)
        .map_err(|e| {
            eprintln!("Failed to parse AI response: {}\nContent: {}", e, content);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(GenerateScenarioResponse {
        success: true,
        scenario: Some(scenario),
        message: None,
    }))
}

#[derive(Deserialize)]
struct StartGeneratedRequest {
    scenario: GeneratedScenario,
    difficulty: String,
    mode: String,
}

async fn start_generated_scenario(
    State(state): State<AppState>,
    Json(req): Json<StartGeneratedRequest>,
) -> Result<Json<StartResponse>, StatusCode> {
    // Конвертируем сгенерированный сценарий в формат движка
    let mut dialogue_tree = HashMap::new();

    // Создаем начальный узел
    let start_node = DialogueNode {
        id: "start".to_string(),
        speaker: req.scenario.partner_name.clone(),
        text: req.scenario.initial_context.clone(),
        responses: vec![],
        is_ending: false,
        score: 0,
    };
    dialogue_tree.insert("start".to_string(), start_node);

    let scenario = Scenario {
        id: format!("gen_{}", uuid::Uuid::new_v4()),
        title: req.scenario.title,
        description: req.scenario.description,
        difficulty: req.difficulty.clone(),
        sphere: req.scenario.sphere.unwrap_or_else(|| "Сгенерированный".to_string()),
        partner_name: req.scenario.partner_name.clone(),
        partner_role: req.scenario.partner_role.clone(),
        partner_goals: vec![req.scenario.partner_goal],
        initial_context: req.scenario.initial_context,
        dialogue_tree,
        endings: vec![],
        partner_batna: req.scenario.partner_batna,
        player_batna: req.scenario.player_batna,
    };

    let session_id = uuid::Uuid::new_v4().to_string();
    let config = NegotiationConfig {
        scenario_id: "generated".to_string(),
        difficulty: req.difficulty,
        mode: req.mode.clone(),
        partner_tone: "нейтральный".to_string(),
        sphere: scenario.sphere.clone(),
    };

    let session = NegotiationSession::new(session_id.clone(), scenario, config, req.mode);

    let partner_name = session.scenario.partner_name.clone();
    let partner_role = session.scenario.partner_role.clone();

    let initial_node = DialogueNode {
        id: "start".to_string(),
        speaker: partner_name.clone(),
        text: format!("{}: {}", partner_name, session.scenario.initial_context),
        responses: vec![],
        is_ending: false,
        score: 0,
    };

    let mut sessions = state.sessions.write().await;
    sessions.insert(session_id.clone(), session);

    Ok(Json(StartResponse {
        session_id,
        partner_name,
        partner_role,
        initial_node,
    }))
}
