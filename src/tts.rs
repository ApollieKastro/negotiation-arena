use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::{Deserialize, Serialize};
use std::process::Command;
use tracing::info;

// ─────────────────────────────────────────────────────────────
// Пути к Python-скриптам
// ─────────────────────────────────────────────────────────────

fn scripts_dir() -> String {
    // scripts/ рядом с src/ в корне проекта
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    format!("{}/scripts", manifest_dir)
}

fn python_bin() -> String {
    // .venv/bin/python3 рядом с проектом
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    format!("{}/.venv/bin/python3", manifest_dir)
}

fn tts_script() -> String {
    format!("{}/tts.py", scripts_dir())
}

fn stt_script() -> String {
    format!("{}/stt.py", scripts_dir())
}

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
// TTS: текст → аудио (WAV base64)
// Вызывает Python-скрипт с Piper TTS
// ─────────────────────────────────────────────────────────────

pub async fn text_to_speech(text: &str, _speaker: &str) -> Result<String, String> {
    let text_owned = text.to_string();

    // Выполняем в blocking thread чтобы не блокировать async runtime
    let wav_bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, String> {
        let output = Command::new(python_bin())
            .arg(tts_script())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to spawn TTS process: {}", e))?;

        // Пишем текст в stdin
        if let Some(ref mut stdin) = output.stdin.as_ref() {
            use std::io::Write;
            stdin.write_all(text_owned.as_bytes())
                .map_err(|e| format!("Failed to write to TTS stdin: {}", e))?;
        }

        let result = output.wait_with_output()
            .map_err(|e| format!("TTS process failed: {}", e))?;

        if !result.status.success() {
            let stderr = String::from_utf8_lossy(&result.stderr);
            return Err(format!("TTS error: {}", stderr));
        }

        Ok(result.stdout)
    })
    .await
    .map_err(|e| format!("TTS task failed: {}", e))?;

    let wav = wav_bytes?;
    let b64 = BASE64.encode(&wav);
    let preview = if text.len() > 40 { &text[..40] } else { text };
    info!("TTS: '{}' → {} bytes audio", preview, wav.len());
    Ok(b64)
}

// ─────────────────────────────────────────────────────────────
// STT: аудио (bytes) → текст
// Вызывает Python-скрипт с faster-whisper
// ─────────────────────────────────────────────────────────────

pub async fn speech_to_text(audio_bytes: Vec<u8>, _filename: &str) -> Result<String, String> {
    // Выполняем в blocking thread чтобы не блокировать async runtime
    let text = tokio::task::spawn_blocking(move || -> Result<String, String> {
        let output = Command::new(python_bin())
            .arg(stt_script())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to spawn STT process: {}", e))?;

        // Пишем аудио-данные в stdin
        if let Some(ref mut stdin) = output.stdin.as_ref() {
            use std::io::Write;
            stdin.write_all(&audio_bytes)
                .map_err(|e| format!("Failed to write to STT stdin: {}", e))?;
        }

        let result = output.wait_with_output()
            .map_err(|e| format!("STT process failed: {}", e))?;

        if !result.status.success() {
            let stderr = String::from_utf8_lossy(&result.stderr);
            return Err(format!("STT error: {}", stderr));
        }

        let text = String::from_utf8_lossy(&result.stdout).trim().to_string();
        Ok(text)
    })
    .await
    .map_err(|e| format!("STT task failed: {}", e))?;

    let recognized = text?;
    let preview = if recognized.len() > 60 { &recognized[..60] } else { &recognized };
    info!("STT: audio → '{}'", preview);
    Ok(recognized)
}
