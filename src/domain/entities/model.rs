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
///
/// JSON-имена совпадают с [`ProviderKind::as_str`] / [`ProviderKind::from_str`]
/// (и со значением в БД), чтобы API и хранилище не расходились.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderKind {
    /// Любой провайдер с OpenAI-совместимым API (OpenAI, Groq, OpenRouter,
    /// Together, Mistral, DeepSeek, Ollama, LM Studio, vLLM, LocalAI...).
    #[serde(rename = "openai_compatible")]
    OpenAiCompatible,
    /// Anthropic Messages API.
    #[serde(rename = "anthropic")]
    Anthropic,
    /// Google Gemini API.
    #[serde(rename = "gemini")]
    Gemini,
    /// ElevenLabs (синтез речи).
    #[serde(rename = "elevenlabs")]
    ElevenLabs,
    /// Deepgram (распознавание и синтез речи).
    #[serde(rename = "deepgram")]
    Deepgram,
    /// Локальный запуск через внешнюю команду/сервис (whisper.cpp, Piper и т.п.).
    #[serde(rename = "local")]
    Local,
    /// Демо-режим без API-ключа: офлайн-собеседник и генератор сценариев
    /// для демонстрации и разработки, когда ключи ещё не подключены.
    #[serde(rename = "mock")]
    Mock,
}

impl ProviderKind {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "openai_compatible" => Some(ProviderKind::OpenAiCompatible),
            "anthropic" => Some(ProviderKind::Anthropic),
            "gemini" => Some(ProviderKind::Gemini),
            "elevenlabs" => Some(ProviderKind::ElevenLabs),
            "deepgram" => Some(ProviderKind::Deepgram),
            "local" => Some(ProviderKind::Local),
            "mock" => Some(ProviderKind::Mock),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderKind::OpenAiCompatible => "openai_compatible",
            ProviderKind::Anthropic => "anthropic",
            ProviderKind::Gemini => "gemini",
            ProviderKind::ElevenLabs => "elevenlabs",
            ProviderKind::Deepgram => "deepgram",
            ProviderKind::Local => "local",
            ProviderKind::Mock => "mock",
        }
    }

    /// Все варианты для выпадающего списка в админке.
    pub fn all() -> &'static [ProviderKind] {
        &[
            ProviderKind::OpenAiCompatible,
            ProviderKind::Anthropic,
            ProviderKind::Gemini,
            ProviderKind::ElevenLabs,
            ProviderKind::Deepgram,
            ProviderKind::Local,
            ProviderKind::Mock,
        ]
    }

    pub fn title(&self) -> &'static str {
        match self {
            ProviderKind::OpenAiCompatible => "OpenAI-совместимый",
            ProviderKind::Anthropic => "Anthropic",
            ProviderKind::Gemini => "Google Gemini",
            ProviderKind::ElevenLabs => "ElevenLabs",
            ProviderKind::Deepgram => "Deepgram",
            ProviderKind::Local => "Локальный",
            ProviderKind::Mock => "Демо (офлайн)",
        }
    }

    /// API-ключ не нужен (локальные модели и демо-режим работают без ключа).
    pub fn requires_api_key(&self) -> bool {
        !matches!(self, ProviderKind::Local | ProviderKind::Mock)
    }

    /// Провайдер умеет диалог (LLM-порт доступен).
    pub fn supports_chat(&self) -> bool {
        matches!(
            self,
            ProviderKind::OpenAiCompatible
                | ProviderKind::Anthropic
                | ProviderKind::Gemini
                | ProviderKind::Mock
        )
    }

    /// Провайдер умеет распознавание речи.
    pub fn supports_stt(&self) -> bool {
        matches!(
            self,
            ProviderKind::OpenAiCompatible | ProviderKind::Deepgram | ProviderKind::Local
        )
    }

    /// Провайдер умеет синтез речи.
    pub fn supports_tts(&self) -> bool {
        matches!(
            self,
            ProviderKind::OpenAiCompatible
                | ProviderKind::ElevenLabs
                | ProviderKind::Deepgram
                | ProviderKind::Local
        )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_kind_roundtrips_through_str() {
        for kind in ProviderKind::all() {
            let parsed = ProviderKind::from_str(kind.as_str())
                .unwrap_or_else(|| panic!("не распознан вид {:?}", kind.as_str()));
            assert_eq!(parsed, *kind);
        }
        assert!(ProviderKind::from_str("unknown").is_none());
    }

    #[test]
    fn capability_matrix_matches_stage_2() {
        assert!(ProviderKind::OpenAiCompatible.supports_chat());
        assert!(ProviderKind::OpenAiCompatible.supports_stt());
        assert!(ProviderKind::OpenAiCompatible.supports_tts());
        assert!(ProviderKind::Anthropic.supports_chat());
        assert!(!ProviderKind::Anthropic.supports_stt());
        assert!(!ProviderKind::Anthropic.supports_tts());
        assert!(ProviderKind::Gemini.supports_chat());
        assert!(!ProviderKind::ElevenLabs.supports_chat());
        assert!(ProviderKind::ElevenLabs.supports_tts());
        assert!(ProviderKind::Deepgram.supports_stt());
        assert!(ProviderKind::Deepgram.supports_tts());
        assert!(!ProviderKind::Local.requires_api_key());
        assert!(ProviderKind::Mock.supports_chat());
        assert!(!ProviderKind::Mock.requires_api_key());
    }
}
