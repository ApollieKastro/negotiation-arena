//! Адаптеры провайдеров ИИ (LLM / STT / TTS) и фабрика их сборки.
//!
//! Каждый адаптер реализует порты `domain::ports::providers`:
//! `ChatModel`, `SpeechToText`, `TextToSpeech`, `ModelCatalog`.
//!
//! Поддерживаемые семейства:
//! * `openai_compat` — OpenAI, Groq, OpenRouter, Ollama, LM Studio, vLLM...;
//! * `anthropic` — Messages API (диалог);
//! * `gemini` — Google AI (диалог);
//! * `elevenlabs` — синтез речи;
//! * `deepgram` — распознавание и синтез речи;
//! * `local` — локальные модели (каталог; вызовы подключаются на этапе 7);
//! * `mock` — демо-режим без API-ключей (офлайн-диалог и генерация сценариев).

pub mod anthropic;
pub mod deepgram;
pub mod elevenlabs;
pub mod factory;
pub mod gemini;
pub mod local;
pub mod mock;
pub mod openai_compat;

// Реэкспорт фабрики и локального менеджера — для composition root
// и админских сервисов (задействуются с этапов 3–5).
#[allow(unused_imports)]
pub use factory::{ProviderFactory, ProviderHandle};
#[allow(unused_imports)]
pub use local::{LocalModelFile, LocalModelManager};

use crate::error::{AppError, AppResult};

/// Склеивает base URL и путь без двойных слэшей.
pub(crate) fn join_url(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

/// Достаёт человекочитаемое сообщение об ошибке из JSON-тела ответа.
/// Понимает форматы OpenAI/Anthropic/Gemini/ElevenLabs/Deepgram.
pub(crate) fn extract_error_message(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let pick = |ptr: &str| {
        value
            .pointer(ptr)
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    };
    let msg = pick("/error/message")
        .or_else(|| pick("/detail/message"))
        .or_else(|| pick("/message"))
        .or_else(|| {
            value
                .get("error")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .or_else(|| pick("/messages/0"))?;
    let truncated: String = msg.chars().take(400).collect();
    Some(truncated)
}

/// Превращает не-2xx ответ в `AppError::Upstream` (тело — по делу, без ключей).
pub(crate) async fn http_error(provider: &str, response: reqwest::Response) -> AppError {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let detail = extract_error_message(&body)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            let trimmed = body.trim();
            if trimmed.is_empty() {
                "пустое тело ответа".to_string()
            } else {
                trimmed.chars().take(300).collect()
            }
        });
    AppError::upstream(provider, format!("HTTP {status}: {detail}"))
}

/// Ошибка транспорта (сеть, таймаут) → `Upstream`.
pub(crate) fn transport_error(provider: &str, err: reqwest::Error) -> AppError {
    AppError::upstream(provider, format!("запрос не выполнен: {err}"))
}

/// Читает JSON-тело успешного ответа; не-JSON — ошибка провайдера.
pub(crate) async fn read_json(
    provider: &str,
    response: reqwest::Response,
) -> AppResult<serde_json::Value> {
    response
        .json::<serde_json::Value>()
        .await
        .map_err(|e| AppError::upstream(provider, format!("некорректный JSON в ответе: {e}")))
}

#[cfg(test)]
pub(crate) mod testkit {
    /// Поднимает мок-сервер на эфемерном порту, возвращает его base URL.
    pub async fn spawn(router: axum::Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind мок-сервера");
        let addr = listener.local_addr().expect("addr мок-сервера");
        tokio::spawn(async move {
            axum::serve(listener, router)
                .await
                .expect("serve мок-сервера");
        });
        format!("http://{addr}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_url_avoids_double_slash() {
        assert_eq!(
            join_url("https://api.openai.com/v1/", "/chat/completions"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(join_url("https://x", "models"), "https://x/models");
    }

    #[test]
    fn extract_error_from_openai_shape() {
        let body = r#"{"error":{"message":"Invalid API key","type":"auth"}}"#;
        assert_eq!(
            extract_error_message(body).as_deref(),
            Some("Invalid API key")
        );
    }

    #[test]
    fn extract_error_from_anthropic_shape() {
        let body = r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#;
        assert_eq!(extract_error_message(body).as_deref(), Some("Overloaded"));
    }

    #[test]
    fn extract_error_from_flat_and_detail_shapes() {
        assert_eq!(
            extract_error_message(r#"{"error":"quota exceeded"}"#).as_deref(),
            Some("quota exceeded")
        );
        assert_eq!(
            extract_error_message(r#"{"detail":{"message":"voice not found"}}"#).as_deref(),
            Some("voice not found")
        );
        assert_eq!(
            extract_error_message(r#"{"messages":["bad model"]}"#).as_deref(),
            Some("bad model")
        );
    }

    #[test]
    fn extract_error_gives_up_on_non_json() {
        assert!(extract_error_message("<html>502</html>").is_none());
    }
}
