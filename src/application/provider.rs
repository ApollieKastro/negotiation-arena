//! CRUD провайдеров, шифрование API-ключей, discovery, назначение ролей,
//! фасады LLM/STT/TTS через [`ProviderFactory`].

use std::sync::Arc;

use crate::application::auth::AuthContext;
use crate::domain::entities::model::{ModelDescriptor, ModelRole};
use crate::domain::entities::provider::{
    ModelRecord, Provider, RoleAssignment, UserModelPreference,
};
use crate::domain::ports::{
    AuditRepository, ChatModel, ModelCatalog, ProviderRepository, SpeechToText, TextToSpeech,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::crypto::{mask_secret, SecretCipher};
use crate::infrastructure::db::repos::SqliteRepos;
use crate::infrastructure::providers::{
    DownloadLocalModelRequest, LocalModelFile, ProviderFactory, ProviderHandle,
};

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

        // Локальный base_url (Ollama и т.п.) — ключ не обязателен; см. Provider::requires_api_key.
        if provider.requires_api_key() && provider.api_key_encrypted.is_none() {
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

    /// Снимает модель с роли (назначение очищается).
    ///
    /// После очистки резолв роли падает с 503 «не настроен», пока админ
    /// не назначит новую модель. Право — `ManageProviders`.
    pub fn clear_role_assignment(&self, actor: &AuthContext, role: ModelRole) -> AppResult<()> {
        actor.require(super::Permission::ManageProviders)?;
        self.repos.providers.clear_role_assignment(role.slug())?;
        self.audit(actor, "role.clear", role.slug(), None);
        Ok(())
    }

    // ── Пользовательские предпочтения моделей ──

    /// Список предпочтений: свои — `ManageOwnSettings`, чужие — `ManageProviders`.
    pub fn user_preferences(
        &self,
        actor: &AuthContext,
        user_id: &str,
    ) -> AppResult<Vec<UserModelPreference>> {
        ensure_pref_access(actor, user_id)?;
        self.repos.providers.user_preferences(user_id)
    }

    /// Предпочтение одной роли; `None` — используется глобальное назначение.
    pub fn user_preference(
        &self,
        actor: &AuthContext,
        user_id: &str,
        role: ModelRole,
    ) -> AppResult<Option<UserModelPreference>> {
        ensure_pref_access(actor, user_id)?;
        self.repos.providers.user_preference(user_id, role.slug())
    }

    /// Выбор пользователем (или админом за пользователя) модели под роль.
    ///
    /// «Разрешённые админом модели» = включённые (`is_enabled`) модели с
    /// matching-ролью у включённого провайдера (отдельной allow-таблицы нет).
    pub fn set_user_preference(
        &self,
        actor: &AuthContext,
        user_id: &str,
        role: ModelRole,
        model_id: &str,
    ) -> AppResult<()> {
        ensure_pref_access(actor, user_id)?;
        let model = self
            .repos
            .providers
            .get_model(model_id)?
            .ok_or_else(|| AppError::NotFound("модель не найдена".into()))?;
        if model.role != role {
            return Err(AppError::BadRequest(format!(
                "модель предназначена для роли `{}`, нельзя выбрать на `{}`",
                model.role.slug(),
                role.slug()
            )));
        }
        if !model.is_enabled {
            return Err(AppError::BadRequest("модель отключена".into()));
        }
        let provider = self
            .repos
            .providers
            .get(&model.provider_id)?
            .ok_or_else(|| AppError::NotFound("провайдер модели не найден".into()))?;
        if !provider.is_enabled {
            return Err(AppError::BadRequest("провайдер отключён".into()));
        }

        self.repos
            .providers
            .set_user_preference(user_id, role.slug(), model_id)?;
        if let Err(err) = self.repos.audit.append(
            Some(&actor.user_id),
            "model_preference.set",
            Some("user"),
            Some(user_id),
            Some(role.slug()),
        ) {
            tracing::warn!(error = %err, "не удалось записать аудит предпочтений");
        }
        Ok(())
    }

    /// Возврат к глобальному назначению роли.
    pub fn delete_user_preference(
        &self,
        actor: &AuthContext,
        user_id: &str,
        role: ModelRole,
    ) -> AppResult<()> {
        ensure_pref_access(actor, user_id)?;
        self.repos
            .providers
            .delete_user_preference(user_id, role.slug())?;
        if let Err(err) = self.repos.audit.append(
            Some(&actor.user_id),
            "model_preference.clear",
            Some("user"),
            Some(user_id),
            Some(role.slug()),
        ) {
            tracing::warn!(error = %err, "не удалось записать аудит предпочтений");
        }
        Ok(())
    }

    /// Кандидаты для выбора: включённые модели роли (для любого авторизованного —
    /// extractor `AuthUser` уже guarantees, что запрос аутентифицирован).
    pub fn model_options(
        &self,
        _actor: &AuthContext,
        role: ModelRole,
    ) -> AppResult<Vec<ModelRecord>> {
        Ok(self
            .repos
            .providers
            .list_models(None)?
            .into_iter()
            .filter(|m| m.role == role && m.is_enabled)
            .collect())
    }

    // ── Локальные модели (MODELS_DIR) ──

    /// Список файлов локальных моделей (рекурсивный, без `.cache`).
    pub fn list_local_models(&self, actor: &AuthContext) -> AppResult<Vec<LocalModelFile>> {
        actor.require(super::Permission::ManageProviders)?;
        let manager = self.local_manager();
        manager.list()
    }

    /// Скачивает модель: прямой URL **или** HF repo `org/name`
    /// (+ опциональный `filename` для одного файла; без него — весь репо через `hf`).
    pub async fn download_local_model(
        &self,
        actor: &AuthContext,
        req: DownloadLocalModelRequest,
    ) -> AppResult<LocalModelFile> {
        actor.require(super::Permission::ManageProviders)?;
        let source = req.source.trim();
        if source.is_empty() {
            return Err(AppError::BadRequest(
                "укажите URL или HuggingFace repo (org/name)".into(),
            ));
        }

        let manager = self.local_manager();
        let file = if crate::infrastructure::providers::local::is_hf_repo_id(source)
            && !source.starts_with("http://")
            && !source.starts_with("https://")
        {
            manager
                .download_hf(
                    source,
                    req.filename.as_deref().map(str::trim),
                    req.name.as_deref().map(str::trim),
                )
                .await?
        } else if source.starts_with("http://") || source.starts_with("https://") {
            let name = req
                .name
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| {
                    source
                        .rsplit('/')
                        .next()
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .unwrap_or_default()
                });
            if name.is_empty() {
                return Err(AppError::BadRequest(
                    "не удалось вывести имя файла из URL — укажите `name`".into(),
                ));
            }
            manager.download(source, &name).await?
        } else {
            // Явный HF repo id без схемы (даже если не прошёл is_hf_repo_id строго).
            manager
                .download_hf(
                    source,
                    req.filename.as_deref().map(str::trim),
                    req.name.as_deref().map(str::trim),
                )
                .await?
        };

        self.audit(actor, "local_model.download", &file.name, Some(source));
        Ok(file)
    }

    /// Удаляет файл локальной модели по относительному пути.
    pub fn delete_local_model(&self, actor: &AuthContext, name: &str) -> AppResult<()> {
        actor.require(super::Permission::ManageProviders)?;
        let manager = self.local_manager();
        manager.delete(name)?;
        self.audit(actor, "local_model.delete", name, None);
        Ok(())
    }

    fn local_manager(&self) -> crate::infrastructure::providers::LocalModelManager {
        crate::infrastructure::providers::LocalModelManager::new(
            self.factory.models_dir().clone(),
            self.factory.http().clone(),
        )
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
    ///
    /// LLM не настроена (нет назначения роли, модель отключена/удалена,
    /// провайдер без адаптера диалога) → [`AppError::ServiceUnavailable`]
    /// (HTTP 503) с внятным сообщением, а не 500.
    pub async fn resolve_chat(&self) -> AppResult<(Arc<dyn ChatModel>, String)> {
        let (chat, model_key, _handle) = self.resolve_chat_detailed().await?;
        Ok((chat, model_key))
    }

    /// Как [`resolve_chat`], но учитывает предпочтение пользователя `user_id`
    /// (иначе — глобальное назначение роли).
    pub async fn resolve_chat_for(&self, user_id: &str) -> AppResult<(Arc<dyn ChatModel>, String)> {
        let (chat, model_key, _handle) = self.resolve_chat_detailed_for(user_id).await?;
        Ok((chat, model_key))
    }

    /// Как [`resolve_chat_detailed`], но учитывает предпочтение пользователя.
    pub async fn resolve_chat_detailed_for(
        &self,
        user_id: &str,
    ) -> AppResult<(Arc<dyn ChatModel>, String, ProviderHandle)> {
        let (model, provider) = self
            .assigned_model_for(ModelRole::Llm, user_id)
            .map_err(|e| unconfigured(DIALOG_UNCONFIGURED, e))?;
        let (_p, handle) = self
            .build_for(&provider.id)
            .map_err(|e| unconfigured(DIALOG_UNCONFIGURED, e))?;
        let chat = handle.chat.clone().ok_or_else(|| {
            AppError::ServiceUnavailable(format!(
                "{DIALOG_UNCONFIGURED}: у провайдера `{}` нет адаптера диалога (LLM)",
                provider.name
            ))
        })?;
        Ok((chat, model.model_key.clone(), handle))
    }

    /// Как [`resolve_chat_detailed`], без учёта пользователя (генерация сценариев).
    pub async fn resolve_chat_detailed(
        &self,
    ) -> AppResult<(Arc<dyn ChatModel>, String, ProviderHandle)> {
        let (model, provider) = self
            .assigned_model(ModelRole::Llm)
            .map_err(|e| unconfigured(DIALOG_UNCONFIGURED, e))?;
        let (_p, handle) = self
            .build_for(&provider.id)
            .map_err(|e| unconfigured(DIALOG_UNCONFIGURED, e))?;
        let chat = handle.chat.clone().ok_or_else(|| {
            AppError::ServiceUnavailable(format!(
                "{DIALOG_UNCONFIGURED}: у провайдера `{}` нет адаптера диалога (LLM)",
                provider.name
            ))
        })?;
        Ok((chat, model.model_key.clone(), handle))
    }

    /// Резолвит назначенную STT-модель: `(распознавание речи, model_key)`.
    ///
    /// Как [`resolve_chat_for`]: предпочтение пользователя → глобальное назначение.
    /// Голос не настроен (нет назначения, модели или адаптера) →
    /// [`AppError::ServiceUnavailable`] (HTTP 503).
    pub async fn resolve_stt_for(
        &self,
        user_id: &str,
    ) -> AppResult<(Arc<dyn SpeechToText>, String)> {
        let (model, provider) = self.voice_assignment(ModelRole::Stt, user_id, "STT")?;
        let (_p, handle) = self
            .build_for(&provider.id)
            .map_err(|e| voice_unconfigured("STT", e))?;
        let stt = handle.stt.ok_or_else(|| {
            AppError::ServiceUnavailable(format!(
                "Голосовой сервис не настроен (STT): у провайдера `{}` нет адаптера распознавания речи",
                provider.name
            ))
        })?;
        Ok((stt, model.model_key.clone()))
    }

    /// Резолвит назначенную TTS-модель: `(синтез речи, model_key)`.
    ///
    /// Как [`resolve_chat_for`]: предпочтение пользователя → глобальное назначение.
    /// Голос не настроен (нет назначения, модели или адаптера) →
    /// [`AppError::ServiceUnavailable`] (HTTP 503).
    pub async fn resolve_tts_for(
        &self,
        user_id: &str,
    ) -> AppResult<(Arc<dyn TextToSpeech>, String)> {
        let (model, provider) = self.voice_assignment(ModelRole::Tts, user_id, "TTS")?;
        let (_p, handle) = self
            .build_for(&provider.id)
            .map_err(|e| voice_unconfigured("TTS", e))?;
        let tts = handle.tts.ok_or_else(|| {
            AppError::ServiceUnavailable(format!(
                "Голосовой сервис не настроен (TTS): у провайдера `{}` нет адаптера синтеза речи",
                provider.name
            ))
        })?;
        Ok((tts, model.model_key.clone()))
    }

    /// Назначение голосовой роли; «не настроено» → 503, а не 500.
    fn voice_assignment(
        &self,
        role: ModelRole,
        user_id: &str,
        label: &str,
    ) -> AppResult<(ModelRecord, Provider)> {
        self.assigned_model_for(role, user_id)
            .map_err(|e| voice_unconfigured(label, e))
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

    /// Как [`assigned_model`], но сначала смотрит предпочтение пользователя.
    ///
    /// Протухшее/отключённое предпочтение (модель удалена или выключена)
    /// молча игнорируется — происходит fallback на глобальное назначение.
    pub fn assigned_model_for(
        &self,
        role: ModelRole,
        user_id: &str,
    ) -> AppResult<(ModelRecord, Provider)> {
        if let Some(pref) = self.repos.providers.user_preference(user_id, role.slug())? {
            if let Some(model) = self.repos.providers.get_model(&pref.model_id)? {
                if model.role == role && model.is_enabled {
                    if let Some(provider) = self.repos.providers.get(&model.provider_id)? {
                        if provider.is_enabled {
                            return Ok((model, provider));
                        }
                    }
                }
            }
        }
        self.assigned_model(role)
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

/// Доступ к пользовательским предпочтениям моделей: свои — `ManageOwnSettings`,
/// чужие — `ManageProviders` (как в настройках UI).
fn ensure_pref_access(actor: &AuthContext, user_id: &str) -> AppResult<()> {
    if user_id == actor.user_id {
        actor.require(super::Permission::ManageOwnSettings)
    } else {
        actor.require(super::Permission::ManageProviders)
    }
}

/// Префикс сообщения, когда не настроена LLM (диалог/генерация).
const DIALOG_UNCONFIGURED: &str = "Диалог не настроен";

/// Маппинг «не настроено» (нет назначения роли, удалена/отключена модель
/// или провайдер) в [`AppError::ServiceUnavailable`] — это состояние
/// конфигурации, а не сбой 500. `context` — человекочитаемый префикс.
fn unconfigured(context: &str, err: AppError) -> AppError {
    match err {
        AppError::Config(msg) | AppError::NotFound(msg) => {
            AppError::ServiceUnavailable(format!("{context}: {msg}"))
        }
        other => other,
    }
}

/// Как [`unconfigured`], но с голосовым префиксом.
fn voice_unconfigured(label: &str, err: AppError) -> AppError {
    unconfigured(&format!("Голосовой сервис не настроен ({label})"), err)
}

#[cfg(test)]
mod preference_tests {
    use super::*;
    use crate::application::testsupport::{ctx, setup};
    use crate::domain::entities::user::UserRole;
    use crate::domain::ports::UserRepository as _;

    fn seed_provider_and_models(svc: &crate::application::Services) -> (String, String, String) {
        let now = chrono::Utc::now().to_rfc3339();
        let provider = Provider {
            id: "prov-local".into(),
            name: "Локальный".into(),
            kind: crate::domain::entities::model::ProviderKind::Local,
            base_url: None,
            api_key_encrypted: None,
            api_key_hint: None,
            is_enabled: true,
            created_at: now.clone(),
            updated_at: None,
        };
        svc.repos.providers.upsert(&provider).unwrap();

        let model_a = ModelRecord {
            id: "model-a".into(),
            provider_id: provider.id.clone(),
            role: ModelRole::Llm,
            model_key: "model-a".into(),
            display_name: "Модель A".into(),
            is_enabled: true,
            metadata: serde_json::json!({}),
            created_at: now.clone(),
        };
        let model_b = ModelRecord {
            id: "model-b".into(),
            provider_id: provider.id.clone(),
            role: ModelRole::Llm,
            model_key: "model-b".into(),
            display_name: "Модель B".into(),
            is_enabled: true,
            metadata: serde_json::json!({}),
            created_at: now,
        };
        svc.repos.providers.upsert_model(&model_a).unwrap();
        svc.repos.providers.upsert_model(&model_b).unwrap();
        svc.repos
            .providers
            .set_role_assignment("llm", &model_a.id)
            .unwrap();
        (provider.id, model_a.id, model_b.id)
    }

    fn user_ctx(svc: &crate::application::Services, login: &str) -> AuthContext {
        let u = svc
            .repos
            .users
            .create(login, "hash", UserRole::User, None)
            .unwrap();
        AuthContext {
            user_id: u.id,
            login: login.into(),
            role: UserRole::User,
        }
    }

    #[test]
    fn preference_overrides_global_and_falls_back_when_cleared() {
        let (_db, svc) = setup().unwrap();
        let (_prov, model_a, model_b) = seed_provider_and_models(&svc);
        let user = user_ctx(&svc, "alice");
        let admin = ctx("admin-1", UserRole::Admin);

        // До выбора — глобальное назначение (A).
        let (m, _) = svc
            .providers
            .assigned_model_for(ModelRole::Llm, &user.user_id)
            .unwrap();
        assert_eq!(m.id, model_a);

        // Выбор B → резолв по пользователю.
        svc.providers
            .set_user_preference(&user, &user.user_id, ModelRole::Llm, &model_b)
            .unwrap();
        let (m, _) = svc
            .providers
            .assigned_model_for(ModelRole::Llm, &user.user_id)
            .unwrap();
        assert_eq!(m.id, model_b);

        // Чужой пользователь без предпочтения — глобальное.
        let other = user_ctx(&svc, "bob");
        let (m, _) = svc
            .providers
            .assigned_model_for(ModelRole::Llm, &other.user_id)
            .unwrap();
        assert_eq!(m.id, model_a);

        // Очистка → снова глобальное.
        svc.providers
            .delete_user_preference(&user, &user.user_id, ModelRole::Llm)
            .unwrap();
        let (m, _) = svc
            .providers
            .assigned_model_for(ModelRole::Llm, &user.user_id)
            .unwrap();
        assert_eq!(m.id, model_a);
        assert!(svc
            .providers
            .user_preference(&user, &user.user_id, ModelRole::Llm)
            .unwrap()
            .is_none());
        // Админ может читать предпочтения пользователя.
        assert!(svc
            .providers
            .user_preferences(&admin, &user.user_id)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn plain_user_cannot_set_someone_elses_preference() {
        let (_db, svc) = setup().unwrap();
        let (_p, _a, model_b) = seed_provider_and_models(&svc);
        let alice = user_ctx(&svc, "alice");
        let bob = user_ctx(&svc, "bob");
        let err = svc
            .providers
            .set_user_preference(&alice, &bob.user_id, ModelRole::Llm, &model_b)
            .unwrap_err();
        assert!(matches!(err, crate::error::AppError::Forbidden(_)));
    }

    #[test]
    fn preference_rejects_wrong_role_disabled_model_and_unknown() {
        let (_db, svc) = setup().unwrap();
        let (_p, model_a, model_b) = seed_provider_and_models(&svc);
        let user = user_ctx(&svc, "alice");

        // Неизвестная модель.
        assert!(matches!(
            svc.providers
                .set_user_preference(&user, &user.user_id, ModelRole::Llm, "nope")
                .unwrap_err(),
            crate::error::AppError::NotFound(_)
        ));

        // Модель другой роли (STT vs LLM).
        let stt = ModelRecord {
            id: "model-stt".into(),
            provider_id: "prov-local".into(),
            role: ModelRole::Stt,
            model_key: "whisper".into(),
            display_name: "Whisper".into(),
            is_enabled: true,
            metadata: serde_json::json!({}),
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        svc.repos.providers.upsert_model(&stt).unwrap();
        assert!(matches!(
            svc.providers
                .set_user_preference(&user, &user.user_id, ModelRole::Llm, &stt.id)
                .unwrap_err(),
            crate::error::AppError::BadRequest(_)
        ));

        // Отключённая модель.
        let mut disabled = svc.repos.providers.get_model(&model_b).unwrap().unwrap();
        disabled.is_enabled = false;
        svc.repos.providers.upsert_model(&disabled).unwrap();
        assert!(matches!(
            svc.providers
                .set_user_preference(&user, &user.user_id, ModelRole::Llm, &model_b)
                .unwrap_err(),
            crate::error::AppError::BadRequest(_)
        ));
        // model_a ещё валидна — happy path не сломан.
        assert!(svc
            .providers
            .set_user_preference(&user, &user.user_id, ModelRole::Llm, &model_a)
            .is_ok());
    }

    #[test]
    fn stale_preference_falls_back_to_global() {
        let (_db, svc) = setup().unwrap();
        let (_p, model_a, model_b) = seed_provider_and_models(&svc);
        let user = user_ctx(&svc, "alice");
        svc.providers
            .set_user_preference(&user, &user.user_id, ModelRole::Llm, &model_b)
            .unwrap();
        // Модель выключили после выбора.
        let mut b = svc.repos.providers.get_model(&model_b).unwrap().unwrap();
        b.is_enabled = false;
        svc.repos.providers.upsert_model(&b).unwrap();

        let (m, _) = svc
            .providers
            .assigned_model_for(ModelRole::Llm, &user.user_id)
            .unwrap();
        assert_eq!(m.id, model_a, "нужен fallback на глобальное");
    }

    #[test]
    fn model_options_lists_only_enabled_models_of_role() {
        let (_db, svc) = setup().unwrap();
        let (_p, _a, _b) = seed_provider_and_models(&svc);
        let user = user_ctx(&svc, "alice");
        let opts = svc.providers.model_options(&user, ModelRole::Llm).unwrap();
        assert_eq!(opts.len(), 2);
        assert!(opts
            .iter()
            .all(|m| m.role == ModelRole::Llm && m.is_enabled));
        assert!(svc
            .providers
            .model_options(&user, ModelRole::Tts)
            .unwrap()
            .is_empty());
    }

    // ── LLM без назначения → 503, а не 500 ──

    #[tokio::test]
    async fn resolve_chat_without_assignment_is_service_unavailable() {
        let (_db, svc) = setup().unwrap();
        let user = user_ctx(&svc, "alice");

        // Arc<dyn ChatModel> не реализует Debug → без unwrap_err().
        let err = match svc.providers.resolve_chat().await {
            Ok(_) => panic!("без назначения llm ожидали ServiceUnavailable"),
            Err(e) => e,
        };
        assert!(
            matches!(err, crate::error::AppError::ServiceUnavailable(_)),
            "{err}"
        );
        let msg = err.to_string();
        assert!(msg.contains("не настроен"), "{msg}");
        assert!(msg.contains("llm"), "{msg}");

        let err = match svc.providers.resolve_chat_for(&user.user_id).await {
            Ok(_) => panic!("без назначения llm ожидали ServiceUnavailable"),
            Err(e) => e,
        };
        assert!(
            matches!(err, crate::error::AppError::ServiceUnavailable(_)),
            "{err}"
        );
    }

    #[tokio::test]
    async fn resolve_chat_with_disabled_model_is_service_unavailable() {
        let (_db, svc) = setup().unwrap();
        let (_p, model_a, _b) = seed_provider_and_models(&svc);
        let mut m = svc.repos.providers.get_model(&model_a).unwrap().unwrap();
        m.is_enabled = false;
        svc.repos.providers.upsert_model(&m).unwrap();

        let err = match svc.providers.resolve_chat().await {
            Ok(_) => panic!("с отключённой моделью ожидали ServiceUnavailable"),
            Err(e) => e,
        };
        assert!(
            matches!(err, crate::error::AppError::ServiceUnavailable(_)),
            "{err}"
        );
        assert!(err.to_string().contains("не настроен"), "{err}");
    }

    #[test]
    fn clear_role_assignment_removes_row_and_allows_reassign() {
        let (_db, svc) = setup().unwrap();
        let (_p, model_a, model_b) = seed_provider_and_models(&svc);
        let admin = ctx("admin-1", UserRole::Admin);

        // Назначение есть до очистки.
        let assigned = svc.providers.role_assignments(&admin).unwrap();
        assert!(assigned.iter().any(|a| a.role == ModelRole::Llm));

        svc.providers
            .clear_role_assignment(&admin, ModelRole::Llm)
            .unwrap();
        let assigned = svc.providers.role_assignments(&admin).unwrap();
        assert!(
            assigned.iter().all(|a| a.role != ModelRole::Llm),
            "назначение llm должно быть снято: {assigned:?}"
        );

        // Повторное назначение после очистки работает.
        svc.providers
            .assign_role(&admin, ModelRole::Llm, &model_b)
            .unwrap();
        let assigned = svc.providers.role_assignments(&admin).unwrap();
        assert!(assigned
            .iter()
            .any(|a| a.role == ModelRole::Llm && a.model_id == model_b));

        // Обычный пользователь не может снимать назначения.
        let user = user_ctx(&svc, "alice");
        let err = svc
            .providers
            .clear_role_assignment(&user, ModelRole::Llm)
            .unwrap_err();
        assert!(matches!(err, crate::error::AppError::Forbidden(_)));
        let _ = model_a;
    }
}
