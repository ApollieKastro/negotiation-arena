//! SQLite-реализации репозиториев домена.

pub mod provider;
pub mod scenario;
pub mod session;
pub mod settings;
pub mod user;

use std::sync::Arc;

use crate::infrastructure::db::Database;

/// Набор репозиториев над одним подключением к БД.
///
/// Хранит `Arc<Database>`, чтобы каждый репозиторий мог работать
/// независимо, но пользоваться общим соединением.
pub struct SqliteRepos {
    pub scenarios: scenario::SqliteScenarioRepo,
    pub users: user::SqliteUserRepo,
    pub sessions: session::SqliteSessionRepo,
    pub providers: provider::SqliteProviderRepo,
    pub settings: settings::SqliteSettingsRepo,
    pub audit: settings::SqliteAuditRepo,
    pub llm_usage: settings::SqliteLlmUsageRepo,
}

impl SqliteRepos {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            scenarios: scenario::SqliteScenarioRepo::new(db.clone()),
            users: user::SqliteUserRepo::new(db.clone()),
            sessions: session::SqliteSessionRepo::new(db.clone()),
            providers: provider::SqliteProviderRepo::new(db.clone()),
            settings: settings::SqliteSettingsRepo::new(db.clone()),
            audit: settings::SqliteAuditRepo::new(db.clone()),
            llm_usage: settings::SqliteLlmUsageRepo::new(db),
        }
    }
}
