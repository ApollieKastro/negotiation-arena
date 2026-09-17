use reqwest::Client;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::{Deserialize, Serialize};
use tracing::info;

const SILERO_URL: &str = "http://127.0.0.1:8080";

// ─────────────────────────────────────────────────────────────
// Типы
// ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct TtsRequest {
    pub text: String,
    pub speaker: Option<String>,
}

#[derive(Serialize)]
pub struct TtsResponse {
    pub audio_base64: String,
}

#[derive(Serialize)]
pub struct SttResponse {
    pub text: String,
}

// ─────────────────────────────────────────────────────────────
// TTS: текст → аудио (WAV)
// ─────────────────────────────────────────────────────────────

pub async fn text_to_speech(text: &str, speaker: &str) -> Result<String, String> {
    let client = Client::new();

    let form = reqwest::multipart::Form::new()
        .text("text", text.to_string())
        .text("speaker", speaker.to_string())
        .text("sample_rate", "24000")
        .text("normalize", "true")
        .text("postprocess", "true");

    let resp = client.post(format!("{}/tts", SILERO_URL))
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("TTS request failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("TTS error: {}", resp.status()));
    }

    let audio_bytes = resp.bytes().await
        .map_err(|e| format!("TTS read failed: {}", e))?;

    let b64 = BASE64.encode(&audio_bytes);
    info!("TTS: '{}' → {} bytes audio", &text[..text.len().min(40)], audio_bytes.len());
    Ok(b64)
}

// ─────────────────────────────────────────────────────────────
// STT: аудио (bytes) → текст
// ─────────────────────────────────────────────────────────────

pub async fn speech_to_text(audio_bytes: Vec<u8>, filename: &str) -> Result<String, String> {
    let client = Client::new();

    let part = reqwest::multipart::Part::bytes(audio_bytes)
        .file_name(filename.to_string())
        .mime_str("audio/webm")
        .map_err(|e| format!("MIME error: {}", e))?;

    let form = reqwest::multipart::Form::new()
        .part("file", part);

    let resp = client.post(format!("{}/stt", SILERO_URL))
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("STT request failed: {}", e))?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("STT error: {}", body));
    }

    let result: serde_json::Value = resp.json().await
        .map_err(|e| format!("STT parse failed: {}", e))?;

    let text = result["text"].as_str().unwrap_or("").to_string();
    info!("STT: audio → '{}'", &text[..text.len().min(60)]);
    Ok(text)
}
