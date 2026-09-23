//! Настройки: глобальные (платформа) и пользовательские (UI).

use std::sync::Arc;

use crate::application::auth::AuthContext;
use crate::domain::ports::{AuditRepository, SettingsRepository};
use crate::error::{AppError, AppResult};
use crate::infrastructure::db::repos::SqliteRepos;

/// Ключи глобальных настроек с понятными значениями по умолчанию.
pub mod global_keys {
    /// Название площадки в шапке.
    pub const SITE_NAME: &str = "platform.site_name";
    /// Тема по умолчанию для новых пользователей (`light` / `dark` / `system`).
    pub const DEFAULT_THEME: &str = "platform.default_theme";
    /// Размер шрифта по умолчанию (`sm` / `md` / `lg`).
    pub const DEFAULT_FONT_SIZE: &str = "platform.default_font_size";
    /// Язык интерфейса по умолчанию (`ru`).
    pub const DEFAULT_LOCALE: &str = "platform.default_locale";
    /// Максимум ходов в сессии (строка с числом).
    pub const MAX_TURNS: &str = "platform.max_turns";
    /// Дневной лимит LLM-токенов на пользователя (0 = лимит выключен).
    pub const LLM_DAILY_TOKEN_LIMIT: &str = "platform.llm_daily_token_limit";
}

/// Ключи пользовательских настроек UI (хранятся с префиксом `user:{id}:`).
pub mod user_keys {
    pub const THEME: &str = "theme";
    pub const FONT_SIZE: &str = "font_size";
    pub const LOCALE: &str = "locale";
    pub const SOUND_ENABLED: &str = "sound_enabled";
}

const DEFAULTS: &[(&str, &str)] = &[
    (global_keys::SITE_NAME, "Арена переговоров"),
    (global_keys::DEFAULT_THEME, "system"),
    (global_keys::DEFAULT_FONT_SIZE, "md"),
    (global_keys::DEFAULT_LOCALE, "ru"),
    (global_keys::MAX_TURNS, "40"),
    (global_keys::LLM_DAILY_TOKEN_LIMIT, "0"),
];

/// Доступ к настройкам.
pub struct SettingsService {
    repos: Arc<SqliteRepos>,
}

impl SettingsService {
    pub fn new(repos: Arc<SqliteRepos>) -> Self {
        Self { repos }
    }

    // ── Глобальные ──

