//! Адаптер ElevenLabs (синтез речи).

use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};

use crate::domain::entities::model::{ModelDescriptor, ModelRole};
use crate::domain::ports::providers::{
    ModelCatalog, SynthesisRequest, SynthesizedAudio, TextToSpeech,
};
use crate::error::{AppError, AppResult};

use super::{http_error, join_url, read_json, transport_error};

const DEFAULT_BASE_URL: &str = "https://api.elevenlabs.io";

/// Модели ElevenLabs, доступные без discovery (стабильный набор).
const TTS_MODELS: &[(&str, &str)] = &[
    ("eleven_multilingual_v2", "Eleven Multilingual v2"),
    ("eleven_flash_v2_5", "Eleven Flash v2.5"),
    ("eleven_turbo_v2_5", "Eleven Turbo v2.5"),
    ("eleven_monolingual_v1", "Eleven Monolingual v1"),
];

/// HTTP-клиент ElevenLabs TTS.
#[derive(Debug)]
pub struct ElevenLabs {
    http: Client,
    base_url: String,
    api_key: Option<String>,
    provider_name: String,
}

impl ElevenLabs {
    pub fn new(
        http: Client,
        base_url: Option<String>,
        api_key: Option<String>,
        provider_name: impl Into<String>,
    ) -> Self {
        let base = base_url
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        Self {
            http,
            base_url: base.trim_end_matches('/').to_string(),
            api_key,
            provider_name: provider_name.into(),
        }
    }

    fn with_auth(&self, req: reqwest::RequestBuilder) -> AppResult<reqwest::RequestBuilder> {
        let key = self
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                AppError::upstream(&self.provider_name, "API-ключ не задан".to_string())
            })?;
        Ok(req.header("xi-api-key", key))
    }

    async fn require_success(&self, resp: reqwest::Response) -> AppResult<reqwest::Response> {
        if resp.status().is_success() {
            Ok(resp)
        } else {
            Err(http_error(&self.provider_name, resp).await)
        }
    }
}

/// `output_format` для POST /v1/text-to-speech.
pub(crate) fn output_format_for(format: &str) -> &str {
    match format {
        "" | "mp3" => "mp3_44100_128",
        "pcm" => "pcm_24000",
        "ulaw_8000" => "ulaw_8000",
        "alaw_8000" => "alaw_8000",
        other if other.contains('_') => other,
        _ => "mp3_44100_128",
    }
}

/// JSON-тело запроса синтеза.
pub(crate) fn synthesis_request_json(request: &SynthesisRequest) -> Value {
    json!({
        "text": request.text,
        "model_id": if request.model.is_empty() { "eleven_multilingual_v2" } else { request.model.as_str() },
    })
}

#[async_trait]
impl TextToSpeech for ElevenLabs {
    async fn synthesize(&self, request: SynthesisRequest) -> AppResult<SynthesizedAudio> {
        let voice = request.voice.trim();
        if voice.is_empty() {
            return Err(AppError::upstream(
                &self.provider_name,
                "не задан голос (voice_id)",
            ));
        }

        let url = join_url(&self.base_url, &format!("v1/text-to-speech/{voice}"));
        let body = synthesis_request_json(&request);
        let format = output_format_for(&request.format);

        let req = self
            .http
            .post(&url)
            .query(&[("output_format", format)])
            .json(&body);
        let req = self.with_auth(req)?;

        let resp = req
            .send()
            .await
            .map_err(|e| transport_error(&self.provider_name, e))?;
        let resp = self.require_success(resp).await?;

        let mime_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("audio/mpeg")
            .to_string();
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| transport_error(&self.provider_name, e))?;

        Ok(SynthesizedAudio {
            bytes: bytes.to_vec(),
            mime_type,
        })
    }
}

#[async_trait]
impl ModelCatalog for ElevenLabs {
    async fn list_models(&self, role: ModelRole) -> AppResult<Vec<ModelDescriptor>> {
        if role != ModelRole::Tts {
            return Ok(Vec::new());
        }
        Ok(TTS_MODELS
            .iter()
            .map(|(key, title)| ModelDescriptor {
                display_name: (*title).to_string(),
                model_key: (*key).to_string(),
                role,
                supports_streaming: false,
                notes: Some("голоса задаются отдельно (voice_id)".into()),
            })
            .collect())
    }

