//! Адаптеры провайдеров ИИ (LLM / STT / TTS).
//!
//! Каждый провайдер реализует порты `domain::ports`:
//! `ChatModel`, `SpeechToText`, `TextToSpeech`, `ModelCatalog`.
//!
//! Этап 2: openai_compat, anthropic, gemini, локальные whisper/piper.