    /// Читает глобальную настройку; при отсутствии — дефолт из [`DEFAULTS`].
    pub fn get_global(&self, key: &str) -> AppResult<String> {
        if let Some(value) = self.repos.settings.get(key)? {
            return Ok(value);
        }
        Ok(DEFAULTS
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.to_string())
            .unwrap_or_default())
    }

    /// Пишет глобальную настройку (admin).
    pub fn set_global(&self, actor: &AuthContext, key: &str, value: &str) -> AppResult<()> {
        actor.require(super::Permission::ManagePlatformSettings)?;
        let key = key.trim();
        if key.is_empty() {
            return Err(AppError::BadRequest(
                "ключ настройки не может быть пустым".into(),
            ));
        }
        if key.len() > MAX_KEY_CHARS {
            return Err(AppError::BadRequest(format!(
                "ключ настройки не длиннее {MAX_KEY_CHARS} символов"
            )));
        }
        if value.chars().count() > MAX_VALUE_CHARS {
            return Err(AppError::BadRequest(format!(
                "значение настройки не длиннее {MAX_VALUE_CHARS} символов"
            )));
        }
        if key.starts_with("user:") {
            return Err(AppError::BadRequest(
                "пользовательские ключи нельзя писать как глобальные".into(),
            ));
        }
        self.repos.settings.set(key, value)?;
        self.audit(actor, key);
        Ok(())
    }

    /// Все глобальные настройки (записанные + дефолты, не перезаписывая).
    pub fn all_global(&self) -> AppResult<Vec<(String, String)>> {
        let stored = self.repos.settings.all()?;
        let mut map: std::collections::BTreeMap<String, String> = DEFAULTS
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        for (k, v) in stored {
            if !k.starts_with("user:") {
                map.insert(k, v);
            }
        }
        Ok(map.into_iter().collect())
    }

    // ── Пользовательские ──

    /// Читает пользовательскую настройку: своя или (для админа) чужая.
    pub fn get_user(&self, actor: &AuthContext, user_id: &str, key: &str) -> AppResult<String> {
        if user_id != actor.user_id {
            actor.require(super::Permission::ManagePlatformSettings)?;
        } else {
            actor.require(super::Permission::ManageOwnSettings)?;
        }
        let storage_key = user_setting_key(user_id, key)?;
        match self.repos.settings.get(&storage_key)? {
            Some(v) => Ok(v),
            None => Ok(default_user_value(key).to_string()),
        }
    }

    /// Пишет пользовательскую настройку (только себе, либо admin любой).
    pub fn set_user(
        &self,
        actor: &AuthContext,
        user_id: &str,
        key: &str,
        value: &str,
    ) -> AppResult<()> {
        if user_id != actor.user_id {
            actor.require(super::Permission::ManagePlatformSettings)?;
        } else {
            actor.require(super::Permission::ManageOwnSettings)?;
        }
        let storage_key = user_setting_key(user_id, key)?;
        if value.chars().count() > MAX_VALUE_CHARS {
            return Err(AppError::BadRequest(format!(
                "значение настройки не длиннее {MAX_VALUE_CHARS} символов"
            )));
        }
        self.repos.settings.set(&storage_key, value)?;
        self.audit_with_action(actor, "settings.set_user", &storage_key);
        Ok(())
    }

    /// Пакетная выгрузка пользовательских настроек для UI.
    pub fn user_settings(
        &self,
        actor: &AuthContext,
        user_id: &str,
    ) -> AppResult<std::collections::BTreeMap<String, String>> {
        if user_id != actor.user_id {
            actor.require(super::Permission::ManagePlatformSettings)?;
        } else {
            actor.require(super::Permission::ManageOwnSettings)?;
        }

        let prefix = format!("user:{user_id}:");
        let mut out: std::collections::BTreeMap<String, String> = [
            (user_keys::THEME, default_user_value(user_keys::THEME)),
            (
                user_keys::FONT_SIZE,
                default_user_value(user_keys::FONT_SIZE),
            ),
            (user_keys::LOCALE, default_user_value(user_keys::LOCALE)),
            (
                user_keys::SOUND_ENABLED,
                default_user_value(user_keys::SOUND_ENABLED),
            ),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

        for (k, v) in self.repos.settings.all()? {
            if let Some(short) = k.strip_prefix(&prefix) {
                out.insert(short.to_string(), v);
            }
        }
        Ok(out)
    }

    fn audit(&self, actor: &AuthContext, key: &str) {
        self.audit_with_action(actor, "settings.set_global", key);
    }

    fn audit_with_action(&self, actor: &AuthContext, action: &str, key: &str) {
        if let Err(err) = self.repos.audit.append(
            Some(&actor.user_id),
            action,
            Some("setting"),
            Some(key),
            None,
        ) {
            tracing::warn!(error = %err, action, "не удалось записать аудит настроек");
        }
    }
}

/// Максимальная длина ключа настройки (символы).
const MAX_KEY_CHARS: usize = 128;

/// Максимальная длина значения настройки (символы).
const MAX_VALUE_CHARS: usize = 4096;

fn user_setting_key(user_id: &str, key: &str) -> AppResult<String> {
    let key = key.trim();
    if key.is_empty() || key.contains(':') {
        return Err(AppError::BadRequest(
            "некорректный ключ пользовательской настройки".into(),
        ));
    }
    if user_id.trim().is_empty() {
        return Err(AppError::BadRequest("не задан пользователь".into()));
    }
    Ok(format!("user:{user_id}:{key}"))
}

