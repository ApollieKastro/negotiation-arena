//! Локальные модели: менеджер файлов (URL + HuggingFace) + каталог для админки.
//!
//! STT/TTS-инференс — `local_voice` (subprocess к `scripts/local_*.py`).

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::domain::entities::model::{ModelDescriptor, ModelRole};
use crate::domain::ports::providers::ModelCatalog;
use crate::error::{AppError, AppResult};

use super::{http_error, transport_error};

/// Файл локальной модели в каталоге `MODELS_DIR` (имя — относительный путь).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalModelFile {
    /// Относительный путь от root (например `voice.onnx` или `org__repo/file.gguf`).
    pub name: String,
    pub size_bytes: u64,
    /// Роль, определённая по имени/расширению.
    pub role: ModelRole,
}

/// Запрос скачивания локальной модели.
#[derive(Debug, Clone, Deserialize)]
pub struct DownloadLocalModelRequest {
    /// Прямой URL **или** HF repo id вида `org/name`.
    pub source: String,
    /// Файл внутри HF repo (для URL — необязателен, имя берётся из path).
    #[serde(default)]
    pub filename: Option<String>,
    /// Явное имя файла (basename) внутри `MODELS_DIR`.
    #[serde(default)]
    pub name: Option<String>,
}

/// Менеджер локальных моделей: рекурсивный список, удаление, скачивание.
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

    pub fn http(&self) -> &Client {
        &self.http
    }

    /// Определяет роль модели по имени файла/каталога.
    ///
    /// STT-маркеры проверяются **до** TTS (`.gguf`/ASR-имена не должны
    /// проваливаться в LLM только из-за `gguf`).
    pub fn guess_role(name: &str) -> ModelRole {
        let lower = name.to_lowercase();
        if lower.contains("asr")
            || lower.contains("nemotron")
            || lower.contains("streaming")
            || lower.contains("parakeet")
            || lower.contains("speech-to-text")
            || lower.contains("whisper")
            || lower.contains("stt")
            || lower.contains("paraformer")
            || lower.contains("transcribe")
            // whisper.cpp: ggml-base.bin, ggml-small.bin…
            || lower.contains("ggml")
            || lower.contains("fastconformer")
        {
            ModelRole::Stt
        } else if lower.ends_with(".onnx")
            || lower.ends_with(".onnx.json")
            || lower.contains("piper")
            || lower.contains("tts")
            || lower.contains("silero")
            || lower.contains("text-to-speech")
        {
            ModelRole::Tts
        } else {
            // *.gguf и прочее по умолчанию — диалоговые.
            ModelRole::Llm
        }
    }

    /// Валидация относительного имени/пути: без `..`, без абсолютных путей,
    /// без пустых сегментов. Слэши в середине разрешены (вложенные HF/пайпы).
    fn validate_rel_path(name: &str) -> AppResult<()> {
        let name = name.trim();
        if name.is_empty() {
            return Err(AppError::BadRequest("пустое имя файла".into()));
        }
        if name.contains('\\') {
            return Err(AppError::BadRequest(
                "недопустимое имя файла (используйте `/`)".into(),
            ));
        }
        if name.starts_with('/') || Path::new(name).is_absolute() {
            return Err(AppError::BadRequest(
                "недопустимое имя файла (абсолютный путь)".into(),
            ));
        }
        if name.split('/').any(|seg| seg == ".." || seg == ".") {
            return Err(AppError::BadRequest(
                "недопустимое имя файла (путевая traversal)".into(),
            ));
        }
        Ok(())
    }

    fn resolve_under_root(&self, rel: &str) -> AppResult<PathBuf> {
        Self::validate_rel_path(rel)?;
        let root = self
            .root
            .canonicalize()
            .unwrap_or_else(|_| self.root.clone());
        let joined = self.root.join(rel);
        // Если root существует — проверяем, что путь не вышел наружу.
        if root.exists() {
            let canon = joined
                .canonicalize()
                .map_err(|_| AppError::NotFound(format!("файл {rel} не найден")))?;
            if !canon.starts_with(&root) {
                return Err(AppError::BadRequest(
                    "недопустимый путь (вне каталога моделей)".into(),
                ));
            }
            return Ok(joined);
        }
        Ok(joined)
    }

    /// Рекурсивный список файлов моделей (пропуск `.cache`, `.git`, скрытых).
    pub fn list(&self) -> AppResult<Vec<LocalModelFile>> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }
        let mut files = Vec::new();
        self.walk(&self.root, &mut files)?;
        files.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(files)
    }

    fn walk(&self, dir: &Path, out: &mut Vec<LocalModelFile>) -> AppResult<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let file_name = entry.file_name().to_string_lossy().to_string();
            if file_name.starts_with('.') {
                continue; // .cache, .git, .lock
            }
            let path = entry.path();
            if file_type.is_dir() {
                self.walk(&path, out)?;
            } else if file_type.is_file() {
                let rel = path
                    .strip_prefix(&self.root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                let size_bytes = entry.metadata()?.len();
                let role = Self::guess_role(&rel);
                out.push(LocalModelFile {
                    name: rel,
                    size_bytes,
                    role,
                });
            }
        }
        Ok(())
    }

    /// Удаляет файл модели по относительному пути (или пустую цепочку каталогов).
    pub fn delete(&self, name: &str) -> AppResult<()> {
        let path = self.resolve_under_root(name)?;
        if path.is_file() {
            std::fs::remove_file(&path)?;
            // Подчистить пустые родительские каталоги (HF subdirs) до root.
            self.prune_empty_parents(&path);
            return Ok(());
        }
        if path.is_dir() {
            std::fs::remove_dir_all(&path)?;
            self.prune_empty_parents(&path);
            return Ok(());
        }
        Err(AppError::NotFound(format!("файл {name} не найден")))
    }

    fn prune_empty_parents(&self, path: &Path) {
        let mut cur = path.parent().map(Path::to_path_buf);
        while let Some(dir) = cur {
            if !dir.starts_with(&self.root) || dir == self.root {
                break;
            }
            let mut rd = match std::fs::read_dir(&dir) {
                Ok(rd) => rd,
                Err(_) => break,
            };
            if rd.next().is_none() {
                let _ = std::fs::remove_dir(&dir);
                cur = dir.parent().map(Path::to_path_buf);
            } else {
                break;
            }
        }
    }

    /// Скачивает файл модели по URL (streaming на диск) в `MODELS_DIR`.
    ///
    /// `name` — basename; вложенные пути не поддерживаются (для HF используйте
    /// [`download_hf`]). HF resolve-URL работают как обычные URL.
    pub async fn download(&self, url: &str, name: &str) -> AppResult<LocalModelFile> {
        let name = name.trim();
        if name.contains('/') || name.contains('\\') {
            return Err(AppError::BadRequest(
                "имя файла не должно содержать path-разделители (для HF каталогов — download_hf)"
                    .into(),
            ));
        }
        Self::validate_rel_path(name)?;
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

        let path = self.root.join(name);
        let tmp = self.root.join(format!("{name}.part"));
        let mut file = std::fs::File::create(&tmp)?;
        let mut stream = resp.bytes_stream();
        let mut total: u64 = 0;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| transport_error("local", e))?;
            use std::io::Write;
            file.write_all(&chunk)?;
            total += chunk.len() as u64;
        }
        drop(file);
        std::fs::rename(&tmp, &path)?;

        let role = Self::guess_role(name);
        Ok(LocalModelFile {
            name: name.to_string(),
            size_bytes: total,
            role,
        })
    }

    /// Скачивание с HuggingFace: `repo_id` (`org/name`) + опциональный файл.
    ///
    /// - `filename: Some` → resolve URL `https://huggingface.co/{repo}/resolve/main/{file}`
    ///   (имя файла — basename или `dest_name`).
    /// - `filename: None` → CLI `hf download {repo} --local-dir {root}/{safe}`
    ///   (полный репозиторий; кешируется под HF).
    pub async fn download_hf(
        &self,
        repo_id: &str,
        filename: Option<&str>,
        dest_name: Option<&str>,
    ) -> AppResult<LocalModelFile> {
        let repo = repo_id.trim().trim_matches('/');
        if !is_hf_repo_id(repo) {
            return Err(AppError::BadRequest(format!(
                "некорректный HF repo id: `{repo_id}` (ожидается org/name)"
            )));
        }
        match filename.map(str::trim).filter(|s| !s.is_empty()) {
            Some(file) => {
                Self::validate_rel_path(file)?;
                let file_trim = file.trim_start_matches('/');
                let url = format!(
                    "https://huggingface.co/{repo}/resolve/main/{}",
                    file_trim.trim_start_matches('/')
                );
                let base = dest_name
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .unwrap_or_else(|| {
                        Path::new(file_trim)
                            .file_name()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_else(|| file_trim.to_string())
                    });
                self.download(&url, &base).await
            }
            None => self.download_hf_repo(repo).await,
        }
    }

    async fn download_hf_repo(&self, repo: &str) -> AppResult<LocalModelFile> {
        std::fs::create_dir_all(&self.root)?;
        let safe = repo.replace('/', "__");
        let dest = self.root.join(&safe);
        std::fs::create_dir_all(&dest)?;

        let output = tokio::process::Command::new("hf")
            .args([
                "download",
                repo,
                "--local-dir",
                dest.to_string_lossy().as_ref(),
            ])
            .output()
            .await
            .map_err(|e| {
                AppError::BadRequest(format!(
                    "не удалось запустить CLI `hf` (установите huggingface-cli): {e}"
                ))
            })?;
        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            let err = if err.trim().is_empty() {
                String::from_utf8_lossy(&output.stdout)
            } else {
                err
            };
            return Err(AppError::upstream(
                "huggingface",
                format!(
                    "hf download failed: {}",
                    err.trim().chars().take(400).collect::<String>()
                ),
            ));
        }

        // Итоговый размер/имя — по первому найденному файлу (или каталогу).
        let mut files = Vec::new();
        self.walk(&dest, &mut files)?;
        let size = files.iter().map(|f| f.size_bytes).sum();
        let role = Self::guess_role(repo);
        Ok(LocalModelFile {
            name: safe,
            size_bytes: size,
            role,
        })
    }

    /// Резолвит относительный путь модели к абсолютному (для инференса).
    pub fn resolve_model_path(&self, model_key: &str) -> AppResult<PathBuf> {
        let key = model_key.trim();
        Self::validate_rel_path(key)?;
        let path = self.root.join(key);
        if !path.exists() {
            return Err(AppError::NotFound(format!(
                "локальная модель `{key}` не найдена в {}",
                self.root.display()
            )));
        }
        Ok(path)
    }
}

