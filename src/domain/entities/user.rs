//! Пользователи, роли и права.

use serde::{Deserialize, Serialize};

/// Роль пользователя в системе.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserRole {
    Admin,
    User,
}

impl UserRole {
    pub fn slug(&self) -> &'static str {
        match self {
            UserRole::Admin => "admin",
            UserRole::User => "user",
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            UserRole::Admin => "Администратор",
            UserRole::User => "Пользователь",
        }
    }

    pub fn from_slug(slug: &str) -> Option<Self> {
        match slug {
            "admin" => Some(UserRole::Admin),
            "user" => Some(UserRole::User),
            _ => None,
        }
    }
}

/// Пользователь (без секретов — пароль и аватар хранятся отдельно).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub login: String,
    pub display_name: Option<String>,
    pub role: UserRole,
    pub is_active: bool,
    pub created_at: String,
    /// Есть ли загруженный аватар (байты не отдаются в JSON — только endpoint).
    #[serde(default)]
    pub has_avatar: bool,
}

/// Пользователь вместе с хешем пароля и состоянием lockout.
///
/// Возвращается только при аутентификации; хеш не сериализуется.
#[derive(Debug, Clone)]
pub struct UserWithSecret {
    pub user: User,
    pub password_hash: String,
    /// Число неудачных входов в текущем окне.
    pub failed_login_count: i64,
    /// RFC3339: последняя неудачная попытка (окно накопления).
    pub last_failed_login_at: Option<String>,
    /// RFC3339: учётка заблокирована до этого момента (`None` — не заблокирована).
    pub locked_until: Option<String>,
}

/// Состояние lockout после фиксации неудачной попытки.
#[derive(Debug, Clone, Default)]
pub struct LoginAttemptUpdate {
    pub failed_login_count: i64,
    pub last_failed_login_at: String,
    pub locked_until: Option<String>,
}

/// Определение роли и её прав.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Role {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub permissions: Vec<String>,
}
