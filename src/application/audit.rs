//! Аудит-лог: чтение записей для админ-панели (этап 5).

use std::sync::Arc;

use crate::application::auth::{AuthContext, Permission};
use crate::domain::entities::audit::AuditEntry;
use crate::domain::ports::AuditRepository;
use crate::error::AppResult;
use crate::infrastructure::db::repos::SqliteRepos;

/// Верхняя граница `limit` (защита от раздувания выборки).
const MAX_AUDIT_LIMIT: u32 = 200;

/// Дефолтный `limit` для списка аудита.
pub const DEFAULT_AUDIT_LIMIT: u32 = 50;

/// Просмотр аудит-лога (RBAC: `ViewAuditLog`, только admin).
pub struct AuditService {
    repos: Arc<SqliteRepos>,
}

impl AuditService {
    pub fn new(repos: Arc<SqliteRepos>) -> Self {
        Self { repos }
    }

    /// Последние события: `limit` clamp 1..=200, фильтры `user_id` / `action`.
    pub fn list(
        &self,
        actor: &AuthContext,
        limit: u32,
        user_id: Option<&str>,
        action: Option<&str>,
    ) -> AppResult<Vec<AuditEntry>> {
        actor.require(Permission::ViewAuditLog)?;
        let limit = limit.clamp(1, MAX_AUDIT_LIMIT);
        self.repos.audit.list(limit, user_id, action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::testsupport::{ctx, setup};
    use crate::domain::entities::user::UserRole;

    #[test]
    fn admin_can_list_audit_and_user_is_forbidden() {
        let (_db, svc) = setup().unwrap();
        svc.repos
            .audit
            .append(Some("u1"), "auth.login", None, None, None)
            .unwrap();
        svc.repos
            .audit
            .append(Some("u1"), "user.role", Some("user"), Some("u2"), None)
            .unwrap();

        let admin = ctx("a1", UserRole::Admin);
        let entries = svc.audit.list(&admin, 50, None, None).unwrap();
        assert_eq!(entries.len(), 2);
        // Свежие сверху (created_at может совпасть в одну миллисекунду —
        // проверяем состав, а не порядок).
        assert!(entries.iter().any(|e| e.action == "auth.login"));

        let user = ctx("u1", UserRole::User);
        let err = svc.audit.list(&user, 50, None, None).unwrap_err();
        assert!(matches!(err, crate::error::AppError::Forbidden(_)));
    }

    #[test]
    fn audit_filters_by_action_and_user() {
        let (_db, svc) = setup().unwrap();
        svc.repos
            .audit
            .append(Some("u1"), "auth.login", None, None, None)
            .unwrap();
        svc.repos
            .audit
            .append(
                Some("u2"),
                "session.start",
                Some("session"),
                Some("s1"),
                None,
            )
            .unwrap();

        let admin = ctx("a1", UserRole::Admin);
        let by_action = svc
            .audit
            .list(&admin, 50, None, Some("auth.login"))
            .unwrap();
        assert_eq!(by_action.len(), 1);
        assert_eq!(by_action[0].action, "auth.login");

        let by_user = svc.audit.list(&admin, 50, Some("u2"), None).unwrap();
        assert_eq!(by_user.len(), 1);
        assert_eq!(by_user[0].user_id.as_deref(), Some("u2"));

        let none = svc.audit.list(&admin, 50, Some("ghost"), None).unwrap();
        assert!(none.is_empty());
    }

    #[test]
    fn audit_limit_is_clamped() {
        let (_db, svc) = setup().unwrap();
        for i in 0..5 {
            svc.repos
                .audit
                .append(None, "test.event", None, Some(&i.to_string()), None)
                .unwrap();
        }
        let admin = ctx("a1", UserRole::Admin);
        // limit 0 → clamp to 1; huge limit → MAX.
        assert_eq!(svc.audit.list(&admin, 0, None, None).unwrap().len(), 1);
        assert_eq!(
            svc.audit.list(&admin, u32::MAX, None, None).unwrap().len(),
            5
        );
    }
}
