//! Аутентификация (JWT) и авторизация (RBAC) без дыр.
//!
//! Решения:
//! * пароли — Argon2id через [`hash_password`]/[`verify_password`];
//! * токен — JWT (`sub`, `login`, `role`, `iat`, `exp`);
//! * при каждом запросе [`AuthService::verify`] роль читается из БД —
//!   смена роли действует сразу, без переиздания токена;
//! * права заданы явной таблицей [`authorize`], без `match` с `_ => true`.

use std::sync::Arc;

use chrono::Utc;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

use crate::domain::entities::user::{User, UserRole};
use crate::domain::ports::{AuditRepository, UserRepository};
use crate::error::{AppError, AppResult};
use crate::infrastructure::crypto::password::{hash_password, verify_password};
use crate::infrastructure::db::repos::SqliteRepos;

/// Минимальная длина пароля (синхронизировано с `ADMIN_PASSWORD` в config).
pub const MIN_PASSWORD_LEN: usize = 6;

// ─────────────────────────────────────────────────────────────
// Права (RBAC)
// ─────────────────────────────────────────────────────────────

/// Действие, требующее права.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    /// Проходить сценарии.
    Play,
    /// Смотреть свою статистику.
    ViewOwnStats,
    /// Свои настройки UI.
    ManageOwnSettings,
    /// Сценарии (admin).
    ManageScenarios,
    /// Пользователи (admin).
    ManageUsers,
    /// Провайдеры и ключи (admin).
    ManageProviders,
    /// Глобальные настройки платформы (admin).
    ManagePlatformSettings,
    /// Сводная статистика и лидерборд (admin).
    ViewAllStats,
}

impl Permission {
    pub fn title(&self) -> &'static str {
        match self {
            Permission::Play => "прохождение сценариев",
            Permission::ViewOwnStats => "своя статистика",
            Permission::ManageOwnSettings => "свои настройки",
            Permission::ManageScenarios => "управление сценариями",
            Permission::ManageUsers => "управление пользователями",
            Permission::ManageProviders => "управление провайдерами",
            Permission::ManagePlatformSettings => "настройки платформы",
            Permission::ViewAllStats => "общая статистика",
        }
    }
}

/// Проверка права роли. Админ — всё; пользователь — только свои операции.
///
/// Нет ветки «по умолчанию разрешено»: новый [`Permission`] без описания
/// в `User` автоматически запрещён.
pub fn authorize(role: UserRole, permission: Permission) -> AppResult<()> {
    let allowed = match role {
        UserRole::Admin => true,
        UserRole::User => matches!(
            permission,
            Permission::Play | Permission::ViewOwnStats | Permission::ManageOwnSettings
        ),
    };

    if allowed {
        Ok(())
    } else {
        Err(AppError::Forbidden(format!(
            "недостаточно прав: {}",
            permission.title()
        )))
    }
}

// ─────────────────────────────────────────────────────────────
// Контекст и токены
// ─────────────────────────────────────────────────────────────

/// Аутентифицированный контекст запроса (из middleware или login).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthContext {
    pub user_id: String,
    pub login: String,
    pub role: UserRole,
}

impl AuthContext {
    /// Проверяет право и возвращает `Forbidden` при отказе.
    pub fn require(&self, permission: Permission) -> AppResult<()> {
        authorize(self.role, permission)
    }

    pub fn is_admin(&self) -> bool {
        self.role == UserRole::Admin
    }
}

/// Результат входа: JWT + срок жизни + профиль.
#[derive(Debug, Clone, Serialize)]
pub struct AuthSession {
    pub token: String,
    pub expires_in: u64,
    pub user: User,
}

/// Полезная нагрузка JWT.
#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,
    login: String,
    role: String,
    iat: usize,
    exp: usize,
}

// ─────────────────────────────────────────────────────────────
// AuthService
// ─────────────────────────────────────────────────────────────

/// Аутентификация, выдача/проверка JWT, администрирование пользователей.
pub struct AuthService {
    repos: Arc<SqliteRepos>,
    jwt_secret: String,
    jwt_ttl_seconds: u64,
}

impl AuthService {
    pub fn new(
        repos: Arc<SqliteRepos>,
        jwt_secret: impl Into<String>,
        jwt_ttl_seconds: u64,
    ) -> Self {
        Self {
            repos,
            jwt_secret: jwt_secret.into(),
            jwt_ttl_seconds,
        }
    }

    // ── Публичные операции ──