    async fn ping(&self) -> AppResult<()> {
        let url = join_url(&self.base_url, "v1/voices");
        let req = self.http.get(&url);
        let req = self.with_auth(req)?;
        let resp = req
            .send()
            .await
            .map_err(|e| transport_error(&self.provider_name, e))?;
        let resp = self.require_success(resp).await?;
        // Проверяем, что тело вообще похоже на список голосов.
        let _value: Value = read_json(&self.provider_name, resp).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::providers::testkit;
    use axum::{routing::get, routing::post, Json, Router};

    #[test]
    fn output_format_defaults_and_passes_custom() {
        assert_eq!(output_format_for(""), "mp3_44100_128");
        assert_eq!(output_format_for("mp3"), "mp3_44100_128");
        assert_eq!(output_format_for("pcm"), "pcm_24000");
        assert_eq!(output_format_for("mp3_22050_32"), "mp3_22050_32");
    }

    #[test]
    fn synthesis_body_defaults_model() {
        let body =
            synthesis_request_json(&SynthesisRequest::new("", "EXAUkiYmazOVierLJVKR", "Привет"));
        assert_eq!(body["model_id"], "eleven_multilingual_v2");
        assert_eq!(body["text"], "Привет");

        let custom = synthesis_request_json(&SynthesisRequest::new(
            "eleven_flash_v2_5",
            "voice",
            "Текст",
        ));
        assert_eq!(custom["model_id"], "eleven_flash_v2_5");
    }

    #[tokio::test]
    async fn synthesize_against_mock() {
        async fn handler(
            axum::extract::Path(voice): axum::extract::Path<String>,
            Json(body): Json<Value>,
        ) -> ([(axum::http::header::HeaderName, &'static str); 1], Vec<u8>) {
            assert_eq!(voice, "Rachel");
            assert_eq!(body["model_id"], "eleven_multilingual_v2");
            (
                [(axum::http::header::CONTENT_TYPE, "audio/mpeg")],
                b"ID3audio".to_vec(),
            )
        }

        let router = Router::new().route("/v1/text-to-speech/:voice", post(handler));
        let base = testkit::spawn(router).await;
        let adapter = ElevenLabs::new(Client::new(), Some(base), Some("k".into()), "ElevenLabs");

        let audio = adapter
            .synthesize(SynthesisRequest::new(
                "eleven_multilingual_v2",
                "Rachel",
                "Привет",
            ))
            .await
            .unwrap();
        assert_eq!(audio.bytes, b"ID3audio");
        assert_eq!(audio.mime_type, "audio/mpeg");
    }

    #[tokio::test]
    async fn empty_voice_is_rejected_before_http() {
        let adapter = ElevenLabs::new(Client::new(), None, Some("k".into()), "ElevenLabs");
        let err = adapter
            .synthesize(SynthesisRequest::new("m", "  ", "text"))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("голос"));
    }

    #[tokio::test]
    async fn catalog_returns_static_tts_models() {
        let adapter = ElevenLabs::new(Client::new(), None, Some("k".into()), "ElevenLabs");
        let tts = adapter.list_models(ModelRole::Tts).await.unwrap();
        assert!(!tts.is_empty());
        assert!(tts.iter().all(|m| m.role == ModelRole::Tts));
        let llm = adapter.list_models(ModelRole::Llm).await.unwrap();
        assert!(llm.is_empty());
    }

    #[tokio::test]
    async fn ping_hits_voices() {
        async fn handler() -> Json<Value> {
            Json(json!({ "voices": [] }))
        }
        let router = Router::new().route("/v1/voices", get(handler));
        let base = testkit::spawn(router).await;
        let adapter = ElevenLabs::new(Client::new(), Some(base), Some("k".into()), "ElevenLabs");
        adapter.ping().await.unwrap();
    }
}
