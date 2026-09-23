-- 0002_providers_and_models: провайдеры ИИ, их модели и назначения по ролям.

-- API-ключ шифруется на стороне приложения (AES-256-GCM),
-- api_key_hint — маска для отображения в UI.
CREATE TABLE IF NOT EXISTS providers (
    id                TEXT PRIMARY KEY,
    name              TEXT NOT NULL,
    kind              TEXT NOT NULL,
    base_url          TEXT,
    api_key_encrypted TEXT,
    api_key_hint      TEXT,
    is_enabled        INTEGER NOT NULL DEFAULT 1,
    created_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at        TEXT
);

-- Модели, доступные у провайдера. role: llm | stt | tts.
CREATE TABLE IF NOT EXISTS models (
    id          TEXT PRIMARY KEY,
    provider_id TEXT NOT NULL REFERENCES providers (id) ON DELETE CASCADE,
    role        TEXT NOT NULL CHECK (role IN ('llm', 'stt', 'tts')),
    model_key   TEXT NOT NULL,
    display_name TEXT NOT NULL,
    is_enabled  INTEGER NOT NULL DEFAULT 1,
    metadata    TEXT NOT NULL DEFAULT '{}',
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (provider_id, role, model_key)
);

CREATE INDEX IF NOT EXISTS idx_models_role ON models (role);

-- Какая модель используется по каждой роли (llm / stt / tts).
CREATE TABLE IF NOT EXISTS role_assignments (
    role       TEXT PRIMARY KEY CHECK (role IN ('llm', 'stt', 'tts')),
    model_id   TEXT NOT NULL REFERENCES models (id) ON DELETE RESTRICT,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- Глобальные настройки ИИ: генерация сценариев, параметры диалога.
INSERT OR IGNORE INTO app_settings (key, value, description) VALUES
    ('llm_temperature',      '0.85', 'Температура LLM в диалоге'),
    ('llm_max_tokens',       '400',  'Максимум токенов на ответ LLM'),
    ('scenario_generation_model', '', 'Ключ/индекс модели для генерации сценариев (пусто = роль llm)'),
    ('max_turns',            '10',   'Максимум ходов в диалоге');
