//! Провайдеры ИИ, их модели и назначения по ролям.

use serde::{Deserialize, Serialize};

use crate::domain::entities::model::{ModelRole, ProviderKind};

/// Провайдер: endpoint + API-ключ (храним зашифрованным).
///
/// `api_key_encrypted` — непрозрачная шифротекста (AES-256-GCM), наружу
/// никогда не отдаётся; для UI используется `api_key_hint`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub kind: ProviderKind,
    pub base_url: Option<String>,
    /// Шифротекст ключа: не сериализуется наружу и не принимается из запроса
    /// (пишется только сервисом через шифрование).
    #[serde(skip_serializing, skip_deserializing)]
    pub api_key_encrypted: Option<String>,
    /// Маска ключа для UI, например `••••abcd`.
    pub api_key_hint: Option<String>,
    pub is_enabled: bool,
    pub created_at: String,
    pub updated_at: Option<String>,
}

/// Модель, доступная у провайдера.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRecord {
    pub id: String,
    pub provider_id: String,
    pub role: ModelRole,
    pub model_key: String,
    pub display_name: String,
    pub is_enabled: bool,
    /// JSON с доп. данными: голоса, размер, примечания.
    pub metadata: serde_json::Value,
    pub created_at: String,
}

/// Какая модель используется по роли (`llm` / `stt` / `tts`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleAssignment {
    pub role: ModelRole,
    pub model_id: String,
    pub updated_at: String,
}

/// Пользовательское предпочтение модели для роли.
///
/// Отсутствие записи = используется глобальное назначение роли
/// ([`RoleAssignment`]). Название/ключ модели присоединяются из `models`
/// и `providers`, чтобы UI не делал N+1 запросов.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserModelPreference {
    pub user_id: String,
    pub role: ModelRole,
    pub model_id: String,
    pub model_key: String,
    pub model_display_name: String,
    pub provider_id: String,
    pub provider_name: String,
    pub updated_at: String,
}
