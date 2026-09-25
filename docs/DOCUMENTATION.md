# Документация — «Арена Переговоров»

> Задача 2: архитектура и логика симуляции, инструкция запуска и демо,
> библиотеки/сервисы, переменные окружения.
> Продукт — в [`CONCEPT.md`](CONCEPT.md), план — в [`ROADMAP.md`](ROADMAP.md),
> краткий старт — в [`../README.md`](../README.md).

---

## 1. Архитектура

Модульный монолит: **Rust/Axum** отдаёт HTTP API `/api/v1/*` и SPA из `dist/` (без сборщика).

```
Клиент (dist/js SPA)
    │  fetch /api/v1/...  (JWT Bearer)
    ▼
web/          роуты, middleware (auth, RBAC, rate-limit), хендлеры, статика
    ▼
application/  auth, session, scenario, stats, settings, provider, voice, audit
    ▼
domain/       сущности, порты (трейты), services: analysis, judge, scoring, progress
    ▼
infrastructure/  SQLite-репо + миграции, адаптеры LLM/STT/TTS, AES-256-GCM, seeds
```

| Слой | Ответственность | Примеры |
|------|-----------------|---------|
| `domain` | Бизнес-правила без I/O | `Session`, `Scenario`, `analyze()`, `build_report()`, `progress_for_xp()` |
| `application` | Сценарии использования, транзакционность, аудит | `SessionService::submit_turn`, `finish` |
| `infrastructure` | SQLite, провайдеры, шифрование | `SqliteRepos`, `MockChat`, `OpenAiCompat` |
| `web` | HTTP, валидация, маппинг ошибок | `POST /sessions/:id/turn` → 400/403/503 |

**БД:** SQLite (`rusqlite` bundled), версионные миграции в `src/infrastructure/db/migrations/` (9 штук), идемпотентные сиды (admin, 6 сценариев, mock-LLM).

**Фронтенд:** hash-роутер (`#/…`), модули `dist/js/core/*` + `dist/js/pages/*`, i18n `core/i18n.js` (ru/en), дизайн-токены `dist/css/app.css`. Страница «О команде» — `#/settings/team` (`pages/team.js`, логотипы в `static/`).

---

## 2. Логика симуляции

### 2.1 Жизненный цикл сессии

```
start  →  status=active, opening (partner)
turn*  →  LLM-ответ ∥ LLM-судья + анализ реплики игрока + commit_turn (одна транзакция)
finish →  build_report → status=finished, ending, feedback, XP += total_score
abandon→  status=abandoned (без отчёта/XP)
```

- Лимит ходов: `platform.max_turns` (default **40**), иначе fallback 40.
- Длина реплики: ≤ 4000 символов; пустая → 400.
- Мутации (`turn`/`finish`/`abandon`) — только **владелец** сессии (`ensure_owner_strict`); админ читает чужие отчёты.

### 2.2 System prompt собеседника

Собирается из сценария (`SessionService::system_prompt`):

- роль, имя, компания, цели, BATNA, цель игрока (вслух не называть);
- **сложность** (`easy`/`medium`/`hard`) — блоки поведения:
  - easy: уступает после 1–2 убедительных аргументов, простая лексика;
  - medium: уступает только после чисел/фактов, держит позицию 2–3 реплики;
  - hard: не уступает без жёстких доказательств, использует BATNA, торгуется долго;
- правила: 1–4 предложения, без списков, **отвечать на языке игрока**, не раскрывать тайные цели раньше времени.

### 2.3 Скоринг хода (`domain/services/analysis` + `scoring::apply_analysis`)

| Фактор | Вклад |
|--------|--------|
| Стратегия: сотрудничество / компромисс / конфронтация | +15 / +10 / +5 к strategy_score |
| Сила аргументации | `argument_strength * 10` |
| Тон | `tone_impact * 5` (округл.) |
| SPIN-вопрос обнаружен | +5 technique_bonus, счётчик S/P/I/N |
| Фокус на интересах | +5 |
| Объективные критерии | +8 |

`total_score` = сумма всех дельт по сессии; `TurnOutcome.total_score` — **накопленный** итог, не дельта хода.

**LLM-судья поверх эвристики** (`domain/services/judge`): параллельно ответу собеседника выполняется отдельный короткий запрос к назначенной LLM (`judge::SYSTEM_PROMPT`), который возвращает строго `{"strategy":0-10,"argument":0-10,"tone":0-10}`. Баллы хода смешиваются (`scoring::blend`):

```
round((1 - w) * эвристика + w * LLM)     w = scoring.llm_judge_weight (default 0.4)
```

Шкалы судьи приводятся к диапазонам эвристики: стратегия 0…15, аргумент 0…10, тон −5…+5 (оценка 5 = нейтральный тон). Бонус за техники (SPIN/интересы/критерии) и все счётчики для feedback считает **только эвристика** — смешивание меняет лишь числа баллов.

Настройки: `scoring.llm_judge_enabled` (default `true`), `scoring.llm_judge_weight` (default `0.4`).

