//! Сценарии: CRUD, импорт/экспорт JSON, генерация через LLM.

use std::sync::Arc;

use crate::application::auth::AuthContext;
use crate::application::provider::ProviderService;
use crate::application::settings::{global_keys, SettingsService};
use crate::domain::entities::scenario::{Difficulty, Ending, Scenario};
use crate::domain::ports::{
    AuditRepository, ChatMessage, ChatRequest, LlmUsageRepository, ScenarioRepository,
    SessionRepository,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::db::repos::SqliteRepos;

/// Максимальная длина брифа для ИИ-генерации (символы).
const MAX_BRIEF_CHARS: usize = 4000;

/// Управление сценариями (RBAC: `ManageScenarios` для записи; чтение — все).
pub struct ScenarioService {
    repos: Arc<SqliteRepos>,
    providers: Arc<ProviderService>,
    settings: Arc<SettingsService>,
}

impl ScenarioService {
    pub fn new(
        repos: Arc<SqliteRepos>,
        providers: Arc<ProviderService>,
        settings: Arc<SettingsService>,
    ) -> Self {
        Self {
            repos,
            providers,
            settings,
        }
    }

    // ── Чтение ──

    /// Список сценариев. Обычный пользователь видит только активные.
    pub fn list(&self, actor: &AuthContext, active_only: bool) -> AppResult<Vec<Scenario>> {
        let only_active = active_only || !actor.is_admin();
        self.repos.scenarios.list(only_active)
    }

    pub fn get(&self, actor: &AuthContext, id: &str) -> AppResult<Scenario> {
        let scenario = self
            .repos
            .scenarios
            .get(id)?
            .ok_or_else(|| AppError::NotFound("сценарий не найден".into()))?;
        if !scenario.is_active && !actor.is_admin() {
            return Err(AppError::NotFound("сценарий не найден".into()));
        }
        Ok(scenario)
    }

    // ── Запись (admin) ──

    pub fn create(&self, actor: &AuthContext, mut scenario: Scenario) -> AppResult<Scenario> {
        actor.require(super::Permission::ManageScenarios)?;
        self.prepare_new(&mut scenario)?;
        scenario.created_by = Some(actor.user_id.clone());

        if self.repos.scenarios.get(&scenario.id)?.is_some() {
            return Err(AppError::Conflict(
                "сценарий с таким id уже существует".into(),
            ));
        }
        self.repos.scenarios.upsert(&scenario)?;
        self.audit(actor, "scenario.create", &scenario.id);
        Ok(scenario)
    }

    pub fn update(&self, actor: &AuthContext, scenario: Scenario) -> AppResult<Scenario> {
        actor.require(super::Permission::ManageScenarios)?;
        let mut stored = self
            .repos
            .scenarios
            .get(&scenario.id)?
            .ok_or_else(|| AppError::NotFound("сценарий не найден".into()))?;

        // Обновляем контент, сохраняем provenance.
        stored.title = scenario.title;
        stored.description = scenario.description;
        stored.sphere = scenario.sphere;
        stored.difficulty = scenario.difficulty;
        stored.player_role = scenario.player_role;
        stored.player_company = scenario.player_company;
        stored.player_goal = scenario.player_goal;
        stored.player_batna = scenario.player_batna;
        stored.partner_name = scenario.partner_name;
        stored.partner_role = scenario.partner_role;
        stored.partner_company = scenario.partner_company;
        stored.partner_goal = scenario.partner_goal;
        stored.partner_goals = scenario.partner_goals;
        stored.partner_batna = scenario.partner_batna;
        stored.partner_personality = scenario.partner_personality;
        stored.opening_context = scenario.opening_context;
        stored.endings = scenario.endings;
        stored.is_active = scenario.is_active;
        stored.updated_at = Some(chrono::Utc::now().to_rfc3339());

        self.validate_content(&stored)?;
        if stored.endings.is_empty() {
            stored.endings = default_endings();
        }

        self.repos.scenarios.upsert(&stored)?;
        self.audit(actor, "scenario.update", &stored.id);
        Ok(stored)
    }

    pub fn delete(&self, actor: &AuthContext, id: &str) -> AppResult<()> {
        actor.require(super::Permission::ManageScenarios)?;
        if self.repos.scenarios.get(id)?.is_none() {
            return Err(AppError::NotFound("сценарий не найден".into()));
        }
        // FK `ON DELETE CASCADE` уничтожил бы связанные сессии и реплики —
        // явный 409 вместо молчаливого каскада.
        if self.repos.sessions.count_by_scenario(id)? > 0 {
            return Err(AppError::Conflict(
                "нельзя удалить сценарий: с ним связаны сессии".into(),
            ));
        }
        self.repos.scenarios.delete(id)?;
        self.audit(actor, "scenario.delete", id);
        Ok(())
    }

    pub fn set_active(
        &self,
        actor: &AuthContext,
        id: &str,
        is_active: bool,
    ) -> AppResult<Scenario> {
        actor.require(super::Permission::ManageScenarios)?;
        let mut scenario = self
            .repos
            .scenarios
            .get(id)?
            .ok_or_else(|| AppError::NotFound("сценарий не найден".into()))?;
        scenario.is_active = is_active;
        scenario.updated_at = Some(chrono::Utc::now().to_rfc3339());
        self.repos.scenarios.upsert(&scenario)?;
        self.audit(
            actor,
            if is_active {
                "scenario.activate"
            } else {
                "scenario.deactivate"
            },
            id,
        );
        Ok(scenario)
    }

    // ── Импорт / экспорт ──

    pub fn export_json(&self, actor: &AuthContext, id: &str) -> AppResult<String> {
        let scenario = self.get(actor, id)?;
        let json = serde_json::to_string_pretty(&scenario)
            .map_err(|e| AppError::internal(format!("сериализация сценария: {e}")))?;
        Ok(json)
    }

    /// Импортирует сценарий из JSON. При пустом `id` генерируется новый.
    pub fn import_json(&self, actor: &AuthContext, json: &str) -> AppResult<Scenario> {
        actor.require(super::Permission::ManageScenarios)?;
        let mut scenario: Scenario = serde_json::from_str(json)
            .map_err(|e| AppError::BadRequest(format!("некорректный JSON сценария: {e}")))?;

        if scenario.id.trim().is_empty() {
            scenario.id = uuid::Uuid::new_v4().to_string();
            scenario.created_at = chrono::Utc::now().to_rfc3339();
        } else if let Some(prev) = self.repos.scenarios.get(&scenario.id)? {
            scenario.created_at = prev.created_at;
        }
        scenario.created_by = Some(actor.user_id.clone());
        scenario.updated_at = None;
        self.prepare_new(&mut scenario)?;
        // prepare_new мог сгенерировать id — но id уже был: keep.
        // prepare_new не трогает id, если он задан.

        self.repos.scenarios.upsert(&scenario)?;
        self.audit(actor, "scenario.import", &scenario.id);
        Ok(scenario)
    }

    // ── ИИ-генератор ──

    /// Дневной лимит LLM-токенов (0 = выключен).
    fn llm_daily_token_limit(&self) -> AppResult<u64> {
        let raw = self
            .settings
            .get_global(global_keys::LLM_DAILY_TOKEN_LIMIT)?;
        Ok(raw.trim().parse::<u64>().unwrap_or(0))
    }

    /// Проверяет квоту перед вызовом LLM; при исчерпании → 429.
    fn ensure_llm_quota(&self, user_id: &str, limit: u64) -> AppResult<()> {
        if limit == 0 {
            return Ok(());
        }
        let day = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let used = self.repos.llm_usage.tokens_on(user_id, &day)?;
        if used >= limit {
            return Err(AppError::TooManyRequests(format!(
                "дневной лимит LLM-токенов исчерпан ({used}/{limit}); попробуйте завтра"
            )));
        }
        Ok(())
    }

    /// Начисляет факт использования (всегда, даже при лимите 0 — для учёта).
    fn record_llm_usage(&self, user_id: &str, tokens: u64) {
        let day = chrono::Utc::now().format("%Y-%m-%d").to_string();
        if let Err(err) = self.repos.llm_usage.add_usage(user_id, &day, tokens) {
            tracing::warn!(error = %err, "не удалось записать LLM-квоту");
        }
    }

    /// Генерирует черновик сценария по брифу через назначенную LLM.
    ///
    /// Сценарий сохраняется как неактивный (`is_active = false`) — админ
    /// вычитывает и публикует [`set_active`].
    pub async fn generate(
        &self,
        actor: &AuthContext,
        brief: &str,
        difficulty: Difficulty,
        sphere: Option<&str>,
    ) -> AppResult<Scenario> {
        actor.require(super::Permission::ManageScenarios)?;
        let brief = brief.trim();
        if brief.is_empty() {
            return Err(AppError::BadRequest(
                "бриф для генерации не может быть пустым".into(),
            ));
        }
        if brief.chars().count() > MAX_BRIEF_CHARS {
            return Err(AppError::BadRequest(format!(
                "бриф не длиннее {MAX_BRIEF_CHARS} символов"
            )));
        }

        // Дневная квота LLM-токенов (0 = off) — та же, что в диалоге.
        let limit = self.llm_daily_token_limit()?;
        self.ensure_llm_quota(&actor.user_id, limit)?;

        let (chat, model_key) = self.providers.resolve_chat().await?;

        let system = ChatMessage::system(GENERATOR_SYSTEM);
        let user = ChatMessage::user(format!(
            "Сложность: {} ({})\nСфера: {}\nБриф:\n{}",
            difficulty.title(),
            difficulty.slug(),
            sphere.unwrap_or("не указана"),
            brief
        ));

        let request = ChatRequest::new(model_key, vec![system, user])
            .with_temperature(0.6)
            .with_max_tokens(2500);
        let response = chat.chat(request).await?;
        let tokens = response
            .usage
            .total_tokens
            .map(u64::from)
            .unwrap_or_else(|| {
                let chars = response.content.chars().count() as u64;
                chars.div_ceil(4).max(1)
            });
        self.record_llm_usage(&actor.user_id, tokens);

        let mut scenario = parse_scenario_json(&response.content)?;
        // id от LLM не принимаем: иначе случайный/враждебный id перезапишет
        // существующий сценарий через upsert.
        scenario.id = uuid::Uuid::new_v4().to_string();
        scenario.created_at = chrono::Utc::now().to_rfc3339();
        scenario.updated_at = None;
        scenario.created_by = Some(actor.user_id.clone());
        scenario.ai_generated = true;
        scenario.is_active = false;
        scenario.difficulty = difficulty;
        if let Some(s) = sphere {
            if scenario.sphere.trim().is_empty() {
                scenario.sphere = s.to_string();
            }
        }
        if scenario.endings.is_empty() {
            scenario.endings = default_endings();
        }
        self.validate_content(&scenario)?;

        self.repos.scenarios.upsert(&scenario)?;
        self.audit(actor, "scenario.generate", &scenario.id);
        Ok(scenario)
    }

    // ── Внутреннее ──

    fn prepare_new(&self, scenario: &mut Scenario) -> AppResult<()> {
        if scenario.id.trim().is_empty() {
            scenario.id = uuid::Uuid::new_v4().to_string();
            scenario.created_at = chrono::Utc::now().to_rfc3339();
        }
        if scenario.created_at.trim().is_empty() {
            scenario.created_at = chrono::Utc::now().to_rfc3339();
        }
        scenario.updated_at = None;
        if scenario.endings.is_empty() {
            scenario.endings = default_endings();
        }
        self.validate_content(scenario)
    }

    fn validate_content(&self, scenario: &Scenario) -> AppResult<()> {
        require_non_empty("название", &scenario.title)?;
        require_non_empty("цель игрока", &scenario.player_goal)?;
        require_non_empty("цель собеседника", &scenario.partner_goal)?;
        require_non_empty("реплика открытия", &scenario.opening_context)?;
        if scenario.endings.is_empty() {
            return Err(AppError::BadRequest(
                "у сценария должны быть финалы (endings)".into(),
            ));
        }
        for ending in &scenario.endings {
            require_non_empty("название финала", &ending.title)?;
        }
        Ok(())
    }

    fn audit(&self, actor: &AuthContext, action: &str, entity_id: &str) {
        if let Err(err) = self.repos.audit.append(
            Some(&actor.user_id),
            action,
            Some("scenario"),
            Some(entity_id),
            None,
        ) {
            tracing::warn!(error = %err, action, "не удалось записать аудит");
        }
    }
}

fn require_non_empty(field: &str, value: &str) -> AppResult<()> {
    if value.trim().is_empty() {
        return Err(AppError::BadRequest(format!(
            "поле «{field}» не может быть пустым"
        )));
    }
    Ok(())
}

/// Три финала по умолчанию, если автор их не описал.
pub fn default_endings() -> Vec<Ending> {
    vec![
        Ending {
            id: "win".into(),
            title: "Отличный результат".into(),
            text: "Вам удалось договориться на условиях, близких к вашей цели.".into(),
            outcome: "Сделка заключена".into(),
            min_score: 60,
        },
        Ending {
            id: "partial".into(),
            title: "Компромисс".into(),
            text: "Достигнута частичная договорённость, есть зоны уступок.".into(),
            outcome: "Условия согласованы частично".into(),
            min_score: 25,
        },
        Ending {
            id: "fail".into(),
            title: "Переговоры сорвались".into(),
            text: "Соглашение достичь не удалось.".into(),
            outcome: "Сделки нет".into(),
            min_score: 0,
        },
    ]
}

const GENERATOR_SYSTEM: &str = "\
Ты — редактор обучающих сценариев тренажёра переговоров (SPIN, Гарвардский метод, \
interest-based bargaining). Составь ОДИН полноценный сценарий для ролевой игры.

Ответь СТРОГО валидным JSON без markdown-обёрток и без пояснений. Схема:
{
  \"id\": \"\",
  \"title\": \"string\",
  \"description\": \"string (2-4 предложения для карточки)\",
  \"sphere\": \"string (отрасль)\",
  \"difficulty\": \"easy|medium|hard\",
  \"player_role\": \"string\",
  \"player_company\": \"string или null\",
  \"player_goal\": \"string (конкретная измеримая цель)\",
  \"player_batna\": \"string\",
  \"partner_name\": \"string\",
  \"partner_role\": \"string\",
  \"partner_company\": \"string или null\",
  \"partner_goal\": \"string\",
  \"partner_goals\": [\"string\"],
  \"partner_batna\": \"string\",
  \"partner_personality\": {\"tone\": \"string\", \"style\": \"string\", \"traits\": \"string\"},
  \"opening_context\": \"первая реплика собеседника от первого лица, 1-3 предложения\",
  \"endings\": [
    {\"id\": \"win\", \"title\": \"...\", \"text\": \"...\", \"outcome\": \"...\", \"min_score\": 60},
    {\"id\": \"partial\", \"title\": \"...\", \"text\": \"...\", \"outcome\": \"...\", \"min_score\": 25},
    {\"id\": \"fail\", \"title\": \"...\", \"text\": \"...\", \"outcome\": \"...\", \"min_score\": 0}
  ],
  \"ai_generated\": true,
  \"is_active\": false,
  \"created_by\": null,
  \"created_at\": \"\",
  \"updated_at\": null
}

Требования: у обеих сторон должны быть реальные интересы и BATNA; \
в opening_context собеседник уже говорит, не здоровается дважды; \
тексты на русском.";

/// Достаёт первый с JSON-объект из ответа LLM (снимает ```json и текст).
fn parse_scenario_json(raw: &str) -> AppResult<Scenario> {
    let trimmed = raw.trim();
    let json_candidate = strip_code_fence(trimmed);
    if let Ok(scenario) = serde_json::from_str::<Scenario>(json_candidate) {
        return Ok(scenario);
    }

    let start = trimmed
        .find('{')
        .ok_or_else(|| AppError::BadRequest("LLM не вернул JSON сценария".into()))?;
    let end = trimmed
        .rfind('}')
        .filter(|e| *e > start)
        .ok_or_else(|| AppError::BadRequest("LLM вернул неполный JSON".into()))?;
    serde_json::from_str(&trimmed[start..=end]).map_err(|e| {
        AppError::BadRequest(format!("не удалось разобрать JSON сценария от LLM: {e}"))
    })
}

fn strip_code_fence(text: &str) -> &str {
    let t = text.trim();
    let open = "```json";
    let open_bare = "```";
    if let Some(rest) = t.strip_prefix(open) {
        let rest = rest.strip_prefix('\n').unwrap_or(rest);
        return rest.strip_suffix("```").map(str::trim_end).unwrap_or(rest);
    }
    if t.starts_with(open_bare) && t.ends_with(open_bare) && t.len() > 6 {
        return t[3..t.len() - 3].trim();
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::testsupport::{ctx, setup};
    use crate::domain::entities::scenario::PartnerPersonality;
    use crate::domain::entities::user::UserRole;
    use crate::domain::ports::UserRepository as _;

    fn seed_user(svc: &crate::application::Services, login: &str) -> AuthContext {
        let u = svc
            .repos
            .users
            .create(login, "hash", UserRole::Admin, None)
            .unwrap();
        AuthContext {
            user_id: u.id,
            login: u.login,
            role: UserRole::Admin,
        }
    }

    fn sample_scenario() -> Scenario {
        Scenario {
            id: String::new(),
            title: "Аренда офиса".into(),
            description: "Переговоры о долгосрочной аренде.".into(),
            sphere: "Недвижимость".into(),
            difficulty: Difficulty::Medium,
            player_role: "Арендатор".into(),
            player_company: Some("ООО Ромашка".into()),
            player_goal: "Снизить ставку до 50 тыс. ₽/мес".into(),
            player_batna: "Переехать в БЦ через дорогу".into(),
            partner_name: "Ирина".into(),
            partner_role: "Арендодатель".into(),
            partner_company: None,
            partner_goal: "Сохранить ставку 60 тыс. ₽".into(),
            partner_goals: vec!["Долгий контракт".into()],
            partner_batna: "Сдать этаж другим".into(),
            partner_personality: PartnerPersonality::default(),
            opening_context: "Здравствуйте, про условия аренды.".into(),
            endings: vec![],
            ai_generated: false,
            is_active: true,
            created_by: None,
            created_at: String::new(),
            updated_at: None,
        }
    }

    #[test]
    fn create_get_update_delete_roundtrip() {
        let (_db, svc) = setup().unwrap();
        let admin = seed_user(&svc, "admin1");
        let admin_id = admin.user_id.clone();

        let created = svc.scenarios.create(&admin, sample_scenario()).unwrap();
        assert!(!created.id.is_empty());
        assert_eq!(created.endings.len(), 3, "подставляемые финалы");
        assert_eq!(created.created_by.as_deref(), Some(admin_id.as_str()));

        let fetched = svc.scenarios.get(&admin, &created.id).unwrap();
        assert_eq!(fetched.title, "Аренда офиса");

        let mut updated = fetched;
        updated.title = "Аренда офиса v2".into();
        let updated = svc.scenarios.update(&admin, updated).unwrap();
        assert_eq!(updated.title, "Аренда офиса v2");
        assert!(updated.updated_at.is_some());

        svc.scenarios.delete(&admin, &created.id).unwrap();
        assert!(svc.scenarios.get(&admin, &created.id).is_err());
    }

    #[test]
    fn plain_user_cannot_write_but_sees_active_only() {
        let (_db, svc) = setup().unwrap();
        let admin = seed_user(&svc, "admin1");
        let user = ctx("user-1", UserRole::User);

        let mut sc = sample_scenario();
        sc.is_active = true;
        let active = svc.scenarios.create(&admin, sc).unwrap();

        let mut draft = sample_scenario();
        draft.is_active = false;
        draft.title = "Черновик".into();
        let hidden = svc.scenarios.create(&admin, draft).unwrap();

        assert!(svc.scenarios.create(&user, sample_scenario()).is_err());
        assert!(svc.scenarios.delete(&user, &active.id).is_err());

        let visible = svc.scenarios.list(&user, false).unwrap();
        assert!(visible.iter().all(|s| s.is_active));
        assert!(visible.iter().any(|s| s.id == active.id));
        assert!(!visible.iter().any(|s| s.id == hidden.id));

        // Обычный пользователь не видит неактивный по id.
        assert!(svc.scenarios.get(&user, &hidden.id).is_err());
        // Админ видит.
        assert!(svc.scenarios.get(&admin, &hidden.id).is_ok());
    }

    #[test]
    fn import_export_roundtrip() {
        let (_db, svc) = setup().unwrap();
        let admin = seed_user(&svc, "admin1");
        let admin_id = admin.user_id.clone();

        let created = svc.scenarios.create(&admin, sample_scenario()).unwrap();
        let json = svc.scenarios.export_json(&admin, &created.id).unwrap();
        assert!(json.contains("Аренда офиса"));

        // Импорт с тем же id перезаписывает.
        let imported = svc.scenarios.import_json(&admin, &json).unwrap();
        assert_eq!(imported.id, created.id);
        assert_eq!(imported.created_by.as_deref(), Some(admin_id.as_str()));

        // Импорт без id — новая запись.
        let mut no_id: serde_json::Value = serde_json::from_str(&json).unwrap();
        no_id["id"] = serde_json::Value::String(String::new());
        let fresh = svc
            .scenarios
            .import_json(&admin, &no_id.to_string())
            .unwrap();
        assert_ne!(fresh.id, created.id);

        assert!(svc.scenarios.import_json(&admin, "{not json").is_err());
    }

    #[test]
    fn validation_rejects_empty_goals() {
        let (_db, svc) = setup().unwrap();
        let admin = seed_user(&svc, "admin1");
        let mut sc = sample_scenario();
        sc.player_goal = "  ".into();
        let err = svc.scenarios.create(&admin, sc).unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)));
    }

    #[test]
    fn set_active_toggles_visibility() {
        let (_db, svc) = setup().unwrap();
        let admin = seed_user(&svc, "admin1");
        let user = ctx("user-1", UserRole::User);

        let mut sc = sample_scenario();
        sc.is_active = false;
        let created = svc.scenarios.create(&admin, sc).unwrap();

        assert!(svc.scenarios.get(&user, &created.id).is_err());
        svc.scenarios.set_active(&admin, &created.id, true).unwrap();
        assert!(svc.scenarios.get(&user, &created.id).is_ok());
    }

    #[test]
    fn parse_json_strips_fences_and_surrounding_text() {
        let raw = "Вот сценарий:\n```json\n{\"id\":\"\",\"title\":\"T\",\"description\":\"d\",\
            \"sphere\":\"s\",\"difficulty\":\"easy\",\"player_role\":\"p\",\
            \"player_company\":null,\"player_goal\":\"g\",\"player_batna\":\"b\",\
            \"partner_name\":\"n\",\"partner_role\":\"r\",\"partner_company\":null,\
            \"partner_goal\":\"g2\",\"partner_goals\":[],\"partner_batna\":\"b2\",\
            \"partner_personality\":{},\"opening_context\":\"o\",\"endings\":[],\
            \"ai_generated\":false,\"is_active\":true,\"created_by\":null,\
            \"created_at\":\"\",\"updated_at\":null}\n```\nГотово.";
        let scenario = parse_scenario_json(raw).unwrap();
        assert_eq!(scenario.title, "T");
    }
}
