//! Адаптер Deepgram: STT (`/v1/listen`) и TTS (`/v1/speak`).

use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};

use crate::domain::entities::model::{ModelDescriptor, ModelRole};
use crate::domain::ports::providers::{
    ModelCatalog, SpeechToText, SynthesisRequest, SynthesizedAudio, TextToSpeech, Transcription,
    TranscriptionRequest,
};
use crate::error::{AppError, AppResult};

use super::{http_error, join_url, read_json, transport_error};

const DEFAULT_BASE_URL: &str = "https://api.deepgram.com";

/// HTTP-клиент Deepgram.
#[derive(Debug)]
pub struct Deepgram {
    http: Client,
    base_url: String,
    api_key: Option<String>,
    provider_name: String,
}

impl Deepgram {
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
        Ok(req.header("Authorization", format!("Token {key}")))
    }

    async fn require_success(&self, resp: reqwest::Response) -> AppResult<reqwest::Response> {
        if resp.status().is_success() {
            Ok(resp)
        } else {
            Err(http_error(&self.provider_name, resp).await)
        }
    }
}

/// Query-параметры для POST /v1/listen.
pub(crate) fn listen_query(request: &TranscriptionRequest) -> Vec<(&'static str, String)> {
    let mut params: Vec<(&'static str, String)> = vec![
        ("model", request.model.clone()),
        ("smart_format", "true".into()),
    ];
    if let Some(language) = &request.language {
        params.push(("language", language.clone()));
    }
    params
}

/// Разбирает транскрипцию Deepgram.
pub(crate) fn parse_transcription(value: &Value) -> AppResult<Transcription> {
    let text = value
        .pointer("/results/channels/0/alternatives/0/transcript")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let language = value
        .pointer("/results/language")
        .and_then(Value::as_str)
        .map(str::to_string);
    Ok(Transcription { text, language })
}

/// JSON-тело для POST /v1/speak.
pub(crate) fn speak_request_json(request: &SynthesisRequest) -> Value {
    json!({ "text": request.text })
}

#[async_trait]
impl SpeechToText for Deepgram {
    async fn transcribe(&self, request: TranscriptionRequest) -> AppResult<Transcription> {
        let url = join_url(&self.base_url, "v1/listen");
        let params = listen_query(&request);

        let req = self.http.post(&url).query(&params);
        let req = self
            .with_auth(req)?
            .header(
                reqwest::header::CONTENT_TYPE,
                request.audio.mime_type.clone(),
            )
            .body(request.audio.bytes);

        let resp = req
            .send()
            .await
            .map_err(|e| transport_error(&self.provider_name, e))?;
        let resp = self.require_success(resp).await?;
        let value = read_json(&self.provider_name, resp).await?;
        parse_transcription(&value)
    }
}

