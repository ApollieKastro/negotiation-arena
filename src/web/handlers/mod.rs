//! HTTP-обработчики `/api/v1`: тонкая обёртка над прикладными сервисами.
//!
//! Хендлеры не содержат бизнес-логики: извлекают запрос, передают контекст
//! и сервисный вызов, отдают JSON. Ошибки — [`AppError`](crate::error::AppError).

pub mod audit;
pub mod auth;
pub mod model_prefs;
pub mod providers;
pub mod scenarios;
pub mod sessions;
pub mod settings;
pub mod stats;
pub mod users;
pub mod voice;