    /// Регистрирует пользователя с ролью `user`.
    pub fn register(
        &self,
        login: &str,
        password: &str,
        display_name: Option<&str>,
    ) -> AppResult<User> {
        let login = normalize_login(login)?;
        validate_password(password)?;
        self.create_user_internal(&login, password, UserRole::User, display_name)
    }

    /// Вход по логину и паролю. Ошибки не раскрывают, существует ли логин.
    ///
    /// Логин нормализуется так же, как при регистрации (`trim` + lowercase),
    /// иначе `Alice` / `alice` — разные учётки. При неизвестном логине
    /// выполняется dummy-проверка пароля, чтобы время ответа не выдавало
    /// existence-oracle.
    pub fn login(&self, login: &str, password: &str) -> AppResult<AuthSession> {
        let login = login.trim().to_lowercase();
        if login.is_empty() || password.is_empty() {
            return Err(AppError::Unauthorized("неверный логин или пароль".into()));
        }

        let found = self.repos.users.by_login(&login)?;
        let Some(with_secret) = found else {
            crate::infrastructure::crypto::password::dummy_verify(password);
            self.audit(None, "auth.login_failed", None, Some(login.as_str()), None);
            return Err(AppError::Unauthorized("неверный логин или пароль".into()));
        };
        if !verify_password(password, &with_secret.password_hash) {
            self.audit(
                Some(&with_secret.user.id),
                "auth.login_failed",
                None,
                None,
                None,
            );
            return Err(AppError::Unauthorized("неверный логин или пароль".into()));
        }
        if !with_secret.user.is_active {
            return Err(AppError::Unauthorized("учётная запись отключена".into()));
        }

        self.audit(Some(&with_secret.user.id), "auth.login", None, None, None);
        self.issue_token(with_secret.user)
    }

    /// Продлевает валидный токен: проверяет подпись и активность пользователя.
    pub fn refresh(&self, token: &str) -> AppResult<AuthSession> {
        let claims = self.decode(token)?;
        let user = self
            .repos
            .users
            .by_id(&claims.sub)?
            .ok_or_else(|| AppError::Unauthorized("пользователь не найден".into()))?;
        if !user.is_active {
            return Err(AppError::Unauthorized("учётная запись отключена".into()));
        }
        self.issue_token(user)
    }

    /// Проверяет токен и строит [`AuthContext`].
    ///
    /// Роль берётся из БД, а не из токена: разжалование действует сразу.
    pub fn verify(&self, token: &str) -> AppResult<AuthContext> {
        let claims = self.decode(token)?;
        let user = self
            .repos
            .users
            .by_id(&claims.sub)?
            .ok_or_else(|| AppError::Unauthorized("сессия недействительна".into()))?;
        if !user.is_active {
            return Err(AppError::Unauthorized("учётная запись отключена".into()));
        }
        Ok(AuthContext {
            user_id: user.id,
            login: user.login,
            role: user.role,
        })
    }

    // ── Управление пользователями (RBAC: ManageUsers) ──

    pub fn create_user(
        &self,
        actor: &AuthContext,
        login: &str,
        password: &str,
        role: UserRole,
        display_name: Option<&str>,
    ) -> AppResult<User> {
        actor.require(Permission::ManageUsers)?;
        let login = normalize_login(login)?;
        validate_password(password)?;
        let user = self.create_user_internal(&login, password, role, display_name)?;
        self.audit(
            Some(&actor.user_id),
            "user.create",
            Some("user"),
            Some(&user.id),
            Some(login.as_str()),
        );
        Ok(user)
    }

    pub fn list_users(&self, actor: &AuthContext) -> AppResult<Vec<User>> {
        actor.require(Permission::ManageUsers)?;
        self.repos.users.list()
    }

    /// Меняет роль. Нельзя менять роль самому себе (защита от lockout).
    pub fn set_role(&self, actor: &AuthContext, user_id: &str, role: UserRole) -> AppResult<()> {
        actor.require(Permission::ManageUsers)?;
        if actor.user_id == user_id {
            return Err(AppError::BadRequest(
                "нельзя изменить собственную роль".into(),
            ));
        }
        if self.repos.users.by_id(user_id)?.is_none() {
            return Err(AppError::NotFound("пользователь не найден".into()));
        }
        self.repos.users.update_role(user_id, role)?;
        self.audit(
            Some(&actor.user_id),
            "user.role",
            Some("user"),
            Some(user_id),
            Some(role.slug()),
        );
        Ok(())
    }

