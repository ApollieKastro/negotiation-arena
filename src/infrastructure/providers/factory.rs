//! Фабрика провайдеров: `Provider` + расшифрованный ключ → готовые адаптеры.

use std::path::PathBuf;
use std::sync::Arc;

use reqwest::Client;

use crate::domain::entities::model::ProviderKind;
use crate::domain::entities::provider::Provider;
use crate::domain::ports::providers::{ChatModel, ModelCatalog, SpeechToText, TextToSpeech};
use crate::error::{AppError, AppResult};

use super::anthropic::Anthropic;
use super::deepgram::Deepgram;
use super::elevenlabs::ElevenLabs;
use super::gemini::Gemini;
use super::local::{LocalCatalog, LocalModelManager};
use super::local_voice::{LocalSpeechToText, LocalTextToSpeech};
use super::mock::MockChat;
use super::openai_compat::OpenAiCompat;

/// Готовые адаптеры одного провайдера; `None` — роль провайдером не поддерживается.
///
/// `Debug` вручную: trait-объекты (`Arc<dyn …>`) не реализуют `Debug`.
#[derive(Default)]
pub struct ProviderHandle {
    pub chat: Option<Arc<dyn ChatModel>>,
    pub stt: Option<Arc<dyn SpeechToText>>,
    pub tts: Option<Arc<dyn TextToSpeech>>,
    pub catalog: Option<Arc<dyn ModelCatalog>>,
}

impl std::fmt::Debug for ProviderHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderHandle")
            .field("chat", &self.chat.is_some())
            .field("stt", &self.stt.is_some())
            .field("tts", &self.tts.is_some())
            .field("catalog", &self.catalog.is_some())
            .finish()
    }
}

/// Собирает адаптеры по записям провайдера из БД.
///
/// Таймаут клиента — 120 c: STT/TTS могут отвечать долго.
#[derive(Debug)]
pub struct ProviderFactory {
    http: Client,
    models_dir: PathBuf,
}

