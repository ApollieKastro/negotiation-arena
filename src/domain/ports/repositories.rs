//! Репозитории: контракты хранения данных.
//!
//! Реализации — в `infrastructure::db::repos`. Здесь только сигнатуры:
//! домен не знает о SQLite.

use crate::domain::entities::provider::{ModelRecord, Provider, RoleAssignment};
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

pub trait SessionRepository: Send + Sync {
    fn create(&self, session: &Session) -> AppResult<()>;
    fn get(&self, id: &str) -> AppResult<Option<Session>>;
    fn list_by_user(&self, user_id: &str, limit: u32) -> AppResult<Vec<Session>>;
    fn update(&self, session: &Session) -> AppResult<()>;
    fn append_message(&self, message: &SessionMessage) -> AppResult<()>;
    fn messages(&self, session_id: &str) -> AppResult<Vec<SessionMessage>>;
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
}
