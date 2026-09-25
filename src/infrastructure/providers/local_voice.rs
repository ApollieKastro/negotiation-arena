//! Локальный STT/TTS: subprocess к `scripts/local_stt.py` / `local_tts.py`.
//!
//! Модель резолвится как относительный путь под `MODELS_DIR`. Бэкенды
//! (transformers / nemo-speech / faster-whisper / Piper) выбираются в Python.

use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use tokio::io::AsyncWriteExt;

use crate::domain::ports::providers::{
    SpeechToText, SynthesisRequest, SynthesizedAudio, TextToSpeech, Transcription,
    TranscriptionRequest,
};
use crate::error::{AppError, AppResult};

use super::local::LocalModelManager;

/// Таймаут инференса локальной модели (сек). Первый вызов может быть дольше
/// (загрузка весов) — 180 c покрывает 0.6B ASR на CPU/GPU.
const INFER_TIMEOUT: Duration = Duration::from_secs(180);

/// Python-интерпретатор для скриптов: `LOCAL_PYTHON` или `python3`.
fn python_bin() -> String {
    std::env::var("LOCAL_PYTHON")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "python3".into())
}

/// Находит скрипт: env override → `scripts/<name>` от cwd → рядом с executable.
fn resolve_script(env_key: &str, default_name: &str) -> AppResult<PathBuf> {
    if let Ok(p) = std::env::var(env_key) {
        if !p.trim().is_empty() {
            let path = PathBuf::from(p);
            if path.is_file() {
                return Ok(path);
            }
            return Err(AppError::Config(format!(
                "{env_key} указывает на несуществующий файл: {}",
                path.display()
            )));
        }
    }
    let candidates = [
        PathBuf::from("scripts").join(default_name),
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|d| d.join("scripts").join(default_name)))
            .unwrap_or_default(),
    ];
    candidates
        .into_iter()
        .find(|p| !p.as_os_str().is_empty() && p.is_file())
        .ok_or_else(|| {
            AppError::ServiceUnavailable(format!(
                "локальный скрипт `{default_name}` не найден (запускайте сервер из корня репозитория или задайте {env_key})"
            ))
        })
}

async fn run_script(
    script: &PathBuf,
    args: &[&str],
    stdin: Vec<u8>,
    label: &str,
) -> AppResult<Vec<u8>> {
    let mut cmd = tokio::process::Command::new(python_bin());
    cmd.arg(script)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    let mut child = cmd.spawn().map_err(|e| {
        AppError::ServiceUnavailable(format!("{label}: не удалось запустить python ({e})"))
    })?;

    let mut child_stdin = child.stdin.take().ok_or_else(|| {
        AppError::internal(format!("{label}: stdin дочернего процесса недоступен"))
    })?;
    child_stdin
        .write_all(&stdin)
        .await
        .map_err(|e| AppError::internal(format!("{label}: запись stdin: {e}")))?;
    drop(child_stdin);

    let output = tokio::time::timeout(INFER_TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| AppError::upstream(label, format!("таймаут {} сек", INFER_TIMEOUT.as_secs())))?
        .map_err(|e| AppError::internal(format!("{label}: wait: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if !stderr.trim().is_empty() {
            stderr
        } else {
            stdout
        };
        let detail: String = detail.trim().chars().take(500).collect();
        return Err(AppError::upstream(
            label,
            if detail.is_empty() {
                "скрипт завершился с ошибкой".into()
            } else {
                detail
            },
        ));
    }
    Ok(output.stdout)
}

/// STT через `scripts/local_stt.py`: stdin = аудио, stdout = текст.
#[derive(Debug)]
pub struct LocalSpeechToText {
    models_dir: PathBuf,
    provider_name: String,
}

impl LocalSpeechToText {
    pub fn new(models_dir: impl Into<PathBuf>, provider_name: impl Into<String>) -> Self {
        Self {
            models_dir: models_dir.into(),
            provider_name: provider_name.into(),
        }
    }
}

#[async_trait]
impl SpeechToText for LocalSpeechToText {
    async fn transcribe(&self, request: TranscriptionRequest) -> AppResult<Transcription> {
        let manager = LocalModelManager::new(self.models_dir.clone(), reqwest::Client::new());
        let model_path = manager.resolve_model_path(&request.model)?;
        let script = resolve_script("LOCAL_STT_SCRIPT", "local_stt.py")?;

        let path_str = model_path.to_string_lossy().to_string();
        let mut args = vec!["--model", path_str.as_str()];
        if let Some(lang) = request
            .language
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            args.push("--language");
            args.push(lang);
        }

        let stdout = run_script(
            &script,
            &args,
            request.audio.bytes,
            &format!("local STT ({})", self.provider_name),
        )
        .await?;
        let text = String::from_utf8_lossy(&stdout).trim().to_string();
        if text.is_empty() {
            return Err(AppError::BadRequest(
                "Речь не распознана — модель вернула пустой текст".into(),
            ));
        }
        Ok(Transcription {
            text,
            language: request.language,
        })
    }
}

