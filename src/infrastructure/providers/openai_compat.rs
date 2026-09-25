//! OpenAI-совместимый адаптер: Chat Completions + audio/transcriptions + audio/speech.
//!
//! Покрывает OpenAI, Groq, OpenRouter, Together, Ollama, LM Studio, vLLM и т.п.

use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};

use crate::domain::entities::model::{ModelDescriptor, ModelRole};
use crate::domain::ports::providers::{
    ChatMessage, ChatModel, ChatRequest, ChatResponse, ModelCatalog, SpeechToText,
    SynthesisRequest, SynthesizedAudio, TextToSpeech, Transcription, TranscriptionRequest, Usage,
};
use crate::error::{AppError, AppResult};

use super::{http_error, join_url, read_json, transport_error};

/// HTTP-клиент для любого OpenAI-совместимого endpoint.
#[derive(Debug)]
pub struct OpenAiCompat {
    http: Client,
    base_url: String,
    api_key: Option<String>,
    provider_name: String,
}

impl OpenAiCompat {
    pub fn new(
        http: Client,
        base_url: impl Into<String>,
        api_key: Option<String>,
        provider_name: impl Into<String>,
    ) -> Self {
        Self {
            http,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key,
            provider_name: provider_name.into(),
        }
    }

    fn bearer(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.api_key {
            Some(key) if !key.trim().is_empty() => req.bearer_auth(key),
            _ => req,
        }
    }

    async fn require_success(&self, resp: reqwest::Response) -> AppResult<reqwest::Response> {
        if resp.status().is_success() {
            Ok(resp)
        } else {
            Err(http_error(&self.provider_name, resp).await)
        }
    }
}

fn role_str(role: crate::domain::ports::providers::ChatRole) -> &'static str {
    use crate::domain::ports::providers::ChatRole;
    match role {
        ChatRole::System => "system",
        ChatRole::User => "user",
        ChatRole::Assistant => "assistant",
    }
}

/// Собирает JSON-тело `POST /chat/completions`.
pub(crate) fn chat_request_json(request: &ChatRequest) -> Value {
    let messages: Vec<Value> = request
        .messages
        .iter()
        .map(|m: &ChatMessage| json!({ "role": role_str(m.role), "content": m.content }))
        .collect();

    let mut body = json!({ "model": request.model, "messages": messages });
    if let Some(temperature) = request.temperature {
        body["temperature"] = json!(temperature);
    }
    if let Some(max_tokens) = request.max_tokens {
        body["max_tokens"] = json!(max_tokens);
    }
    body
}

/// Разбирает ответ `POST /chat/completions`.
pub(crate) fn chat_response_json(value: &Value) -> AppResult<ChatResponse> {
    let choice = value
        .pointer("/choices/0")
        .ok_or_else(|| AppError::upstream("openai", "в ответе нет choices"))?;

    let content = choice
        .pointer("/message/content")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let finish_reason = choice
        .get("finish_reason")
        .and_then(Value::as_str)
        .map(str::to_string);

    let model = value
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let usage = Usage {
        prompt_tokens: value
            .pointer("/usage/prompt_tokens")
            .and_then(Value::as_u64)
            .map(|v| v as u32),
        completion_tokens: value
            .pointer("/usage/completion_tokens")
            .and_then(Value::as_u64)
            .map(|v| v as u32),
        total_tokens: value
            .pointer("/usage/total_tokens")
            .and_then(Value::as_u64)
            .map(|v| v as u32),
    };

    Ok(ChatResponse {
        content,
        model,
        finish_reason,
        usage,
    })
}

/// Эвристический фильтр `/models` по роли (OpenAI-совместимые не отдают роль явно).
pub(crate) fn matches_role(model_key: &str, role: ModelRole) -> bool {
    let key = model_key.to_lowercase();
    match role {
        ModelRole::Llm => {
            !key.contains("whisper")
                && !key.contains("tts")
                && !key.contains("speech-to-text")
                && !key.contains("text-to-speech")
                && !key.contains("embedding")
                && !key.contains("dall-e")
                && !key.contains("moderation")
                && !key.contains("transcribe")
                && !key.contains("stable-diffusion")
                && !key.contains("image")
        }
        ModelRole::Stt => {
            key.contains("whisper")
                || key.contains("stt")
                || key.contains("speech-to-text")
                || key.contains("transcribe")
                || key.contains("paraformer")
                || key.contains("distil-whisper")
                || key.contains("nemotron")
                || key.contains("parakeet")
                || key.contains("asr")
                || key.contains("fastconformer")
        }
        ModelRole::Tts => {
            key.contains("tts")
                || key.contains("text-to-speech")
                || key.contains("playai-tts")
                || (key.contains("voice") && !key.contains("whisper"))
        }
    }
}

#[async_trait]
impl ChatModel for OpenAiCompat {
    async fn chat(&self, request: ChatRequest) -> AppResult<ChatResponse> {
        let url = join_url(&self.base_url, "chat/completions");
        let body = chat_request_json(&request);

        let resp = self
            .bearer(self.http.post(&url).json(&body))
            .send()
            .await
            .map_err(|e| transport_error(&self.provider_name, e))?;
        let resp = self.require_success(resp).await?;
        let value = read_json(&self.provider_name, resp).await?;
        chat_response_json(&value)
    }
}

