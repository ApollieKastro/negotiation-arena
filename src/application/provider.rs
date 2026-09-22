//! CRUD провайдеров, шифрование API-ключей, discovery, назначение ролей,
//! фасады LLM/STT/TTS через [`ProviderFactory`].

use std::sync::Arc;

use crate::application::auth::AuthContext;
use crate::domain::entities::model::{ModelDescriptor, ModelRole};
use crate::domain::entities::provider::{ModelRecord, Provider, RoleAssignment};
use crate::domain::ports::{AuditRepository, ChatModel, ModelCatalog, ProviderRepository};
use crate::error::{AppError, AppResult};
use crate::infrastructure::crypto::{mask_secret, SecretCipher};
use crate::infrastructure::db::repos::SqliteRepos;
use crate::infrastructure::providers::{ProviderFactory, ProviderHandle};

/// Управление провайдерами и моделями (RBAC: `ManageProviders`).
pub struct ProviderService {
    repos: Arc<SqliteRepos>,
    factory: Arc<ProviderFactory>,
    cipher: Arc<SecretCipher>,
}

impl ProviderService {
    pub fn new(
        repos: Arc<SqliteRepos>,
        factory: Arc<ProviderFactory>,
        cipher: Arc<SecretCipher>,
    ) -> Self {
        Self {
            repos,
            factory,
            cipher,
        }
    }

    // ── Провайдеры ──

    pub fn list(&self, actor: &AuthContext) -> AppResult<Vec<Provider>> {
        actor.require(super::Permission::ManageProviders)?;
        self.repos.providers.list()
    }

    pub fn get(&self, actor: &AuthContext, id: &str) -> AppResult<Provider> {
        actor.require(super::Permission::ManageProviders)?;
        self.repos
            .providers
            .get(id)?
            .ok_or_else(|| AppError::NotFound("провайдер не найден".into()))
    }

    /// Создаёт или обновляет провайдер.
    ///
    /// `api_key: None` — ключ не меняется; `Some("")` — очистить;
    /// `Some(value)` — зашифровать и сохранить.
    pub fn upsert(
        &self,
        actor: &AuthContext,
        mut provider: Provider,
        api_key: Option<&str>,
    ) -> AppResult<Provider> {
        actor.require(super::Permission::ManageProviders)?;
        if provider.name.trim().is_empty() {
            return Err(AppError::BadRequest(
                "имя провайдера не может быть пустым".into(),
            ));
        }
        provider.name = provider.name.trim().to_string();

        let existing = if provider.id.trim().is_empty() {
            None
        } else {
            self.repos.providers.get(&provider.id)?
        };

        if provider.id.trim().is_empty() {
            provider.id = uuid::Uuid::new_v4().to_string();
            provider.created_at = chrono::Utc::now().to_rfc3339();
        } else if let Some(prev) = &existing {
            provider.created_at = prev.created_at.clone();
        }

        match api_key {
            None => {
                // Сохраняем прежний шифротекст и маску.
                if let Some(prev) = &existing {
                    provider.api_key_encrypted = prev.api_key_encrypted.clone();
                    provider.api_key_hint = prev.api_key_hint.clone();
                }
            }
            Some(key) if key.trim().is_empty() => {
                provider.api_key_encrypted = None;
                provider.api_key_hint = None;
            }
            Some(key) => {
                provider.api_key_encrypted = Some(self.cipher.encrypt(key.trim())?);
                provider.api_key_hint = Some(mask_secret(key.trim()));
            }
        }

        if provider.kind.requires_api_key() && provider.api_key_encrypted.is_none() {
            return Err(AppError::BadRequest(
                "для этого типа провайдера нужен API-ключ".into(),
            ));
        }

        self.repos.providers.upsert(&provider)?;
        self.audit(
            actor,
            "provider.upsert",
            &provider.id,
            Some(provider.name.as_str()),
        );

        // Возвращаем без шифротекста — наружу секрет не уходит.
        provider.api_key_encrypted = None;
        Ok(provider)
    }

