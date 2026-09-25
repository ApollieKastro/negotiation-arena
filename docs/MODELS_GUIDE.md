# Гайд: выбор моделей (LLM / STT / TTS)

> Какую нейросеть поставить «голове» (диалогу), голосу и распознаванию;
> где взять API-ключ; какие модели подходят; как скачать локальные.

Краткая карта ролей:

| Роль | Что делает | Где назначается |
|------|------------|-----------------|
| **LLM («голова»)** | Ответы собеседника, генерация сценариев | Админка → Провайдеры → Назначения ролей `llm` |
| **STT** | Голос игрока → текст | … роль `stt` |
| **TTS** | Реплики собеседника → звук | … роль `tts` |

Итог: **админ** подключает провайдера и назначает роли; **игрок** в **Настройки → Модели** может выбрать свою модель из списка, если админ разрешил свободный выбор.

---

## 1. Быстрый старт (3 маршрута)

| Маршрут | Что нужно | Когда подходит |
|---------|-----------|----------------|
| **A. Демо (mock)** | Ничего | Хотите попробовать диалог сразу, без ключей |
| **B. Облачный API** | Регистрация у провайдера + ключ | Нужен реальный умный диалог / облачный голос |
| **C. Локально (Ollama / файлы)** | GPU/CPU, дисковое пространство | Приватность, без платы за токены, свой железо |

Минимум для полного голоса: **LLM** (или mock) + **STT** + **TTS**. Без ролей `stt`/`tts` голосовые кнопки отвечают 503 «Голосовой сервис не настроен».

---

## 2. LLM — «голова» собеседника

### 2.1 Облачные провайдеры (тип «OpenAI-совместимый» или специализированный)

