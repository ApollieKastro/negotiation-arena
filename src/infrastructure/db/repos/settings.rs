//! Репозитории настроек и аудита (SQLite).

use std::sync::Arc;

use rusqlite::{params, OptionalExtension};

use crate::domain::ports::{AuditRepository, SettingsRepository};
use crate::error::AppResult;
use crate::infrastructure::db::Database;

pub struct SqliteSettingsRepo {
    db: Arc<Database>,
}

impl SqliteSettingsRepo {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
}

impl SettingsRepository for SqliteSettingsRepo {
    fn get(&self, key: &str) -> AppResult<Option<String>> {
        let conn = self.db.conn();
        let value = conn
            .query_row(
                "SELECT value FROM app_settings WHERE key = ?1",
                params![key],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        Ok(value)
    }

    fn set(&self, key: &str, value: &str) -> AppResult<()> {
        let conn = self.db.conn();
        conn.execute(
            "INSERT INTO app_settings (key, value, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            params![key, value, chrono::Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    fn all(&self) -> AppResult<Vec<(String, String)>> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare("SELECT key, value FROM app_settings ORDER BY key")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

pub struct SqliteAuditRepo {
    db: Arc<Database>,
}

impl SqliteAuditRepo {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
}

impl AuditRepository for SqliteAuditRepo {
    fn append(
        &self,
        user_id: Option<&str>,
        action: &str,
        entity: Option<&str>,
        entity_id: Option<&str>,
        details: Option<&str>,
    ) -> AppResult<()> {
        let conn = self.db.conn();
        conn.execute(
            "INSERT INTO audit_log (id, user_id, action, entity, entity_id, details, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                uuid::Uuid::new_v4().to_string(),
                user_id,
                action,
                entity,
                entity_id,
                details,
                chrono::Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }
}
