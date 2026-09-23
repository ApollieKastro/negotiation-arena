-- 0005_sessions: сессии прохождения сценария и реплики диалога.

CREATE TABLE IF NOT EXISTS sessions (
    id           TEXT PRIMARY KEY,
    user_id      TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    scenario_id  TEXT NOT NULL REFERENCES scenarios (id) ON DELETE CASCADE,
    mode         TEXT NOT NULL DEFAULT 'text' CHECK (mode IN ('text', 'voice')),
    status       TEXT NOT NULL DEFAULT 'active'
                 CHECK (status IN ('active', 'finished', 'abandoned')),
    total_score  INTEGER NOT NULL DEFAULT 0,
    turn_count   INTEGER NOT NULL DEFAULT 0,
    -- Метрики техник для обратной связи.
    strategy_score INTEGER NOT NULL DEFAULT 0,
    argument_score INTEGER NOT NULL DEFAULT 0,
    tone_score     INTEGER NOT NULL DEFAULT 0,
    spin_counts    TEXT NOT NULL DEFAULT '{}',
    ending_id     TEXT,
    ending_title  TEXT,
    feedback      TEXT,
    created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    finished_at   TEXT
);

CREATE INDEX IF NOT EXISTS idx_sessions_user ON sessions (user_id, created_at);
CREATE INDEX IF NOT EXISTS idx_sessions_scenario ON sessions (scenario_id);
CREATE INDEX IF NOT EXISTS idx_sessions_status ON sessions (status);

-- Реплики диалога (порядок по turn_index, role: player | partner).
CREATE TABLE IF NOT EXISTS session_messages (
    id          TEXT PRIMARY KEY,
    session_id  TEXT NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
    turn_index  INTEGER NOT NULL,
    role        TEXT NOT NULL CHECK (role IN ('player', 'partner')),
    content     TEXT NOT NULL,
    strategy    TEXT,
    score_delta INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_session_messages_session
    ON session_messages (session_id, turn_index);
