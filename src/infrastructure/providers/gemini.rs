//! Адаптер Google Gemini (generateContent).

use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};

use crate::domain::entities::model::{ModelDescriptor, ModelRole};
use crate::domain::ports::providers::{
    ChatModel, ChatRequest, ChatResponse, ChatRole, ModelCatalog, Usage,
};
use crate::error::{AppError, AppResult};

use super::{http_error, join_url, read_json, transport_error};

const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com";

/// HTTP-клиент Google Gemini API.
#[derive(Debug)]
pub struct Gemini {
    http: Client,
    base_url: String,
    api_key: Option<String>,
    provider_name: String,
}

impl Gemini {
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

    fn require_api_key(&self) -> AppResult<&str> {
        self.api_key
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| AppError::upstream(&self.provider_name, "API-ключ не задан".to_string()))
    }

    async fn require_success(&self, resp: reqwest::Response) -> AppResult<reqwest::Response> {
        if resp.status().is_success() {
            Ok(resp)
        } else {
            Err(http_error(&self.provider_name, resp).await)
        }
    }
}

/// Собирает JSON-тело `POST .../models/{model}:generateContent`.
pub(crate) fn chat_request_json(request: &ChatRequest) -> Value {
    let contents: Vec<Value> = request
        .dialogue()
        .into_iter()
        .map(|m| {
            let role = match m.role {
                ChatRole::User | ChatRole::System => "user",
                ChatRole::Assistant => "model",
            };
            json!({ "role": role, "parts": [{ "text": m.content }] })
        })
        .collect();

    let mut body = json!({ "contents": contents });
    if let Some(system) = request.system_text() {
        body["systemInstruction"] = json!({ "parts": [{ "text": system }] });
    }

    let mut generation = json!({});
    if let Some(temperature) = request.temperature {
        generation["temperature"] = json!(temperature);
    }
    if let Some(max_tokens) = request.max_tokens {
        generation["maxOutputTokens"] = json!(max_tokens);
    }
    if generation
        .as_object()
        .map(|o| !o.is_empty())
        .unwrap_or(false)
    {
        body["generationConfig"] = generation;
    }
    body
}