    /// Включает/отключает учётную запись. Нельзя отключить самого себя.
    pub fn set_active(&self, actor: &AuthContext, user_id: &str, is_active: bool) -> AppResult<()> {
        actor.require(Permission::ManageUsers)?;
        if actor.user_id == user_id && !is_active {
            return Err(AppError::BadRequest(
                "нельзя отключить собственную учётную запись".into(),
            ));
        }
        if self.repos.users.by_id(user_id)?.is_none() {
            return Err(AppError::NotFound("пользователь не найден".into()));
        }
        self.repos.users.set_active(user_id, is_active)?;
        self.audit(
            Some(&actor.user_id),
            if is_active {
                "user.enable"
            } else {
                "user.disable"
            },
            Some("user"),
            Some(user_id),
            None,
        );
        Ok(())
    }

    /// Удаляет пользователя (репозиторий защищает записи `admin`).
    pub fn delete_user(&self, actor: &AuthContext, user_id: &str) -> AppResult<()> {
        actor.require(Permission::ManageUsers)?;
        if actor.user_id == user_id {
            return Err(AppError::BadRequest("нельзя удалить самого себя".into()));
        }
        let target = self
            .repos
            .users
            .by_id(user_id)?
            .ok_or_else(|| AppError::NotFound("пользователь не найден".into()))?;
        // Репозиторий молча пропускает `role = admin` (DELETE ... AND role != 'admin'),
        // поэтому явный отказ здесь — иначе API отвечает ok, ничего не удалив.
        if target.role == UserRole::Admin {
            return Err(AppError::BadRequest(
                "нельзя удалить учётную запись администратора".into(),
            ));
        }
        self.repos.users.delete(user_id)?;
        self.audit(
            Some(&actor.user_id),
            "user.delete",
            Some("user"),
            Some(user_id),
            None,
        );
        Ok(())
    }

    /// Профиль по id (для админки и проверок).
    pub fn get_user(&self, actor: &AuthContext, user_id: &str) -> AppResult<User> {
        actor.require(Permission::ManageUsers)?;
        self.repos
            .users
            .by_id(user_id)?
            .ok_or_else(|| AppError::NotFound("пользователь не найден".into()))
    }

    // ── Внутреннее ──

    fn create_user_internal(
        &self,
        login: &str,
        password: &str,
        role: UserRole,
        display_name: Option<&str>,
    ) -> AppResult<User> {
        if self.repos.users.by_login(login)?.is_some() {
            return Err(AppError::Conflict(
                "пользователь с таким логином уже есть".into(),
            ));
        }
        let hash = hash_password(password);
        let display = display_name.map(str::trim).filter(|s| !s.is_empty());
        self.repos.users.create(login, &hash, role, display)
    }

    fn issue_token(&self, user: User) -> AppResult<AuthSession> {
        let now = Utc::now().timestamp();
        let exp = now + self.jwt_ttl_seconds as i64;
        let claims = Claims {
            sub: user.id.clone(),
            login: user.login.clone(),
            role: user.role.slug().to_string(),
            iat: now.max(0) as usize,
            exp: exp.max(0) as usize,
        };
        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(self.jwt_secret.as_bytes()),
        )
        .map_err(|e| AppError::internal(format!("выдача JWT: {e}")))?;

        Ok(AuthSession {
            token,
            expires_in: self.jwt_ttl_seconds,
            user,
        })
    }

    fn decode(&self, token: &str) -> AppResult<Claims> {
        let token = token.trim();
        // Схема авторизации нечувствительна к регистру: `BEARER x` == `Bearer x`.
        let token = token
            .split_once(' ')
            .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
            .map(|(_, rest)| rest)
            .unwrap_or(token)
            .trim();
        decode::<Claims>(
            token,
            &DecodingKey::from_secret(self.jwt_secret.as_bytes()),
            &Validation::default(),
        )
        .map(|data| data.claims)
        .map_err(|_| AppError::Unauthorized("недействительный токен".into()))
    }

    fn audit(
        &self,
        user_id: Option<&str>,
        action: &str,
        entity: Option<&str>,
        entity_id: Option<&str>,
        details: Option<&str>,
    ) {
        if let Err(err) = self
            .repos
            .audit
            .append(user_id, action, entity, entity_id, details)
        {
            tracing::warn!(error = %err, action, "не удалось записать аудит");
        }
    }
}

fn normalize_login(login: &str) -> AppResult<String> {
    let login = login.trim().to_lowercase();
    if login.is_empty() {
        return Err(AppError::BadRequest("логин не может быть пустым".into()));
    }
    if login.len() < 3 {
        return Err(AppError::BadRequest(
            "логин должен быть не короче 3 символов".into(),
        ));
    }
    if !login
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        return Err(AppError::BadRequest(
            "логин: только латиница, цифры, `_`, `-`, `.`".into(),
        ));
    }
    Ok(login)
}

