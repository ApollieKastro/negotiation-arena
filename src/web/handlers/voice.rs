//! Голосовые эндпоинты: синтез речи (TTS) и распознавание (STT).
//!
//! Доступны любому авторизованному пользователю (`AuthUser`, не admin-only).

use axum::extract::State;
use axum::http::{header, HeaderValue};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::json;

use crate::error::{AppError, AppResult};
use crate::web::middleware::{AppJson, AppMultipart, AuthUser};
use crate::web::state::AppState;

/// Резервный MIME для бинарного ответа TTS, если провайдер не сообщил свой.
const FALLBACK_AUDIO_MIME: &str = "audio/mpeg";

#[derive(Debug, Deserialize)]
pub struct TtsRequest {
    pub text: String,
    pub voice: Option<String>,
}

/// `POST /api/v1/voice/tts` — синтез речи; наружу binary (`audio/*`).
pub async fn tts(
    State(state): State<AppState>,
    user: AuthUser,
    AppJson(req): AppJson<TtsRequest>,
) -> AppResult<Response> {
    let (bytes, mime) = state
        .services
        .voice
        .synthesize(user.user_id(), &req.text, req.voice.as_deref())
        .await?;
    Ok(audio_response(&mime, bytes))
}

/// `POST /api/v1/voice/stt` — распознавание аудио (`multipart/form-data`, поле `file`).
pub async fn stt(
    State(state): State<AppState>,
    user: AuthUser,
    AppMultipart(mut multipart): AppMultipart,
) -> AppResult<Json<serde_json::Value>> {
    let mut file = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(format!("чтение multipart: {e}")))?
    {
        if field.name() != Some("file") {
            continue;
        }
        let filename = field
            .file_name()
            .map(str::to_string)
            .unwrap_or_else(|| "audio".to_string());
        let mime = field
            .content_type()
            .map(str::to_string)
            .unwrap_or_else(|| "application/octet-stream".to_string());
        let bytes = field
            .bytes()
            .await
            .map_err(|e| AppError::BadRequest(format!("чтение файла: {e}")))?;
        file = Some((filename, mime, bytes.to_vec()));
        break;
    }

    let (filename, mime, bytes) =
        file.ok_or_else(|| AppError::BadRequest("в форме нет поля file".into()))?;
    let text = state
        .services
        .voice
        .transcribe(user.user_id(), &filename, &mime, bytes)
        .await?;
    Ok(Json(json!({ "text": text })))
}

/// Бинарный ответ с content-type провайдера (fallback — [`FALLBACK_AUDIO_MIME`]).
fn audio_response(mime: &str, bytes: Vec<u8>) -> Response {
    let content_type = HeaderValue::from_str(mime.trim())
        .ok()
        .filter(|v| !v.as_bytes().is_empty())
        .unwrap_or_else(|| HeaderValue::from_static(FALLBACK_AUDIO_MIME));
    ([(header::CONTENT_TYPE, content_type)], bytes).into_response()
}
