//! Голосовые операции: синтез речи (TTS) и распознавание (STT).
//!
//! Оба метода сначала валидируют вход, затем резолвят назначенную модель роли
//! через [`ProviderService::resolve_tts_for`] / [`ProviderService::resolve_stt_for`]
//! (предпочтение пользователя → глобальное назначение) и вызывают адаптер.

use std::sync::Arc;

use crate::application::provider::ProviderService;
use crate::domain::ports::providers::{AudioChunk, SynthesisRequest, TranscriptionRequest};
use crate::error::{AppError, AppResult};

/// Максимальная длина текста для синтеза в символах.
const MAX_TTS_TEXT_CHARS: usize = 2000;

/// Максимальный размер аудио для распознавания: 12 МБ.
const MAX_STT_BYTES: usize = 12 * 1024 * 1024;

/// Голос по умолчанию (OpenAI-совместимые TTS; Deepgram голос не использует,
/// ElevenLabs требует передать `voice_id` явно).
const DEFAULT_VOICE: &str = "alloy";

/// Синтез и распознавание речи поверх назначенных голосовых моделей.
pub struct VoiceService {
    providers: Arc<ProviderService>,
}

impl VoiceService {
    pub fn new(providers: Arc<ProviderService>) -> Self {
        Self { providers }
    }

    /// Текст → аудио (TTS). Возвращает `(bytes, mime_type)`.
    ///
    /// Текст: не пустой, не длиннее [`MAX_TTS_TEXT_CHARS`] символов.
    /// `voice: None` или пустая строка → [`DEFAULT_VOICE`].
    pub async fn synthesize(
        &self,
        user_id: &str,
        text: &str,
        voice: Option<&str>,
    ) -> AppResult<(Vec<u8>, String)> {
        let text = text.trim();
        if text.is_empty() {
            return Err(AppError::BadRequest(
                "текст для синтеза не может быть пустым".into(),
            ));
        }
        if text.chars().count() > MAX_TTS_TEXT_CHARS {
            return Err(AppError::BadRequest(format!(
                "текст для синтеза не длиннее {MAX_TTS_TEXT_CHARS} символов"
            )));
        }

        let (tts, model_key) = self.providers.resolve_tts_for(user_id).await?;
        let voice = voice
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(DEFAULT_VOICE);
        let audio = tts
            .synthesize(SynthesisRequest::new(model_key, voice, text))
            .await?;
        Ok((audio.bytes, audio.mime_type))
    }

    /// Аудио → текст (STT). Возвращает распознанный текст.
    ///
    /// MIME должен начинаться с `audio/`, размер — не больше [`MAX_STT_BYTES`].
    pub async fn transcribe(
        &self,
        user_id: &str,
        filename: &str,
        mime: &str,
        bytes: Vec<u8>,
    ) -> AppResult<String> {
        if !mime.starts_with("audio/") {
            return Err(AppError::BadRequest(format!(
                "ожидается аудио (audio/*), получен MIME: {mime}"
            )));
        }
        if bytes.len() > MAX_STT_BYTES {
            return Err(AppError::BadRequest(format!(
                "аудио не больше {} МБ",
                MAX_STT_BYTES / (1024 * 1024)
            )));
        }

        let (stt, model_key) = self.providers.resolve_stt_for(user_id).await?;
        let filename = if filename.trim().is_empty() {
            "audio"
        } else {
            filename
        };
        let result = stt
            .transcribe(TranscriptionRequest {
                model: model_key,
                audio: AudioChunk {
                    bytes,
                    mime_type: mime.to_string(),
                    filename: filename.to_string(),
                },
                // Домен приложения — русский; локальный whisper/piper ждут ru.
                language: Some("ru".into()),
            })
            .await?;
        let text = result.text.trim().to_string();
        if text.is_empty() {
            return Err(AppError::BadRequest(
                "Речь не распознана — говорите громче, дольше и ближе к микрофону".into(),
            ));
        }
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::testsupport::setup;

    // До резолва: валидация входа не требует настроенных провайдеров.

    #[tokio::test]
    async fn synthesize_rejects_empty_and_blank_text() {
        let (_db, svc) = setup().unwrap();
        let err = svc.voice.synthesize("u1", "", None).await.unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)), "{err}");
        let err = svc
            .voice
            .synthesize("u1", "   \n ", None)
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)), "{err}");
    }

    #[tokio::test]
    async fn synthesize_rejects_overlong_text() {
        let (_db, svc) = setup().unwrap();
        let long = "ы".repeat(MAX_TTS_TEXT_CHARS + 1);
        let err = svc.voice.synthesize("u1", &long, None).await.unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)), "{err}");

        // Ровно по лимиту проходит валидацию (упирается в «не настроен»).
        let exact = "ы".repeat(MAX_TTS_TEXT_CHARS);
        let err = svc.voice.synthesize("u1", &exact, None).await.unwrap_err();
        assert!(matches!(err, AppError::ServiceUnavailable(_)), "{err}");
    }

    #[tokio::test]
    async fn synthesize_without_tts_config_is_service_unavailable() {
        let (_db, svc) = setup().unwrap();
        let err = svc
            .voice
            .synthesize("u1", "Привет", None)
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::ServiceUnavailable(_)), "{err}");
        let msg = err.to_string();
        assert!(msg.contains("TTS"), "сообщение про TTS: {msg}");
        assert!(msg.contains("не настроен"), "{msg}");
    }

    #[tokio::test]
    async fn transcribe_rejects_non_audio_mime_and_empty_ok_audio() {
        let (_db, svc) = setup().unwrap();
        let err = svc
            .voice
            .transcribe("u1", "a.txt", "text/plain", b"x".to_vec())
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)), "{err}");

        // audio/* проходит MIME-проверку и упирается в «не настроен».
        let err = svc
            .voice
            .transcribe("u1", "a.wav", "audio/wav", b"RIFF".to_vec())
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::ServiceUnavailable(_)), "{err}");
        let msg = err.to_string();
        assert!(msg.contains("STT"), "сообщение про STT: {msg}");
    }

    #[tokio::test]
    async fn transcribe_rejects_oversize_audio() {
        let (_db, svc) = setup().unwrap();
        let big = vec![0u8; MAX_STT_BYTES + 1];
        let err = svc
            .voice
            .transcribe("u1", "a.wav", "audio/wav", big)
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)), "{err}");
        assert!(err.to_string().contains("12 МБ"), "{err}");
    }
}