fn default_user_value(key: &str) -> &'static str {
    match key {
        user_keys::THEME => "system",
        user_keys::FONT_SIZE => "md",
        user_keys::LOCALE => "ru",
        user_keys::SOUND_ENABLED => "true",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::testsupport::{ctx, setup};
    use crate::domain::entities::user::UserRole;

    #[test]
    fn global_defaults_and_override() {
        let (_db, svc) = setup().unwrap();
        assert_eq!(
            svc.settings.get_global(global_keys::SITE_NAME).unwrap(),
            "Арена переговоров"
        );
        assert_eq!(
            svc.settings.get_global(global_keys::DEFAULT_THEME).unwrap(),
            "system"
        );
        assert_eq!(svc.settings.get_global("unknown.key").unwrap(), "");

        let admin = ctx("admin-1", UserRole::Admin);
        svc.settings
            .set_global(&admin, global_keys::SITE_NAME, "Новая арена")
            .unwrap();
        assert_eq!(
            svc.settings.get_global(global_keys::SITE_NAME).unwrap(),
            "Новая арена"
        );
    }

    #[test]
    fn plain_user_cannot_set_global() {
        let (_db, svc) = setup().unwrap();
        let user = ctx("u1", UserRole::User);
        let err = svc
            .settings
            .set_global(&user, global_keys::SITE_NAME, "x")
            .unwrap_err();
        assert!(matches!(err, AppError::Forbidden(_)));
    }

    #[test]
    fn reject_user_prefix_in_global_keys() {
        let (_db, svc) = setup().unwrap();
        let admin = ctx("admin-1", UserRole::Admin);
        assert!(svc
            .settings
            .set_global(&admin, "user:abc:theme", "dark")
            .is_err());
        assert!(svc.settings.set_global(&admin, "  ", "x").is_err());
    }

    #[test]
    fn user_settings_are_namespaced() {
        let (_db, svc) = setup().unwrap();
        let admin = ctx("admin-1", UserRole::Admin);
        let user = ctx("user-1", UserRole::User);

        assert_eq!(
            svc.settings
                .get_user(&user, &user.user_id, user_keys::THEME)
                .unwrap(),
            "system"
        );

        svc.settings
            .set_user(&user, &user.user_id, user_keys::THEME, "dark")
            .unwrap();
        assert_eq!(
            svc.settings
                .get_user(&user, &user.user_id, user_keys::THEME)
                .unwrap(),
            "dark"
        );

        // Чужой пользователь не пишет в чужие ключи без admin-права.
        let other = ctx("user-2", UserRole::User);
        assert!(svc
            .settings
            .set_user(&other, &user.user_id, user_keys::THEME, "light")
            .is_err());
        // Админ может.
        svc.settings
            .set_user(&admin, &user.user_id, user_keys::THEME, "light")
            .unwrap();

        let pack = svc.settings.user_settings(&user, &user.user_id).unwrap();
        assert_eq!(
            pack.get(user_keys::THEME).map(String::as_str),
            Some("light")
        );
        assert!(pack.contains_key(user_keys::FONT_SIZE));
    }

    #[test]
    fn bad_user_keys_rejected() {
        let (_db, svc) = setup().unwrap();
        let user = ctx("u1", UserRole::User);
        assert!(svc
            .settings
            .set_user(&user, &user.user_id, "bad:key", "v")
            .is_err());
        assert!(svc
            .settings
            .set_user(&user, &user.user_id, " ", "v")
            .is_err());
        // Слишком длинное значение — отказ, а не раздувание БД.
        let huge = "ы".repeat(MAX_VALUE_CHARS + 1);
        assert!(svc
            .settings
            .set_user(&user, &user.user_id, user_keys::THEME, &huge)
            .is_err());
        let huge_key = "k".repeat(MAX_KEY_CHARS + 1);
        assert!(svc
            .settings
            .set_global(&ctx("a9", UserRole::Admin), &huge_key, "v")
            .is_err());
    }

    #[test]
    fn all_global_includes_defaults_and_stored() {
        let (_db, svc) = setup().unwrap();
        let admin = ctx("admin-1", UserRole::Admin);
        svc.settings
            .set_global(&admin, "custom.flag", "on")
            .unwrap();

        let all = svc.settings.all_global().unwrap();
        assert!(all
            .iter()
            .any(|(k, v)| k == global_keys::SITE_NAME && v == "Арена переговоров"));
        assert!(all.iter().any(|(k, v)| k == "custom.flag" && v == "on"));
        assert!(!all.iter().any(|(k, _)| k.starts_with("user:")));
    }
}
