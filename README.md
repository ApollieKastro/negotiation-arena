# Арена Переговоров

**Тренажёр переговорных навыков с ИИ-собеседником (MVP v2).**

Вы ведёте диалог с виртуальным партнёром по заданному сценарию (продажи, закупки, HR, партнёрство…), а в конце получаете скоринг, разбор по методикам (SPIN, гарвардский метод) и рекомендации. Генерацию речи собеседника обеспечивают внешние LLM-провайдеры, подключаемые через админку — без привязки к одному вендору.

## Архитектура

Модульный монолит на Rust (Axum) + SPA на vanilla-JS без сборщика:

- **Backend** — Rust/Axum, слои:
  - `domain/` — сущности, порты (трейты репозиториев и провайдеров), сервисы скоринга/анализа;
  - `application/` — прикладные сервисы (auth, сессии, сценарии, провайдеры, статистика, голос, настройки, аудит);
  - `infrastructure/` — SQLite-репозитории, миграции, адаптеры LLM/STT/TTS, шифрование AES-256-GCM;
  - `web/` — роуты `/api/v1`, middleware, хендлеры, отдача статики.
- **БД** — SQLite (rusqlite, bundled) с версионными миграциями (`src/infrastructure/db/migrations/`), идемпотентные сиды (роль admin, 6 базовых сценариев).
- **Фронтенд** — SPA vanilla-JS в `dist/` (history-API router, CSS), отдаётся Axum как есть, без этапа сборки.
- **HTTP API** — версионирован: `/api/v1/...`; health: `GET /health`.

## Быстрый старт

