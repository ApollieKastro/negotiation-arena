//! Конфигурация приложения: загрузка из окружения (.env) и валидация.

use std::path::PathBuf;

use crate::error::{AppError, AppResult};

/// Корневая конфигурация. Читается один раз при старте, дальше неизменна.
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub security: SecurityConfig,
    pub storage: StorageConfig,
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone)]
pub struct SecurityConfig {
    /// Секрет подписи JWT.
    pub jwt_secret: String,
    /// Время жизни токена, секунды.
    pub jwt_ttl_seconds: u64,
    /// Пароль начального администратора (только при первом запуске).
    pub admin_password: String,
    /// Секрет, из которого выводится ключ шифрования API-ключей провайдеров.
    /// В продакшене задавайте явный `ENCRYPTION_KEY`, а не `JWT_SECRET`.
    pub encryption_secret: String,
}

#[derive(Debug, Clone)]
pub struct StorageConfig {
    pub db_path: PathBuf,
    /// Каталог для скачиваемых локальных моделей (STT/TTS).
    pub models_dir: PathBuf,
}

impl AppConfig {
    pub fn from_env() -> AppResult<Self> {
        let config = Self {
            server: ServerConfig {
                host: env_or("HOST", "0.0.0.0"),
                port: env_parse("PORT", 3001)?,
            },
            security: SecurityConfig {
                jwt_secret: env_or("JWT_SECRET", "dev-only-jwt-secret-change-me-in-production"),
                jwt_ttl_seconds: env_parse("JWT_TTL_SECONDS", 86_400)?,
                admin_password: env_or("ADMIN_PASSWORD", "admin123"),
                encryption_secret: env_or(
                    "ENCRYPTION_KEY",
                    // Фоллбэк: ключ шифрования наследуем от JWT-секрета.
                    // Этого достаточно для разработки, но в проде задайте ENCRYPTION_KEY.
                    &env_or("JWT_SECRET", "dev-only-jwt-secret-change-me-in-production"),
                ),
            },
            storage: StorageConfig {
                db_path: PathBuf::from(env_or("DB_PATH", "negotiation_arena.db")),
                models_dir: PathBuf::from(env_or("MODELS_DIR", "models")),
            },
        };

        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> AppResult<()> {
        if self.server.host.trim().is_empty() {
            return Err(AppError::Config("HOST не может быть пустым".into()));
        }
        if self.security.jwt_secret.len() < 16 {
            return Err(AppError::Config(
                "JWT_SECRET должен быть не короче 16 символов".into(),
            ));
        }
        if self.security.admin_password.len() < 6 {
            return Err(AppError::Config(
                "ADMIN_PASSWORD должен быть не короче 6 символов".into(),
            ));
        }
        if self.security.jwt_ttl_seconds < 60 {
            return Err(AppError::Config(
                "JWT_TTL_SECONDS должен быть не меньше 60".into(),
            ));
        }
        Ok(())
    }
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn env_parse<T: std::str::FromStr>(key: &str, default: T) -> AppResult<T> {
    match std::env::var(key) {
        Ok(raw) if !raw.trim().is_empty() => raw.trim().parse::<T>().map_err(|_| {
            AppError::Config(format!(
                "переменная {key} содержит неверное значение: {raw}"
            ))
        }),
        _ => Ok(default),
    }
}
