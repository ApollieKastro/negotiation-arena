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
    #[serde(skip_serializing)]
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
