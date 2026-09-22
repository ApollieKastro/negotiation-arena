//! Инфраструктурный слой: адаптеры к внешнему миру.
//!
//! * `db`         — SQLite, версионные миграции, репозитории
//! * `crypto`     — шифрование API-ключей провайдеров (AES-256-GCM)
//! * `providers`  — адаптеры LLM/STT/TTS (этап 2)

pub mod crypto;
pub mod db;
pub mod providers;
