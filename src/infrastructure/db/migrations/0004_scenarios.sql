-- 0004_scenarios: сценарии как стартовый контекст диалога.

CREATE TABLE IF NOT EXISTS scenarios (
    id                 TEXT PRIMARY KEY,
    title              TEXT NOT NULL,
    description        TEXT,
    sphere             TEXT,
    difficulty         TEXT NOT NULL DEFAULT 'Средняя',

    -- Роль игрока: кто он такой и чего добивается.
    player_role        TEXT,
    player_goal        TEXT,
    player_batna       TEXT,

    -- Собеседник: персона, компания, интересы.
    partner_name       TEXT,
    partner_role       TEXT,
    partner_company    TEXT,
    partner_goal       TEXT,
    partner_batna      TEXT,
    partner_personality TEXT,   -- JSON: тон, манера речи

    -- Стартовый контекст: реплика/ситуация, с которой начинается диалог.
    opening_context    TEXT NOT NULL DEFAULT '',

    ai_generated       INTEGER NOT NULL DEFAULT 0,
    is_active          INTEGER NOT NULL DEFAULT 1,
    created_by         TEXT REFERENCES users (id) ON DELETE SET NULL,
    created_at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at         TEXT
);

CREATE INDEX IF NOT EXISTS idx_scenarios_active ON scenarios (is_active);
CREATE INDEX IF NOT EXISTS idx_scenarios_sphere ON scenarios (sphere);
