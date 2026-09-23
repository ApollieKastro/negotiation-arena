# Арена переговоров v2 — План работ и прогресс

> Файл ведётся по ходу разработки. Отмечаем выполненное `[x]`, в работе `[~]`, ожидает `[ ]`.
> Архитектура: **модульный монолит-сервис** (domain / application / infrastructure / web).
> БД: **SQLite + версионные миграции**. Провайдеры ИИ: **единая абстракция, любые провайдеры**.
> Git-процесс: `feature/*` → `dev` (по одобрению) → `main` (релиз, по отдельному одобрению).

## Принятые решения

| Вопрос | Решение |
|--------|---------|
| Архитектура | Модульный монолит-сервис, слои + порты/адаптеры |
| Фронтенд | **SPA на vanilla-JS в `dist/`** (history-API router, без сборщика/TS) — *решение пересмотрено вместо Tera* |
| БД | SQLite + версионные миграции, репозитории за трейтами |
| Провайдеры | Любые: OpenAI-совместимые, Anthropic, Gemini, локальные whisper/Piper, ElevenLabs/Deepgram и т.д. |
| Роли моделей | 1) LLM (голова диалога) 2) STT 3) TTS |
| API-ключи | Шифрование AES-256-GCM, мастер-ключ из env |
| Сценарий | Стартовый контекст диалога (кто ты, кто собеседник, компания, цель, BATNA) |
| Обычные настройки | Тема, шрифт, размер текста, язык, выбор только разрешённых админом моделей |
| Порядок | Бэкенд → админка → UI |

---

## Этапы

### Этап 0. Фундамент — `[x]` выполнен
- [x] Ветки: `dev`, `feature/stage-0-foundation` (main не трогаем)
- [x] Файл прогресса `docs/ROADMAP.md`
- [x] Зависимости и структура слоёв (`domain` / `application` / `infrastructure` / `web`)
- [x] `config` — конфигурация из env, валидация
- [x] `error` — единый тип ошибок + маппинг в HTTP
- [x] Логирование (tracing, env-filter)
- [x] БД: подключение + версионные миграции (runner, 5 миграций, идемпотентность)
- [x] Схема БД-базовая линия (providers, models, users, scenarios, sessions) — *перенесено сюда из этапа 1*
- [x] `crypto` — AES-256-GCM для секретов (тесты roundtrip/nonces/wrong-key)
- [x] Порты (трейты) `ChatModel`, `SpeechToText`, `TextToSpeech`, `ModelCatalog`
- [x] Минимальный сервер: `/health`, graceful shutdown
- [x] CI: fmt + clippy + test (`.github/workflows/ci.yml`)
- [x] Удаление старого монолита из рабочей ветки (сохранён в `legacy/wip`)
- [x] Дымовой тест: сервер + миграции + `/health` (200) + 404

**Проверки:** `cargo fmt --check` ✅ · `cargo clippy -- -D warnings` ✅ · `cargo test` ✅ (6 тестов)

### Этап 1. Домен и данные — `[x]` выполнен
- [x] Миграции: полная схема (перенесены в этап 0)
- [x] Сущности домена (User, Role, Scenario, Session, Message, Provider-записи)
- [x] Репозитории (трейты + SQLite-реализации)
- [x] Сиды: роли, админ, базовые сценарии (перенос из старого `engine.rs`)
- [x] Unit-тесты домена (скоринг, SPIN-анализ)

**Проверки:** `cargo fmt --check` ✅ · `cargo clippy -- -D warnings` ✅ · `cargo test` ✅ (26 тестов)

### Этап 2. Провайдеры ИИ — `[x]` выполнен
- [x] Адаптеры: OpenAI-совместимый (chat/stt/tts)
- [x] Адаптеры: Anthropic, Google Gemini
- [~] Локальные: whisper.cpp/faster-whisper (STT), Piper/Silero (TTS) — *каталог и менеджер готовы, запуск бинарников на этапе 7*
- [x] Облачные STT/TTS: OpenAI, Groq, ElevenLabs, Deepgram
- [x] Discovery моделей по API-ключу, тест соединения
- [x] Шифрование ключей, маскирование в UI
- [x] Менеджер локальных моделей (скачивание/удаление/список)
- [x] Фабрика провайдеров: `Provider` + расшифрованный ключ → `ProviderHandle`