impl ProviderFactory {
    pub fn new(models_dir: impl Into<PathBuf>) -> AppResult<Self> {
        let http = Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| AppError::internal(format!("HTTP-клиент провайдеров: {e}")))?;
        Ok(Self {
            http,
            models_dir: models_dir.into(),
        })
    }

    pub fn http(&self) -> &Client {
        &self.http
    }

    pub fn models_dir(&self) -> &PathBuf {
        &self.models_dir
    }

    /// Строит handle. `api_key` — уже расшифрованный секрет.
    ///
    /// Ключ обязателен только когда [`Provider::requires_api_key`]:
    /// `Local` / `Mock` и OpenAI-совместимые на локальном `base_url`
    /// (Ollama, LM Studio) работают без ключа.
    pub fn build(&self, provider: &Provider, api_key: Option<&str>) -> AppResult<ProviderHandle> {
        let key = api_key
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        if provider.requires_api_key() && key.is_none() {
            return Err(AppError::upstream(
                &provider.name,
                "API-ключ не задан (расшифровка или запись провайдера повреждены)",
            ));
        }
        if !provider.is_enabled {
            return Err(AppError::upstream(&provider.name, "провайдер отключён"));
        }

        let base = provider.base_url.clone().filter(|s| !s.trim().is_empty());

        let mut handle = ProviderHandle::default();

        match provider.kind {
            ProviderKind::OpenAiCompatible => {
                let base = base
                    .or_else(|| Some("https://api.openai.com/v1".to_string()))
                    .expect("base_url задан по умолчанию");
                let adapter = Arc::new(OpenAiCompat::new(
                    self.http.clone(),
                    base,
                    key,
                    provider.name.clone(),
                ));
                handle.chat = Some(adapter.clone());
                handle.stt = Some(adapter.clone());
                handle.tts = Some(adapter.clone());
                handle.catalog = Some(adapter);
            }
            ProviderKind::Anthropic => {
                let adapter = Arc::new(Anthropic::new(
                    self.http.clone(),
                    base,
                    key,
                    provider.name.clone(),
                ));
                handle.chat = Some(adapter.clone());
                handle.catalog = Some(adapter);
            }
            ProviderKind::Gemini => {
                let adapter = Arc::new(Gemini::new(
                    self.http.clone(),
                    base,
                    key,
                    provider.name.clone(),
                ));
                handle.chat = Some(adapter.clone());
                handle.catalog = Some(adapter);
            }
            ProviderKind::ElevenLabs => {
                let adapter = Arc::new(ElevenLabs::new(
                    self.http.clone(),
                    base,
                    key,
                    provider.name.clone(),
                ));
                handle.tts = Some(adapter.clone());
                handle.catalog = Some(adapter);
            }
            ProviderKind::Deepgram => {
                let adapter = Arc::new(Deepgram::new(
                    self.http.clone(),
                    base,
                    key,
                    provider.name.clone(),
                ));
                handle.stt = Some(adapter.clone());
                handle.tts = Some(adapter.clone());
                handle.catalog = Some(adapter);
            }
            ProviderKind::Local => {
                // Локальный каталог + subprocess STT/TTS (scripts/local_*.py).
                let manager = LocalModelManager::new(self.models_dir.clone(), self.http.clone());
                handle.catalog = Some(Arc::new(LocalCatalog::new(manager)));
                handle.stt = Some(Arc::new(LocalSpeechToText::new(
                    self.models_dir.clone(),
                    provider.name.clone(),
                )));
                handle.tts = Some(Arc::new(LocalTextToSpeech::new(
                    self.models_dir.clone(),
                    provider.name.clone(),
                )));
            }
            ProviderKind::Mock => {
                // Офлайн-режим: ключ не нужен, chat + catalog для демо и разработки.
                let chat = Arc::new(MockChat::new(provider.name.clone()));
                handle.catalog = Some(Arc::new(chat.catalog()));
                handle.chat = Some(chat);
            }
        }

        Ok(handle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entities::model::ProviderKind;

    fn provider(kind: ProviderKind, enabled: bool) -> Provider {
        Provider {
            id: "p1".into(),
            name: "Test".into(),
            kind,
            base_url: None,
            api_key_encrypted: None,
            api_key_hint: None,
            is_enabled: enabled,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: None,
        }
    }

    fn provider_with_base(kind: ProviderKind, base: &str) -> Provider {
        let mut p = provider(kind, true);
        p.base_url = Some(base.to_string());
        p
    }

    #[test]
    fn openai_compatible_exposes_all_roles() {
        let factory = ProviderFactory::new("models").unwrap();
        let handle = factory
            .build(
                &provider(ProviderKind::OpenAiCompatible, true),
                Some("sk-x"),
            )
            .unwrap();
        assert!(handle.chat.is_some());
        assert!(handle.stt.is_some());
        assert!(handle.tts.is_some());
        assert!(handle.catalog.is_some());
    }

    #[test]
    fn anthropic_has_chat_only() {
        let factory = ProviderFactory::new("models").unwrap();
        let handle = factory
            .build(&provider(ProviderKind::Anthropic, true), Some("sk-ant"))
            .unwrap();
        assert!(handle.chat.is_some());
        assert!(handle.stt.is_none());
        assert!(handle.tts.is_none());
        assert!(handle.catalog.is_some());
    }

    #[test]
    fn elevenlabs_has_tts_only() {
        let factory = ProviderFactory::new("models").unwrap();
        let handle = factory
            .build(&provider(ProviderKind::ElevenLabs, true), Some("xi-key"))
            .unwrap();
        assert!(handle.chat.is_none());
        assert!(handle.stt.is_none());
        assert!(handle.tts.is_some());
    }

    #[test]
    fn deepgram_has_stt_and_tts() {
        let factory = ProviderFactory::new("models").unwrap();
        let handle = factory
            .build(&provider(ProviderKind::Deepgram, true), Some("dg-key"))
            .unwrap();
        assert!(handle.chat.is_none());
        assert!(handle.stt.is_some());
        assert!(handle.tts.is_some());
    }

    #[test]
    fn local_needs_no_key_and_exposes_stt_tts_catalog() {
        let factory = ProviderFactory::new("models").unwrap();
        let handle = factory
            .build(&provider(ProviderKind::Local, true), None)
            .unwrap();
        assert!(handle.chat.is_none());
        assert!(handle.stt.is_some());
        assert!(handle.tts.is_some());
        assert!(handle.catalog.is_some());
    }

    #[test]
    fn mock_has_chat_and_catalog_without_key() {
        let factory = ProviderFactory::new("models").unwrap();
        let handle = factory
            .build(&provider(ProviderKind::Mock, true), None)
            .unwrap();
        assert!(handle.chat.is_some());
        assert!(handle.catalog.is_some());
        assert!(handle.stt.is_none());
        assert!(handle.tts.is_none());
    }

    #[test]
    fn missing_key_for_cloud_is_error() {
        let factory = ProviderFactory::new("models").unwrap();
        let err = factory
            .build(&provider(ProviderKind::OpenAiCompatible, true), None)
            .unwrap_err();
        assert!(err.to_string().contains("API-ключ"));
    }

    #[test]
    fn local_ollama_openai_compat_builds_without_key() {
        let factory = ProviderFactory::new("models").unwrap();
        let handle = factory
            .build(
                &provider_with_base(ProviderKind::OpenAiCompatible, "http://127.0.0.1:11434/v1"),
                None,
            )
            .unwrap();
        assert!(handle.chat.is_some());
        assert!(handle.catalog.is_some());
    }

    #[test]
    fn disabled_provider_is_error() {
        let factory = ProviderFactory::new("models").unwrap();
        let err = factory
            .build(&provider(ProviderKind::OpenAiCompatible, false), Some("sk"))
            .unwrap_err();
        assert!(err.to_string().contains("отключён"));
    }
}
