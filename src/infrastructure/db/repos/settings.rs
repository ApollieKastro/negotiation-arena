//! Репозитории настроек, аудита и LLM-квоты (SQLite).

use std::sync::Arc;

use rusqlite::{params, OptionalExtension};

use crate::domain::entities::audit::AuditEntry;
use crate::domain::ports::{AuditRepository, LlmUsageRepository, SettingsRepository};
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

    fn list(
        &self,
        limit: u32,
        user_id: Option<&str>,
        action: Option<&str>,
    ) -> AppResult<Vec<AuditEntry>> {
        let conn = self.db.conn();

        let mut sql = String::from(
            "SELECT id, user_id, action, entity, entity_id, details, created_at FROM audit_log",
        );
        let mut conditions: Vec<String> = Vec::new();
        let mut bound: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(uid) = user_id {
            conditions.push(format!("user_id = ?{}", bound.len() + 1));
            bound.push(Box::new(uid.to_string()));
        }
        if let Some(act) = action {
            conditions.push(format!("action = ?{}", bound.len() + 1));
            bound.push(Box::new(act.to_string()));
        }
        if !conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }
        sql.push_str(" ORDER BY created_at DESC, id DESC LIMIT ?");
        bound.push(Box::new(limit));

        let mut stmt = conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::ToSql> = bound.iter().map(|b| b.as_ref()).collect();
        let rows = stmt.query_map(params.as_slice(), |row| {
            Ok(AuditEntry {
                id: row.get(0)?,
                user_id: row.get(1)?,
                action: row.get(2)?,
                entity: row.get(3)?,
                entity_id: row.get(4)?,
                details: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

pub struct SqliteLlmUsageRepo {
    db: Arc<Database>,
}

impl SqliteLlmUsageRepo {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
}

impl LlmUsageRepository for SqliteLlmUsageRepo {
    fn tokens_on(&self, user_id: &str, day: &str) -> AppResult<u64> {
        let conn = self.db.conn();
        let tokens: i64 = conn
            .query_row(
                "SELECT COALESCE(tokens, 0) FROM llm_daily_usage
                 WHERE user_id = ?1 AND day = ?2",
                params![user_id, day],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0);
        Ok(tokens.max(0) as u64)
    }

    fn add_usage(&self, user_id: &str, day: &str, tokens: u64) -> AppResult<()> {
        let conn = self.db.conn();
        conn.execute(
            "INSERT INTO llm_daily_usage (user_id, day, tokens, calls, updated_at)
             VALUES (?1, ?2, ?3, 1, ?4)
             ON CONFLICT(user_id, day) DO UPDATE SET
                tokens = tokens + excluded.tokens,
                calls = calls + 1,
                updated_at = excluded.updated_at",
            params![user_id, day, tokens as i64, chrono::Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    fn purge_before(&self, before_day: &str) -> AppResult<()> {
        let conn = self.db.conn();
        conn.execute(
            "DELETE FROM llm_daily_usage WHERE day < ?1",
            params![before_day],
        )?;
        Ok(())
    }
}
