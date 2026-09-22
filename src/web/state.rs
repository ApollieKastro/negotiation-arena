//! Общее состояние приложения, передаваемое в обработчики через `State`.

use std::sync::Arc;

use crate::config::AppConfig;
use crate::infrastructure::{crypto::SecretCipher, db::Database};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub db: Arc<Database>,
    pub cipher: Arc<SecretCipher>,
}