Сбой судьи **не валит ход** — включая выключенную настройку, исчерпанную дневную квоту, ненастроенную модель, таймаут 20 с и неразборчивый ответ — баллы берутся с эвристики целиком, `TurnOutcome.judge = null`. Успешная оценка приходит в ответе хода как `judge` и показывается в UI бейджем `LLM s/a/t`.

**Категория стратегии — тоже решение судьи.** Модель дополнительно возвращает `category` (`collaboration` / `compromise` / `confrontation`; распознаются и русские названия). Если она есть, то именно эта категория попадает в бейдж под баллом, в `TurnOutcome.strategy_slug`, в сообщение БД (бейдж переживает перезагрузку) и в счётчики `collaboration/compromise/confrontation_count` отчёта — вместо эвристического «вопрос → сотрудничество, иначе компромисс». Без категории (судья молчит или выключен) берётся эвристика.

### 2.4 Итоговый отчёт (`scoring::build_report`)

Вход: `scenario`, `metrics`, `turn_count`, **`locale`** (`ru` | `en`).

1. `total_score` = `metrics.total_score()`.
2. `ending` = ending с максимальным `min_score ≤ total_score` (иначе дефолтный, локализованный).
3. `feedback` — markdown-разбор: стратегия, SPIN, интересы vs позиции, объективные критерии (ru/en).
4. `recommendations` — до 6 коротких пунктов «что улучшить» (ru/en).

Локаль берётся в `finish` / `report` из настройки пользователя `locale` (`SettingsService::get_user`, ключ `user_keys::LOCALE`), по умолчанию `ru`.

**Отчёт не зависит от LLM** — только от детерминированного анализа текста.

### 2.5 Прогрессия (XP / уровни)

`domain/services/progress.rs`:

- `xp_from_score(total_score)` — XP за завершённую сессию = положительный итоговый балл;
- `progress_for_xp(xp)` → `{ level, progress_pct, xp_to_next, … }`;
- пороги `LEVEL_THRESHOLDS = [0,100,250,500,850,1300,1800,2400,3100,3900]` (10 уровней);
- названия уровней — i18n `level.1`…`level.10`;
- агрегаты: `UserStats` / `LeaderboardRow` содержат `xp`, `level` (SQL + domain);
- лидерборд сортируется по **xp**.

### 2.6 Резолв модели (LLM)

1. Предпочтение пользователя: `user_model_preferences` по роли `llm` (`resolve_chat_for`).
2. Глобальное назначение роли `llm` (админ).
3. Ничего → **503** «Диалог не настроен» (и аналогично для generate / voice).

**Mock (демо без ключей):** сид `demo-mock` + модель `demo-mock-llm` (провайдер `Mock`), авто-назначение `llm` один раз (флаг `platform.demo_llm_assigned`); после ручного снятия назначения seed не восстанавливает mock.

**Ollama (локально, без ключа):** при заданном `OLLAMA_BASE_URL` сид создаёт провайдер `ollama` (`openai_compatible`, пустой API-ключ) и модель из `OLLAMA_MODEL` (default `qwen3.5:4b`). Назначение `llm`: всегда при `OLLAMA_SEED_ASSIGN=1`, иначе — только если роль пуста. Keyless действует для любых OpenAI-совместимых с локальным `base_url` (`localhost` / `127.0.0.0/8` / RFC1918) — `Provider::requires_api_key`.

### 2.7 Локализация (i18n)

| Область | Язык |
|---------|------|
| Player-страницы + shell (nav, titles) | ru / en (`dist/js/core/i18n.js`) |
| Селектор языка | Настройки → «Язык интерфейса» → `PUT /settings/me/locale` |
| Feedback / recommendations / default ending | ru / en по `locale` пользователя |
| System prompt диалога | Язык **игрока** (правило в prompt) |
| Admin-страницы | только ru (ограничение MVP) |
| Сообщения ошибок API | ru (текст из `ApiError.message`) |

---

## 3. Запуск и демо

### 3.1 Локально (Rust)

```bash
cp .env.example .env   # при желании
cargo run              # http://localhost:3001
```

Health: `GET /health` → `{"database":"up","status":"ok","version":…}`.

### 3.2 Docker

```bash
docker compose up -d
```

Образ multi-stage, volume `negotiation_data` (`/app/data`), healthcheck на `/health`.

### 3.3 Демо-путь (без внешней LLM)

1. Открыть **http://localhost:3001**.
2. Войти: `admin` / `admin123` (или зарегистрировать игрока).
3. **Играть → Сценарии → Начать** — сид `demo-mock-llm` отвечает офлайн.
4. 1–N ходов → **Завершить** → страница результата (балл, финал, SPIN, рекомендации, +XP).
5. **Кабинет** — уровень/XP, **История** — прошлые сессии.

Смена языка: **Настройки → Язык интерфейса** (ru/en) — shell и player-страницы перерисовываются; отчёт пересобирается на языке настройки.

### 3.4 Подключение настоящего LLM (опционально)

