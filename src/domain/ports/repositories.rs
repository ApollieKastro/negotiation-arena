//! Репозитории: контракты хранения данных.
//!
//! Реализации — в `infrastructure::db::repos`. Здесь только сигнатуры:
//! домен не знает о SQLite.

use crate::domain::entities::audit::AuditEntry;
use crate::domain::entities::provider::{
    ModelRecord, Provider, RoleAssignment, UserModelPreference,
};
use crate::domain::entities::scenario::Scenario;
use crate::domain::entities::session::{Session, SessionMessage};
use crate::domain::entities::user::{User, UserRole, UserWithSecret};
use crate::error::AppResult;

// ─────────────────────────────────────────────────────────────
// Сценарии
// ─────────────────────────────────────────────────────────────

pub trait ScenarioRepository: Send + Sync {
    /// Список сценариев; `active_only` — только опубликованные.
    fn list(&self, active_only: bool) -> AppResult<Vec<Scenario>>;
    fn get(&self, id: &str) -> AppResult<Option<Scenario>>;
    /// Создаёт или обновляет сценарий по `id`.
    fn upsert(&self, scenario: &Scenario) -> AppResult<()>;
    fn delete(&self, id: &str) -> AppResult<()>;
    fn count(&self) -> AppResult<u64>;
}

// ─────────────────────────────────────────────────────────────
// Пользователи
// ─────────────────────────────────────────────────────────────

pub trait UserRepository: Send + Sync {
    fn create(
        &self,
        login: &str,
        password_hash: &str,
        role: UserRole,
        display_name: Option<&str>,
    ) -> AppResult<User>;

    fn by_id(&self, id: &str) -> AppResult<Option<User>>;
    fn by_login(&self, login: &str) -> AppResult<Option<UserWithSecret>>;
    fn list(&self) -> AppResult<Vec<User>>;
    fn update_role(&self, id: &str, role: UserRole) -> AppResult<()>;
    fn set_active(&self, id: &str, is_active: bool) -> AppResult<()>;
    fn delete(&self, id: &str) -> AppResult<()>;
    fn count(&self) -> AppResult<u64>;
}

// ─────────────────────────────────────────────────────────────
// Сессии
// ─────────────────────────────────────────────────────────────

/// Агрегаты сессий одного пользователя (считаются в SQL, без выборки строк).
#[derive(Debug, Clone, Default)]
pub struct UserSessionsAggregate {
    pub total: u32,
    pub finished: u32,
    pub active: u32,
    pub abandoned: u32,
    pub best_score: i32,
    pub avg_score: i32,
    pub last_session_at: Option<String>,
}

/// Строка лидерборда, построенная одним SQL-запросом (JOIN + GROUP BY).
#[derive(Debug, Clone)]
pub struct LeaderboardRow {
    pub user_id: String,
    pub login: String,
    pub display_name: Option<String>,
    pub is_active: bool,
    pub finished: u32,
    pub best_score: i32,
    pub avg_score: i32,
}

/// Сводка по всем сессиям платформы (один запрос).
#[derive(Debug, Clone, Default)]
pub struct PlatformSessionsAggregate {
    pub total: u64,
    pub finished: u64,
    pub active: u64,
    pub avg_finished_score: i32,
    pub best_score: i32,
}

pub trait SessionRepository: Send + Sync {
    fn create(&self, session: &Session) -> AppResult<()>;
    fn get(&self, id: &str) -> AppResult<Option<Session>>;
    fn list_by_user(&self, user_id: &str, limit: u32) -> AppResult<Vec<Session>>;
    fn update(&self, session: &Session) -> AppResult<()>;
    fn append_message(&self, message: &SessionMessage) -> AppResult<()>;
    fn messages(&self, session_id: &str) -> AppResult<Vec<SessionMessage>>;

    /// Агрегаты сессий пользователя одной строкой SQL.
    fn user_stats_aggregate(&self, user_id: &str) -> AppResult<UserSessionsAggregate>;
    /// Лидерборд: JOIN users + GROUP BY user_id (только `finished`).
    fn leaderboard_rows(&self) -> AppResult<Vec<LeaderboardRow>>;
    /// Общая сводка по таблице сессий.
    fn platform_sessions_aggregate(&self) -> AppResult<PlatformSessionsAggregate>;
    /// Счётчик сессий по дням (`YYYY-MM-DD`) начиная с `since` (RFC3339/дата).
    fn activity_by_day(&self, since: &str) -> AppResult<Vec<(String, u32)>>;
    /// Сколько сессий привязано к сценарию (для отказа в удалении).
    fn count_by_scenario(&self, scenario_id: &str) -> AppResult<u64>;
}

// ─────────────────────────────────────────────────────────────
// Провайдеры и модели
// ─────────────────────────────────────────────────────────────

pub trait ProviderRepository: Send + Sync {
    fn list(&self) -> AppResult<Vec<Provider>>;
    fn get(&self, id: &str) -> AppResult<Option<Provider>>;
    fn upsert(&self, provider: &Provider) -> AppResult<()>;
    fn delete(&self, id: &str) -> AppResult<()>;

    fn list_models(&self, provider_id: Option<&str>) -> AppResult<Vec<ModelRecord>>;
    fn get_model(&self, id: &str) -> AppResult<Option<ModelRecord>>;
    fn upsert_model(&self, model: &ModelRecord) -> AppResult<()>;
    fn delete_model(&self, id: &str) -> AppResult<()>;

    fn role_assignments(&self) -> AppResult<Vec<RoleAssignment>>;
    fn set_role_assignment(&self, role: &str, model_id: &str) -> AppResult<()>;
    /// Снимает назначение роли (удаляет строку; нет строки — no-op).
    fn clear_role_assignment(&self, role: &str) -> AppResult<()>;

    /// Все предпочтения пользователя (с JOIN имён моделей/провайдеров).
    fn user_preferences(&self, user_id: &str) -> AppResult<Vec<UserModelPreference>>;
    /// Предпочтение одной роли; `None` — используется глобальное назначение.
    fn user_preference(&self, user_id: &str, role: &str) -> AppResult<Option<UserModelPreference>>;
    /// Upsert предпочтения по `(user_id, role)`.
    fn set_user_preference(&self, user_id: &str, role: &str, model_id: &str) -> AppResult<()>;
    /// Удаляет предпочтение (возврат к глобальному назначению).
    fn delete_user_preference(&self, user_id: &str, role: &str) -> AppResult<()>;
}

// ─────────────────────────────────────────────────────────────
// Настройки и аудит
// ─────────────────────────────────────────────────────────────

pub trait SettingsRepository: Send + Sync {
    fn get(&self, key: &str) -> AppResult<Option<String>>;
    fn set(&self, key: &str, value: &str) -> AppResult<()>;
    fn all(&self) -> AppResult<Vec<(String, String)>>;
}

pub trait AuditRepository: Send + Sync {
    fn append(
        &self,
        user_id: Option<&str>,
        action: &str,
        entity: Option<&str>,
        entity_id: Option<&str>,
        details: Option<&str>,
    ) -> AppResult<()>;

    /// Последние записи: `limit` (сверху вниз по `created_at`),
    /// опциональный фильтр по исполнителю и коду действия.
    fn list(
        &self,
        limit: u32,
        user_id: Option<&str>,
        action: Option<&str>,
    ) -> AppResult<Vec<AuditEntry>>;
}
