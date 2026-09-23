//! Порты (контракты) домена.
//!
//! Это «гнёзда», в которые подставляются адаптеры провайдеров из
//! `infrastructure::providers` и репозитории из `infrastructure::db`.
//! Бизнес-логика не знает, кто именно отвечает: Groq, OpenAI, Anthropic,
//! Gemini или локальный whisper.

pub mod providers;
pub mod repositories;

// Реэкспорт портов провайдеров — единая точка входа `domain::ports::*`
// для application-слоя (задействуется с этапа 3).
#[allow(unused_imports)]
pub use providers::*;
pub use repositories::*;