Требуется [Rust](https://rustup.rs) (stable).

```bash
cp .env.example .env   # при желании поправьте значения
cargo run
```

Сервер поднимется на **http://localhost:3001** (портом управляет `PORT`).

**Вход в админку:** логин `admin`, пароль `admin123` (значение `ADMIN_PASSWORD`; меняется при первом запуске — сид создаёт пользователя один раз).

## Docker

```bash
docker compose up -d
# или, если плагин compose не установлен:
docker-compose up -d
```

Образ собирается по `Dockerfile` (multi-stage: `rust:1-bookworm` → `debian:bookworm-slim`), приложение слушает `3001/tcp`, SQLite и локальные модели лежат в volume `negotiation_data` (`/app/data`). Healthcheck опрашивает `GET /health`.

Конфигурация берётся из окружения — передайте `JWT_SECRET`, `ENCRYPTION_KEY`, `ADMIN_PASSWORD` через `.env` рядом с `docker-compose.yml` (compose читает его автоматически) или через `environment:`.

## Конфигурация

Все переменные читаются из окружения (`.env` подхватывается dotenvy при `cargo run`). Источник истины — [`src/config.rs`](src/config.rs); шаблон со значениями по умолчанию — [`.env.example`](.env.example).

| Переменная | Описание | По умолчанию |
|---|---|---|
| `HOST` | Адрес для bind HTTP-сервера | `0.0.0.0` |
| `PORT` | Порт HTTP-сервера | `3001` |
| `JWT_SECRET` | Секрет подписи JWT (мин. 16 символов) | dev-значение (сменить в проде!) |
| `JWT_TTL_SECONDS` | TTL access-токена, сек (мин. 60) | `86400` |
| `JWT_REFRESH_MAX_AGE_SECONDS` | Окно refresh после exp access-токена, сек (≥ `JWT_TTL_SECONDS`) | `604800` |
| `ADMIN_PASSWORD` | Пароль начального админа при первом запуске (мин. 6 символов) | `admin123` |
| `ENCRYPTION_KEY` | Мастер-ключ AES-256-GCM для API-ключей провайдеров; без него наследуется из `JWT_SECRET` | — |
| `ALLOWED_ORIGINS` | CORS-allowlist через запятую (`https://…`); пусто — Any (dev) | — |
| `AUTH_RATE_LIMIT_MAX` | Rate-limit auth (login/register/refresh) по IP: запросов в окне; `0` — off | `20` |
| `AUTH_RATE_LIMIT_WINDOW_SECS` | Окно rate-limit, сек | `60` |
| `LOGIN_LOCKOUT_MAX_FAILURES` | Неудачных входов до блокировки учётки; `0` — off | `5` |
| `LOGIN_LOCKOUT_WINDOW_SECS` | Окно накопления неудач lockout, сек | `900` |
| `LOGIN_LOCKOUT_DURATION_SECS` | Длительность lockout, сек | `900` |
| `DB_PATH` | Путь к файлу SQLite | `negotiation_arena.db` |
| `MODELS_DIR` | Каталог локальных моделей (STT/TTS) | `models` |
| `LOCAL_PYTHON` | Python для `scripts/local_*.py` | `python3` |

Также используется `RUST_LOG` (фильтр tracing, например `info,axum=info`).

## Настройка LLM (обязательно для диалога)

Диалог и генерация сценариев работают только после подключения LLM-провайдера:

1. Войдите под админом → **Админка → Провайдеры**.
2. **Добавить провайдер** (OpenAI-совместимый, Anthropic, Gemini, Groq, локальный и т.п.) — укажите base URL и API-ключ (ключ шифруется, в UI показывается замаскированным).
3. Нажмите **Discover** — провайдер подтянет список моделей; убедитесь, что у нужной модели `role = llm`.
4. **Назначьте роль** `llm` на модель (раздел назначений / `PUT /api/v1/model-assignments/llm`).

Пока роль `llm` не назначена (или модель отключена/удалена), диалог и AI-генерация возвращают **503 «Диалог не настроен»**.

### Локальный Ollama (без API-ключа)

Для OpenAI-совместимых провайдеров с **локальным** `base_url` (`localhost`, `127.0.0.0/8`, приватные сети) API-ключ не обязателен.

```bash
# .env
OLLAMA_BASE_URL=http://127.0.0.1:11434/v1   # сид создаст провайдера Ollama
OLLAMA_MODEL=qwen3.5:4b                      # ollama pull qwen3.5:4b
OLLAMA_SEED_ASSIGN=1                         # 1 — при старте назначать роль llm на Ollama
```

Либо вручную: **Провайдеры → Добавить** → тип «OpenAI-совместимый», Base URL `http://127.0.0.1:11434/v1`, ключ не нужен → **Discover** → назначить роль `llm`.

## Роли моделей и голос

Модели в системе имеют роли:

- **LLM** — «голова» диалога собеседника и генерация сценариев;
- **STT** — распознавание речи (голосовой ввод);
- **TTS** — синтез речи (озвучка собеседника).

Голосовые эндпоинты (нужны назначенные роли `stt`/`tts`):

- `POST /api/v1/voice/tts` — текст → аудио;
- `POST /api/v1/voice/stt` — аудио (multipart, до 12 МБ) → текст.

### Локальные модели STT/TTS (URL / HuggingFace)

1. Установите зависимости инференса: `pip install -r scripts/requirements-voice.txt`
   (для Nemotron ASR также нужен `nemo-speech` **или** HF-каталог с `config.json`).
2. **Админка → Провайдеры → Локальные файлы** → укажите URL файла **или**
   HF repo `org/name` (+ опциональный `filename`) → **Скачать**.
   API: `POST /api/v1/local-models/download`, `GET|DELETE /api/v1/local-models`.
3. **В модель…** → выбрать роль `stt`/`tts` → в табе **Назначения ролей** назначить активную модель.
4. Голос пойдёт через локальный subprocess (`scripts/local_stt.py` / `local_tts.py`).

Пример Nemotron ASR:

- source: `nvidia/nemotron-3.5-asr-streaming-0.6b`
- filename: `nemotron-3.5-asr-streaming-0.6b.q8_0.gguf` (или пусто — весь репозиторий через `hf`)

Голосовые провайдеры:

Без назначения голосовых ролей они отвечают 503 «Голосовой сервис не настроен».

## Тесты

Полная проверка (та же, что в CI):

```bash
cargo fmt --check \
  && cargo clippy --all-targets -- -D warnings \
  && cargo test
```

## Структура каталогов

```
negotiation-arena/
├── src/
│   ├── main.rs                 # composition root, graceful shutdown
│   ├── config.rs               # env-конфигурация + валидация
│   ├── error.rs                # единый AppError → HTTP
│   ├── domain/                 # сущности, порты, скоринг/анализ
│   ├── application/            # прикладные сервисы (auth, session, …)
│   ├── infrastructure/         # SQLite, миграции, адаптеры провайдеров, crypto
│   └── web/                    # роуты /api/v1, middleware, хендлеры
├── dist/                       # SPA vanilla-JS (index.html, css, js) — без сборщика
├── static/                     # статические ассеты (/static)
├── docs/CONCEPT.md              # продуктовая концепция
├── docs/DOCUMENTATION.md        # архитектура, симуляция, запуск, env
├── docs/MODELS_GUIDE.md         # выбор LLM/STT/TTS, API-ключи, локальные модели
├── docs/ROADMAP.md              # план работ и прогресс
├── docs/presentation.pptx       # презентация для жюри (11 слайдов) + .pdf/.txt
├── docs/presentation.pdf        # то же в PDF
├── .env.example                # шаблон переменных окружения
├── Dockerfile                  # multi-stage сборка образа
├── docker-compose.yml          # сервис arena + volume + healthcheck
└── SECURITY.md                 # секреты, шифрование, обработка уязвимостей
```

## Git-процесс

Работа ведётся через ветки:

```
feature/*  →  dev  →  main
```

1. каждая фича/фикс — в отдельной ветке `feature/...` (или `fix/...`);
2. после ревью и зелёных проверок — слияние в `dev`;
3. `dev` → `main` — релиз, по отдельному одобрению.

CI (`.github/workflows/ci.yml`) гоняет `fmt` + `clippy -D warnings` + `test` на push/PR в `main`, `dev` и `feature/**`.

## См. также

- [`docs/CONCEPT.md`](docs/CONCEPT.md) — продуктовая концепция (ЦА, ценность, границы MVP)
- [`docs/DOCUMENTATION.md`](docs/DOCUMENTATION.md) — архитектура, логика симуляции, запуск/демо, env
- [`docs/presentation.pptx`](docs/presentation.pptx) / [PDF](docs/presentation.pdf) / [TXT](docs/presentation.txt) — презентация для жюри (генератор: `scripts/make_presentation.py`)
- [`docs/MODELS_GUIDE.md`](docs/MODELS_GUIDE.md) — выбор LLM/STT/TTS, API-ключи, локальные модели
- [`docs/ROADMAP.md`](docs/ROADMAP.md) — план этапов и журнал прогресса
- [`SECURITY.md`](SECURITY.md) — секреты, шифрование, как сообщить об уязвимости

## Демо без LLM

Сид создаёт встроенный **mock-провайдер** (`demo-mock` / модель `demo-mock-llm`) и один раз назначает роль `llm`, если назначения нет. Детали — [документация, §3.3](docs/DOCUMENTATION.md#33-демо-путь-без-внешней-llm).