fn validate_password(password: &str) -> AppResult<()> {
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err(AppError::BadRequest(format!(
            "пароль должен быть не короче {MIN_PASSWORD_LEN} символов"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::testsupport::{can, ctx, setup};
    use crate::domain::ports::UserRepository;

    #[test]
    fn permission_matrix_has_no_holes_for_user() {
        // Пользователь: только свои операции.
        assert!(can(UserRole::User, Permission::Play));
        assert!(can(UserRole::User, Permission::ViewOwnStats));
        assert!(can(UserRole::User, Permission::ManageOwnSettings));

        assert!(!can(UserRole::User, Permission::ManageScenarios));
        assert!(!can(UserRole::User, Permission::ManageUsers));
        assert!(!can(UserRole::User, Permission::ManageProviders));
        assert!(!can(UserRole::User, Permission::ManagePlatformSettings));
        assert!(!can(UserRole::User, Permission::ViewAllStats));
    }

    #[test]
    fn admin_has_all_permissions() {
        for p in [
            Permission::Play,
            Permission::ViewOwnStats,
            Permission::ManageOwnSettings,
            Permission::ManageScenarios,
            Permission::ManageUsers,
            Permission::ManageProviders,
            Permission::ManagePlatformSettings,
            Permission::ViewAllStats,
        ] {
            assert!(can(UserRole::Admin, p), "админ должен иметь {:?}", p);
        }
    }

    #[test]
    fn login_unknown_user_and_wrong_password_are_indistinguishable() {
        let (_db, svc) = setup().unwrap();
        let a = svc.auth.login("ghost", "whatever").unwrap_err().to_string();
        svc.auth.register("alice", "secret1", None).unwrap();
        let b = svc.auth.login("alice", "wrong-pw").unwrap_err().to_string();
        assert_eq!(a, b, "ошибки входа должны совпадать");
    }

    #[test]
    fn login_success_returns_bearer_and_verify_matches_role() {
        let (_db, svc) = setup().unwrap();
        svc.auth.register("bob", "secret1", None).unwrap();
        let session = svc.auth.login("bob", "secret1").unwrap();
        assert!(!session.token.is_empty());
        assert_eq!(session.expires_in, 3600);

        let ctx = svc.auth.verify(&session.token).unwrap();
        assert_eq!(ctx.login, "bob");
        assert_eq!(ctx.role, UserRole::User);
    }

    #[test]
    fn inactive_user_cannot_login_or_verify() {
        let (_db, svc) = setup().unwrap();
        svc.auth.register("carol", "secret1", None).unwrap();
        svc.auth.register("root", "secret1", None).unwrap();

        // Повышаем root до админа через репозиторий, чтобы он мог блокировать.
        let root = svc.repos.users.by_login("root").unwrap().unwrap().user;
        svc.repos
            .users
            .update_role(&root.id, UserRole::Admin)
            .unwrap();
        let admin_ctx = svc
            .auth
            .verify(&svc.auth.login("root", "secret1").unwrap().token)
            .unwrap();

        let carol = svc.repos.users.by_login("carol").unwrap().unwrap().user;
        svc.auth.set_active(&admin_ctx, &carol.id, false).unwrap();

        // Вход после блокировки — отказ.
        assert!(svc.auth.login("carol", "secret1").is_err());
        // Токен, выпущенный до блокировки, всё ещё подписан верно, но verify
        // обязан отказать: активность проверяется из БД на каждый запрос.
        let pre_block_token = {
            // carol уже отключена — login не пройдёт; выпускаем токен напрямую
            // через refresh-путь нельзя, поэтому поднимаем активность, логинимся
            // и снова блокируем.
            svc.repos.users.set_active(&carol.id, true).unwrap();
            let tok = svc.auth.login("carol", "secret1").unwrap().token;
            svc.repos.users.set_active(&carol.id, false).unwrap();
            tok
        };
        assert!(
            svc.auth.verify(&pre_block_token).is_err(),
            "отключённый пользователь не должен проходить verify по старому токену"
        );
        // Мусорный токен — Unauthorized.
        assert!(svc.auth.verify("Bearer garbage").is_err());
    }

    #[test]
    fn login_is_case_insensitive_like_register() {
        let (_db, svc) = setup().unwrap();
        svc.auth.register("Masha", "secret1", None).unwrap();
        // Регистрация нормализует в lowercase; вход с другим регистром должен работать.
        let session = svc.auth.login("MASHA", "secret1").unwrap();
        assert_eq!(session.user.login, "masha");
        assert!(svc.auth.login("  masha  ", "secret1").is_ok());
    }

    #[test]
    fn refresh_extends_valid_token() {
        let (_db, svc) = setup().unwrap();
        svc.auth.register("dave", "secret1", None).unwrap();
        let first = svc.auth.login("dave", "secret1").unwrap();
        let second = svc.auth.refresh(&first.token).unwrap();
        assert_eq!(second.user.id, first.user.id);
        assert!(!second.token.is_empty());
    }

    #[test]
    fn garbage_token_is_rejected() {
        let (_db, svc) = setup().unwrap();
        assert!(svc.auth.verify("not-a-jwt").is_err());
        assert!(svc.auth.refresh("").is_err());
    }

    #[test]
    fn bearer_scheme_is_case_insensitive() {
        let (_db, svc) = setup().unwrap();
        svc.auth.register("casey", "secret1", None).unwrap();
        let token = svc.auth.login("casey", "secret1").unwrap().token;

        // decode() strip_prefix: "Bearer " и "bearer " — равнозначны.
        for prefix in ["Bearer ", "bearer ", "BEARER ", "BeArEr "] {
            let full = format!("{prefix}{token}");
            assert!(
                svc.auth.verify(&full).is_ok(),
                "префикс {prefix:?} должен приниматься"
            );
        }
        // Без схемы — тоже работает (strip идёт опционально).
        assert!(svc.auth.verify(&token).is_ok());
        // Мусорная схема не равна Bearer.
        assert!(svc.auth.verify(&format!("Token {token}")).is_err());
    }

    #[test]
    fn duplicate_login_is_conflict() {
        let (_db, svc) = setup().unwrap();
        svc.auth.register("erin", "secret1", None).unwrap();
        let err = svc.auth.register("erin", "secret2", None).unwrap_err();
        assert!(matches!(err, AppError::Conflict(_)));
    }

    #[test]
    fn short_password_and_bad_login_are_rejected() {
        let (_db, svc) = setup().unwrap();
        assert!(svc.auth.register("ok", "12345", None).is_err());
        assert!(svc.auth.register("a", "secret1", None).is_err());
        assert!(svc.auth.register("bad login!", "secret1", None).is_err());
    }

    #[test]
    fn actor_without_permission_is_forbidden() {
        let user = ctx("u1", UserRole::User);
        let err = user.require(Permission::ManageUsers).unwrap_err();
        assert!(matches!(err, AppError::Forbidden(_)));
        // Админ проходит.
        ctx("a1", UserRole::Admin)
            .require(Permission::ManageUsers)
            .unwrap();
    }

    #[test]
    fn admin_can_manage_users_and_rbac_applies() {
        let (_db, svc) = setup().unwrap();
        // Создаём админа напрямую через репозиторий (seed роли — этап 1).
        let hash = crate::infrastructure::crypto::password::hash_password("secret1");
        svc.repos
            .users
            .create("root", &hash, UserRole::Admin, None)
            .unwrap();
        let root = svc.auth.login("root", "secret1").unwrap();
        let admin_ctx = svc.auth.verify(&root.token).unwrap();

        let created = svc
            .auth
            .create_user(
                &admin_ctx,
                "moderator",
                "secret1",
                UserRole::User,
                Some("Модератор"),
            )
            .unwrap();
        assert_eq!(created.role, UserRole::User);

        // Обычный пользователь не может листать пользователей.
        let plain = svc.auth.register("plain", "secret1", None).unwrap();
        let plain_ctx = svc
            .auth
            .verify(&svc.auth.login("plain", "secret1").unwrap().token)
            .unwrap();
        assert!(plain.id != created.id);
        assert!(svc.auth.list_users(&plain_ctx).is_err());

        // Админ может менять роль другому, но не себе.
        svc.auth
            .set_role(&admin_ctx, &created.id, UserRole::Admin)
            .unwrap();
        assert!(svc
            .auth
            .set_role(&admin_ctx, &admin_ctx.user_id, UserRole::User)
            .is_err());

        // Отключить себя нельзя; другого — можно.
        assert!(svc
            .auth
            .set_active(&admin_ctx, &admin_ctx.user_id, false)
            .is_err());
        svc.auth.set_active(&admin_ctx, &created.id, false).unwrap();

        // Отключённый не входит.
        assert!(svc.auth.login("moderator", "secret1").is_err());
    }
}