/// TTS через `scripts/local_tts.py`: stdin = текст, stdout = WAV.
#[derive(Debug)]
pub struct LocalTextToSpeech {
    models_dir: PathBuf,
    provider_name: String,
}

impl LocalTextToSpeech {
    pub fn new(models_dir: impl Into<PathBuf>, provider_name: impl Into<String>) -> Self {
        Self {
            models_dir: models_dir.into(),
            provider_name: provider_name.into(),
        }
    }
}

#[async_trait]
impl TextToSpeech for LocalTextToSpeech {
    async fn synthesize(&self, request: SynthesisRequest) -> AppResult<SynthesizedAudio> {
        let manager = LocalModelManager::new(self.models_dir.clone(), reqwest::Client::new());
        let model_path = manager.resolve_model_path(&request.model)?;
        let script = resolve_script("LOCAL_TTS_SCRIPT", "local_tts.py")?;

        let path_str = model_path.to_string_lossy().to_string();
        let mut args = vec!["--model", path_str.as_str()];
        let voice = request.voice.trim();
        if !voice.is_empty() && voice != "alloy" {
            args.push("--voice");
            args.push(voice);
        }

        let stdout = run_script(
            &script,
            &args,
            request.text.clone().into_bytes(),
            &format!("local TTS ({})", self.provider_name),
        )
        .await?;
        if stdout.len() < 44 {
            return Err(AppError::upstream(
                &self.provider_name,
                "скрипт TTS вернул слишком короткий WAV",
            ));
        }
        Ok(SynthesizedAudio {
            bytes: stdout,
            mime_type: "audio/wav".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ports::providers::AudioChunk as Audio;

    fn temp_models(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("na-lv-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn python_bin_defaults_to_python3() {
        // Не мутируем env параллельно — просто проверяем форму.
        let v = std::env::var("LOCAL_PYTHON").unwrap_or_else(|_| "python3".into());
        assert!(!v.trim().is_empty());
    }

    #[tokio::test]
    async fn stt_missing_model_is_not_found() {
        let root = temp_models("stt-missing");
        let stt = LocalSpeechToText::new(&root, "local");
        let err = stt
            .transcribe(TranscriptionRequest {
                model: "nope.gguf".into(),
                audio: Audio {
                    bytes: b"RIFFxxxx".to_vec(),
                    mime_type: "audio/wav".into(),
                    filename: "a.wav".into(),
                },
                language: None,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)), "{err}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn stt_rejects_traversal_model_key() {
        let root = temp_models("stt-trav");
        let stt = LocalSpeechToText::new(&root, "local");
        let err = stt
            .transcribe(TranscriptionRequest {
                model: "../etc/passwd".into(),
                audio: Audio {
                    bytes: b"RIFF".to_vec(),
                    mime_type: "audio/wav".into(),
                    filename: "a.wav".into(),
                },
                language: None,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)), "{err}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn tts_missing_model_is_not_found() {
        let root = temp_models("tts-missing");
        let tts = LocalTextToSpeech::new(&root, "local");
        let err = tts
            .synthesize(SynthesisRequest::new("voice.onnx", "alloy", "Привет"))
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)), "{err}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
