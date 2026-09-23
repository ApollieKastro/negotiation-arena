//! Версионные миграции SQLite.
//!
//! Миграции — упорядоченный список `(version, name, sql)`. Применяются
//! атомарно (BEGIN/COMMIT), версии записываются в `schema_migrations`.
//! Обратного пути нет: миграции только добавляются в конец списка.

use rusqlite::{params, Connection};

use crate::error::AppResult;

struct Migration {
    version: i64,
    name: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "bootstrap",
        sql: include_str!("migrations/0001_bootstrap.sql"),
    },
    Migration {
        version: 2,
        name: "providers_and_models",
        sql: include_str!("migrations/0002_providers_and_models.sql"),
    },
    Migration {
        version: 3,
        name: "users_and_auth",
        sql: include_str!("migrations/0003_users_and_auth.sql"),
    },
    Migration {
        version: 4,
        name: "scenarios",
        sql: include_str!("migrations/0004_scenarios.sql"),
    },
    Migration {
        version: 5,
        name: "sessions",
        sql: include_str!("migrations/0005_sessions.sql"),
    },
    Migration {
        version: 6,
        name: "scenario_and_session_details",
        sql: include_str!("migrations/0006_scenario_and_session_details.sql"),
    },
    Migration {
        version: 7,
        name: "unify_max_turns",
        sql: include_str!("migrations/0007_unify_max_turns.sql"),
    },
    Migration {
        version: 8,
        name: "auth_lockout_and_refresh",
        sql: include_str!("migrations/0008_auth_lockout_and_refresh.sql"),
    },
];

/// Общее число известных миграций (для тестов).
pub const MIGRATION_COUNT: usize = MIGRATIONS.len();

/// Применяет недостающие миграции. Возвращает число применённых.
pub fn run(conn: &Connection) -> AppResult<usize> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version    INTEGER PRIMARY KEY,
            name       TEXT NOT NULL,
            applied_at TEXT NOT NULL
        );",
    )?;

    let applied = applied_versions(conn)?;
    let mut count = 0;

    for migration in MIGRATIONS {
        if applied.contains(&migration.version) {
            continue;
        }
        apply_one(conn, migration)?;
        count += 1;
        tracing::info!(
            version = migration.version,
            name = migration.name,
            "миграция применена"
        );
    }

    Ok(count)
}

fn applied_versions(conn: &Connection) -> AppResult<Vec<i64>> {
    let mut stmt = conn.prepare("SELECT version FROM schema_migrations ORDER BY version")?;
    let versions = stmt
        .query_map([], |row| row.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(versions)
}

fn apply_one(conn: &Connection, migration: &Migration) -> AppResult<()> {
    conn.execute_batch("BEGIN")?;

    let result = (|| -> AppResult<()> {
        conn.execute_batch(migration.sql)?;
        conn.execute(
            "INSERT INTO schema_migrations (version, name, applied_at)
             VALUES (?1, ?2, ?3)",
            params![
                migration.version,
                migration.name,
                chrono::Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    })();

    match result {
        Ok(()) => {
            conn.execute_batch("COMMIT")?;
            Ok(())
        }
        Err(err) => {
            let _ = conn.execute_batch("ROLLBACK");
            tracing::error!(
                version = migration.version,
                name = migration.name,
                error = %err,
                "миграция откатена"
            );
            Err(err)
        }
    }
}