    pub fn delete(&self, actor: &AuthContext, id: &str) -> AppResult<()> {
        actor.require(super::Permission::ManageProviders)?;
        if self.repos.providers.get(id)?.is_none() {
            return Err(AppError::NotFound("провайдер не найден".into()));
        }
        self.repos.providers.delete(id)?;
        self.audit(actor, "provider.delete", id, None);
        Ok(())
    }

    /// Проверка соединения: `ping` каталога с расшифрованным ключом.
    pub async fn ping(&self, actor: &AuthContext, provider_id: &str) -> AppResult<()> {
        actor.require(super::Permission::ManageProviders)?;
        let (provider, handle) = self.build_for(provider_id)?;
        let catalog = handle.catalog.ok_or_else(|| {
            AppError::upstream(&provider.name, "у провайдера нет каталога моделей")
        })?;
        catalog.ping().await
    }

    /// Discovery моделей у провайдера для роли.
    pub async fn discover_models(
        &self,
        actor: &AuthContext,
        provider_id: &str,
        role: ModelRole,
    ) -> AppResult<Vec<ModelDescriptor>> {
        actor.require(super::Permission::ManageProviders)?;
        let (provider, handle) = self.build_for(provider_id)?;
        let catalog = handle.catalog.ok_or_else(|| {
            AppError::upstream(&provider.name, "у провайдера нет каталога моделей")
        })?;
        catalog.list_models(role).await
    }

    // ── Модели ──

    pub fn list_models(
        &self,
        actor: &AuthContext,
        provider_id: Option<&str>,
    ) -> AppResult<Vec<ModelRecord>> {
        actor.require(super::Permission::ManageProviders)?;
        self.repos.providers.list_models(provider_id)
    }

    pub fn upsert_model(&self, actor: &AuthContext, model: ModelRecord) -> AppResult<ModelRecord> {
        actor.require(super::Permission::ManageProviders)?;
        if model.model_key.trim().is_empty() {
            return Err(AppError::BadRequest(
                "ключ модели не может быть пустым".into(),
            ));
        }
        if self.repos.providers.get(&model.provider_id)?.is_none() {
            return Err(AppError::NotFound("провайдер не найден".into()));
        }

        let mut model = model;
        if model.id.trim().is_empty() {
            model.id = uuid::Uuid::new_v4().to_string();
            model.created_at = chrono::Utc::now().to_rfc3339();
        }
        model.model_key = model.model_key.trim().to_string();
        model.display_name = if model.display_name.trim().is_empty() {
            model.model_key.clone()
        } else {
            model.display_name.trim().to_string()
        };

        self.repos.providers.upsert_model(&model)?;
        self.audit(
            actor,
            "model.upsert",
            &model.id,
            Some(model.model_key.as_str()),
        );
        Ok(model)
    }

    pub fn delete_model(&self, actor: &AuthContext, id: &str) -> AppResult<()> {
        actor.require(super::Permission::ManageProviders)?;
        self.repos.providers.delete_model(id)?;
        self.audit(actor, "model.delete", id, None);
        Ok(())
    }

    // ── Назначения по ролям ──

    pub fn role_assignments(&self, actor: &AuthContext) -> AppResult<Vec<RoleAssignment>> {
        actor.require(super::Permission::ManageProviders)?;
        self.repos.providers.role_assignments()
    }

    /// Назначает модель на роль (`llm` / `stt` / `tts`).
    pub fn assign_role(
        &self,
        actor: &AuthContext,
        role: ModelRole,
        model_id: &str,
    ) -> AppResult<()> {
        actor.require(super::Permission::ManageProviders)?;
        let model = self
            .repos
            .providers
            .get_model(model_id)?
            .ok_or_else(|| AppError::NotFound("модель не найдена".into()))?;
        if model.role != role {
            return Err(AppError::BadRequest(format!(
                "модель предназначена для роли `{}`, нельзя назначить на `{}`",
                model.role.slug(),
                role.slug()
            )));
        }
        let provider = self
            .repos
            .providers
            .get(&model.provider_id)?
            .ok_or_else(|| AppError::NotFound("провайдер не найден".into()))?;
        if !provider.is_enabled {
            return Err(AppError::BadRequest("провайдер отключён".into()));
        }

        self.repos
            .providers
            .set_role_assignment(role.slug(), model_id)?;
        self.audit(actor, "role.assign", model_id, Some(role.slug()));
        Ok(())
    }

