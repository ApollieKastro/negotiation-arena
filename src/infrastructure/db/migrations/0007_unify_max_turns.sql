-- 0007_unify_max_turns: единый источник лимита ходов.
--
-- В 0002 сидился легаси-ключ `max_turns = 10`, что расходилось с
-- `platform.max_turns = 40` (DEFAULTS) и старой const в session.rs.
-- Правда — настройка `platform.max_turns`; значение приводим к 40.
-- Старый ключ также обновляем, чтобы не оставалось противоречия.

INSERT INTO app_settings (key, value, description) VALUES
    ('platform.max_turns', '40', 'Максимум ходов в сессии')
ON CONFLICT(key) DO UPDATE SET value = '40';

UPDATE app_settings
SET value = '40'
WHERE key = 'max_turns';