#[async_trait]
impl SpeechToText for OpenAiCompat {
    async fn transcribe(&self, request: TranscriptionRequest) -> AppResult<Transcription> {
        let url = join_url(&self.base_url, "audio/transcriptions");

        let file_part = reqwest::multipart::Part::bytes(request.audio.bytes)
            .file_name(request.audio.filename)
            .mime_str(&request.audio.mime_type)
            .map_err(|e| AppError::upstream(&self.provider_name, format!("MIME: {e}")))?;

        let mut form = reqwest::multipart::Form::new()
            .part("file", file_part)
            .text("model", request.model)
            .text("response_format", "json");
        if let Some(language) = request.language {
            form = form.text("language", language);
        }

        let resp = self
            .bearer(self.http.post(&url).multipart(form))
            .send()
            .await
            .map_err(|e| transport_error(&self.provider_name, e))?;
        let resp = self.require_success(resp).await?;
        let value = read_json(&self.provider_name, resp).await?;

        let text = value
            .get("text")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::upstream(&self.provider_name, "в ответе нет text"))?
            .to_string();
        let language = value
            .get("language")
            .and_then(Value::as_str)
            .map(str::to_string);

        Ok(Transcription { text, language })
    }
}

#[async_trait]
impl TextToSpeech for OpenAiCompat {
    async fn synthesize(&self, request: SynthesisRequest) -> AppResult<SynthesizedAudio> {
        let url = join_url(&self.base_url, "audio/speech");
        let body = json!({
            "model": request.model,
            "voice": request.voice,
            "input": request.text,
            "response_format": if request.format.is_empty() { "mp3" } else { request.format.as_str() },
        });

        let resp = self
            .bearer(self.http.post(&url).json(&body))
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
impl ModelCatalog for OpenAiCompat {
    async fn list_models(&self, role: ModelRole) -> AppResult<Vec<ModelDescriptor>> {
        let url = join_url(&self.base_url, "models");
        let resp = self
            .bearer(self.http.get(&url))
            .send()
            .await
            .map_err(|e| transport_error(&self.provider_name, e))?;
        let resp = self.require_success(resp).await?;
        let value = read_json(&self.provider_name, resp).await?;

        let ids: Vec<String> = value
            .pointer("/data")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.get("id").and_then(Value::as_str).map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();

        Ok(ids
            .into_iter()
            .filter(|id| matches_role(id, role))
            .map(|id| ModelDescriptor {
                display_name: id.clone(),
                model_key: id,
                role,
                supports_streaming: role == ModelRole::Llm,
                notes: None,
            })
            .collect())
    }

    async fn ping(&self) -> AppResult<()> {
        let url = join_url(&self.base_url, "models");
        let resp = self
            .bearer(self.http.get(&url))
            .send()
            .await
            .map_err(|e| transport_error(&self.provider_name, e))?;
        self.require_success(resp).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::providers::testkit;
    use axum::{extract::Multipart, routing::post, Json, Router};

    #[test]
    fn chat_request_includes_optionals_only_when_set() {
        let plain = chat_request_json(&ChatRequest::new(
            "llama-3.3-70b",
            vec![ChatMessage::user("Привет")],
        ));
        assert_eq!(plain["model"], "llama-3.3-70b");
        assert!(plain.get("temperature").is_none());
        assert!(plain.get("max_tokens").is_none());
        assert_eq!(plain["messages"][0]["role"], "user");

        let rich = chat_request_json(
            &ChatRequest::new(
                "gpt-4o-mini",
                vec![
                    ChatMessage::system("Ты — продавец."),
                    ChatMessage::user("Здравствуйте"),
                    ChatMessage::assistant("Добрый день!"),
                ],
            )
            .with_temperature(0.4)
            .with_max_tokens(512),
        );
        assert_eq!(rich["model"], "gpt-4o-mini");
        assert!((rich["temperature"].as_f64().unwrap() - 0.4).abs() < 1e-6);
        assert_eq!(rich["max_tokens"], 512);
        assert_eq!(rich["messages"][0]["role"], "system");
        assert_eq!(rich["messages"][1]["role"], "user");
        assert_eq!(rich["messages"][2]["role"], "assistant");
    }

    #[test]
    fn chat_response_parses_usage_and_finish() {
        let value = json!({
            "model": "gpt-4o-mini",
            "choices": [{
                "message": { "role": "assistant", "content": "Отвечаю" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 12, "completion_tokens": 34, "total_tokens": 46 }
        });
        let parsed = chat_response_json(&value).unwrap();
        assert_eq!(parsed.content, "Отвечаю");
        assert_eq!(parsed.model, "gpt-4o-mini");
        assert_eq!(parsed.finish_reason.as_deref(), Some("stop"));
        assert_eq!(parsed.usage.prompt_tokens, Some(12));
        assert_eq!(parsed.usage.total_tokens, Some(46));
    }

    #[test]
    fn chat_response_without_choices_is_error() {
        assert!(chat_response_json(&json!({ "model": "x" })).is_err());
    }

    #[test]
    fn role_filter_splits_llm_stt_tts() {
        assert!(matches_role("llama-3.3-70b-versatile", ModelRole::Llm));
        assert!(!matches_role("whisper-large-v3", ModelRole::Llm));
        assert!(matches_role("whisper-large-v3", ModelRole::Stt));
        assert!(!matches_role("llama-3.3-70b", ModelRole::Stt));
        assert!(matches_role("tts-1", ModelRole::Tts));
        assert!(matches_role("playai-tts", ModelRole::Tts));
        assert!(!matches_role("llama-3.3-70b", ModelRole::Tts));
    }

    #[tokio::test]
    async fn chat_against_mock_server() {
        async fn handler(Json(body): Json<Value>) -> Json<Value> {
            assert_eq!(body["model"], "test-model");
            Json(json!({
                "model": "test-model",
                "choices": [{
                    "message": { "role": "assistant", "content": "pong" },
                    "finish_reason": "stop"
                }],
                "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 }
            }))
        }

        let router = Router::new().route("/v1/chat/completions", post(handler));
        let base = testkit::spawn(router).await;
        let adapter = OpenAiCompat::new(
            Client::new(),
            format!("{base}/v1"),
            Some("test-key".into()),
            "Mock",
        );

        let resp = adapter
            .chat(
                ChatRequest::new("test-model", vec![ChatMessage::user("ping")])
                    .with_temperature(0.1),
            )
            .await
            .unwrap();
        assert_eq!(resp.content, "pong");
        assert_eq!(resp.usage.total_tokens, Some(2));
    }

    #[tokio::test]
    async fn transcribe_against_mock_server() {
        async fn handler(mut multipart: Multipart) -> Json<Value> {
            let mut saw_file = false;
            let mut saw_model = false;
            while let Some(field) = multipart.next_field().await.unwrap() {
                let name = field.name().unwrap_or_default().to_string();
                if name == "file" {
                    saw_file = true;
                }
                if name == "model" {
                    saw_model = field.text().await.unwrap() == "whisper-1";
                }
            }
            assert!(saw_file);
            assert!(saw_model);
            Json(json!({ "text": "привет", "language": "ru" }))
        }

        let router = Router::new().route("/v1/audio/transcriptions", post(handler));
        let base = testkit::spawn(router).await;
        let adapter = OpenAiCompat::new(
            Client::new(),
            format!("{base}/v1"),
            Some("test-key".into()),
            "Mock",
        );

        let result = adapter
            .transcribe(TranscriptionRequest {
                model: "whisper-1".into(),
                audio: crate::domain::ports::providers::AudioChunk {
                    bytes: b"RIFFdata".to_vec(),
                    mime_type: "audio/wav".into(),
                    filename: "clip.wav".into(),
                },
                language: Some("ru".into()),
            })
            .await
            .unwrap();
        assert_eq!(result.text, "привет");
        assert_eq!(result.language.as_deref(), Some("ru"));
    }

    #[tokio::test]
    async fn error_body_is_extracted_into_upstream() {
        async fn handler() -> (axum::http::StatusCode, Json<Value>) {
            (
                axum::http::StatusCode::UNAUTHORIZED,
                Json(json!({ "error": { "message": "Invalid API key" } })),
            )
        }

        let router = Router::new().route("/v1/models", axum::routing::get(handler));
        let base = testkit::spawn(router).await;
        let adapter = OpenAiCompat::new(
            Client::new(),
            format!("{base}/v1"),
            Some("bad".into()),
            "Groq",
        );

        let err = adapter.ping().await.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("Invalid API key"),
            "неожиданное сообщение: {msg}"
        );
        assert!(msg.contains("401"), "нет статуса: {msg}");
    }

    #[tokio::test]
    async fn list_models_filters_by_role() {
        async fn handler() -> Json<Value> {
            Json(json!({
                "data": [
                    { "id": "llama-3.3-70b-versatile" },
                    { "id": "whisper-large-v3" },
                    { "id": "tts-1" },
                    { "id": "text-embedding-3-small" }
                ]
            }))
        }

        let router = Router::new().route("/v1/models", axum::routing::get(handler));
        let base = testkit::spawn(router).await;
        let adapter = OpenAiCompat::new(Client::new(), format!("{base}/v1"), None, "Mock");

        let llm = adapter.list_models(ModelRole::Llm).await.unwrap();
        assert_eq!(llm.len(), 1);
        assert_eq!(llm[0].model_key, "llama-3.3-70b-versatile");

        let stt = adapter.list_models(ModelRole::Stt).await.unwrap();
        assert_eq!(stt.len(), 1);
        assert_eq!(stt[0].model_key, "whisper-large-v3");

        let tts = adapter.list_models(ModelRole::Tts).await.unwrap();
        assert_eq!(tts.len(), 1);
        assert_eq!(tts[0].model_key, "tts-1");
    }
}