/// `org/name` — два непустых сегмента, без схемы и без пробелов.
pub fn is_hf_repo_id(s: &str) -> bool {
    if s.contains("://") || s.contains(' ') {
        return false;
    }
    let parts: Vec<&str> = s.split('/').collect();
    parts.len() == 2
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        })
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
        // Nemotron ASR GGUF — STT, а не LLM.
        assert_eq!(
            LocalModelManager::guess_role("nemotron-3.5-asr-streaming-0.6b.q8_0.gguf"),
            ModelRole::Stt
        );
        assert_eq!(
            LocalModelManager::guess_role("nvidia__nemotron-3.5-asr-streaming-0.6b/config.json"),
            ModelRole::Stt
        );
    }

    #[test]
    fn list_is_recursive_and_skips_cache() {
        let root = temp_root("recursive");
        std::fs::write(root.join("top.bin"), b"a").unwrap();
        std::fs::create_dir_all(root.join("org__repo")).unwrap();
        std::fs::write(root.join("org__repo/model.gguf"), b"bb").unwrap();
        std::fs::create_dir_all(root.join(".cache")).unwrap();
        std::fs::write(root.join(".cache/hidden"), b"secret").unwrap();
        std::fs::create_dir_all(root.join("ru/ru_RU/irina")).unwrap();
        std::fs::write(root.join("ru/ru_RU/irina/v.onnx"), b"c").unwrap();

        let manager = LocalModelManager::new(&root, Client::new());
        let files = manager.list().unwrap();
        let names: Vec<_> = files.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"top.bin"));
        assert!(names.contains(&"org__repo/model.gguf"));
        assert!(names.contains(&"ru/ru_RU/irina/v.onnx"));
        assert!(!names.iter().any(|n| n.contains(".cache")));
        let _ = std::fs::remove_dir_all(&root);
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
        assert!(manager.delete("a/../b").is_err());
        assert!(manager.delete("").is_err());
        assert!(manager.delete("/etc/passwd").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn is_hf_repo_id_parses() {
        assert!(is_hf_repo_id("nvidia/nemotron-3.5-asr-streaming-0.6b"));
        assert!(is_hf_repo_id("org/name"));
        assert!(!is_hf_repo_id("https://huggingface.co/x"));
        assert!(!is_hf_repo_id("single"));
        assert!(!is_hf_repo_id("a/b/c"));
        assert!(!is_hf_repo_id("a b/c"));
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
    async fn download_hf_with_filename_builds_resolve_url() {
        async fn handler() -> Vec<u8> {
            b"gguf-bytes".to_vec()
        }
        // Мок: полный HF resolve path — чтобы не ходить наружу в тесте.
        let router = Router::new().route(
            "/nvidia/nemotron-3.5-asr-streaming-0.6b/resolve/main/m.gguf",
            get(handler),
        );
        let base = testkit::spawn(router).await;
        // download_hf хардкодит huggingface.co — для unit-теста вызываем download
        // с URL, который он построил бы (контракт).
        let root = temp_root("hf");
        let manager = LocalModelManager::new(&root, Client::new());
        let file = manager
            .download(
                &format!("{base}/nvidia/nemotron-3.5-asr-streaming-0.6b/resolve/main/m.gguf"),
                "nemotron-3.5-asr-streaming-0.6b.q8_0.gguf",
            )
            .await
            .unwrap();
        assert_eq!(file.size_bytes, 10);
        assert_eq!(file.role, ModelRole::Stt);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn download_hf_rejects_bad_repo() {
        let root = temp_root("hf-bad");
        let manager = LocalModelManager::new(&root, Client::new());
        let err = manager
            .download_hf("not-a-repo", Some("file.bin"), None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("org/name"), "{err}");
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
