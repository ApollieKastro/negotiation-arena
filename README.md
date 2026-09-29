# Арена Переговоров

**Тренажёр переговорных навыков с ИИ-собеседником (MVP v2).**

Вы ведёте диалог с виртуальным партнёром по заданному сценарию (продажи, закупки, HR, партнёрство…), а в конце получаете скоринг, разбор по методикам (SPIN, гарвардский метод) и рекомендации. Генерацию речи собеседника обеспечивают внешние LLM-провайдеры, подключаемые через админку — без привязки к одному вендору.

## 📑 Навигация

- [Быстрый старт](#-быстрый-старт)
- [Docker (рекомендуется)](#-вариант-1--docker)
- [Linux / macOS (Rust + install.sh)](#-вариант-2--локально-rust)
- [Windows (PowerShell + install.ps1)](#-windows-powershell)
- [Конфигурация](#конфигурация)
- [Голос (TTS/STT)](#голос-ttsstt)
- [Архитектура](#архитектура)

## 🚀 Быстрый старт

| Платформа | Команда |
|---|---|
| **Docker** | `git clone ... && cd negotiation-arena && cp .env.example .env && docker compose up -d --build` |
| **Linux / macOS** | `curl -fsSL https://raw.githubusercontent.com/ApollieKastro/negotiation-arena/main/install.sh \| bash` |
| **Windows (PowerShell)** | `iex (irm https://raw.githubusercontent.com/ApollieKastro/negotiation-arena/main/install.ps1)` |

После установки откройте **http://localhost:3001/#/login** — логин `admin` / пароль `admin123`.

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

## Установка

Два варианта: **Docker** (рекомендуется — ничего ставить не нужно) или локальная сборка на Rust.

### Вариант 1 — Docker

```bash
git clone https://github.com/ApollieKastro/negotiation-arena.git
cd negotiation-arena
cp .env.example .env          # при желании поправьте секреты/пароль
docker compose up -d --build  # сборка образа + запуск
```

Проверка: `curl http://localhost:3001/health` → `{"database":"up","status":"ok","version":"0.2.0"}`.

Если compose-плагина нет:

```bash
docker build -t negotiation-arena:latest .
docker run -d --name negotiation-arena \
  -p 3001:3001 \
  -v negotiation_data:/app/data \
  --env-file .env \
  negotiation-arena:latest
```

- **Стоп:** `docker compose down` — данные (SQLite, модели) остаются в volume `negotiation_data`.
- **Полный сброс до чистого состояния:** `docker compose down -v` — удаляет volume со всеми историями сессий, аудитом и записями пользователей; при следующем старте БД создаётся заново (сид: роль `admin`, 6 базовых сценариев, демо-модель для офлайн-игры).

Образ собирается multi-stage (`rust:1-bookworm` → `debian:bookworm-slim`), работает под непривилегированным пользователем, healthcheck опрашивает `GET /health`. СPython-зависимостями голоса (см. ниже) размер образа ~1.1 ГБ.

### 🔊 Голос (TTS/STT) в Docker

Образ уже содержит Python, `piper-tts`, `faster-whisper` и скрипты `scripts/local_*.py` — доустанавливать ничего не нужно. При первом старте entrypoint сидит русский Piper-голос в volume; в админке назначьте его на роль **TTS**: `Провайдеры → Local → Модели → piper/ru_RU-ruslan-medium.onnx → Назначения ролей → tts`. Без голоса в сборке: `docker compose build --build-arg PIPER_VOICE_DOWNLOAD=0`. Для STT скачайте, например, HF repo `Systran/faster-whisper-base` (админка → Локальные файлы) и назначьте на роль `stt`.

### 🐧 Вариант 2 — локально (Linux / macOS / Rust)

**Установка одним скриптом (рекомендуется):**

```bash
curl -fsSL https://raw.githubusercontent.com/ApollieKastro/negotiation-arena/main/install.sh | bash
# или из клона: ./install.sh   → меню:
#   1) Установить   2) Обновить (GitHub main + пересборка)   3) Удалить
```

Установщик клонирует репозиторий (если его нет), собирает release, создаёт
ярлык приложения «Negotiation Arena», команды `arena` / `negotiation-arena`
в `~/.local/bin` и алиас `arena` в `.zshrc`. Повторный запуск идемпотентен.

После установки: ярлык поднимает сервер в фоне и открывает браузер на `http://localhost:3001/#/login` (повторный
клик — только вкладка, второй экземпляр не поднимается). Остановка:
`kill "$(cat ~/.cache/negotiation-arena.pid)"`, лог: `~/.cache/negotiation-arena.log`.

Требуется [Rust](https://rustup.rs) stable (для ручной сборки).

```bash
cp .env.example .env   # при желании поправьте значения
cargo run
```

Сервер поднимётся на **http://localhost:3001/#/login** (портом управляет `PORT`).

- **Полный сброс до чистого состояния:** удалите файлы `negotiation_arena.db*` — при следующем запуске БД создаётся заново с теми же сидами.

### 🪟 Windows (PowerShell)

**Установка одним скриптом (рекомендуется):**

```powershell
# Вариант 1: из PowerShell (pwsh 7+ или Windows PowerShell 5.1)
iex (irm https://raw.githubusercontent.com/ApollieKastro/negotiation-arena/main/install.ps1)
# → откроется интерактивное меню: 1) Установить / 2) Обновить / 3) Удалить
```

```powershell
# Вариант 2: из клонированного репозитория
.\install.ps1            # интерактивное меню
.\install.ps1 install    # установить
.\install.ps1 update     # обновить (git pull + пересборка)
.\install.ps1 uninstall  # удалить
```

Установщик клонирует репозиторий, собирает release, создаёт ярлык «Negotiation Arena» в меню Пуске и на рабочем столе, добавляет команды `negotiation-arena` / `arena` в PATH и алиас `arena` в PowerShell профиль. Повторный запуск идемпотентен.

После установки: ярлык поднимает сервер в фоне и открывает браузер на `http://localhost:3001/#/login` (повторный клик — только вкладка, второй экземпляр не поднимается). Остановка: `Stop-Process -Id (Get-Content $env:LOCALAPPDATA\negotiation-arena.pid)`, лог: `$env:LOCALAPPDATA\negotiation-arena.log`.

Требуется [Rust](https://rustup.rs) stable и Python (для голоса/STT).

**Вход в админку:** логин `admin`, пароль `admin123` (значение `ADMIN_PASSWORD`; задаётся только при первом запуске — сид создаёт пользователя один раз).

Конфигурация (оба варианта) — переменные окружения: compose подхватывает `.env` автоматически, список переменных см. в [Конфигурация](#конфигурация) и [`.env.example`](.env.example).

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
OLLAMA_MODEL=qwen2.5:3b                      # ollama pull qwen2.5:3b (быстрый, русский)
OLLAMA_SEED_ASSIGN=1                         # 1 — при старте назначать роль llm на Ollama
```

Либо вручную: **Провайдеры → Добавить** → тип «OpenAI-совместимый», Base URL `http://127.0.0.1:11434/v1`, ключ не нужен → **Discover** → назначить роль `llm`.

⚠️ **Чего не брать**: reasoning-модели (`qwen3`, `qwen3.5`) отвечают пустым
текстом через `/v1`, а `gemma3:*` (SWA) пересчитывают промпт с нуля на каждом
ходе — на CPU диалог растягивается до 20–30 с. Подробности и настройки
systemd Ollama — в [docs/MODELS_GUIDE.md](docs/MODELS_GUIDE.md).


## Роли моделей и голос

Модели в системе имеют роли:

- **LLM** — «голова» диалога собеседника и генерация сценариев;
- **STT** — распознавание речи (голосовой ввод);
- **TTS** — синтез речи (озвучка собеседника).

Голосовые эндпоинты (нужны назначенные роли `stt`/`tts`):

- `POST /api/v1/voice/tts` — текст → аудио;
- `POST /api/v1/voice/stt` — аудио (multipart, до 12 МБ) → текст.

### Локальные модели STT/TTS (URL / HuggingFace)

**Быстрый старт одной кнопкой:** **Админка → Провайдеры → Локальные файлы** →
секция «Быстрый старт» → пресет «Piper (русский голос)» или «Whisper base/tiny»
→ **Установить**: файл скачается, зарегистрируется и сразу назначится на роль
`tts`/`stt` — вручную ничего править не нужно (уже установленное можно
переустановить). Ручная установка через URL — ниже.

**Проверенная офлайн-связка** (Piper TTS + faster-whisper STT — работает и локально, и в Docker):

```bash
mkdir -p models/piper models/faster-whisper-base
B=https://huggingface.co/rhasspy/piper-voices/resolve/main/ru/ru_RU/ruslan/medium/ru_RU-ruslan-medium.onnx
curl -L "$B"      -o models/piper/ru_RU-ruslan-medium.onnx       # голос (63 МБ)
curl -L "$B.json" -o models/piper/ru_RU-ruslan-medium.onnx.json  # конфиг — обязателен

for f in model.bin config.json tokenizer.json vocabulary.txt; do   # STT (~145 МБ)
  curl -L "https://huggingface.co/Systran/faster-whisper-base/resolve/main/$f" \
    -o "models/faster-whisper-base/$f"
done
```

Дальше: **В модель…** → роли, ключи — `piper/ru_RU-ruslan-medium.onnx` (`tts`)
и `faster-whisper-base` (`stt`) → **Назначения ролей**. В Docker русский голос уже
засеян entrypoint'ом — достаточно назначить (см. «Голос в Docker»).

⚠️ Скачивание в админке сохраняет тело URL как есть: URL должен вести на **файл**
(`…/resolve/main/…`), иначе вместо модели запишется HTML-страница HuggingFace
(симптомы: «не найден .onnx голос», «не найдена модель»).

1. Установите зависимости инференса: `pip install -r scripts/requirements-voice.txt`
   (в Docker-образе уже установлены; внутри контейнера можно допакетировать:
   `docker exec -it negotiation-arena pip3 install <пакет>`).
   Для Nemotron ASR также нужен `nemo-speech` **или** HF-каталог с `config.json`.
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
├── install.sh                  # установщик: 1 Установить / 2 Обновить (GitHub) / 3 Удалить + ярлык
├── Dockerfile                  # multi-stage сборка образа
├── docker-compose.yml          # сервис arena + volume + healthcheck
├── docker-entrypoint.sh        # сидинг Piper-голоса в volume перед стартом контейнера
├── scripts/                    # локальный голос: local_tts.py, local_stt.py, requirements-voice.txt
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
