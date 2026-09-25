//! Репозитории: контракты хранения данных.
//!
//! Реализации — в `infrastructure::db::repos`. Здесь только сигнатуры:
//! домен не знает о SQLite.

use crate::domain::entities::audit::AuditEntry;
use crate::domain::entities::provider::{
    ModelRecord, Provider, RoleAssignment, UserModelPreference,
};
use crate::domain::entities::scenario::Scenario;
use crate::domain::entities::session::{Session, SessionBranch, SessionMessage};
use crate::domain::entities::user::{LoginAttemptUpdate, User, UserRole, UserWithSecret};
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
    /// Как [`Self::by_id`], но с хешем пароля (смена пароля, аутентификация).
    fn by_id_with_secret(&self, id: &str) -> AppResult<Option<UserWithSecret>>;
    fn by_login(&self, login: &str) -> AppResult<Option<UserWithSecret>>;
    fn list(&self) -> AppResult<Vec<User>>;
    fn update_role(&self, id: &str, role: UserRole) -> AppResult<()>;
    fn set_active(&self, id: &str, is_active: bool) -> AppResult<()>;
    /// Смена логина и отображаемого имени (сам профиль, `display_name` — `None` не трогает? см. impl).
    fn update_profile(&self, id: &str, login: &str, display_name: Option<&str>) -> AppResult<()>;
    /// Замена хеша пароля.
    fn update_password(&self, id: &str, password_hash: &str) -> AppResult<()>;
    /// Загрузка/замена аватара.
    fn set_avatar(&self, id: &str, mime: &str, data: &[u8]) -> AppResult<()>;
    /// Удаление аватара (нет строки — no-op).
    fn clear_avatar(&self, id: &str) -> AppResult<()>;
    /// Байты аватара: `(mime, data)`; `None` — аватара нет.
    fn avatar(&self, id: &str) -> AppResult<Option<(String, Vec<u8>)>>;
    fn delete(&self, id: &str) -> AppResult<()>;
    fn count(&self) -> AppResult<u64>;

    // ── Lockout неудачных входов ──
    /// Фиксирует неудачную попытку: инкремент/сброс окна, опциональная блокировка.
    fn record_login_failure(
        &self,
        id: &str,
        window_secs: u64,
        max_failures: u32,
        lockout_secs: u64,
    ) -> AppResult<LoginAttemptUpdate>;
    /// Сбрасывает счётчик и блокировку (успешный вход).
    fn clear_login_failures(&self, id: &str) -> AppResult<()>;

    // ── Ротация refresh (single-use jti) ──
    /// Помечает jti использованным. `Ok(false)` — jti уже был использован.
    fn mark_refresh_jti_used(&self, jti: &str, user_id: &str, purge_after: &str)
        -> AppResult<bool>;
    /// Удаляет протухшие записи used_refresh_jtis (opportunist cleanup).
    fn purge_expired_refresh_jtis(&self, now_rfc3339: &str) -> AppResult<u64>;
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
    /// Суммарный XP: положительные total_score завершённых сессий.
    pub xp: i64,
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
    /// Суммарный XP (для сортировки/отображения в лидерборде).
    pub xp: i64,
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
    /// Создаёт сессию и main-ветку (`id` ветки = `id` сессии).
    fn create(&self, session: &Session) -> AppResult<()>;
    /// Создаёт сессию, main-ветку и opening-сообщение **в одной транзакции**.
    fn create_with_opening(&self, session: &Session, opening: &SessionMessage) -> AppResult<()>;
    fn get(&self, id: &str) -> AppResult<Option<Session>>;
    /// История сессий пользователя: `LIMIT/OFFSET` + общий счётчик.
    fn list_by_user(
        &self,
        user_id: &str,
        limit: u32,
        offset: u32,
    ) -> AppResult<(Vec<Session>, u64)>;
    fn update(&self, session: &Session) -> AppResult<()>;
    /// Фиксирует ход: UPDATE сессии и текущей ветки + 1–2 сообщения
    /// **в одной транзакции**.
    fn commit_turn(
        &self,
        session: &Session,
        branch: &SessionBranch,
        player: &SessionMessage,
        partner: Option<&SessionMessage>,
    ) -> AppResult<()>;
    fn append_message(&self, message: &SessionMessage) -> AppResult<()>;
    /// Сообщения **текущей** (`is_current`) ветки сессии, по порядку ходов.
    fn messages(&self, session_id: &str) -> AppResult<Vec<SessionMessage>>;
    /// Сообщения конкретной ветки (id ветки должен принадлежать сессии).
    fn messages_in_branch(
        &self,
        session_id: &str,
        branch_id: &str,
    ) -> AppResult<Vec<SessionMessage>>;
    /// Реплика по id (`None` — не найдена).
    fn message(&self, id: &str) -> AppResult<Option<SessionMessage>>;

    // ── Ветвление диалога ──
    /// Все ветки сессии в порядке создания (main — первая).
    fn branches(&self, session_id: &str) -> AppResult<Vec<SessionBranch>>;
    /// Ветка по id (`None` — не найдена).
    fn branch(&self, id: &str) -> AppResult<Option<SessionBranch>>;
    /// Текущая (`is_current`) ветка сессии.
    fn current_branch(&self, session_id: &str) -> AppResult<Option<SessionBranch>>;
    /// Число веток сессии (включая main).
    fn count_branches(&self, session_id: &str) -> AppResult<u64>;
    /// Атомарно создаёт форк: сбрасывает `is_current` у остальных веток,
    /// вставляет новую ветку и копии префикса реплик, обновляет сессию.
    fn fork_branch(
        &self,
        branch: &SessionBranch,
        copies: &[SessionMessage],
        session: &Session,
    ) -> AppResult<()>;
    /// Атомарно делает ветку текущей и подставляет её состояние в сессию.
    fn switch_branch(&self, branch_id: &str, session: &Session) -> AppResult<()>;

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

/// Дневной учёт LLM-токенов на пользователя (квота `platform.llm_daily_token_limit`).
pub trait LlmUsageRepository: Send + Sync {
    /// Сколько токенов израсходовано пользователем за `day` (`YYYY-MM-DD`).
    fn tokens_on(&self, user_id: &str, day: &str) -> AppResult<u64>;
    /// Начисляет `tokens` (и +1 call) за `day` (upsert).
    fn add_usage(&self, user_id: &str, day: &str, tokens: u64) -> AppResult<()>;
    /// Удаляет записи старше `before_day` (`YYYY-MM-DD`, включая границу).
    fn purge_before(&self, before_day: &str) -> AppResult<()>;
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
