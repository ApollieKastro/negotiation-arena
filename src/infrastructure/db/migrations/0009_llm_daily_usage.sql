-- 0009_llm_daily_usage: дневной учёт LLM-токенов на пользователя (квота).

-- Одна строка на (user_id, day): day = 'YYYY-MM-DD' (UTC).
-- Лимит берётся из app_settings platform.llm_daily_token_limit (0 = off).
CREATE TABLE IF NOT EXISTS llm_daily_usage (
    user_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    day     TEXT NOT NULL,
    tokens  INTEGER NOT NULL DEFAULT 0,
    calls   INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (user_id, day)
);

CREATE INDEX IF NOT EXISTS idx_llm_daily_usage_day ON llm_daily_usage (day);
