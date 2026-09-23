//! Конфигурация приложения: загрузка из окружения (.env) и валидация.

use std::path::PathBuf;

use crate::error::{AppError, AppResult};

/// Минимальная длина пароля: единый полис для `ADMIN_PASSWORD` и регистрации.
pub const MIN_PASSWORD_LEN: usize = 6;

/// Корневая конфигурация. Читается один раз при старте, дальше неизменна.
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub security: SecurityConfig,
    pub storage: StorageConfig,
    pub rate_limit: RateLimitConfig,
    pub lockout: LockoutConfig,
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
    /// Разрешённые CORS-origin (например `https://app.example.com`).
    /// Пусто — `Any` (удобно в dev; в проде задайте allowlist через `ALLOWED_ORIGINS`).
    pub allowed_origins: Vec<String>,
    /// Окно, в течение которого ещё можно продлить сессию через refresh
    /// после exp access-токена (от `iat`). Обычно ≥ `jwt_ttl_seconds`.
    pub jwt_refresh_max_age_secs: u64,
}

#[derive(Debug, Clone)]
pub struct StorageConfig {
    pub db_path: PathBuf,
    /// Каталог для скачиваемых локальных моделей (STT/TTS).
    pub models_dir: PathBuf,
}

/// Rate-limit на публичные auth-эндпоинты (login/register/refresh) по IP.
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Максимум запросов на IP в окне. `0` — отключено (удобно в тестах).
    pub auth_max: u32,
    /// Длина окна, секунды.
    pub auth_window_secs: u64,
}

/// Lockout учётной записи после серии неудачных входов (per-login, БД).
#[derive(Debug, Clone)]
pub struct LockoutConfig {
    /// Неудачных попыток до блокировки. `0` — отключено (удобно в тестах).
    pub max_failures: u32,
    /// Окно накопления неудач, секунды (счётчик сбрасывается после паузы).
    pub window_secs: u64,
    /// Длительность блокировки после превышения лимита, секунды.
    pub lockout_secs: u64,
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
                allowed_origins: env_list("ALLOWED_ORIGINS"),
                jwt_refresh_max_age_secs: env_parse("JWT_REFRESH_MAX_AGE_SECONDS", 604_800)?,
            },
            storage: StorageConfig {
                db_path: PathBuf::from(env_or("DB_PATH", "negotiation_arena.db")),
                models_dir: PathBuf::from(env_or("MODELS_DIR", "models")),
            },
            rate_limit: RateLimitConfig {
                auth_max: env_parse("AUTH_RATE_LIMIT_MAX", 20)?,
                auth_window_secs: env_parse("AUTH_RATE_LIMIT_WINDOW_SECS", 60)?,
            },
            lockout: LockoutConfig {
                max_failures: env_parse("LOGIN_LOCKOUT_MAX_FAILURES", 5)?,
                window_secs: env_parse("LOGIN_LOCKOUT_WINDOW_SECS", 900)?,
                lockout_secs: env_parse("LOGIN_LOCKOUT_DURATION_SECS", 900)?,
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
        if self.security.admin_password.len() < MIN_PASSWORD_LEN {
            return Err(AppError::Config(format!(
                "ADMIN_PASSWORD должен быть не короче {MIN_PASSWORD_LEN} символов"
            )));
        }
        if self.security.jwt_ttl_seconds < 60 {
            return Err(AppError::Config(
                "JWT_TTL_SECONDS должен быть не меньше 60".into(),
            ));
        }
        if self.security.jwt_refresh_max_age_secs < self.security.jwt_ttl_seconds {
            return Err(AppError::Config(format!(
                "JWT_REFRESH_MAX_AGE_SECONDS ({}) должен быть не меньше JWT_TTL_SECONDS ({})",
                self.security.jwt_refresh_max_age_secs, self.security.jwt_ttl_seconds
            )));
        }
        if self.rate_limit.auth_max > 0 && self.rate_limit.auth_window_secs == 0 {
            return Err(AppError::Config(
                "AUTH_RATE_LIMIT_WINDOW_SECS должен быть больше 0, если лимит включён".into(),
            ));
        }
        if self.lockout.max_failures > 0
            && (self.lockout.window_secs == 0 || self.lockout.lockout_secs == 0)
        {
            return Err(AppError::Config(
                "LOGIN_LOCKOUT_WINDOW_SECS и LOGIN_LOCKOUT_DURATION_SECS должны быть больше 0, если lockout включён".into(),
            ));
        }
        for origin in &self.security.allowed_origins {
            if origin != "*" && !origin.starts_with("http://") && !origin.starts_with("https://") {
                return Err(AppError::Config(format!(
                    "ALLOWED_ORIGINS: некорректный origin `{origin}` (ожидается http(s)://…)"
                )));
            }
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

/// Список значений через запятую: `A, B , C` → `["A", "B", "C"]`.
fn env_list(key: &str) -> Vec<String> {
    env_or(key, "")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
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