/// Разбирает ответ `generateContent`.
pub(crate) fn chat_response_json(value: &Value) -> AppResult<ChatResponse> {
    let parts = value
        .pointer("/candidates/0/content/parts")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::upstream("gemini", "в ответе нет candidates"))?;

    let content: String = parts
        .iter()
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect();

    let finish_reason = value
        .pointer("/candidates/0/finishReason")
        .and_then(Value::as_str)
        .map(str::to_string);

    let model = value
        .get("modelVersion")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let usage = Usage {
        prompt_tokens: value
            .pointer("/usageMetadata/promptTokenCount")
            .and_then(Value::as_u64)
            .map(|v| v as u32),
        completion_tokens: value
            .pointer("/usageMetadata/candidatesTokenCount")
            .and_then(Value::as_u64)
            .map(|v| v as u32),
        total_tokens: value
            .pointer("/usageMetadata/totalTokenCount")
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

/// Убирает префикс `models/` у ключа модели Gemini.
pub(crate) fn strip_model_prefix(name: &str) -> &str {
    name.strip_prefix("models/").unwrap_or(name)
}

#[async_trait]
impl ChatModel for Gemini {
    async fn chat(&self, request: ChatRequest) -> AppResult<ChatResponse> {
        let key = self.require_api_key()?;
        let url = format!(
            "{}/v1beta/models/{}:generateContent",
            self.base_url, request.model
        );
        let body = chat_request_json(&request);

        let resp = self
            .http
            .post(&url)
            .query(&[("key", key)])
            .json(&body)
            .send()
            .await
            .map_err(|e| transport_error(&self.provider_name, e))?;
        let resp = self.require_success(resp).await?;
        let value = read_json(&self.provider_name, resp).await?;
        chat_response_json(&value)
    }
}

#[async_trait]
impl ModelCatalog for Gemini {
    async fn list_models(&self, role: ModelRole) -> AppResult<Vec<ModelDescriptor>> {
        if role != ModelRole::Llm {
            return Ok(Vec::new());
        }
        let key = self.require_api_key()?;
        let url = join_url(&self.base_url, "v1beta/models");
        let resp = self
            .http
            .get(&url)
            .query(&[("key", key)])
            .send()
            .await
            .map_err(|e| transport_error(&self.provider_name, e))?;
        let resp = self.require_success(resp).await?;
        let value = read_json(&self.provider_name, resp).await?;

        let models: Vec<ModelDescriptor> = value
            .get("models")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter(|item| {
                        item.get("supportedGenerationMethods")
                            .and_then(Value::as_array)
                            .map(|methods| {
                                methods
                                    .iter()
                                    .any(|m| m.as_str() == Some("generateContent"))
                            })
                            .unwrap_or(false)
                    })
                    .filter_map(|item| {
                        let name = item.get("name").and_then(Value::as_str)?;
                        let model_key = strip_model_prefix(name).to_string();
                        let display = item
                            .get("displayName")
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
        let key = self.require_api_key()?;
        let url = join_url(&self.base_url, "v1beta/models");
        let resp = self
            .http
            .get(&url)
            .query(&[("key", key)])
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
    use crate::domain::ports::providers::ChatMessage;
    use crate::infrastructure::providers::testkit;
    use axum::{routing::get, routing::post, Json, Router};

    #[test]
    fn request_uses_model_role_and_system_instruction() {
        let body = chat_request_json(
            &ChatRequest::new(
                "gemini-2.0-flash",
                vec![
                    ChatMessage::system("Ты — покупатель."),
                    ChatMessage::user("Сколько стоит?"),
                    ChatMessage::assistant("Обсудим."),
                ],
            )
            .with_temperature(0.2),
        );

        assert_eq!(
            body["systemInstruction"]["parts"][0]["text"],
            "Ты — покупатель."
        );
        assert_eq!(body["contents"][0]["role"], "user");
        assert_eq!(body["contents"][1]["role"], "model");
        assert!((body["generationConfig"]["temperature"].as_f64().unwrap() - 0.2).abs() < 1e-6);
    }

    #[test]
    fn response_parses_candidate_and_usage() {
        let value = json!({
            "modelVersion": "gemini-2.0-flash",
            "candidates": [{
                "content": { "role": "model", "parts": [{ "text": "Ответ" }] },
                "finishReason": "STOP"
            }],
            "usageMetadata": {
                "promptTokenCount": 8,
                "candidatesTokenCount": 5,
                "totalTokenCount": 13
            }
        });
        let parsed = chat_response_json(&value).unwrap();
        assert_eq!(parsed.content, "Ответ");
        assert_eq!(parsed.model, "gemini-2.0-flash");
        assert_eq!(parsed.finish_reason.as_deref(), Some("STOP"));
        assert_eq!(parsed.usage.total_tokens, Some(13));
    }

    #[test]
    fn strip_prefix_works() {
        assert_eq!(
            strip_model_prefix("models/gemini-1.5-pro"),
            "gemini-1.5-pro"
        );
        assert_eq!(strip_model_prefix("gemini-1.5-pro"), "gemini-1.5-pro");
    }

    #[tokio::test]
    async fn chat_and_catalog_against_mock() {
        async fn chat_handler(Json(body): Json<Value>) -> Json<Value> {
            assert!(body.get("systemInstruction").is_some());
            Json(json!({
                "modelVersion": "gemini-2.0-flash",
                "candidates": [{
                    "content": { "role": "model", "parts": [{ "text": "pong" }] },
                    "finishReason": "STOP"
                }],
                "usageMetadata": { "promptTokenCount": 1, "candidatesTokenCount": 1, "totalTokenCount": 2 }
            }))
        }
        async fn models_handler() -> Json<Value> {
            Json(json!({
                "models": [
                    {
                        "name": "models/gemini-2.0-flash",
                        "displayName": "Gemini 2.0 Flash",
                        "supportedGenerationMethods": ["generateContent", "countTokens"]
                    },
                    {
                        "name": "models/aqa",
                        "displayName": "AQA",
                        "supportedGenerationMethods": ["countTokens"]
                    }
                ]
            }))
        }

        let router = Router::new()
            .route(
                "/v1beta/models/gemini-2.0-flash:generateContent",
                post(chat_handler),
            )
            .route("/v1beta/models", get(models_handler));
        let base = testkit::spawn(router).await;
        let adapter = Gemini::new(Client::new(), Some(base), Some("k".into()), "Gemini");

        let resp = adapter
            .chat(ChatRequest::new(
                "gemini-2.0-flash",
                vec![
                    ChatMessage::system("Ты — ассистент."),
                    ChatMessage::user("ping"),
                ],
            ))
            .await
            .unwrap();
        assert_eq!(resp.content, "pong");

        let models = adapter.list_models(ModelRole::Llm).await.unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].model_key, "gemini-2.0-flash");
        assert_eq!(models[0].display_name, "Gemini 2.0 Flash");
    }

    #[tokio::test]
    async fn missing_key_is_upstream_error() {
        let adapter = Gemini::new(Client::new(), None, None, "Gemini");
        let err = adapter
            .chat(ChatRequest::new("m", vec![ChatMessage::user("x")]))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("API-ключ"));
    }
}