**Проверки:** `cargo fmt --check` ✅ · `cargo clippy -- -D warnings` ✅ · `cargo test` ✅ (76 тестов)

### Этап 3. Прикладные сервисы — `[x]` выполнен
- [x] AuthService + RBAC без дыр (JWT, роли, права)
- [x] ScenarioService (CRUD, импорт/экспорт, ИИ-генератор)
- [x] SessionService (диалог, скоринг, анализ, финал)
- [x] StatsService (статистика пользователей, активность, лидерборд)
- [x] SettingsService (глобальные + пользовательские настройки)
- [x] ProviderService (CRUD ключей, discovery, назначение ролей LLM/STT/TTS)
- [x] Сборка `Services` в composition root, `AppState.services`

**Проверки:** `cargo fmt --check` · `cargo clippy -- -D warnings` · `cargo test`

### Этап 4. HTTP API — `[x]` выполнен
- [x] Роуты admin/user, версионирование `/api/v1`
- [x] Валидация запросов, единая обработка ошибок
- [x] Middleware: auth, RBAC, аудит
- [x] Интеграционные тесты роутов (auth flow, RBAC, 401/403/404/400)

### Этап 5. Админ-панель (UI) — `[x]` выполнен
- [x] Новый дизайн-система (токены, компоненты, тёмная/светлая тема) — `dist/css/app.css`, `dist/js/core/components.js`
- [x] Дашборд: статистика пользователей, графики, активность — `dist/js/pages/admin/dashboard.js`
- [x] Пользователи: роли, блокировка, статистика — `dist/js/pages/admin/users.js`
- [x] Сценарии: конструктор + ИИ-генератор + импорт/экспорт — `dist/js/pages/admin/scenarios.js`
- [x] Провайдеры и API-ключи: добавление, тест, выбор моделей по ролям (вкл. снятие назначения `DELETE /model-assignments/:role`) — `dist/js/pages/admin/providers.js`
- [x] Настройки платформы, аудит-лог — `dist/js/pages/admin/{settings,audit}.js`

### Этап 6. Пользовательский UI — `[x]` выполнен
- [x] Auth / регистрация — `dist/js/pages/login.js`
- [x] Личный кабинет (история, прогресс, сценарии) — home/history/leaderboard
- [x] Страница диалога: карточки «ты / собеседник / компания», сложность, чат, ввод, голос — `dist/js/pages/session.js`
- [x] Страница результата: цель, баллы, что улучшить, повторить / на главную — `dist/js/pages/result.js`
- [x] Обычные настройки (тема, шрифт, размер, язык, выбор разрешённых моделей) — `dist/js/pages/settings.js`

### Этап 7. Голос — `[x]` выполнен
- [x] STT/TTS через выбранные провайдеры — `VoiceService`, `POST /api/v1/voice/{tts,stt}` (multipart, limit 14 МБ)
- [x] Голосовой ввод/вывод на странице диалога (session.js)
- [x] Ошибки «не настроено» → 503 с человекочитаемым сообщением; маппинг `voice_unconfigured`/`unconfigured`
- [ ] Стриминг, fallback по голосовым провайдерам — *deferred, не критично для MVP*