    // ── Фасады портов (для SessionService / ScenarioService) ──

    /// Строит handle провайдера с расшифрованным ключом (без RBAC — вызывается
    /// внутри сервисов после собственных проверок).
    pub fn build_for(&self, provider_id: &str) -> AppResult<(Provider, ProviderHandle)> {
        let provider = self
            .repos
            .providers
            .get(provider_id)?
            .ok_or_else(|| AppError::NotFound("провайдер не найден".into()))?;
        let key = match &provider.api_key_encrypted {
            Some(enc) => Some(self.cipher.decrypt(enc)?),
            None => None,
        };
        let handle = self.factory.build(&provider, key.as_deref())?;
        Ok((provider, handle))
    }

    /// Резолвит назначенную LLM-модель: `(чат, model_key)`.
    pub async fn resolve_chat(&self) -> AppResult<(Arc<dyn ChatModel>, String)> {
        let (chat, model_key, _handle) = self.resolve_chat_detailed().await?;
        Ok((chat, model_key))
    }

    /// Как [`resolve_chat`], но возвращает ещё и handle (для STT/TTS).
    pub async fn resolve_chat_detailed(
        &self,
    ) -> AppResult<(Arc<dyn ChatModel>, String, ProviderHandle)> {
        let (model, provider) = self.assigned_model(ModelRole::Llm)?;
        let (_p, handle) = self.build_for(&provider.id)?;
        let chat = handle.chat.clone().ok_or_else(|| {
            AppError::upstream(
                &provider.name,
                "назначенный провайдер не поддерживает диалог (LLM)",
            )
        })?;
        Ok((chat, model.model_key.clone(), handle))
    }

    /// Модель + провайдер, назначенные на роль.
    pub fn assigned_model(&self, role: ModelRole) -> AppResult<(ModelRecord, Provider)> {
        let assignments = self.repos.providers.role_assignments()?;
        let assignment = assignments.iter().find(|a| a.role == role).ok_or_else(|| {
            AppError::Config(format!(
                "модель для роли `{}` не назначена в настройках",
                role.slug()
            ))
        })?;

        let model = self
            .repos
            .providers
            .get_model(&assignment.model_id)?
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "назначенная модель `{}` не найдена",
                    assignment.model_id
                ))
            })?;
        if !model.is_enabled {
            return Err(AppError::Config(format!(
                "модель `{}` отключена",
                model.display_name
            )));
        }
        let provider = self
            .repos
            .providers
            .get(&model.provider_id)?
            .ok_or_else(|| AppError::NotFound("провайдер модели не найден".into()))?;
        Ok((model, provider))
    }

    /// Каталог назначенной роли (для STT/TTS voice-этапа).
    pub async fn resolve_catalog(&self, role: ModelRole) -> AppResult<Arc<dyn ModelCatalog>> {
        let (_model, provider) = self.assigned_model(role)?;
        let (_p, handle) = self.build_for(&provider.id)?;
        handle
            .catalog
            .clone()
            .ok_or_else(|| AppError::upstream(&provider.name, "у провайдера нет каталога"))
    }

    fn audit(&self, actor: &AuthContext, action: &str, entity_id: &str, details: Option<&str>) {
        if let Err(err) =
            self.repos
                .audit
                .append(Some(&actor.user_id), action, None, Some(entity_id), details)
        {
            tracing::warn!(error = %err, action, "не удалось записать аудит");
        }
    }
}