1. Админка → **Провайдеры** → добавить провайдер (OpenAI-совместимый / Anthropic / …), API-ключ шифруется.
2. **Discover** → назначить роль `llm` на модель (или `PUT /model-assignments/llm`).
3. Либо игроку: Настройки → Модели → своя модель вместо глобальной.

Голос: назначить роли `stt` / `tts`, иначе 503 «Голосовой сервис не настроен».

---

## 4. Основные HTTP-маршруты

| Метод | Путь | Доступ |
|-------|------|--------|
| GET | `/health` | public |
| POST | `/api/v1/auth/{login,register,refresh,logout}` | public |
| GET/POST | `/api/v1/scenarios`, GET `/scenarios/:id` | user (inactive — admin) |
| POST | `/api/v1/scenarios/:id/generate` | admin, LLM/mock |
| POST | `/api/v1/sessions` | user |
| GET | `/api/v1/sessions/:id`, `/messages`, `/report` | owner/admin |
| POST | `/api/v1/sessions/:id/{turn,finish,abandon}` | **owner only** |
| GET | `/api/v1/sessions?limit=&offset=` | user → `{items,total,…}` |
| GET | `/api/v1/stats/me`, `/stats/leaderboard` | user / admin |
| GET/PUT/DELETE | `/api/v1/settings/me/{key}`, `/settings/me` | user |
| GET/PUT/DELETE | `/api/v1/model-preferences/…` | user |
| POST | `/api/v1/voice/{tts,stt}` | user, roles |
| CRUD | `/api/v1/admin/…`, providers, assignments, audit | admin |

---

## 5. Переменные окружения

Источник истины — `src/config.rs`, шаблон — `.env.example` (dotenvy при `cargo run`).

| Переменная | Назначение | Default |
|------------|------------|---------|
| `HOST` / `PORT` | bind HTTP | `0.0.0.0` / `3001` |
| `JWT_SECRET` | подпись JWT (≥16) | dev-значение |
| `JWT_TTL_SECONDS` | TTL access | `86400` |
| `JWT_REFRESH_MAX_AGE_SECONDS` | окно refresh single-use | `604800` |
| `ADMIN_PASSWORD` | пароль admin при первом сиде | `admin123` |
| `ENCRYPTION_KEY` | AES-256-GCM для API-ключей | из `JWT_SECRET` |
| `ALLOWED_ORIGINS` | CORS allowlist через `,` | пусто = Any (dev) |
| `AUTH_RATE_LIMIT_MAX` / `_WINDOW_SECS` | rate-limit auth; `0`=off | `20` / `60` |
| `LOGIN_LOCKOUT_*` | lockout неудачных входов; `0`=off | `5`/`900`/`900` |
| `DB_PATH` | файл SQLite | `negotiation_arena.db` |
| `MODELS_DIR` | локальные STT/TTS | `models` |
| `RUST_LOG` | tracing filter | — |

**Глобальные настройки БД** (админка): `platform.site_name`, `platform.default_theme`, `platform.default_font_size`, `platform.default_locale`, `platform.max_turns`, `platform.llm_daily_token_limit` (0=off → 429 при исчерпании), `scoring.llm_judge_enabled` (LLM-судья, default `true`), `scoring.llm_judge_weight` (доля LLM в баллах хода, default `0.4`).

**Пользовательские:** `theme`, `font_size`, `locale`, `sound_enabled`.

---

## 6. Библиотеки и сервисы

### 6.1 Rust (основные)

| Крейт | Зачем |
|-------|--------|
| `axum`, `tower`, `tower-http` | HTTP, middleware, CORS, статика |
| `tokio` | async runtime |
| `rusqlite` (bundled) | SQLite |
| `serde` / `serde_json` | JSON |
| `argon2` | хеши паролей |
| `jsonwebtoken` | JWT |
| `aes-gcm` | шифрование ключей провайдеров |
| `reqwest` | вызовы внешних LLM/STT/TTS |
| `chrono`, `uuid`, `thiserror`, `tracing`, `dotenvy` | время, id, ошибки, логи, env |

### 6.2 Внешние сервисы (подключаемые, не обязательны)

- **LLM:** OpenAI-совместимые, Anthropic, Gemini, Groq, локальные, **Mock** (встроенный).
- **STT/TTS:** OpenAI, Groq, ElevenLabs, Deepgram, локальные модели (`MODELS_DIR`).

### 6.3 Фронтенд

Vanilla ES-модули, без npm/бандлера: `core/{dom,api,store,router,components,i18n}.js`, страницы `pages/*`, админ `pages/admin/*`.

### 6.4 Тесты и CI

```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
find dist/js -name '*.js' -print0 | xargs -0 -n1 node --check
```

CI: `.github/workflows/ci.yml` (push/PR → `main`, `dev`, `feature/**`).

---

## 7. Ограничения (честный scope)

- Админ-UI и тексты ошибок API — только русский.
- Ветвление диалога, публичный деплой, презентация — вне текущего контура агента (см. CONCEPT §6).
- Локальные STT/TTS binary — каталог/менеджер есть, полный runtime — deferred.
