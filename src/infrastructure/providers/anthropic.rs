//! Адаптер Anthropic Messages API (диалог).

use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};

use crate::domain::entities::model::{ModelDescriptor, ModelRole};
use crate::domain::ports::providers::{
    ChatModel, ChatRequest, ChatResponse, ChatRole, ModelCatalog, Usage,
};
use crate::error::{AppError, AppResult};

use super::{http_error, join_url, read_json, transport_error};

const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// HTTP-клиент Anthropic Messages API.
#[derive(Debug)]
pub struct Anthropic {
    http: Client,
    base_url: String,
    api_key: Option<String>,
    provider_name: String,
}

impl Anthropic {
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

    fn with_auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let req = req.header("anthropic-version", ANTHROPIC_VERSION);
        match &self.api_key {
            Some(key) if !key.trim().is_empty() => req.header("x-api-key", key),
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

/// Собирает JSON-тело `POST /v1/messages`.
pub(crate) fn chat_request_json(request: &ChatRequest) -> Value {
    let messages: Vec<Value> = request
        .dialogue()
        .into_iter()
        .map(|m| {
            let role = match m.role {
                ChatRole::User | ChatRole::System => "user",
                ChatRole::Assistant => "assistant",
            };
            json!({ "role": role, "content": m.content })
        })
        .collect();

    let mut body = json!({
        "model": request.model,
        "max_tokens": request.max_tokens.unwrap_or(4096),
        "messages": messages,
    });
    if let Some(system) = request.system_text() {
        body["system"] = json!(system);
    }
    if let Some(temperature) = request.temperature {
        body["temperature"] = json!(temperature);
    }
    body
}

/// Разбирает ответ `POST /v1/messages`.
pub(crate) fn chat_response_json(value: &Value) -> AppResult<ChatResponse> {
    let blocks = value
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::upstream("anthropic", "в ответе нет content"))?;

    let content: String = blocks
        .iter()
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .collect();

    let model = value
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let finish_reason = value
        .get("stop_reason")
        .and_then(Value::as_str)
        .map(str::to_string);

    let input = value
        .pointer("/usage/input_tokens")
        .and_then(Value::as_u64)
        .map(|v| v as u32);
    let output = value
        .pointer("/usage/output_tokens")
        .and_then(Value::as_u64)
        .map(|v| v as u32);
    let total = match (input, output) {
        (Some(i), Some(o)) => Some(i + o),
        _ => None,
    };

    Ok(ChatResponse {
        content,
        model,
        finish_reason,
        usage: Usage {
            prompt_tokens: input,
            completion_tokens: output,
            total_tokens: total,
        },
    })
}

#[async_trait]
impl ChatModel for Anthropic {
    async fn chat(&self, request: ChatRequest) -> AppResult<ChatResponse> {
        let url = join_url(&self.base_url, "v1/messages");
        let body = chat_request_json(&request);

        let resp = self
            .with_auth(self.http.post(&url).json(&body))
            .send()
            .await
            .map_err(|e| transport_error(&self.provider_name, e))?;
        let resp = self.require_success(resp).await?;
        let value = read_json(&self.provider_name, resp).await?;
        chat_response_json(&value)
    }
}

#[async_trait]
impl ModelCatalog for Anthropic {
    async fn list_models(&self, role: ModelRole) -> AppResult<Vec<ModelDescriptor>> {
        if role != ModelRole::Llm {
            return Ok(Vec::new());
        }
        let url = join_url(&self.base_url, "v1/models");
        let resp = self
            .with_auth(self.http.get(&url))
            .send()
            .await
            .map_err(|e| transport_error(&self.provider_name, e))?;
        let resp = self.require_success(resp).await?;
        let value = read_json(&self.provider_name, resp).await?;

        let ids: Vec<(String, Option<String>)> = value
            .pointer("/data")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        let id = item.get("id").and_then(Value::as_str)?.to_string();
                        let display = item
                            .get("display_name")
                            .and_then(Value::as_str)
                            .map(str::to_string);
                        Some((id, display))
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(ids
            .into_iter()
            .map(|(model_key, display)| ModelDescriptor {
                display_name: display.unwrap_or_else(|| model_key.clone()),
                model_key,
                role,
                supports_streaming: false,
                notes: None,
            })
            .collect())
    }

    async fn ping(&self) -> AppResult<()> {
        let url = join_url(&self.base_url, "v1/models");
        let resp = self
            .with_auth(self.http.get(&url))
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
    use axum::{routing::post, Json, Router};

    #[test]
    fn system_goes_to_separate_field() {
        let body = chat_request_json(
            &ChatRequest::new(
                "claude-3-5-haiku-latest",
                vec![
                    ChatMessage::system("Ты — тренер."),
                    ChatMessage::user("Привет"),
                    ChatMessage::assistant("Здравствуйте"),
                ],
            )
            .with_max_tokens(1024),
        );

        assert_eq!(body["system"], "Ты — тренер.");
        assert_eq!(body["max_tokens"], 1024);
        let roles: Vec<&str> = body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|m| m["role"].as_str())
            .collect();
        assert_eq!(roles, vec!["user", "assistant"]);
    }

    #[test]
    fn response_joins_text_blocks_and_usage() {
        let value = json!({
            "model": "claude-3-5-haiku-latest",
            "content": [
                { "type": "text", "text": "Привет, " },
                { "type": "text", "text": "я готов." }
            ],
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 10, "output_tokens": 20 }
        });
        let parsed = chat_response_json(&value).unwrap();
        assert_eq!(parsed.content, "Привет, я готов.");
        assert_eq!(parsed.finish_reason.as_deref(), Some("end_turn"));
        assert_eq!(parsed.usage.prompt_tokens, Some(10));
        assert_eq!(parsed.usage.completion_tokens, Some(20));
        assert_eq!(parsed.usage.total_tokens, Some(30));
    }

    #[tokio::test]
    async fn chat_against_mock_server() {
        async fn handler(Json(body): Json<Value>) -> Json<Value> {
            assert_eq!(body["max_tokens"], 4096);
            assert_eq!(body["system"], "Ты — продавец.");
            Json(json!({
                "model": "claude-3-5-haiku-latest",
                "content": [{ "type": "text", "text": "pong" }],
                "stop_reason": "end_turn",
                "usage": { "input_tokens": 5, "output_tokens": 7 }
            }))
        }

        let router = Router::new().route("/v1/messages", post(handler));
        let base = testkit::spawn(router).await;
        let adapter = Anthropic::new(
            Client::new(),
            Some(base),
            Some("sk-test".into()),
            "Anthropic",
        );

        let resp = adapter
            .chat(ChatRequest::new(
                "claude-3-5-haiku-latest",
                vec![
                    ChatMessage::system("Ты — продавец."),
                    ChatMessage::user("ping"),
                ],
            ))
            .await
            .unwrap();
        assert_eq!(resp.content, "pong");
        assert_eq!(resp.usage.total_tokens, Some(12));
    }
}
