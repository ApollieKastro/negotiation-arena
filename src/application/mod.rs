//! Прикладной слой: сервисы, оркеструющие домен и инфраструктуру.
//!
//! * [`auth`] — аутентификация (JWT) и RBAC-авторизация;
//! * [`scenario`] — CRUD, импорт/экспорт, ИИ-генератор сценариев;
//! * [`session`] — ход диалога, скоринг, анализ, финал;
//! * [`stats`] — статистика, активность, лидерборд;
//! * [`settings`] — глобальные и пользовательские настройки;
//! * [`provider`] — провайдеры, ключи, назначение моделей по ролям.

pub mod auth;
pub mod provider;
pub mod scenario;
pub mod session;
pub mod settings;
pub mod stats;

use std::sync::Arc;

use crate::config::AppConfig;
use crate::error::AppResult;
use crate::infrastructure::crypto::SecretCipher;
use crate::infrastructure::db::repos::SqliteRepos;
use crate::infrastructure::providers::ProviderFactory;
use auth::AuthService;
use provider::ProviderService;
use scenario::ScenarioService;
use session::SessionService;
use settings::SettingsService;
use stats::StatsService;

// Реэкспорт прав — подмодули используют через `super::Permission`;
// `AuthContext`/`authorize` нужны только testsupport (cfg(test)).
pub use auth::Permission;

/// Пакет прикладных сервисов — собирается один раз в composition root.
pub struct Services {
    pub repos: Arc<SqliteRepos>,
    pub factory: Arc<ProviderFactory>,
    pub auth: Arc<AuthService>,
    pub scenarios: Arc<ScenarioService>,
    pub sessions: Arc<SessionService>,
    pub stats: Arc<StatsService>,
    pub settings: Arc<SettingsService>,
    pub providers: Arc<ProviderService>,
}

impl Services {
    pub fn new(
        config: &AppConfig,
        repos: Arc<SqliteRepos>,
        cipher: Arc<SecretCipher>,
    ) -> AppResult<Self> {
        let factory = Arc::new(ProviderFactory::new(config.storage.models_dir.clone())?);

        let providers = Arc::new(ProviderService::new(
            repos.clone(),
            factory.clone(),
            cipher.clone(),
        ));
        let auth = Arc::new(AuthService::new(
            repos.clone(),
            config.security.jwt_secret.clone(),
            config.security.jwt_ttl_seconds,
        ));
        let scenarios = Arc::new(ScenarioService::new(repos.clone(), providers.clone()));
        let settings = Arc::new(SettingsService::new(repos.clone()));
        let sessions = Arc::new(SessionService::new(
            repos.clone(),
            providers.clone(),
            settings.clone(),
        ));
        let stats = Arc::new(StatsService::new(repos.clone()));

        Ok(Self {
            repos,
            factory,
            auth,
            scenarios,
            sessions,
            stats,
            settings,
            providers,
        })
    }
}

/// Общие фикстуры для unit-тестов прикладного слоя (in-memory SQLite).
#[cfg(test)]
pub(crate) mod testsupport {
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::config::{AppConfig, SecurityConfig, ServerConfig, StorageConfig};
    use crate::infrastructure::crypto::SecretCipher;
    use crate::infrastructure::db::repos::SqliteRepos;
    use crate::infrastructure::db::Database;

    use super::{auth, Permission, Services};
    use crate::domain::entities::user::UserRole;
    use crate::error::AppResult;

    pub fn test_config() -> AppConfig {
        AppConfig {
            server: ServerConfig {
                host: "127.0.0.1".into(),
                port: 0,
            },
            security: SecurityConfig {
                jwt_secret: "test-jwt-secret-16ch".into(),
                jwt_ttl_seconds: 3600,
                admin_password: "admin123".into(),
                encryption_secret: "test-encryption-secret".into(),
            },
            storage: StorageConfig {
                db_path: PathBuf::from(":memory:"),
                models_dir: PathBuf::from("models"),
            },
        }
    }

    /// Поднимает сервисы поверх чистой in-memory БД с применёнными миграциями.
    pub fn setup() -> AppResult<(Arc<Database>, Services)> {
        let db = Arc::new(Database::open_in_memory()?);
        db.run_migrations()?;
        let repos = Arc::new(SqliteRepos::new(db.clone()));
        let cipher = Arc::new(SecretCipher::from_secret("test-encryption-secret")?);
        let services = Services::new(&test_config(), repos, cipher)?;
        Ok((db, services))
    }

    /// Аутентифицированный контекст без обращения к БД (для authorize-тестов).
    pub fn ctx(user_id: &str, role: UserRole) -> auth::AuthContext {
        auth::AuthContext {
            user_id: user_id.to_string(),
            login: "tester".into(),
            role,
        }
    }

    /// Разрешены ли права роли (для таблиц истинности).
    pub fn can(role: UserRole, permission: Permission) -> bool {
        auth::authorize(role, permission).is_ok()
    }
}
