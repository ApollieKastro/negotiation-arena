//! Репозиторий пользователей (SQLite).

use std::sync::Arc;

use rusqlite::{params, OptionalExtension, Row};

use crate::domain::entities::user::{LoginAttemptUpdate, User, UserRole, UserWithSecret};
use crate::domain::ports::UserRepository;
use crate::error::AppResult;
use crate::infrastructure::db::Database;

pub struct SqliteUserRepo {
    db: Arc<Database>,
}

impl SqliteUserRepo {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    fn map_row(row: &Row<'_>) -> rusqlite::Result<User> {
        let role_slug: String = row.get(3)?;
        let is_active: i32 = row.get(4)?;
        let has_avatar: i32 = row.get(6)?;
        Ok(User {
            id: row.get(0)?,
            login: row.get(1)?,
            display_name: row.get(2)?,
            role: UserRole::from_slug(&role_slug).unwrap_or(UserRole::User),
            is_active: is_active != 0,
            created_at: row.get(5)?,
            has_avatar: has_avatar != 0,
        })
    }

    /// [`Self::map_row`] + хеш и lockout-поля (индексы 7..=10).
    fn map_secret_row(row: &Row<'_>) -> rusqlite::Result<UserWithSecret> {
        Ok(UserWithSecret {
            user: Self::map_row(row)?,
            password_hash: row.get(7)?,
            failed_login_count: row.get(8)?,
            last_failed_login_at: row.get(9)?,
            locked_until: row.get(10)?,
        })
    }
}

/// Колонки без `avatar_data`: байты читаются только точечным запросом [`Self::avatar`].
const SELECT: &str =
    "SELECT id, login, display_name, role, is_active, created_at, (avatar_mime IS NOT NULL)
     FROM users";

impl UserRepository for SqliteUserRepo {
    fn create(
        &self,
        login: &str,
        password_hash: &str,
        role: UserRole,
        display_name: Option<&str>,
    ) -> AppResult<User> {
        let conn = self.db.conn();
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO users (id, login, password_hash, display_name, role, is_active, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6)",
            params![id, login, password_hash, display_name, role.slug(), now],
        )?;