### Этап 8. Hardening — `[~]` частично
- [x] Интеграционные тесты роутов (165+ тестов зелёные)
- [x] LLM/голос без конфигурации → 503, а не 500
- [x] `DELETE /model-assignments/:role` (снятие назначения роли) + unit/HTTP тесты
- [x] Документация (README/SECURITY/.env.example), Docker (Dockerfile + docker-compose)
- [x] Legacy-БД сохранена как `negotiation_arena.legacy-backup.db`; чистый DB для смоука
- [x] Rate-limit auth (login/register/refresh) по IP → 429 + `Retry-After` (`AUTH_RATE_LIMIT_*`)
- [x] CORS-allowlist через `ALLOWED_ORIGINS` (пусто = Any только для dev)
- [x] Lockout неудачных входов (per-login, БД, миграция 0008) → 429 (`LOGIN_LOCKOUT_*`)
- [x] Ротация refresh single-use (`jti` + `used_refresh_jtis`, окно `JWT_REFRESH_MAX_AGE_SECONDS`)
- [x] Пагинация истории сессий: `GET /sessions?limit=&offset=` → `{items,total,limit,offset}` (count + LIMIT/OFFSET), UI-пейджер на истории
- [x] Session-транзакции: `create_with_opening` (сессия+opening) и `commit_turn` (UPDATE+1–2 сообщения) одной SQLite-транзакцией
- [x] FK-mapping: `FOREIGN KEY`/`UNIQUE` из rusqlite → отдельные сообщения 409 («связанная запись…» / «уже существует»)
- [x] `spawn_blocking` для admin `POST /users` (Argon2), как login/register
- [x] LLM-quota: дневной лимит токенов на пользователя `platform.llm_daily_token_limit` (0=off), учёт в `llm_daily_usage` (миграция 0009), 429 при исчерпании; ход + генерация сценариев
- [ ] Безопасность (CSRF — *не нужен при Bearer*) — *deferred*
- [ ] Покрытие/фаззинг — *deferred*

**Отложено из ревью stage 3–4 (см. журнал):** ~~пагинация, session-транзакции, LLM-quota, FK-mapping, spawn_blocking~~ — **закрыто 2026-09-23**.

---

## Журнал прогресса

