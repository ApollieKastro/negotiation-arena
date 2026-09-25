//! Локальные модели: менеджер файлов + каталог для админки.
//!
//! Вызовы STT/TTS локальными бинарниками (whisper.cpp, Piper) подключаются
//! на этапе 7; здесь — список/удаление/скачивание файлов в `MODELS_DIR`.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::domain::entities::model::{ModelDescriptor, ModelRole};
use crate::domain::ports::providers::ModelCatalog;
use crate::error::{AppError, AppResult};

use super::{http_error, transport_error};

/// Файл локальной модели в каталоге `MODELS_DIR`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalModelFile {
    /// Имя файла (без пути).
    pub name: String,
    pub size_bytes: u64,
    /// Роль, определённая по имени/расширению.
    pub role: ModelRole,
}

/// Менеджер локальных моделей: список, удаление, скачивание.
#[derive(Debug)]
pub struct LocalModelManager {
    root: PathBuf,
    http: Client,
}

impl LocalModelManager {
    pub fn new(root: impl Into<PathBuf>, http: Client) -> Self {
        Self {
            root: root.into(),
            http,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Определяет роль модели по имени файла.
    pub fn guess_role(name: &str) -> ModelRole {
        let lower = name.to_lowercase();
        if lower.ends_with(".onnx")
            || lower.contains("piper")
            || lower.contains("tts")
            || lower.contains("silero")
        {
            ModelRole::Tts
        } else if lower.contains("whisper")
            || lower.contains("stt")
            || lower.contains("speech-to-text")
            || lower.contains("paraformer")
            // Модели whisper.cpp носят имена вида ggml-base.bin, ggml-small.bin…
            || lower.contains("ggml")
        {
            ModelRole::Stt
        } else {
            // *.gguf и прочее по умолчанию считаем диалоговыми.
            ModelRole::Llm
        }
    }

    fn validate_name(name: &str) -> AppResult<()> {
        if name.trim().is_empty() {
            return Err(AppError::BadRequest("пустое имя файла".into()));
        }
        if name.contains('/') || name.contains('\\') || name.contains("..") {
            return Err(AppError::BadRequest(
                "недопустимое имя файла (пути запрещены)".into(),
            ));
        }
        Ok(())
    }

    /// Список файлов моделей в каталоге (сортировка по имени).
    pub fn list(&self) -> AppResult<Vec<LocalModelFile>> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }
        let mut files = Vec::new();
        for entry in std::fs::read_dir(&self.root)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let size_bytes = entry.metadata()?.len();
            let role = Self::guess_role(&name);
            files.push(LocalModelFile {
                name,
                size_bytes,
                role,
            });
        }
        files.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(files)
    }

    /// Удаляет файл модели по имени (без каталога).
    pub fn delete(&self, name: &str) -> AppResult<()> {
        Self::validate_name(name)?;
        let path = self.root.join(name);
        if !path.is_file() {
            return Err(AppError::NotFound(format!("файл {name} не найден")));
        }
        std::fs::remove_file(&path)?;
        Ok(())
    }

    /// Скачивает файл модели по URL в каталог моделей.
    pub async fn download(&self, url: &str, name: &str) -> AppResult<LocalModelFile> {
        Self::validate_name(name)?;
        std::fs::create_dir_all(&self.root)?;

        let resp = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| transport_error("local", e))?;
        if !resp.status().is_success() {
            return Err(http_error("local", resp).await);
        }
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| transport_error("local", e))?;

        let path = self.root.join(name);
        std::fs::write(&path, &bytes)?;

        let role = Self::guess_role(name);
        Ok(LocalModelFile {
            name: name.to_string(),
            size_bytes: bytes.len() as u64,
            role,
        })
    }
}

/// Каталог локальных файлов через порт `ModelCatalog`.
#[derive(Debug)]
pub struct LocalCatalog {
    manager: LocalModelManager,
}

