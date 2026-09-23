//! Middleware и extractor'ы web-слоя: JWT-аутентификация, единые отказы валидации.

pub mod auth;
pub mod json;
pub mod multipart;
pub mod rate_limit;

pub use auth::AuthUser;
pub use json::{AppJson, AppQuery};
pub use multipart::AppMultipart;
