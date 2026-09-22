//! Доступ к данным: подключение SQLite и версионные миграции.
//!
//! Репозитории (трейты домена + реализации) появятся здесь же на этапе 1.

pub mod migrations;

use std::path::Path;

use parking_lot::Mutex;
use rusqlite::Connection;

use crate::error::AppResult;

/// Потокобезопасная обёртка над единственным соединением SQLite.
///
/// Соединение одно: `rusqlite::Connection` не `Sync`, а `parking_lot::Mutex`
/// сериализует доступ. Этого достаточно для нагрузки тренажёра.
pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    /// Открывает (создаёт при необходимости) БД с включёнными WAL и FK.
    pub fn open(path: &Path) -> AppResult<Self> {
        let conn = Connection::open(path)?;
        Self::init_pragmas(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Открывает БД в памяти (для тестов).
    pub fn open_in_memory() -> AppResult<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init_pragmas(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn init_pragmas(conn: &Connection) -> AppResult<()> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Ok(())
    }

    /// Блокирует соединение и возвращает guard.
    ///
    /// Не удерживайте guard во время сетевых вызовов — это заморозит
    /// все остальные запросы к БД.
    pub fn conn(&self) -> parking_lot::MutexGuard<'_, Connection> {
        self.conn.lock()
    }

    /// Применяет недостающие миграции. Возвращает число применённых.
    pub fn run_migrations(&self) -> AppResult<usize> {
        migrations::run(&self.conn.lock())
    }

    /// Проверка живости соединения (для `/health`).
    pub fn ping(&self) -> bool {
        self.conn().query_row("SELECT 1", [], |_| Ok(())).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_memory_database_applies_migrations() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.run_migrations().unwrap(), migrations::MIGRATION_COUNT);
        // Повторный запуск — идемпотентен.
        assert_eq!(db.run_migrations().unwrap(), 0);
        assert!(db.ping());
    }
}