impl LocalCatalog {
    pub fn new(manager: LocalModelManager) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl ModelCatalog for LocalCatalog {
    async fn list_models(&self, role: ModelRole) -> AppResult<Vec<ModelDescriptor>> {
        Ok(self
            .manager
            .list()?
            .into_iter()
            .filter(|file| file.role == role)
            .map(|file| ModelDescriptor {
                display_name: file.name.clone(),
                model_key: file.name,
                role,
                supports_streaming: false,
                notes: Some(format!("{} bytes", file.size_bytes)),
            })
            .collect())
    }

    async fn ping(&self) -> AppResult<()> {
        if self.manager.root().exists() {
            Ok(())
        } else {
            std::fs::create_dir_all(self.manager.root())?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::providers::testkit;
    use axum::{routing::get, Router};

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("na-models-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn guess_role_by_extension_and_name() {
        assert_eq!(
            LocalModelManager::guess_role("ggml-base.bin"),
            ModelRole::Stt
        );
        assert_eq!(
            LocalModelManager::guess_role("whisper-large-v3.bin"),
            ModelRole::Stt
        );
        assert_eq!(
            LocalModelManager::guess_role("piper voices/ru_RU.onnx"),
            ModelRole::Tts
        );
        assert_eq!(
            LocalModelManager::guess_role("my-tts-voice.onnx"),
            ModelRole::Tts
        );
        assert_eq!(
            LocalModelManager::guess_role("llama-3.1-8B.Q4_K_M.gguf"),
            ModelRole::Llm
        );
    }

    #[test]
    fn list_and_delete_roundtrip() {
        let root = temp_root("roundtrip");
        std::fs::write(root.join("whisper-small.bin"), b"abc").unwrap();
        std::fs::write(root.join("voice.onnx"), b"xy").unwrap();

        let manager = LocalModelManager::new(&root, Client::new());
        let files = manager.list().unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|f| f.size_bytes > 0));

        manager.delete("whisper-small.bin").unwrap();
        assert_eq!(manager.list().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn delete_rejects_traversal() {
        let root = temp_root("traversal");
        let manager = LocalModelManager::new(&root, Client::new());
        assert!(manager.delete("../secret").is_err());
        assert!(manager.delete("a/b").is_err());
        assert!(manager.delete("").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn download_writes_file() {
        async fn handler() -> Vec<u8> {
            b"model-bytes".to_vec()
        }
        let router = Router::new().route("/models/thing.bin", get(handler));
        let base = testkit::spawn(router).await;

        let root = temp_root("download");
        let manager = LocalModelManager::new(&root, Client::new());
        let file = manager
            .download(&format!("{base}/models/thing.bin"), "thing.bin")
            .await
            .unwrap();
        assert_eq!(file.size_bytes, 11);
        assert!(root.join("thing.bin").is_file());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn catalog_filters_by_role() {
        let root = temp_root("catalog");
        std::fs::write(root.join("whisper-tiny.bin"), b"1").unwrap();
        std::fs::write(root.join("piper-voice.onnx"), b"2").unwrap();

        let catalog = LocalCatalog::new(LocalModelManager::new(&root, Client::new()));
        let stt = catalog.list_models(ModelRole::Stt).await.unwrap();
        assert_eq!(stt.len(), 1);
        assert_eq!(stt[0].model_key, "whisper-tiny.bin");
        let tts = catalog.list_models(ModelRole::Tts).await.unwrap();
        assert_eq!(tts.len(), 1);
        let llm = catalog.list_models(ModelRole::Llm).await.unwrap();
        assert!(llm.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn ping_creates_missing_dir() {
        let root = std::env::temp_dir().join(format!("na-ping-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let catalog = LocalCatalog::new(LocalModelManager::new(&root, Client::new()));
        catalog.ping().await.unwrap();
        assert!(root.is_dir());
        let _ = std::fs::remove_dir_all(&root);
    }
}
