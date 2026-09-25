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

impl Provider {
    /// Нужен ли API-ключ именно **этому** провайдеру.
    ///
    /// Облачные OpenAI-совместимые endpoint'ы требуют ключ; локальные
    /// (Ollama, LM Studio, vLLM на `localhost` / приватной сети) — нет.
    /// Для `Local` / `Mock` ключ не нужен вовсе (см. [`ProviderKind::requires_api_key`]).
    pub fn requires_api_key(&self) -> bool {
        if !self.kind.requires_api_key() {
            return false;
        }
        if self.kind == ProviderKind::OpenAiCompatible {
            if let Some(base) = self.base_url.as_deref() {
                if !base.trim().is_empty() {
                    return !is_local_base_url(base);
                }
            }
        }
        true
    }
}

/// Хост `base_url` выглядит локальным/приватным → API-ключ не обязателен.
///
/// Понимает `localhost`, `127.0.0.0/8`, `[::1]`, RFC1918 (`10/8`, `172.16/12`, `192.168/16`).
/// Без крейта `url`: разбор достаточно грубый для решения «ключ или нет».
pub(crate) fn is_local_base_url(url: &str) -> bool {
    let raw = url.trim().to_ascii_lowercase();
    let without_scheme = raw
        .strip_prefix("https://")
        .or_else(|| raw.strip_prefix("http://"))
        .unwrap_or(&raw);

    // user:pass@host[:port]/path → host
    let after_auth = without_scheme.rsplit('@').next().unwrap_or(without_scheme);
    // IPv6 в скобках: [::1]:11434
    if after_auth.starts_with('[') {
        if let Some(end) = after_auth.find(']') {
            return &after_auth[..=end] == "[::1]";
        }
        return false;
    }
    let host = after_auth
        .split('/')
        .next()
        .unwrap_or(after_auth)
        .split(':')
        .next()
        .unwrap_or(after_auth);

    if host.is_empty() {
        return false;
    }
    if host == "localhost" || host == "127.0.0.1" || host == "::1" || host == "[::1]" {
        return true;
    }
    if let Some(rest) = host.strip_prefix("127.") {
        // 127.0.0.0/8 — любой octet
        if rest.split('.').count() == 3 {
            return rest
                .split('.')
                .all(|p| p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty());
        }
        return false;
    }
    private_ipv4(host)
}

/// `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`.
fn private_ipv4(host: &str) -> bool {
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    let mut octets = [0u8; 4];
    for (i, p) in parts.iter().enumerate() {
        if p.is_empty() || !p.chars().all(|c| c.is_ascii_digit()) {
            return false;
        }
        match p.parse::<u8>() {
            Ok(v) => octets[i] = v,
            Err(_) => return false,
        }
    }
    match octets {
        [10, ..] => true,
        [192, 168, ..] => true,
        [172, b, ..] => (16..=31).contains(&b),
        _ => false,
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entities::model::ProviderKind;

    fn provider(kind: ProviderKind, base_url: Option<&str>) -> Provider {
        Provider {
            id: "p".into(),
            name: "T".into(),
            kind,
            base_url: base_url.map(str::to_string),
            api_key_encrypted: None,
            api_key_hint: None,
            is_enabled: true,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: None,
        }
    }

    #[test]
    fn local_ollama_base_needs_no_api_key() {
        let p = provider(
            ProviderKind::OpenAiCompatible,
            Some("http://127.0.0.1:11434/v1"),
        );
        assert!(!p.requires_api_key());
        assert!(!provider(
            ProviderKind::OpenAiCompatible,
            Some("http://localhost:11434/v1")
        )
        .requires_api_key());
        assert!(!provider(
            ProviderKind::OpenAiCompatible,
            Some("http://192.168.1.10:11434/v1")
        )
        .requires_api_key());
    }

    #[test]
    fn cloud_openai_compatible_still_requires_key() {
        assert!(provider(
            ProviderKind::OpenAiCompatible,
            Some("https://api.openai.com/v1")
        )
        .requires_api_key());
        assert!(provider(ProviderKind::OpenAiCompatible, None).requires_api_key());
        assert!(provider(ProviderKind::OpenAiCompatible, Some("")).requires_api_key());
        assert!(
            provider(ProviderKind::Anthropic, Some("http://127.0.0.1:11434")).requires_api_key()
        );
    }

    #[test]
    fn local_and_mock_kinds_never_need_key() {
        assert!(!provider(ProviderKind::Local, Some("https://example.com")).requires_api_key());
        assert!(!provider(ProviderKind::Mock, None).requires_api_key());
    }

    #[test]
    fn is_local_base_url_parses_common_forms() {
        assert!(is_local_base_url("http://127.0.0.1:11434/v1"));
        assert!(is_local_base_url("https://localhost/v1"));
        assert!(is_local_base_url("http://[::1]:11434/v1"));
        assert!(is_local_base_url("http://10.0.0.5:8080"));
        assert!(is_local_base_url("http://172.20.0.3:11434"));
        assert!(!is_local_base_url("https://anymodel.org/v1"));
        assert!(!is_local_base_url("http://8.8.8.8/v1"));
    }
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