| Провайдер | Где взять ключ | Base URL (если «OpenAI-совместимый») | Что подойдёт из моделей | Примечание |
|-----------|----------------|----------------------------------------|-------------------------|------------|
| **OpenAI** | [platform.openai.com](https://platform.openai.com/api-keys) → API keys | `https://api.openai.com/v1` (по умолчанию) | `gpt-4o-mini` (дёшево), `gpt-4o`, `gpt-4.1-mini` | Тип: OpenAI-совместимый |
| **Groq** | [console.groq.com](https://console.groq.com/keys) | `https://api.groq.com/openai/v1` | `llama-3.3-70b-versatile`, `llama-3.1-8b-instant` | Очень быстро, дёшево |
| **OpenRouter** | [openrouter.ai/keys](https://openrouter.ai/keys) | `https://openrouter.ai/api/v1` | `openai/gpt-4o-mini`, `anthropic/claude-3.5-haiku`, `meta-llama/llama-3.3-70b-instruct` | Один ключ → сотни моделей |
| **Together** | [api.together.ai](https://api.together.ai/settings/api-keys) | `https://api.together.xyz/v1` | `meta-llama/Llama-3.3-70B-Instruct-Turbo`, `Qwen/Qwen2.5-72B-Instruct` | Открытые веса в облаке |
| **Anthropic** | [console.anthropic.com](https://console.anthropic.com/settings/keys) | тип **Anthropic** (не OpenAI-compat) | `claude-3-5-haiku-latest`, `claude-3-5-sonnet-latest` | Messages API |
| **Google Gemini** | [aistudio.google.com](https://aistudio.google.com/apikey) | тип **Google Gemini** | `gemini-2.0-flash`, `gemini-1.5-flash` | Отдельный адаптер в админке |
| **DeepSeek** | [platform.deepseek.com](https://platform.deepseek.com/api_keys) | `https://api.deepseek.com/v1` | `deepseek-chat` | OpenAI-совместимый |
| **Yandex GPT / др.** | сайт вендора | их OpenAI-compatible endpoint, если есть | — | Иначе не подойдёт |

**Рекомендация для MVP:** `gpt-4o-mini` или `llama-3.3-70b` через **Groq/OpenRouter** — дёшево и достаточно для тренажёра.

### 2.2 Локально без ключа (Ollama)

```bash
# 1. Установить Ollama → https://ollama.com
ollama pull qwen3:4b        # или llama3.2:3b, qwen2.5:7b
# 2. .env проекта
OLLAMA_BASE_URL=http://127.0.0.1:11434/v1
OLLAMA_MODEL=qwen3:4b
# 3. cargo run — сид создаст провайдера (см. .env.example)
```

Или вручную: **Провайдеры → Добавить** → тип «OpenAI-совместимый», Base URL `http://127.0.0.1:11434/v1`, **API-ключ не нужен** (localhost) → Discover → назначить роль `llm`.

**Какие модели Ollama подойдут:** лёгкие чат-модели 3–8B (`qwen3:4b`, `llama3.2:3b`, `gemma3:4b`) — хватает для коротких реплик 1–4 предложений; 14B+ лучше, если хватает RAM/VRAM.

### 2.3 Демо (mock)

Провайдер **Демо (офлайн)**, модель `demo-mock-llm` — сид назначает её на `llm` один раз. Без интернета и ключей; ответы шаблонные, для показа UI и скоринга.

---

## 3. STT — распознавание речи игрока

Микрофон → текст. MIME: `audio/*`, до 12 МБ.

### 3.1 Облачный STT

| Провайдер | Ключ | Base URL | Модели | Где в админке |
|-----------|------|----------|--------|----------------|
| **Groq** | console.groq.com | `https://api.groq.com/openai/v1` | `whisper-large-v3`, `whisper-large-v3-turbo` | OpenAI-совместимый |
| **OpenAI** | platform.openai.com | `https://api.openai.com/v1` | `whisper-1` | OpenAI-совместимый |
| **Deepgram** | console.deepgram.com | — | `nova-2`, `nova` | Тип **Deepgram** (нужен ключ) |
| **OpenRouter** | openrouter.ai | `https://openrouter.ai/api/v1` | `openai/whisper-1` и др. | OpenAI-совместимый |

**Рекомендация:** Whisper на **Groq** (`whisper-large-v3-turbo`) — быстро и дёшево.

### 3.2 Локальный STT (файлы в `MODELS_DIR`)

Зависимости: `pip install -r scripts/requirements-voice.txt`.

| Модель | Как скачать | Формат / бэкенд |
|--------|-------------|-----------------|
| **Nemotron 3.5 ASR 0.6b** (рекомендуется, мультиязык) | Админка → **Локальные файлы** → source `nvidia/nemotron-3.5-asr-streaming-0.6b`, filename `nemotron-3.5-asr-streaming-0.6b.q8_0.gguf` **или** пустой filename (весь repo, нужен `hf`) | `nemo-speech` **или** HF-каталог с `config.json` + transformers |
| **Whisper (faster-whisper)** | URL модели CT2/ggml или repo с весами | `faster-whisper` (`local_stt.py`) |
| Готовый облачный | Не качаем | API выше |

**Пошагово (Nemotron):**

1. `pip install -r scripts/requirements-voice.txt` (и при желании `nemo-speech` для GGUF).
2. **Админка → Провайдеры → Локальные файлы** → URL/HF → **Скачать**.
3. **В модель…** → роль **stt** → **Назначения ролей** → назначить.

Без зависимостей скрипт ответит понятной ошибкой «pip install -r scripts/requirements-voice.txt».

---

## 4. TTS — озвучка собеседника

Текст → аудио (WAV/MP3).

### 4.1 Облачный TTS

| Провайдер | Ключ | Base URL | Модели / голоса | Примечание |
|-----------|------|----------|-----------------|------------|
| **OpenAI** | platform.openai.com | `https://api.openai.com/v1` | `tts-1`, `tts-1-hd`; голоса: alloy, nova, shimmer… | OpenAI-совместимый |
| **Groq** | console.groq.com | `https://api.groq.com/openai/v1` | `playai-tts` | Тот же ключ, что для LLM/STT |
| **Deepgram** | console.deepgram.com | — | `aura-2-thera`, `aura-2-arcas` | Тип Deepgram |
| **ElevenLabs** | elevenlabs.io → Profile | — | `eleven_multilingual_v2` (RU/EN) | Тип **ElevenLabs**; голос берётся из settings UI / API |

**Рекомендация:** ElevenLabs — лучшее качество RU; OpenAI `tts-1` — просто и дёшево.

### 4.2 Локальный TTS (Piper)

Уже можно без скачивания, если в `MODELS_DIR` лежит голос (сид/demo часто кладёт):

| Голос | Файл | Язык |
|-------|------|------|
| **ru_RU-irina-medium** | `models/ru/ru_RU/irina/medium/ru_RU-irina-medium.onnx` | русский |

- Установить: `pip install -r scripts/requirements-voice.txt` (пакет `piper-tts`).
- Другой голос: [rhasspy/piper-voices](https://huggingface.co/rhasspy/piper-voices) → скачать `.onnx` + `.onnx.json` (URL или HF в «Локальных файлах»).
- **В модель…** → роль **tts** → **Назначения ролей** → назначить.

---

## 5. Шаги в админке (checklist)

1. **Провайдеры → Добавить провайдера**  
   - Тип, Base URL, API-ключ (шифруется; для Local/Mock и localhost-OpenAI ключ не нужен).
2. **Ping** — проверка соединения.
3. **Discovery** — подтянуть список моделей (роль llm/stt/tts).
4. **Модели → Добавить** — если discovery не нашёл (локальные пути вручную).
5. **Назначения ролей** — выбрать активную модель для `llm`, `stt`, `tts`.
6. **Локальные файлы** — скачать URL/HF → «В модель…» → назначить (см. §3–4).

Ошибки:

| Симптом | Причина | Лечение |
|---------|---------|---------|
| 503 «Диалог не настроен» | нет назначения `llm` или модель/провайдер выключены | Назначения ролей / включить |
| 503 «Голосовой сервис не настроен» | нет `stt`/`tts` | Назначить роли |
| 502/«Внешний сервис недоступен» | неверный ключ/URL | Проверить Base URL и ключ, Ping |
| STT: «pip install …» | нет Python-зависимостей | `pip install -r scripts/requirements-voice.txt` |

---

## 6. Настройки игрока

**Настройки → Модели:**

- Виден список моделей, которые **админ разрешил** для роли.
- Можно поставить **свою** вместо глобальной (`model-preferences`).
- Кнопка **«Сбросить на глобальную»** вернёт назначение администратора.
- Если список пуст — «Модели не назначены администратором»: обратитесь к админу (это не ошибка клиента).

Игрок **не вводит API-ключи** — ключи только у администратора.

---

## 7. Что выбрать: шпаргалка

| Цель | Вариант |
|------|---------|
| Показать демо за 2 минуты | Mock LLM, голос не нужен |
| Дешёвый умный диалог | Groq `llama-3.3-70b` или OpenAI `gpt-4o-mini` |
| Много моделей одним ключом | OpenRouter |
| Приватно и бесплатно (текст) | Ollama `qwen3:4b` / `llama3.2:3b` |
| Лучший русский голос | ElevenLabs `eleven_multilingual_v2` |
| Дешёвый облачный голос | OpenAI `tts-1` |
| Быстрый русский STT в облаке | Groq `whisper-large-v3-turbo` |
| Полностью локальный голос | Piper (TTS) + Nemotron ASR / faster-whisper (STT) |

---

## 8. Ссылки

- Продукт: [CONCEPT.md](CONCEPT.md) · Техдокументация: [DOCUMENTATION.md](DOCUMENTATION.md) · План: [ROADMAP.md](ROADMAP.md)
- Зависимости локального голоса: [`scripts/requirements-voice.txt`](../scripts/requirements-voice.txt)
- Env: [`.env.example`](../.env.example) — `MODELS_DIR`, `OLLAMA_*`, `LOCAL_*`
- HTTP: `GET/POST/DELETE /api/v1/local-models*`, `PUT /model-assignments/:role`, `GET /model-preferences/options?role=`
