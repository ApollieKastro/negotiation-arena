//! Запись аудит-лога (кто, что и когда сделал).

use serde::{Deserialize, Serialize};

/// Строка `audit_log` для админ-просмотра.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: String,
    /// Исполнитель; `None` — анонимное событие (например, неудачный вход).
    pub user_id: Option<String>,
    /// Код события: `auth.login`, `user.role`, `session.start`, …
    pub action: String,
    pub entity: Option<String>,
    pub entity_id: Option<String>,
    pub details: Option<String>,
    pub created_at: String,
}
