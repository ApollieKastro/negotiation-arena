//! Порты (контракты) домена.
//!
//! Это «гнёзда», в которые подставляются адаптеры провайдеров из
//! `infrastructure::providers` и репозитории из `infrastructure::db`.
//! Бизнес-логика не знает, кто именно отвечает: Groq, OpenAI, Anthropic,
//! Gemini или локальный whisper.

pub mod repositories;

pub use repositories::*;