        Ok(User {
            id,
            login: login.to_string(),
            display_name: display_name.map(str::to_string),
            role,
            is_active: true,
            created_at: now,
            has_avatar: false,
        })
    }

    fn by_id(&self, id: &str) -> AppResult<Option<User>> {
        let conn = self.db.conn();
        let sql = format!("{SELECT} WHERE id = ?1");
        conn.query_row(&sql, params![id], Self::map_row)
            .optional()
            .map_err(Into::into)
    }

    fn by_id_with_secret(&self, id: &str) -> AppResult<Option<UserWithSecret>> {
        let conn = self.db.conn();
        let sql = "SELECT id, login, display_name, role, is_active, created_at,
             (avatar_mime IS NOT NULL), password_hash,
             failed_login_count, last_failed_login_at, locked_until
             FROM users WHERE id = ?1";
        let row = conn
            .query_row(sql, params![id], Self::map_secret_row)
            .optional()?;
        Ok(row)
    }

    fn by_login(&self, login: &str) -> AppResult<Option<UserWithSecret>> {
        let conn = self.db.conn();
        let sql = "SELECT id, login, display_name, role, is_active, created_at,
             (avatar_mime IS NOT NULL), password_hash,
             failed_login_count, last_failed_login_at, locked_until
             FROM users WHERE login = ?1";
        let row = conn
            .query_row(sql, params![login], Self::map_secret_row)
            .optional()?;
        Ok(row)
    }

    fn list(&self) -> AppResult<Vec<User>> {
        let conn = self.db.conn();
        let sql = format!("{SELECT} ORDER BY created_at DESC");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], Self::map_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn update_role(&self, id: &str, role: UserRole) -> AppResult<()> {
        let conn = self.db.conn();
        conn.execute(
            "UPDATE users SET role = ?1, updated_at = ?2 WHERE id = ?3",
            params![role.slug(), chrono::Utc::now().to_rfc3339(), id],
        )?;
        Ok(())
    }

    fn set_active(&self, id: &str, is_active: bool) -> AppResult<()> {
        let conn = self.db.conn();
        conn.execute(
            "UPDATE users SET is_active = ?1, updated_at = ?2 WHERE id = ?3",
            params![is_active as i32, chrono::Utc::now().to_rfc3339(), id],
        )?;
        Ok(())
    }

    fn update_profile(&self, id: &str, login: &str, display_name: Option<&str>) -> AppResult<()> {
        let conn = self.db.conn();
        conn.execute(
            "UPDATE users SET login = ?1, display_name = ?2, updated_at = ?3 WHERE id = ?4",
            params![login, display_name, chrono::Utc::now().to_rfc3339(), id],
        )?;
        Ok(())
    }

    fn update_password(&self, id: &str, password_hash: &str) -> AppResult<()> {
        let conn = self.db.conn();
        conn.execute(
            "UPDATE users SET password_hash = ?1, updated_at = ?2 WHERE id = ?3",
            params![password_hash, chrono::Utc::now().to_rfc3339(), id],
        )?;
        Ok(())
    }

    fn set_avatar(&self, id: &str, mime: &str, data: &[u8]) -> AppResult<()> {
        let conn = self.db.conn();
        conn.execute(
            "UPDATE users SET avatar_mime = ?1, avatar_data = ?2, updated_at = ?3 WHERE id = ?4",
            params![mime, data, chrono::Utc::now().to_rfc3339(), id],
        )?;
        Ok(())
    }

    fn clear_avatar(&self, id: &str) -> AppResult<()> {
        let conn = self.db.conn();
        conn.execute(
            "UPDATE users SET avatar_mime = NULL, avatar_data = NULL, updated_at = ?1
             WHERE id = ?2",
            params![chrono::Utc::now().to_rfc3339(), id],
        )?;
        Ok(())
    }

    fn avatar(&self, id: &str) -> AppResult<Option<(String, Vec<u8>)>> {
        let conn = self.db.conn();
        let row = conn
            .query_row(
                "SELECT avatar_mime, avatar_data FROM users
                 WHERE id = ?1 AND avatar_mime IS NOT NULL",
                params![id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
            )
            .optional()?;
        Ok(row)
    }

    fn delete(&self, id: &str) -> AppResult<()> {
        let conn = self.db.conn();
        conn.execute(
            "DELETE FROM users WHERE id = ?1 AND role != 'admin'",
            params![id],
        )?;
        Ok(())
    }

    fn count(&self) -> AppResult<u64> {
        let conn = self.db.conn();
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))?;
        Ok(count.max(0) as u64)
    }

    fn record_login_failure(
        &self,
        id: &str,
        window_secs: u64,
        max_failures: u32,
        lockout_secs: u64,
    ) -> AppResult<LoginAttemptUpdate> {
        // Текущее состояние читаем в той же транзакции, что и UPDATE,
        // чтобы параллельные попытки не теряли инкремент.
        let conn = self.db.conn();
        let tx = conn.unchecked_transaction()?;

        let current: (i64, Option<String>) = tx
            .query_row(
                "SELECT failed_login_count, last_failed_login_at FROM users WHERE id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or_else(|| crate::error::AppError::NotFound("пользователь не найден".into()))?;

        let now = chrono::Utc::now();
        let now_str = now.to_rfc3339();
        let last = current
            .1
            .as_deref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc));

        // Окно истекло (или первая попытка) — сбрасываем счётчик.
        let in_window = last.is_some_and(|t| (now - t).num_seconds().max(0) as u64 <= window_secs);
        let failed = if in_window {
            current.0.saturating_add(1)
        } else {
            1
        };

        let locked_until = if max_failures > 0 && failed >= i64::from(max_failures) {
            Some((now + chrono::Duration::seconds(lockout_secs as i64)).to_rfc3339())
        } else {
            None
        };

        tx.execute(
            "UPDATE users
             SET failed_login_count = ?1,
                 last_failed_login_at = ?2,
                 locked_until = ?3,
                 updated_at = ?2
             WHERE id = ?4",
            params![failed, now_str, locked_until, id],
        )?;
        tx.commit()?;

        Ok(LoginAttemptUpdate {
            failed_login_count: failed,
            last_failed_login_at: now_str,
            locked_until,
        })
    }

    fn clear_login_failures(&self, id: &str) -> AppResult<()> {
        let conn = self.db.conn();
        conn.execute(
            "UPDATE users
             SET failed_login_count = 0,
                 last_failed_login_at = NULL,
                 locked_until = NULL,
                 updated_at = ?1
             WHERE id = ?2",
            params![chrono::Utc::now().to_rfc3339(), id],
        )?;
        Ok(())
    }

    fn mark_refresh_jti_used(
        &self,
        jti: &str,
        user_id: &str,
        purge_after: &str,
    ) -> AppResult<bool> {
        let conn = self.db.conn();
        let inserted = conn.execute(
            "INSERT INTO used_refresh_jtis (jti, user_id, purge_after)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(jti) DO NOTHING",
            params![jti, user_id, purge_after],
        )?;
        Ok(inserted > 0)
    }

    fn purge_expired_refresh_jtis(&self, now_rfc3339: &str) -> AppResult<u64> {
        let conn = self.db.conn();
        let n = conn.execute(
            "DELETE FROM used_refresh_jtis WHERE purge_after <= ?1",
            params![now_rfc3339],
        )?;
        Ok(n as u64)
    }
}