#[async_trait]
impl TextToSpeech for Deepgram {
    async fn synthesize(&self, request: SynthesisRequest) -> AppResult<SynthesizedAudio> {
        let model = if request.model.is_empty() {
            "aura-asteria-en"
        } else {
            request.model.as_str()
        };
        let url = join_url(&self.base_url, "v1/speak");
        let body = speak_request_json(&request);

        let req = self.http.post(&url).query(&[
            ("model", model),
            (
                "encoding",
                if request.format.is_empty() {
                    "mp3"
                } else {
                    request.format.as_str()
                },
            ),
        ]);
        let req = self.with_auth(req)?.json(&body);

        let resp = req
            .send()
            .await
            .map_err(|e| transport_error(&self.provider_name, e))?;
        let resp = self.require_success(resp).await?;

        let mime_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("audio/mp3")
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
impl ModelCatalog for Deepgram {
    async fn list_models(&self, role: ModelRole) -> AppResult<Vec<ModelDescriptor>> {
        let url = join_url(&self.base_url, "v1/models");
        let req = self.http.get(&url);
        let req = self.with_auth(req)?;
        let resp = req
            .send()
            .await
            .map_err(|e| transport_error(&self.provider_name, e))?;
        let resp = self.require_success(resp).await?;
        let value = read_json(&self.provider_name, resp).await?;

        let bucket = match role {
            ModelRole::Stt => "stt",
            ModelRole::Tts => "tts",
            ModelRole::Llm => return Ok(Vec::new()),
        };

        let models = value
            .get(bucket)
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        let model_key = item.get("name").and_then(Value::as_str)?.to_string();
                        let display = item
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or(&model_key)
                            .to_string();
                        Some(ModelDescriptor {
                            display_name: display,
                            model_key,
                            role,
                            supports_streaming: false,
                            notes: None,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(models)
    }

    async fn ping(&self) -> AppResult<()> {
        let url = join_url(&self.base_url, "v1/models");
        let req = self.http.get(&url);
        let req = self.with_auth(req)?;
        let resp = req
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
    use crate::domain::ports::providers::AudioChunk;
    use crate::infrastructure::providers::testkit;
    use axum::{body::Bytes, routing::post, Json, Router};

    #[test]
    fn listen_query_includes_language_when_set() {
        let req = TranscriptionRequest {
            model: "nova-2".into(),
            audio: AudioChunk {
                bytes: vec![],
                mime_type: "audio/webm".into(),
                filename: "a.webm".into(),
            },
            language: Some("ru".into()),
        };
        let params = listen_query(&req);
        assert!(params.contains(&("model", "nova-2".to_string())));
        assert!(params.contains(&("language", "ru".to_string())));
        assert!(params.contains(&("smart_format", "true".to_string())));
    }

    #[test]
    fn transcription_parses_nested_transcript() {
        let value = json!({
            "metadata": { "models": ["nova-2"] },
            "results": {
                "language": "ru",
                "channels": [{ "alternatives": [{ "transcript": "здравствуйте" }] }]
            }
        });
        let parsed = parse_transcription(&value).unwrap();
        assert_eq!(parsed.text, "здравствуйте");
        assert_eq!(parsed.language.as_deref(), Some("ru"));
    }

    #[tokio::test]
    async fn transcribe_posts_raw_audio() {
        async fn handler(
            axum::extract::Query(params): axum::extract::Query<
                std::collections::HashMap<String, String>,
            >,
            body: Bytes,
        ) -> Json<Value> {
            assert_eq!(params.get("model").map(String::as_str), Some("nova-2"));
            assert_eq!(params.get("language").map(String::as_str), Some("ru"));
            assert_eq!(&body[..], b"fake-audio");
            Json(json!({
                "results": {
                    "language": "ru",
                    "channels": [{ "alternatives": [{ "transcript": "привет" }] }]
                }
            }))
        }

        let router = Router::new().route("/v1/listen", post(handler));
        let base = testkit::spawn(router).await;
        let adapter = Deepgram::new(Client::new(), Some(base), Some("k".into()), "Deepgram");

        let result = adapter
            .transcribe(TranscriptionRequest {
                model: "nova-2".into(),
                audio: AudioChunk {
                    bytes: b"fake-audio".to_vec(),
                    mime_type: "application/octet-stream".into(),
                    filename: "clip.ogg".into(),
                },
                language: Some("ru".into()),
            })
            .await
            .unwrap();
        assert_eq!(result.text, "привет");
    }

    #[tokio::test]
    async fn speak_returns_audio_bytes() {
        async fn handler() -> ([(axum::http::header::HeaderName, &'static str); 1], Vec<u8>) {
            (
                [(axum::http::header::CONTENT_TYPE, "audio/mp3")],
                b"mp3bytes".to_vec(),
            )
        }
        let router = Router::new().route("/v1/speak", post(handler));
        let base = testkit::spawn(router).await;
        let adapter = Deepgram::new(Client::new(), Some(base), Some("k".into()), "Deepgram");

        let audio = adapter
            .synthesize(SynthesisRequest::new("aura-asteria-en", "", "Привет"))
            .await
            .unwrap();
        assert_eq!(audio.bytes, b"mp3bytes");
    }

    #[tokio::test]
    async fn catalog_splits_stt_and_tts() {
        async fn handler() -> Json<Value> {
            Json(json!({
                "stt": [{ "name": "nova-2" }, { "name": "whisper-large" }],
                "tts": [{ "name": "aura-asteria-en" }]
            }))
        }
        let router = Router::new().route("/v1/models", axum::routing::get(handler));
        let base = testkit::spawn(router).await;
        let adapter = Deepgram::new(Client::new(), Some(base), Some("k".into()), "Deepgram");

        let stt = adapter.list_models(ModelRole::Stt).await.unwrap();
        assert_eq!(stt.len(), 2);
        let tts = adapter.list_models(ModelRole::Tts).await.unwrap();
        assert_eq!(tts.len(), 1);
        assert_eq!(tts[0].model_key, "aura-asteria-en");
        let llm = adapter.list_models(ModelRole::Llm).await.unwrap();
        assert!(llm.is_empty());
    }
}
