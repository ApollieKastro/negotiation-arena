//! Порты провайдеров ИИ: диалог (LLM), распознавание речи, синтез речи,
//! каталог моделей и проверка соединения.
//!
//! Реализации — в `infrastructure::providers`. Домен и application зависят
//! только от трейтов и не знают wire-протоколов конкретного провайдера.

use async_trait::async_trait;

use crate::domain::entities::model::{ModelDescriptor, ModelRole};
use crate::error::AppResult;

/// Роль участника диалога (провайдеры кодируют её по-разному — домен единый).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatRole {
    System,
    User,
    Assistant,
}

/// Сообщение диалога.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::System,
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::Assistant,
            content: content.into(),
        }
    }
}

/// Учёт токенов, если провайдер его возвращает.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Usage {
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub total_tokens: Option<u32>,
}

/// Запрос диалога к LLM.
#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}

impl ChatRequest {
    pub fn new(model: impl Into<String>, messages: Vec<ChatMessage>) -> Self {
        Self {
            model: model.into(),
            messages,
            temperature: None,
            max_tokens: None,
        }
    }

    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    /// Склеивает system-сообщения в один текст (для провайдеров,
    /// у которых system — отдельное поле: Anthropic, Gemini).
    pub fn system_text(&self) -> Option<String> {
        let parts: Vec<&str> = self
            .messages
            .iter()
            .filter(|m| m.role == ChatRole::System)
            .map(|m| m.content.as_str())
            .filter(|s| !s.trim().is_empty())
            .collect();
        if parts.is_empty() {
            None
        } else {
            Some(parts.join("\n\n"))
        }
    }

    /// Сообщения диалога без system ( system уходит в отдельное поле).
    pub fn dialogue(&self) -> Vec<&ChatMessage> {
        self.messages
            .iter()
            .filter(|m| m.role != ChatRole::System)
            .collect()
    }
}

/// Ответ LLM.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatResponse {
    pub content: String,
    pub model: String,
    pub finish_reason: Option<String>,
    pub usage: Usage,
}

/// Аудиофайл для распознавания речи.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioChunk {
    pub bytes: Vec<u8>,
    pub mime_type: String,
    pub filename: String,
}

/// Запрос распознавания речи.
#[derive(Debug, Clone)]
pub struct TranscriptionRequest {
    pub model: String,
    pub audio: AudioChunk,
    pub language: Option<String>,
}

/// Результат распознавания.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transcription {
    pub text: String,
    pub language: Option<String>,
}

/// Запрос синтеза речи.
#[derive(Debug, Clone)]
pub struct SynthesisRequest {
    pub model: String,
    pub voice: String,
    pub text: String,
    /// Кодировка на выходе: `mp3` по умолчанию.
    pub format: String,
}

impl SynthesisRequest {
    pub fn new(
        model: impl Into<String>,
        voice: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        Self {
            model: model.into(),
            voice: voice.into(),
            text: text.into(),
            format: "mp3".to_string(),
        }
    }
}

/// Синтезированная аудиозапись.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SynthesizedAudio {
    pub bytes: Vec<u8>,
    pub mime_type: String,
}

// ─────────────────────────────────────────────────────────────
// Порты
// ─────────────────────────────────────────────────────────────

/// Диалог с языковой моделью.
#[async_trait]
pub trait ChatModel: Send + Sync {
    async fn chat(&self, request: ChatRequest) -> AppResult<ChatResponse>;
}

/// Распознавание речи (аудио → текст).
#[async_trait]
pub trait SpeechToText: Send + Sync {
    async fn transcribe(&self, request: TranscriptionRequest) -> AppResult<Transcription>;
}

/// Синтез речи (текст → аудио).
#[async_trait]
pub trait TextToSpeech: Send + Sync {
    async fn synthesize(&self, request: SynthesisRequest) -> AppResult<SynthesizedAudio>;
}

/// Discovery моделей и «тест соединения» в админке.
#[async_trait]
pub trait ModelCatalog: Send + Sync {
    /// Модели, пригодные для роли (фильтрация — на стороне адаптера).
    async fn list_models(&self, role: ModelRole) -> AppResult<Vec<ModelDescriptor>>;

    /// Проверка доступности endpoint и ключа.
    async fn ping(&self) -> AppResult<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_text_joins_and_skips_blank() {
        let req = ChatRequest::new(
            "m",
            vec![
                ChatMessage::system("Ты — продавец."),
                ChatMessage::system("  "),
                ChatMessage::user("Привет"),
            ],
        );
        assert_eq!(req.system_text().as_deref(), Some("Ты — продавец."));
        assert_eq!(req.dialogue().len(), 1);
        assert_eq!(req.dialogue()[0].role, ChatRole::User);
    }

    #[test]
    fn system_text_none_when_absent() {
        let req = ChatRequest::new("m", vec![ChatMessage::user("hi")]);
        assert!(req.system_text().is_none());
    }

    #[test]
    fn builders_set_optional_fields() {
        let req = ChatRequest::new("m", vec![])
            .with_temperature(0.2)
            .with_max_tokens(256);
        assert_eq!(req.temperature, Some(0.2));
        assert_eq!(req.max_tokens, Some(256));
    }
}
