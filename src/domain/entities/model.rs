//! Модели ИИ и провайдеры: доменные типы для трёх ролей — LLM, STT, TTS.

use serde::{Deserialize, Serialize};

/// Роль модели в приложении.
///
/// Ровно три роли, которые администратор назначает в настройках:
/// `Llm` — «голова» диалога, `Stt` — распознавание речи, `Tts` — синтез речи.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRole {
    Llm,
    Stt,
    Tts,
}

impl ModelRole {
    pub const ALL: [ModelRole; 3] = [ModelRole::Llm, ModelRole::Stt, ModelRole::Tts];

    pub fn slug(&self) -> &'static str {
        match self {
            ModelRole::Llm => "llm",
            ModelRole::Stt => "stt",
            ModelRole::Tts => "tts",
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            ModelRole::Llm => "Диалог (LLM)",
            ModelRole::Stt => "Распознавание речи (STT)",
            ModelRole::Tts => "Синтез речи (TTS)",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            ModelRole::Llm => "Модель, разговаривающая с пользователем от лица собеседника",
            ModelRole::Stt => "Превращает речь пользователя в текст (голосовой режим)",
            ModelRole::Tts => "Озвучивает реплики собеседника",
        }
    }

    pub fn from_slug(slug: &str) -> Option<Self> {
        match slug {
            "llm" => Some(ModelRole::Llm),
            "stt" => Some(ModelRole::Stt),
            "tts" => Some(ModelRole::Tts),
            _ => None,
        }
    }
}

/// Тип провайдера. Определяет, какой адаптер используется для вызовов API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    /// Любой провайдер с OpenAI-совместимым API (OpenAI, Groq, OpenRouter,
    /// Together, Mistral, DeepSeek, Ollama, LM Studio, vLLM, LocalAI...).
    OpenAiCompatible,
    /// Anthropic Messages API.
    Anthropic,
    /// Google Gemini API.
    Gemini,
    /// Локальный запуск через внешнюю команду/сервис (whisper.cpp, Piper и т.п.).
    Local,
}

impl ProviderKind {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "openai_compatible" => Some(ProviderKind::OpenAiCompatible),
            "anthropic" => Some(ProviderKind::Anthropic),
            "gemini" => Some(ProviderKind::Gemini),
            "local" => Some(ProviderKind::Local),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderKind::OpenAiCompatible => "openai_compatible",
            ProviderKind::Anthropic => "anthropic",
            ProviderKind::Gemini => "gemini",
            ProviderKind::Local => "local",
        }
    }

    /// Все варианты для выпадающего списка в админке.
    pub fn all() -> &'static [ProviderKind] {
        &[
            ProviderKind::OpenAiCompatible,
            ProviderKind::Anthropic,
            ProviderKind::Gemini,
            ProviderKind::Local,
        ]
    }

    pub fn title(&self) -> &'static str {
        match self {
            ProviderKind::OpenAiCompatible => "OpenAI-совместимый",
            ProviderKind::Anthropic => "Anthropic",
            ProviderKind::Gemini => "Google Gemini",
            ProviderKind::Local => "Локальный",
        }
    }

    /// API-ключ не нужен (локальные модели работают без ключа).
    pub fn requires_api_key(&self) -> bool {
        !matches!(self, ProviderKind::Local)
    }
}

/// Описание модели, полученное от провайдера (discovery) или созданное вручную.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelDescriptor {
    /// Идентификатор модели у провайдера (например `gpt-4o-mini`).
    pub model_key: String,
    /// Человекочитаемое название.
    pub display_name: String,
    /// Роль, для которой подходит модель.
    pub role: ModelRole,
    /// Поддерживается ли потоковая генерация (если провайдер сообщает).
    #[serde(default)]
    pub supports_streaming: bool,
    /// Дополнительные сведения: размер локальной модели, голоса, заметки.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}
