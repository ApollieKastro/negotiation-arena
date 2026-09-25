-- 0003_users_and_auth: пользователи, роли, назначение моделей и UI-настройки.

CREATE TABLE IF NOT EXISTS users (
    id            TEXT PRIMARY KEY,
    login         TEXT UNIQUE NOT NULL,
    password_hash TEXT NOT NULL,
    display_name  TEXT,
    role          TEXT NOT NULL DEFAULT 'user' CHECK (role IN ('admin', 'user')),
    is_active     INTEGER NOT NULL DEFAULT 1,
    created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at    TEXT
);

-- Расширенные роли и их права (JSON-массив permissions).
CREATE TABLE IF NOT EXISTS roles (
    id          TEXT PRIMARY KEY,
    name        TEXT UNIQUE NOT NULL,
    description TEXT,
    permissions TEXT NOT NULL DEFAULT '[]'
);

-- Настройки пользователя: тема, шрифт, язык и т.п.
CREATE TABLE IF NOT EXISTS user_settings (
    user_id     TEXT PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    theme       TEXT NOT NULL DEFAULT 'system',
    font_family TEXT NOT NULL DEFAULT 'inter',
    font_size   TEXT NOT NULL DEFAULT 'md',
    language    TEXT NOT NULL DEFAULT 'ru',
    updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- Пользовательские назначения моделей (только из списка, разрешённого админом).
-- Отсутствие записи = используется глобальное назначение роли.
CREATE TABLE IF NOT EXISTS user_model_preferences (
    user_id    TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    role       TEXT NOT NULL CHECK (role IN ('llm', 'stt', 'tts')),
    model_id   TEXT NOT NULL REFERENCES models (id) ON DELETE CASCADE,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (user_id, role)
);

INSERT OR IGNORE INTO roles (id, name, description, permissions) VALUES
    ('role_admin', 'Администратор', 'Полный доступ к управлению',
     '["manage_users","manage_scenarios","manage_providers","manage_settings","view_analytics"]'),
    ('role_user', 'Пользователь', 'Доступ к тренировкам',
     '["use_scenarios","view_history","manage_own_settings"]');
