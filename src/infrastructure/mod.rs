//! Инфраструктурный слой: адаптеры к внешнему миру.
//!
//! * `db`         — SQLite, версионные миграции, репозитории, сиды
//! * `crypto`     — шифрование API-ключей (AES-256-GCM), хеши паролей
//! * `providers`  — адаптеры LLM/STT/TTS (этап 2)

pub mod crypto;
pub mod db;
pub mod providers;
