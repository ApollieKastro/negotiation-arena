-- 0010_user_profile: аватар пользователя (миниатюра) в учётной записи.
--
-- Храним байты рядом с пользователем: аватар — часть агрегата user,
-- живёт и умирает вместе с ним (ON DELETE не нужен — колонки той же таблицы).
-- Списочные выборки байты не читают: только признак наличия.

ALTER TABLE users ADD COLUMN avatar_mime TEXT;
ALTER TABLE users ADD COLUMN avatar_data BLOB;
