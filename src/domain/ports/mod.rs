//! Порты (контракты) домена.
//!
//! Это «гнёзда», в которые подставляются адаптеры провайдеров из
//! `infrastructure::providers`. Бизнес-логика не знает, кто именно
//! отвечает: Groq, OpenAI, Anthropic, Gemini или локальный whisper.

use async_trait::async_trait;

use crate::domain::entities::model::{ModelDescriptor, ModelRole};
use crate::error::AppResult;

// ─────────────────────────────────────────────────────────────
// Чат (LLM — «голова» диалога)
// ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatRole {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone, Default)]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}

impl ChatRequest {
    pub fn new(messages: Vec<ChatMessage>) -> Self {
        Self {
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
}

#[derive(Debug, Clone)]
pub struct ChatResponse {
    pub content: String,
    /// Фактически использованная модель, если провайдер её сообщает.
    pub model: Option<String>,
}

// ─────────────────────────────────────────────────────────────
// STT — распознавание речи
// ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AudioInput {
    pub bytes: Vec<u8>,
    /// MIME-тип (`audio/webm`, `audio/wav`, `audio/mpeg`...).
    pub mime_type: String,
    /// Исходное имя файла, если есть (для локальных обработчиков).
    pub filename: Option<String>,
}

impl AudioInput {
    pub fn new(bytes: Vec<u8>, mime_type: impl Into<String>) -> Self {
        Self {
            bytes,
            mime_type: mime_type.into(),
            filename: None,
        }
    }

    pub fn with_filename(mut self, filename: impl Into<String>) -> Self {
        self.filename = Some(filename.into());
        self
    }
}

#[derive(Debug, Clone)]
pub struct Transcript {
    pub text: String,
    /// Язык, определённый моделью, если провайдер его сообщает.
    pub language: Option<String>,
}

// ─────────────────────────────────────────────────────────────
// TTS — синтез речи
// ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct TtsRequest {
    pub text: String,
    /// Голос, если у модели есть выбор.
    pub voice: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AudioOutput {
    pub bytes: Vec<u8>,
    /// MIME-тип результата (`audio/mpeg`, `audio/wav`...).
    pub mime_type: String,
}

// ─────────────────────────────────────────────────────────────
// Порты
// ─────────────────────────────────────────────────────────────

/// Модель для диалога (роль LLM).
#[async_trait]
pub trait ChatModel: Send + Sync {
    /// Идентификатор модели у провайдера (для логов и трассировки).
    fn model_key(&self) -> &str;

    async fn complete(&self, request: ChatRequest) -> AppResult<ChatResponse>;
}

/// Речь → текст (роль STT).
#[async_trait]
pub trait SpeechToText: Send + Sync {
    fn model_key(&self) -> &str;

    async fn transcribe(&self, input: AudioInput) -> AppResult<Transcript>;
}

/// Текст → речь (роль TTS).
#[async_trait]
pub trait TextToSpeech: Send + Sync {
    fn model_key(&self) -> &str;

    async fn synthesize(&self, request: TtsRequest) -> AppResult<AudioOutput>;

    /// Голоса модели (пусто, если выбора нет).
    async fn voices(&self) -> AppResult<Vec<String>> {
        Ok(Vec::new())
    }
}

/// Способности провайдера: перечисление моделей и проверка соединения.
/// Реализуется адаптером, у которого есть base_url + ключ.
#[async_trait]
pub trait ModelCatalog: Send + Sync {
    /// Модели провайдера для заданной роли (discovery по API-ключу).
    async fn list_models(&self, role: ModelRole) -> AppResult<Vec<ModelDescriptor>>;

    /// Проверка, что ключ и endpoint живые.
    async fn health_check(&self) -> AppResult<()>;
}