| Дата | Этап | Что сделано |
|------|------|-------------|
| 2026-09-22 | 0 | Ветки `dev` + `feature/stage-0-foundation`, WIP сохранён в `legacy/wip`, план зафиксирован |
| 2026-09-22 | 0 | Слоистая структура, config/error/логирование, миграции (5), AES-256-GCM, порты провайдеров, `/health`, CI; fmt/clippy/test зелёные, дымовой тест пройден |
| 2026-09-22 | 1 | Сущности домена, порты репозиториев, SQLite-реализации, сиды (admin + 6 сценариев), сервисы analysis/scoring, Argon2id, миграция 0006; 26 тестов зелёные |
| 2026-09-22 | 2 | Порты LLM/STT/TTS/каталога, адаптеры OpenAI-compat/Anthropic/Gemini/ElevenLabs/Deepgram, локальный менеджер моделей, фабрика `ProviderHandle`, ProviderKind + elevenlabs/deepgram; 76 тестов зелёные |
| 2026-09-22 | 3 | Прикладные сервисы: AuthService+RBAC/JWT, Scenario (CRUD/import/AI-gen), Session (диалог+скоринг+финиш), Stats, Settings, Provider (ключи/роли); `Services` в `AppState`; 113 тестов зелёные |
| 2026-09-22 | 4 | HTTP API: роуты `/api/v1` (auth, users, scenarios, sessions, settings, stats, providers), middleware `AuthUser`/`AppJson`/`AppQuery`, интеграционные тесты роутов; 130 тестов зелёные |
| 2026-09-23 | 3–4 | Ревью незапушенного диффа (stage 3–4): auto-fix (mask_secret UTF-8, generic Upstream, MIN_PASSWORD_LEN, skip_deserializing, require/is_admin, login casefold+dummy_verify+timing equalizer, MAX_HISTORY_LIMIT, MAX_PLAYER_TEXT_CHARS, накопленный total_score, ensure_owner_strict, MAX_KEY/VALUE_CHARS, spawn_blocking, metrics warn, x-goog-api-key) + решения: SQL-агрегаты stats (N+1), 409 при удалении сценария с сессиями, единый MAX_TURNS из settings + миграция 0007, security-pin тесты. Deferred → stage 8: rate-limit, refresh-ротация, CORS-allowlist, пагинация, session-транзакции, LLM-quota, FK-mapping |
| 2026-09-23 | 5–6 (backend gaps) | Закрыты API-пробелы перед UI: `GET /api/v1/audit` (+ `Permission::ViewAuditLog`, AuditService, фильтры `user_id`/`action`, limit clamp 1..=200); `user_model_preferences` — CRUD `/model-preferences/{me,users}` + `options?role=`, резолв в сессиях через `resolve_chat_for` (fallback на глобальное при протухшем выборе); SPA-fallback: `dist/` as-is → `dist/index.html` → JSON-404, `/api/v1/*` без роута — всегда JSON-404; `tower` в dependencies. 149 тестов зелёные, clippy `-D warnings` ✅ |
| 2026-09-23 | 5–6 (UI SPA) | Полный SPA-фронтенд в `dist/` вместо Tera: каркас (index.html, app.css-дизайн-система, core/{dom,store,api,router,components}, main.js с роутами/shell, login.js), 7 пользовательских страниц (home, scenarios, history, session с голосом, result, settings, leaderboard), 6 админ-страниц (dashboard, users, scenarios, providers, settings, audit); E2E через Playwright; `/dist/` убран из .gitignore (dist — исходники, не build-output); legacy-БД сохранена как `negotiation_arena.legacy-backup.db` |
| 2026-09-23 | 7 | Этап «Голос»: `VoiceService` (src/application/voice.rs), `resolve_stt_for`/`resolve_tts_for`, `AppError::ServiceUnavailable` → 503, `AppMultipart`, хендлеры `POST /api/v1/voice/tts` (binary) и `/voice/stt` (multipart → `{text}`), `DefaultBodyLimit::max(14 МБ)`, тесты → 160, fmt/clippy зелёные |
| 2026-09-23 | 8 (MVP) | Фиксы: (1) LLM без назначения/отключённая → **503 «Диалог не настроен»** вместо 500 (unconfigured + DIALOG_UNCONFIGURED, build_for/chat-adapter → 503); (2) **`DELETE /api/v1/model-assignments/:role`** — порт `clear_role_assignment` → репо → `ProviderService::clear_role_assignment` (ManageProviders + audit `role.clear`) → хендлер `unassign_role` → роут delete; UI-кнопка «Снять назначение» в assignmentCard; unit-тесты resolve_chat/clear + HTTP-тесты `model_assignment_unassign_flow`, `scenario_generate_without_llm_returns_503`; **165 тестов зелёные**, clippy `-D warnings` ✅, JS `node --check` ✅ |
| 2026-09-23 | 8 (hardening) | **Rate-limit** auth (login/register/refresh) по IP: `RateLimiter` (фикс. окно, in-memory), middleware `rate_limit_auth` + `ConnectInfo`/`XFF`, ответ **429** + `Retry-After`, env `AUTH_RATE_LIMIT_MAX`/`_WINDOW_SECS` (default 20/60, 0=off); **CORS-allowlist** `ALLOWED_ORIGINS` (пусто → Any); `AppError::TooManyRequests` → 429; unit-тесты лимитера + HTTP-тесты 429/CORS; документация обновлена; Docker image собран и прогнан (health/login/503 generate) ✅ |
| 2026-09-23 | 8 (auth hardening) | **Lockout** неудачных входов (per-login, БД, миграция 0008): колонки `failed_login_count`/`last_failed_login_at`/`locked_until`, `UserRepository::record_login_failure`/`clear_login_failures`, env `LOGIN_LOCKOUT_MAX_FAILURES`/`_WINDOW_SECS`/`_DURATION_SECS` (default 5/900/900, 0=off) → **429**; **refresh single-use**: `jti` в JWT + таблица `used_refresh_jtis`, повторный refresh → 401, окно `JWT_REFRESH_MAX_AGE_SECONDS` (default 7 дней ≥ TTL) для протухшего access; фронт `api.js` сериализует параллельные refresh; unit/HTTP-тесты lockout+ротация; SECURITY/README/.env.example/ROADMAP обновлены |
| 2026-09-23 | 8 (deferred stage 3–4) | Закрыты deferred из ревью: **session-транзакции** (`SessionRepository::create_with_opening` / `commit_turn` — start и turn одной транзакцией); **пагинация** истории (`GET /sessions?limit=&offset=` → `{items,total,limit,offset}` + UI-пейджер); **FK-mapping** (FK/UNIQUE → отдельные 409); **spawn_blocking** для admin `POST /users`; **LLM-quota** — дневной лимит токенов на пользователя `platform.llm_daily_token_limit` (0=off), таблица `llm_daily_usage` (миграция 0009), 429 при исчерпании в ходе диалога и генерации сценариев, учёт usage всегда. unit/HTTP-тесты; **весь deferred stage 3–4 закрыт**. Тесты зелёные, clippy `-D warnings` ✅ |
