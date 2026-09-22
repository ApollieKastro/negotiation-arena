-- 0006_scenario_and_session_details: детали сценариев и метрики сессий.

-- Финалы сценария: JSON-массив {id, title, text, outcome, min_score}.
ALTER TABLE scenarios ADD COLUMN endings TEXT NOT NULL DEFAULT '[]';

-- Цели оппонента (JSON-массив строк).
ALTER TABLE scenarios ADD COLUMN partner_goals TEXT NOT NULL DEFAULT '[]';

-- Компания, которую представляет игрок.
ALTER TABLE scenarios ADD COLUMN player_company TEXT;

-- Аккумулированные метрики техник сессии (JSON-объект SessionMetrics).
ALTER TABLE sessions ADD COLUMN metrics TEXT NOT NULL DEFAULT '{}';
