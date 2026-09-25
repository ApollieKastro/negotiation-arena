-- 0011_session_branches: ветвление диалога.
--
-- Сессия — дерево реплик, а не линия: игрок может откатиться к реплике
-- собеседника и пойти по альтернативной ветке. Каждая ветка хранит свой
-- снапшот метрик (счёт копии префикса реплик при форке), а сессия
-- отражает состояние текущей (is_current) ветки.

CREATE TABLE IF NOT EXISTS session_branches (
    id              TEXT PRIMARY KEY,
    -- Ветка принадлежит сессии; удаление сессии каскадно убирает ветки.
    session_id      TEXT NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
    -- Ветка, из которой произошёл форк (NULL у main).
    parent_id       TEXT REFERENCES session_branches (id) ON DELETE CASCADE,
    -- 'main' у основной линии, 'fork' у ответвлений (UI нумерует сам).
    label           TEXT NOT NULL DEFAULT 'fork',
    -- turn_index реплики-точки ветвления (0 — opening).
    fork_turn_index INTEGER NOT NULL DEFAULT 0,
    -- Снапшот SessionMetrics (JSON) на момент последнего хода в ветке.
    metrics         TEXT NOT NULL DEFAULT '{}',
    total_score     INTEGER NOT NULL DEFAULT 0,
    turn_count      INTEGER NOT NULL DEFAULT 0,
    -- Ровно одна текущая ветка на сессию (см. частичный UNIQUE-индекс ниже).
    is_current      INTEGER NOT NULL DEFAULT 0 CHECK (is_current IN (0, 1)),
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_session_branches_session
    ON session_branches (session_id, created_at);

-- Инвариант: не больше одной текущей ветки на сессию.
CREATE UNIQUE INDEX IF NOT EXISTS idx_session_branches_current
    ON session_branches (session_id) WHERE is_current = 1;

-- Реплика принадлежит ветке. DEFAULT NULL: SQLite допускает REFERENCES
-- в ADD COLUMN только при значении по умолчанию NULL; backfill ниже.
ALTER TABLE session_messages ADD COLUMN branch_id TEXT
    REFERENCES session_branches (id);

-- Backfill: main-ветка существующих сессий (id ветки = id сессии).
INSERT INTO session_branches (
    id, session_id, parent_id, label, fork_turn_index,
    metrics, total_score, turn_count, is_current, created_at
)
SELECT s.id, s.id, NULL, 'main', 0,
       s.metrics, s.total_score, s.turn_count, 1, s.created_at
FROM sessions s
WHERE NOT EXISTS (
    SELECT 1 FROM session_branches b WHERE b.session_id = s.id
);

-- Старые реплики без ветки привязываем к main (id ветки = id сессии).
UPDATE session_messages
SET branch_id = session_id
WHERE branch_id IS NULL
  AND EXISTS (SELECT 1 FROM session_branches b WHERE b.id = session_id);

CREATE INDEX IF NOT EXISTS idx_session_messages_branch
    ON session_messages (branch_id, turn_index);
