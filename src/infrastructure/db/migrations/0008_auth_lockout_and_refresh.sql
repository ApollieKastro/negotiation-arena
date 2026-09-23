-- 0008_auth_lockout_and_refresh: lockout неудачных входов + одноразовые refresh (jti).

-- Счётчик неудачных попыток входа и окно блокировки (переживает рестарт).
ALTER TABLE users ADD COLUMN failed_login_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE users ADD COLUMN last_failed_login_at TEXT;
ALTER TABLE users ADD COLUMN locked_until TEXT;

-- Использованные jti refresh-токенов: повторный refresh с тем же jti → 401.
-- purge_after — момент, после которого строку можно удалить (iat + refresh max age).
CREATE TABLE IF NOT EXISTS used_refresh_jtis (
    jti         TEXT PRIMARY KEY,
    user_id     TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    used_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    purge_after TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_used_refresh_jtis_purge ON used_refresh_jtis (purge_after);
