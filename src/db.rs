use rusqlite::{Connection, Result, params};
use parking_lot::Mutex;
use std::sync::Arc;
use chrono::Utc;
use uuid::Uuid;

fn jwt_secret() -> Vec<u8> {
    std::env::var("JWT_SECRET")
        .unwrap_or_else(|_| "change_me_to_a_strong_random_secret".to_string())
        .into_bytes()
}

// ─────────────────────────────────────────────────────────────
// Типы данных
// ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct User {
    pub id: String,
    pub login: String,
    pub role: String,       // "admin" или "user"
    pub created_at: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScenarioRow {
    pub id: String,
    pub title: String,
    pub description: String,
    pub sphere: String,
    pub difficulty: String,
    pub partner_name: String,
    pub partner_role: String,
    pub partner_goals: String,   // JSON array
    pub initial_context: String,
    pub dialogue_tree: String,   // JSON object
    pub endings: String,         // JSON array
    pub partner_batna: String,
    pub player_batna: String,
    pub is_active: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RoleRow {
    pub id: String,
    pub name: String,
    pub description: String,
    pub permissions: String, // JSON: ["manage_scenarios", "manage_users", ...]
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SettingRow {
    pub key: String,
    pub value: String,
    pub description: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OpponentProfile {
    pub id: String,
    pub name: String,
    pub role: String,
    pub personality: String,  // JSON: {"tone": "нейтральный", "style": "формальный", ...}
    pub avatar_url: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionHistory {
    pub id: String,
    pub user_id: String,
    pub scenario_id: String,
    pub score: i32,
    pub ending: String,
    pub feedback: String,
    pub created_at: String,
}

// ─────────────────────────────────────────────────────────────
// Хранилище (потокобезопасная обёртка над SQLite)
// ─────────────────────────────────────────────────────────────

pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    pub fn new(db_path: &str) -> Result<Arc<Self>> {
        let conn = Connection::open(db_path)?;
        let store = Arc::new(Self {
            conn: Mutex::new(conn),
        });
        store.init_schema()?;
        store.seed_defaults()?;
        Ok(store)
    }

    // ──────────── Инициализация схемы ────────────

    fn init_schema(&self) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute_batch("
            -- Пользователи
            CREATE TABLE IF NOT EXISTS users (
                id          TEXT PRIMARY KEY,
                login       TEXT UNIQUE NOT NULL,
                password    TEXT NOT NULL,
                role        TEXT NOT NULL DEFAULT 'user',
                created_at  TEXT NOT NULL
            );

            -- Роли (расширенные)
            CREATE TABLE IF NOT EXISTS roles (
                id          TEXT PRIMARY KEY,
                name        TEXT UNIQUE NOT NULL,
                description TEXT,
                permissions TEXT NOT NULL DEFAULT '[]'
            );

            -- Сценарии
            CREATE TABLE IF NOT EXISTS scenarios (
                id              TEXT PRIMARY KEY,
                title           TEXT NOT NULL,
                description     TEXT,
                sphere          TEXT,
                difficulty      TEXT,
                partner_name    TEXT,
                partner_role    TEXT,
                partner_goals   TEXT DEFAULT '[]',
                initial_context TEXT,
                dialogue_tree   TEXT DEFAULT '{}',
                endings         TEXT DEFAULT '[]',
                partner_batna   TEXT,
                player_batna    TEXT,
                is_active       INTEGER NOT NULL DEFAULT 1,
                created_at      TEXT NOT NULL
            );

            -- Профили оппонентов (ИИ)
            CREATE TABLE IF NOT EXISTS opponent_profiles (
                id          TEXT PRIMARY KEY,
                name        TEXT NOT NULL,
                role        TEXT,
                personality TEXT DEFAULT '{}',
                avatar_url  TEXT
            );

            -- Настройки приложения
            CREATE TABLE IF NOT EXISTS settings (
                key         TEXT PRIMARY KEY,
                value       TEXT NOT NULL,
                description TEXT
            );

            -- История сессий (пересоздаём без FK для совместимости)
            DROP TABLE IF EXISTS session_history;
            CREATE TABLE session_history (
                id          TEXT PRIMARY KEY,
                user_id     TEXT NOT NULL DEFAULT 'user',
                scenario_id TEXT NOT NULL,
                score       INTEGER,
                ending      TEXT,
                feedback    TEXT,
                created_at  TEXT NOT NULL
            );
        ")?;
        Ok(())
    }

    // ──────────── Seed начальных данных ────────────

    fn seed_defaults(&self) -> Result<()> {
        let conn = self.conn.lock();

        // Проверяем, есть ли уже admin
        let admin_exists: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM users WHERE role = 'admin'",
            [],
            |row| row.get(0),
        )?;

        if !admin_exists {
            let now = Utc::now().to_rfc3339();
            let admin_password = std::env::var("ADMIN_PASSWORD")
                .unwrap_or_else(|_| "admin123".to_string());
            let password_hash = hash_password(&admin_password);
            conn.execute(
                "INSERT OR IGNORE INTO users (id, login, password, role, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![Uuid::new_v4().to_string(), "admin", password_hash, "admin", now],
            )?;
        }

        // Роли по умолчанию
        conn.execute(
            "INSERT OR IGNORE INTO roles (id, name, description, permissions) VALUES (?1, ?2, ?3, ?4)",
            params![
                "role_admin",
                "Администратор",
                "Полный доступ к управлению",
                r#"["manage_users","manage_scenarios","manage_roles","manage_settings","view_analytics"]"#
            ],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO roles (id, name, description, permissions) VALUES (?1, ?2, ?3, ?4)",
            params![
                "role_user",
                "Пользователь",
                "Доступ к тренировкам",
                r#"["use_scenarios","view_history"]"#
            ],
        )?;

        // Настройки по умолчанию
        let defaults = vec![
            ("llm_provider", "groq", "Провайдер LLM"),
            ("llm_model", "meta-llama/llama-3.3-70b-versatile", "Модель LLM"),
            ("tts_url", "http://127.0.0.1:8080", "URL TTS сервера"),
            ("max_turns", "10", "Максимум ходов в диалоге"),
            ("default_difficulty", "Средняя", "Уровень сложности по умолчанию"),
        ];
        for (key, value, desc) in defaults {
            conn.execute(
                "INSERT OR IGNORE INTO settings (key, value, description) VALUES (?1, ?2, ?3)",
                params![key, value, desc],
            )?;
        }

        Ok(())
    }

    // ──────────── Users ────────────

    pub fn get_user_by_login(&self, login: &str) -> Result<Option<(String, String, String)>> {
        let conn = self.conn.lock();
        let result = conn.query_row(
            "SELECT id, password, role FROM users WHERE login = ?1",
            params![login],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        );
        match result {
            Ok((id, pw, role)) => Ok(Some((id, pw, role))),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub fn create_user(&self, login: &str, password_hash: &str, role: &str) -> Result<String> {
        let conn = self.conn.lock();
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO users (id, login, password, role, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, login, password_hash, role, now],
        )?;
        Ok(id)
    }

    pub fn list_users(&self) -> Result<Vec<User>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, login, role, created_at FROM users ORDER BY created_at DESC"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(User {
                id: row.get(0)?,
                login: row.get(1)?,
                role: row.get(2)?,
                created_at: row.get(3)?,
            })
        })?;
        rows.collect::<Result<Vec<_>>>()
    }

    pub fn delete_user(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM users WHERE id = ?1 AND role != 'admin'", params![id])?;
        Ok(())
    }

    pub fn update_user_role(&self, id: &str, role: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute("UPDATE users SET role = ?1 WHERE id = ?2", params![role, id])?;
        Ok(())
    }

    // ──────────── Scenarios ────────────

    pub fn list_scenarios(&self) -> Result<Vec<ScenarioRow>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, title, description, sphere, difficulty, partner_name, partner_role,
                    partner_goals, initial_context, dialogue_tree, endings,
                    partner_batna, player_batna, is_active, created_at
             FROM scenarios ORDER BY created_at DESC"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ScenarioRow {
                id: row.get(0)?,
                title: row.get(1)?,
                description: row.get(2)?,
                sphere: row.get(3)?,
                difficulty: row.get(4)?,
                partner_name: row.get(5)?,
                partner_role: row.get(6)?,
                partner_goals: row.get(7)?,
                initial_context: row.get(8)?,
                dialogue_tree: row.get(9)?,
                endings: row.get(10)?,
                partner_batna: row.get(11)?,
                player_batna: row.get(12)?,
                is_active: row.get::<_, i32>(13)? != 0,
                created_at: row.get(14)?,
            })
        })?;
        rows.collect::<Result<Vec<_>>>()
    }

    pub fn _get_scenario(&self, id: &str) -> Result<Option<ScenarioRow>> {
        let conn = self.conn.lock();
        let result = conn.query_row(
            "SELECT id, title, description, sphere, difficulty, partner_name, partner_role,
                    partner_goals, initial_context, dialogue_tree, endings,
                    partner_batna, player_batna, is_active, created_at
             FROM scenarios WHERE id = ?1",
            params![id],
            |row| Ok(ScenarioRow {
                id: row.get(0)?,
                title: row.get(1)?,
                description: row.get(2)?,
                sphere: row.get(3)?,
                difficulty: row.get(4)?,
                partner_name: row.get(5)?,
                partner_role: row.get(6)?,
                partner_goals: row.get(7)?,
                initial_context: row.get(8)?,
                dialogue_tree: row.get(9)?,
                endings: row.get(10)?,
                partner_batna: row.get(11)?,
                player_batna: row.get(12)?,
                is_active: row.get::<_, i32>(13)? != 0,
                created_at: row.get(14)?,
            }),
        );
        match result {
            Ok(row) => Ok(Some(row)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub fn upsert_scenario(&self, s: &ScenarioRow) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO scenarios (id, title, description, sphere, difficulty, partner_name,
                    partner_role, partner_goals, initial_context, dialogue_tree, endings,
                    partner_batna, player_batna, is_active, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
             ON CONFLICT(id) DO UPDATE SET
                title=excluded.title, description=excluded.description,
                sphere=excluded.sphere, difficulty=excluded.difficulty,
                partner_name=excluded.partner_name, partner_role=excluded.partner_role,
                partner_goals=excluded.partner_goals, initial_context=excluded.initial_context,
                dialogue_tree=excluded.dialogue_tree, endings=excluded.endings,
                partner_batna=excluded.partner_batna, player_batna=excluded.player_batna,
                is_active=excluded.is_active",
            params![
                s.id, s.title, s.description, s.sphere, s.difficulty,
                s.partner_name, s.partner_role, s.partner_goals, s.initial_context,
                s.dialogue_tree, s.endings, s.partner_batna, s.player_batna,
                s.is_active as i32, s.created_at
            ],
        )?;
        Ok(())
    }

    pub fn delete_scenario(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM scenarios WHERE id = ?1", params![id])?;
        Ok(())
    }

    // ──────────── Roles ────────────

    pub fn list_roles(&self) -> Result<Vec<RoleRow>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT id, name, description, permissions FROM roles")?;
        let rows = stmt.query_map([], |row| {
            Ok(RoleRow {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                permissions: row.get(3)?,
            })
        })?;
        rows.collect::<Result<Vec<_>>>()
    }

    pub fn upsert_role(&self, r: &RoleRow) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO roles (id, name, description, permissions)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name, description=excluded.description, permissions=excluded.permissions",
            params![r.id, r.name, r.description, r.permissions],
        )?;
        Ok(())
    }

    pub fn delete_role(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM roles WHERE id = ?1 AND name NOT IN ('admin', 'user')", params![id])?;
        Ok(())
    }

    // ──────────── Opponent Profiles ────────────

    pub fn list_opponents(&self) -> Result<Vec<OpponentProfile>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT id, name, role, personality, avatar_url FROM opponent_profiles")?;
        let rows = stmt.query_map([], |row| {
            Ok(OpponentProfile {
                id: row.get(0)?,
                name: row.get(1)?,
                role: row.get(2)?,
                personality: row.get(3)?,
                avatar_url: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>>>()
    }

    pub fn upsert_opponent(&self, o: &OpponentProfile) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO opponent_profiles (id, name, role, personality, avatar_url)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name, role=excluded.role,
                personality=excluded.personality, avatar_url=excluded.avatar_url",
            params![o.id, o.name, o.role, o.personality, o.avatar_url],
        )?;
        Ok(())
    }

    pub fn delete_opponent(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM opponent_profiles WHERE id = ?1", params![id])?;
        Ok(())
    }

    // ──────────── Settings ────────────

    pub fn _get_setting(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock();
        let result = conn.query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |row| row.get(0),
        );
        match result {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn list_settings(&self) -> Result<Vec<SettingRow>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT key, value, description FROM settings")?;
        let rows = stmt.query_map([], |row| {
            Ok(SettingRow {
                key: row.get(0)?,
                value: row.get(1)?,
                description: row.get(2)?,
            })
        })?;
        rows.collect::<Result<Vec<_>>>()
    }

    // ──────────── Session History ────────────

    pub fn save_session(&self, h: &SessionHistory) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO session_history (id, user_id, scenario_id, score, ending, feedback, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![h.id, h.user_id, h.scenario_id, h.score, h.ending, h.feedback, h.created_at],
        )?;
        Ok(())
    }

    pub fn get_user_history(&self, user_id: &str) -> Result<Vec<SessionHistory>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, user_id, scenario_id, score, ending, feedback, created_at
             FROM session_history WHERE user_id = ?1 ORDER BY created_at DESC LIMIT 50"
        )?;
        let rows = stmt.query_map(params![user_id], |row| {
            Ok(SessionHistory {
                id: row.get(0)?,
                user_id: row.get(1)?,
                scenario_id: row.get(2)?,
                score: row.get(3)?,
                ending: row.get(4)?,
                feedback: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?;
        rows.collect::<Result<Vec<_>>>()
    }
}

// ─────────────────────────────────────────────────────────────
// Утилиты
// ─────────────────────────────────────────────────────────────

pub fn hash_password(password: &str) -> String {
    use argon2::{Argon2, PasswordHasher};
    use argon2::password_hash::SaltString;
    use rand::rngs::OsRng;

    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    argon2.hash_password(password.as_bytes(), &salt)
        .expect("Failed to hash password")
        .to_string()
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    use argon2::{Argon2, PasswordHash};
    use argon2::password_hash::PasswordVerifier;

    let parsed_hash = match PasswordHash::new(hash) {
        Ok(h) => h,
        Err(_) => return false,
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok()
}

pub fn create_token(user_id: &str, role: &str) -> String {
    use jsonwebtoken::{encode, Header, EncodingKey};
    use serde::Serialize;

    #[derive(Serialize)]
    struct Claims {
        sub: String,
        role: String,
        exp: usize,
    }

    let now = chrono::Utc::now().timestamp() as usize;
    let claims = Claims {
        sub: user_id.to_string(),
        role: role.to_string(),
        exp: now + 86400, // 24 часа
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(&jwt_secret()),
    ).expect("Failed to create token")
}

pub fn verify_token(token: &str) -> Option<(String, String)> {
    use jsonwebtoken::{decode, DecodingKey, Validation, Algorithm};

    #[derive(serde::Deserialize)]
    struct Claims {
        sub: String,
        role: String,
    }

    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_required_spec_claims(&["exp"]);

    match decode::<Claims>(
        token,
        &DecodingKey::from_secret(&jwt_secret()),
        &validation,
    ) {
        Ok(data) => Some((data.claims.sub, data.claims.role)),
        Err(_) => None,
    }
}
