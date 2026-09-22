//! Репозиторий пользователей (SQLite).

use std::sync::Arc;

use rusqlite::{params, OptionalExtension, Row};

use crate::domain::entities::user::{User, UserRole, UserWithSecret};
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
        Ok(User {
            id: row.get(0)?,
            login: row.get(1)?,
            display_name: row.get(2)?,
            role: UserRole::from_slug(&role_slug).unwrap_or(UserRole::User),
            is_active: is_active != 0,
            created_at: row.get(5)?,
        })
    }
}

const SELECT: &str = "SELECT id, login, display_name, role, is_active, created_at FROM users";

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
        })
    }

    fn by_id(&self, id: &str) -> AppResult<Option<User>> {
        let conn = self.db.conn();
        let sql = format!("{SELECT} WHERE id = ?1");
        conn.query_row(&sql, params![id], Self::map_row)
            .optional()
            .map_err(Into::into)
    }

    fn by_login(&self, login: &str) -> AppResult<Option<UserWithSecret>> {
        let conn = self.db.conn();
        let sql = "SELECT id, login, display_name, role, is_active, created_at, password_hash
             FROM users WHERE login = ?1";
        let row = conn
            .query_row(&sql, params![login], |row| {
                Ok((Self::map_row(row)?, row.get::<_, String>(6)?))
            })
            .optional()?;
        Ok(row.map(|(user, password_hash)| UserWithSecret {
            user,
            password_hash,
        }))
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
}
