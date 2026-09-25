//! Сессии: старт, ход диалога (LLM + скоринг), финализация с отчётом.

use std::sync::Arc;

use crate::application::auth::AuthContext;
use crate::application::provider::ProviderService;
use crate::application::settings::{global_keys, user_keys, SettingsService};
use crate::domain::entities::scenario::{Difficulty, Scenario};
use crate::domain::entities::session::{
    MessageRole, Session, SessionMessage, SessionMode, SessionStatus,
};
use crate::domain::ports::{
    AuditRepository, ChatMessage, ChatRequest, LlmUsageRepository, ScenarioRepository,
    SessionRepository,
};
use crate::domain::services::scoring::SessionReport;
use crate::domain::services::{analysis, scoring};
use crate::error::{AppError, AppResult};
use crate::infrastructure::db::repos::SqliteRepos;

/// Резервный максимум ходов, если настройка `platform.max_turns` не задана
/// или не является числом. Правда — в настройках ([`global_keys::MAX_TURNS`]).
const FALLBACK_MAX_TURNS: u32 = 40;

/// Максимальная длина реплики игрока в символах (защита от DoS и раздувания prompt).
const MAX_PLAYER_TEXT_CHARS: usize = 4000;

/// Верхняя граница `limit` для истории сессий.
const MAX_HISTORY_LIMIT: u32 = 200;

/// Грубая оценка токенов, когда провайдер не вернул `usage` (~4 символа на токен).
fn estimate_tokens(text: &str) -> u64 {
    let chars = text.chars().count() as u64;
    chars.div_ceil(4).max(1)
}

/// Итог одного хода игрока.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TurnOutcome {
    pub session: Session,
    pub partner_reply: String,
    pub player_score_delta: i32,
    pub strategy_slug: &'static str,
    pub spin_code: Option<&'static str>,
    pub total_score: i32,
}

/// Результат старта сессии: сессия + сценарий для карточек UI.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionStarted {
    pub session: Session,
    pub scenario: Scenario,
    pub opening: String,
}

/// Логика прохождения сценария.
pub struct SessionService {
    repos: Arc<SqliteRepos>,
    providers: Arc<ProviderService>,
    settings: Arc<SettingsService>,
}

impl SessionService {
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

    /// Лимит ходов: правда из `platform.max_turns`, иначе [`FALLBACK_MAX_TURNS`].
    fn max_turns(&self) -> u32 {
        self.settings
            .get_global(global_keys::MAX_TURNS)
            .ok()
            .and_then(|v| v.trim().parse::<u32>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(FALLBACK_MAX_TURNS)
    }

    // ── Чтение ──

    pub fn get(&self, actor: &AuthContext, session_id: &str) -> AppResult<Session> {
        let session = self.load(session_id)?;
        self.ensure_owner(actor, &session)?;
        Ok(session)
    }

    pub fn messages(
        &self,
        actor: &AuthContext,
        session_id: &str,
    ) -> AppResult<Vec<SessionMessage>> {
        let session = self.load(session_id)?;
        self.ensure_owner(actor, &session)?;
        self.repos.sessions.messages(session_id)
    }

    pub fn history(
        &self,
        actor: &AuthContext,
        limit: u32,
        offset: u32,
    ) -> AppResult<(Vec<Session>, u64)> {
        let limit = limit.clamp(1, MAX_HISTORY_LIMIT);
        self.repos
            .sessions
            .list_by_user(&actor.user_id, limit, offset)
    }

    // ── Старт ──

    pub fn start(
        &self,
        actor: &AuthContext,
        scenario_id: &str,
        mode: SessionMode,
    ) -> AppResult<SessionStarted> {
        let scenario = self
            .repos
            .scenarios
            .get(scenario_id)?
            .ok_or_else(|| AppError::NotFound("сценарий не найден".into()))?;
        if !scenario.is_active && !actor.is_admin() {
            return Err(AppError::NotFound("сценарий не найден".into()));
        }

        let now = chrono::Utc::now().to_rfc3339();
        let session = Session {
            id: uuid::Uuid::new_v4().to_string(),
            user_id: actor.user_id.clone(),
            scenario_id: scenario.id.clone(),
            mode,
            status: SessionStatus::Active,
            total_score: 0,
            turn_count: 0,
            metrics: Default::default(),
            ending_id: None,
            ending_title: None,
            feedback: None,
            created_at: now.clone(),
            finished_at: None,
        };

        let opening = scenario.opening_context.clone();
        let partner_msg = SessionMessage {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session.id.clone(),
            turn_index: 0,
            role: MessageRole::Partner,
            content: opening.clone(),
            strategy: None,
            score_delta: 0,
            created_at: now,
        };
        // Сессия + opening — одной транзакцией: иначе при сбое второй записи
        // в истории останется сессия без первого сообщения.
        self.repos
            .sessions
            .create_with_opening(&session, &partner_msg)?;
        self.audit(actor, "session.start", &session.id);

        Ok(SessionStarted {
            session,
            scenario,
            opening,
        })
    }

    // ── Ход ──

    /// Полный ход: ответ LLM собеседника + анализ и скоринг реплики игрока.
    ///
    /// Порядок: сначала сетевой вызов LLM (в репозитории ничего не трогаем
    /// при ошибке), затем сохранение обоих сообщений и метрик.
    pub async fn submit_turn(
        &self,
        actor: &AuthContext,
        session_id: &str,
        player_text: &str,
    ) -> AppResult<TurnOutcome> {
        let player_text = player_text.trim();
        if player_text.is_empty() {
            return Err(AppError::BadRequest("реплика не может быть пустой".into()));
        }
        if player_text.chars().count() > MAX_PLAYER_TEXT_CHARS {
            return Err(AppError::BadRequest(format!(
                "реплика не длиннее {MAX_PLAYER_TEXT_CHARS} символов"
            )));
        }

        let mut session = self.load(session_id)?;
        self.ensure_owner_strict(actor, &session)?;
        if session.status != SessionStatus::Active {
            return Err(AppError::BadRequest("сессия уже завершена".into()));
        }
        let max_turns = self.max_turns();
        if session.turn_count >= max_turns {
            return Err(AppError::BadRequest(format!(
                "достигнут лимит ходов ({max_turns}); завершите сессию"
            )));
        }

        let scenario = self
            .repos
            .scenarios
            .get(&session.scenario_id)?
            .ok_or_else(|| AppError::NotFound("сценарий сессии не найден".into()))?;

        let history = self.repos.sessions.messages(session_id)?;

        // 1) Ответ собеседника (LLM) — до записи в БД.
        // Резолв модели: предпочтение пользователя → глобальное назначение роли.
        let partner_reply = self
            .partner_reply(&scenario, &history, player_text, &session.user_id)
            .await?;

        // 2) Анализ и скоринг реплики игрока.
        let analysis = analysis::analyze(player_text);
        let score_before = session.total_score;
        let score_delta = scoring::apply_analysis(&mut session.metrics, &analysis);
        session.total_score = session.metrics.total_score();
        session.turn_count += 1;
        let total_score = session.total_score;
        debug_assert_eq!(total_score, score_before + score_delta);

        let now = chrono::Utc::now().to_rfc3339();
        let player_msg = SessionMessage {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session.id.clone(),
            turn_index: (history.len() as u32).max(1),
            role: MessageRole::Player,
            content: player_text.to_string(),
            strategy: Some(analysis.strategy.slug().to_string()),
            score_delta,
            created_at: now.clone(),
        };
        let partner_msg = SessionMessage {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session.id.clone(),
            turn_index: player_msg.turn_index + 1,
            role: MessageRole::Partner,
            content: partner_reply.clone(),
            strategy: None,
            score_delta: 0,
            created_at: now,
        };

        // Счёт + 2 реплики + UPDATE сессии — одна транзакция:
        // иначе при сбое середины ход «повисит» (счёт без сообщений или наоборот).
        self.repos
            .sessions
            .commit_turn(&session, &player_msg, Some(&partner_msg))?;

        Ok(TurnOutcome {
            session,
            partner_reply,
            player_score_delta: score_delta,
            strategy_slug: analysis.strategy.slug(),
            spin_code: analysis.spin.map(|s| s.code()),
            total_score, // накопленный счёт сессии (не дельта этого хода)
        })
    }

    /// Только запись реплики игрока и скоринг — без LLM (для тестов и voice-чертежа).
    pub fn record_player_turn(
        &self,
        actor: &AuthContext,
        session_id: &str,
        player_text: &str,
    ) -> AppResult<TurnOutcome> {
        let player_text = player_text.trim();
        if player_text.is_empty() {
            return Err(AppError::BadRequest("реплика не может быть пустой".into()));
        }
        if player_text.chars().count() > MAX_PLAYER_TEXT_CHARS {
            return Err(AppError::BadRequest(format!(
                "реплика не длиннее {MAX_PLAYER_TEXT_CHARS} символов"
            )));
        }
        let mut session = self.load(session_id)?;
        self.ensure_owner_strict(actor, &session)?;
        if session.status != SessionStatus::Active {
            return Err(AppError::BadRequest("сессия уже завершена".into()));
        }
        let max_turns = self.max_turns();
        if session.turn_count >= max_turns {
            return Err(AppError::BadRequest(format!(
                "достигнут лимит ходов ({max_turns}); завершите сессию"
            )));
        }

        let analysis = analysis::analyze(player_text);
        let score_before = session.total_score;
        let score_delta = scoring::apply_analysis(&mut session.metrics, &analysis);
        session.total_score = session.metrics.total_score();
        session.turn_count += 1;
        let total_score = session.total_score;
        debug_assert_eq!(total_score, score_before + score_delta);

        let history_len = self.repos.sessions.messages(session_id)?.len();
        let msg = SessionMessage {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session.id.clone(),
            turn_index: (history_len as u32).max(1),
            role: MessageRole::Player,
            content: player_text.to_string(),
            strategy: Some(analysis.strategy.slug().to_string()),
            score_delta,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        self.repos.sessions.commit_turn(&session, &msg, None)?;

        Ok(TurnOutcome {
            session,
            partner_reply: String::new(),
            player_score_delta: score_delta,
            strategy_slug: analysis.strategy.slug(),
            spin_code: analysis.spin.map(|s| s.code()),
            total_score,
        })
    }

    // ── Финал ──

    pub fn finish(
        &self,
        actor: &AuthContext,
        session_id: &str,
    ) -> AppResult<(Session, SessionReport)> {
        let mut session = self.load(session_id)?;
        self.ensure_owner_strict(actor, &session)?;
        if session.status == SessionStatus::Finished {
            return Err(AppError::BadRequest("сессия уже завершена".into()));
        }

        let scenario = self
            .repos
            .scenarios
            .get(&session.scenario_id)?
            .ok_or_else(|| AppError::NotFound("сценарий сессии не найден".into()))?;

        let locale = self.report_locale(actor)?;
        let report =
            scoring::build_report(&scenario, &session.metrics, session.turn_count, &locale);
        session.status = SessionStatus::Finished;
        session.total_score = report.total_score;
        session.ending_id = Some(report.ending.id.clone());
        session.ending_title = Some(report.ending.title.clone());
        session.feedback = Some(report.feedback.clone());
        session.finished_at = Some(chrono::Utc::now().to_rfc3339());

        self.repos.sessions.update(&session)?;
        self.audit(actor, "session.finish", &session.id);
        Ok((session, report))
    }

    /// Прерывает сессию (без финального отчёта победы).
    pub fn abandon(&self, actor: &AuthContext, session_id: &str) -> AppResult<Session> {
        let mut session = self.load(session_id)?;
        self.ensure_owner_strict(actor, &session)?;
        if session.status != SessionStatus::Active {
            return Err(AppError::BadRequest("сессия уже не активна".into()));
        }
        session.status = SessionStatus::Abandoned;
        session.finished_at = Some(chrono::Utc::now().to_rfc3339());
        self.repos.sessions.update(&session)?;
        self.audit(actor, "session.abandon", &session.id);
        Ok(session)
    }

    /// Пересобирает отчёт для уже завершённой (или активной) сессии — для UI.
    pub fn report(&self, actor: &AuthContext, session_id: &str) -> AppResult<SessionReport> {
        let session = self.load(session_id)?;
        self.ensure_owner(actor, &session)?;
        let scenario = self
            .repos
            .scenarios
            .get(&session.scenario_id)?
            .ok_or_else(|| AppError::NotFound("сценарий сессии не найден".into()))?;
        let locale = self.report_locale(actor)?;
        Ok(scoring::build_report(
            &scenario,
            &session.metrics,
            session.turn_count,
            &locale,
        ))
    }

    /// Локаль отчёта из пользовательских настроек (`locale`), иначе `ru`.
    fn report_locale(&self, actor: &AuthContext) -> AppResult<String> {
        let locale = self
            .settings
            .get_user(actor, &actor.user_id, user_keys::LOCALE)
            .unwrap_or_else(|_| "ru".to_string());
        let locale = locale.trim().to_ascii_lowercase();
        Ok(if locale == "en" {
            "en".into()
        } else {
            "ru".into()
        })
    }

    // ── Внутреннее ──

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

    async fn partner_reply(
        &self,
        scenario: &Scenario,
        history: &[SessionMessage],
        player_text: &str,
        user_id: &str,
    ) -> AppResult<String> {
        let limit = self.llm_daily_token_limit()?;
        self.ensure_llm_quota(user_id, limit)?;

        let (chat, model_key) = self.providers.resolve_chat_for(user_id).await?;

        let mut messages = Vec::with_capacity(history.len() + 2);
        messages.push(ChatMessage::system(system_prompt(scenario)));
        for msg in history {
            match msg.role {
                MessageRole::Partner => messages.push(ChatMessage::assistant(msg.content.clone())),
                MessageRole::Player => messages.push(ChatMessage::user(msg.content.clone())),
            }
        }
        messages.push(ChatMessage::user(player_text));

        let request = ChatRequest::new(model_key, messages)
            .with_temperature(0.7)
            .with_max_tokens(400);
        let response = chat.chat(request).await?;
        // Провайдер может не вернуть usage — оцениваем ответ по длине (~4 chars/token).
        let tokens = response
            .usage
            .total_tokens
            .map(u64::from)
            .unwrap_or_else(|| estimate_tokens(&response.content));
        self.record_llm_usage(user_id, tokens);

        let content = response.content.trim().to_string();
        if content.is_empty() {
            return Err(AppError::upstream(
                "LLM",
                "пустой ответ собеседника, попробуйте ещё раз",
            ));
        }
        Ok(content)
    }

    fn load(&self, session_id: &str) -> AppResult<Session> {
        self.repos
            .sessions
            .get(session_id)?
            .ok_or_else(|| AppError::NotFound("сессия не найдена".into()))
    }

    fn ensure_owner(&self, actor: &AuthContext, session: &Session) -> AppResult<()> {
        if session.user_id == actor.user_id || actor.is_admin() {
            Ok(())
        } else {
            Err(AppError::Forbidden("чужая сессия".into()))
        }
    }

    /// Строгий владелец для мутаций: админ читает чужие сессии, но не ходит
    /// в них, не завершает и не прерывает — иначе чужой токен admin'а ломает
    /// целостность чужого прогресса.
    fn ensure_owner_strict(&self, actor: &AuthContext, session: &Session) -> AppResult<()> {
        if session.user_id == actor.user_id {
            Ok(())
        } else {
            Err(AppError::Forbidden(
                "изменять сессию может только её владелец".into(),
            ))
        }
    }

    fn audit(&self, actor: &AuthContext, action: &str, entity_id: &str) {
        if let Err(err) = self.repos.audit.append(
            Some(&actor.user_id),
            action,
            Some("session"),
            Some(entity_id),
            None,
        ) {
            tracing::warn!(error = %err, action, "не удалось записать аудит");
        }
    }
}

/// System-prompt собеседника из сценария.
fn system_prompt(scenario: &Scenario) -> String {
    let mut p = String::new();
    p.push_str("Ты — собеседник в тренажёре переговоров. Оставайся в роли до конца диалога.\n\n");
    p.push_str(&format!("Роль: {}.\n", scenario.partner_role));
    p.push_str(&format!("Имя: {}.\n", scenario.partner_name));
    if let Some(company) = scenario
        .partner_company
        .as_deref()
        .filter(|s| !s.is_empty())
    {
        p.push_str(&format!("Компания: {company}.\n"));
    }
    p.push_str(&format!("Твоя цель: {}.\n", scenario.partner_goal));
    if !scenario.partner_goals.is_empty() {
        p.push_str(&format!(
            "Дополнительные цели: {}.\n",
            scenario.partner_goals.join("; ")
        ));
    }
    p.push_str(&format!("Твоя BATNA: {}.\n", scenario.partner_batna));
    p.push_str(&format!(
        "Цель игрока (не называй её вслух, но учитывай): {}.\n",
        scenario.player_goal
    ));
    p.push_str(&format!("BATNA игрока: {}\n", scenario.player_batna));
    p.push_str(&format!(
        "Сложность сценария: {} ({}).\n",
        scenario.difficulty.title(),
        scenario.difficulty.slug()
    ));

    let personality = &scenario.partner_personality;
    let mut style_bits = Vec::new();
    if let Some(tone) = personality.tone.as_deref().filter(|s| !s.is_empty()) {
        style_bits.push(format!("тон: {tone}"));
    }
    if let Some(style) = personality.style.as_deref().filter(|s| !s.is_empty()) {
        style_bits.push(format!("манера: {style}"));
    }
    if let Some(traits) = personality.traits.as_deref().filter(|s| !s.is_empty()) {
        style_bits.push(format!("черты: {traits}"));
    }
    if !style_bits.is_empty() {
        p.push_str(&format!("Стиль общения — {}.\n", style_bits.join(", ")));
    }

    // Поведение по сложности: управляет упорством и разнообразием аргументации.
    match scenario.difficulty {
        Difficulty::Easy => p.push_str(
            "\nПоведение (начальная сложность):\n\
             - Уступай после 1–2 убедительных аргументов игрока, не капризничай.\n\
             - Используй простые формулировки, избегай сложных условий и мелких нюансов.\n",
        ),
        Difficulty::Medium => p.push_str(
            "\nПоведение (средняя сложность):\n\
             - Уступай только после конкретных аргументов и чисел, торговись умеренно.\n\
             - Держи позицию 2–3 реплики, предлагай взаимовыгодные варианты.\n",
        ),
        Difficulty::Hard => p.push_str(
            "\nПоведение (сложная сложность):\n\
             - Не уступай без жёстких доказательств и выгоды для себя.\n\
             - Требуй подтверждений, используй BATNA как рычаг, торговись жёстко и долго.\n\
             - Избегай компромиссов «для вида» — соглашайся только на реальную выгоду.\n",
        ),
    }

    p.push_str(
        "\nПравила:\n\
         - Говори как живой человек, 1–4 коротких предложения, без списков и маркеров.\n\
         - Не раскрывай явно свои тайные цели, пока игрок не выяснит их вопросами.\n\
         - Учитывай его аргументы; не уступай мгновенно без сопротивления.\n\
         - Только текст реплики, без преамбул вроде «Собеседник:».\n\
         - Отвечай на том языке, на котором пишет игрок.\n",
    );
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::testsupport::{ctx, setup};
    use crate::domain::entities::scenario::Difficulty;
    use crate::domain::entities::session::SessionMetrics;
    use crate::domain::entities::user::UserRole;
    use crate::domain::ports::UserRepository as _;

    fn seed_scenario(svc: &crate::application::Services) -> String {
        let now = chrono::Utc::now().to_rfc3339();
        let scenario = Scenario {
            id: "sc-test".into(),
            title: "Тест".into(),
            description: "Тест".into(),
            sphere: "Тест".into(),
            difficulty: Difficulty::Easy,
            player_role: "Игрок".into(),
            player_company: None,
            player_goal: "Договориться".into(),
            player_batna: "Уйти".into(),
            partner_name: "Пётр".into(),
            partner_role: "Клиент".into(),
            partner_company: None,
            partner_goal: "Скидку".into(),
            partner_goals: vec![],
            partner_batna: "Конкурент".into(),
            partner_personality: Default::default(),
            opening_context: "Добрый день, обсудим условия?".into(),
            endings: crate::application::scenario::default_endings(),
            ai_generated: false,
            is_active: true,
            created_by: None,
            created_at: now,
            updated_at: None,
        };
        svc.repos.scenarios.upsert(&scenario).unwrap();
        scenario.id
    }

    fn user_ctx(svc: &crate::application::Services) -> AuthContext {
        svc.repos
            .users
            .create("player", "hash", UserRole::User, None)
            .unwrap();
        AuthContext {
            user_id: svc.repos.users.by_login("player").unwrap().unwrap().user.id,
            login: "player".into(),
            role: UserRole::User,
        }
    }

    #[test]
    fn start_creates_session_with_opening_partner_message() {
        let (_db, svc) = setup().unwrap();
        let sc_id = seed_scenario(&svc);
        let user = user_ctx(&svc);

        let started = svc
            .sessions
            .start(&user, &sc_id, SessionMode::Text)
            .unwrap();
        assert_eq!(started.session.status, SessionStatus::Active);
        assert_eq!(started.opening, "Добрый день, обсудим условия?");

        let msgs = svc.sessions.messages(&user, &started.session.id).unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, MessageRole::Partner);
        assert_eq!(msgs[0].turn_index, 0);
    }

    #[test]
    fn record_player_turn_scores_and_updates_metrics() {
        let (_db, svc) = setup().unwrap();
        let sc_id = seed_scenario(&svc);
        let user = user_ctx(&svc);
        let started = svc
            .sessions
            .start(&user, &sc_id, SessionMode::Text)
            .unwrap();

        let outcome = svc
            .sessions
            .record_player_turn(
                &user,
                &started.session.id,
                "Какие условия поставки для вас оптимальны? Давайте найдём решение, выгодное для обеих сторон. По данным рынка скидка 10% обоснована.",
            )
            .unwrap();

        assert!(
            outcome.player_score_delta > 0,
            "позитивная реплика должна очислять"
        );
        assert_eq!(outcome.session.turn_count, 1);
        assert_eq!(
            outcome.session.total_score,
            outcome.session.metrics.total_score()
        );
        // TurnOutcome.total_score — накопленный итог, а не дельта хода.
        assert_eq!(outcome.total_score, outcome.session.total_score);
        assert!(outcome.session.metrics.collaboration_count >= 1);

        let msgs = svc.sessions.messages(&user, &started.session.id).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[1].role, MessageRole::Player);
        assert_eq!(msgs[1].strategy.as_deref(), Some("collaboration"));
    }

    #[test]
    fn confrontational_reply_scores_lower_than_collaborative() {
        let (_db, svc) = setup().unwrap();
        let sc_id = seed_scenario(&svc);
        let user = user_ctx(&svc);

        let a = svc
            .sessions
            .start(&user, &sc_id, SessionMode::Text)
            .unwrap();
        let b = svc
            .sessions
            .start(&user, &sc_id, SessionMode::Text)
            .unwrap();

        let collab = svc
            .sessions
            .record_player_turn(
                &user,
                &a.session.id,
                "Давайте найдём решение, выгодное для обеих сторон.",
            )
            .unwrap();
        let confront = svc
            .sessions
            .record_player_turn(&user, &b.session.id, "Мы не можем, это неприемлемо.")
            .unwrap();

        assert!(
            collab.player_score_delta > confront.player_score_delta,
            "{} vs {}",
            collab.player_score_delta,
            confront.player_score_delta
        );
    }

    #[test]
    fn finish_builds_report_and_closes_session() {
        let (_db, svc) = setup().unwrap();
        let sc_id = seed_scenario(&svc);
        let user = user_ctx(&svc);
        let started = svc
            .sessions
            .start(&user, &sc_id, SessionMode::Text)
            .unwrap();

        svc.sessions
            .record_player_turn(
                &user,
                &started.session.id,
                "Что для вас важнее — цена или сроки?",
            )
            .unwrap();

        let (session, report) = svc.sessions.finish(&user, &started.session.id).unwrap();
        assert_eq!(session.status, SessionStatus::Finished);
        assert!(session.finished_at.is_some());
        assert_eq!(
            session.ending_id.as_deref(),
            Some(report.ending.id.as_str())
        );
        assert!(!report.feedback.is_empty());
        assert_eq!(report.turn_count, 1);

        // Повторный finish — ошибка.
        assert!(svc.sessions.finish(&user, &started.session.id).is_err());
        // Ход после финиша — ошибка.
        assert!(svc
            .sessions
            .record_player_turn(&user, &started.session.id, "ещё")
            .is_err());
    }

    #[test]
    fn foreign_user_cannot_touch_session() {
        let (_db, svc) = setup().unwrap();
        let sc_id = seed_scenario(&svc);
        let owner = user_ctx(&svc);
        let stranger = svc
            .repos
            .users
            .create("stranger", "hash", UserRole::User, None)
            .unwrap();
        let stranger_ctx = AuthContext {
            user_id: stranger.id,
            login: "stranger".into(),
            role: UserRole::User,
        };

        let started = svc
            .sessions
            .start(&owner, &sc_id, SessionMode::Text)
            .unwrap();
        assert!(svc
            .sessions
            .get(&stranger_ctx, &started.session.id)
            .is_err());
        assert!(svc
            .sessions
            .record_player_turn(&stranger_ctx, &started.session.id, "hi")
            .is_err());
        assert!(svc
            .sessions
            .finish(&stranger_ctx, &started.session.id)
            .is_err());

        // Админ (owner-доступ) — может смотреть, но не мутировать.
        let admin = ctx("admin-1", UserRole::Admin);
        assert!(svc.sessions.get(&admin, &started.session.id).is_ok());
        assert!(
            svc.sessions
                .record_player_turn(&admin, &started.session.id, "hi")
                .is_err(),
            "админ не должен ходить в чужую сессию"
        );
        assert!(svc.sessions.finish(&admin, &started.session.id).is_err());
        assert!(svc.sessions.abandon(&admin, &started.session.id).is_err());
    }

    #[test]
    fn record_player_turn_respects_max_turns() {
        let (_db, svc) = setup().unwrap();
        let sc_id = seed_scenario(&svc);
        let user = user_ctx(&svc);
        let started = svc
            .sessions
            .start(&user, &sc_id, SessionMode::Text)
            .unwrap();

        // Поднимаем счётчик ходов до лимита в обход игрового цикла.
        {
            let mut s = svc
                .repos
                .sessions
                .get(&started.session.id)
                .unwrap()
                .unwrap();
            s.turn_count = FALLBACK_MAX_TURNS;
            svc.repos.sessions.update(&s).unwrap();
        }
        let err = svc
            .sessions
            .record_player_turn(&user, &started.session.id, "ещё ход")
            .unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)));
    }

    #[test]
    fn overlong_player_text_is_rejected() {
        let (_db, svc) = setup().unwrap();
        let sc_id = seed_scenario(&svc);
        let user = user_ctx(&svc);
        let started = svc
            .sessions
            .start(&user, &sc_id, SessionMode::Text)
            .unwrap();
        let huge = "ы".repeat(MAX_PLAYER_TEXT_CHARS + 1);
        assert!(svc
            .sessions
            .record_player_turn(&user, &started.session.id, &huge)
            .is_err());
    }

    #[test]
    fn abandon_marks_session() {
        let (_db, svc) = setup().unwrap();
        let sc_id = seed_scenario(&svc);
        let user = user_ctx(&svc);
        let started = svc
            .sessions
            .start(&user, &sc_id, SessionMode::Text)
            .unwrap();
        let s = svc.sessions.abandon(&user, &started.session.id).unwrap();
        assert_eq!(s.status, SessionStatus::Abandoned);
        assert!(svc.sessions.abandon(&user, &started.session.id).is_err());
    }

    #[test]
    fn empty_player_text_is_rejected() {
        let (_db, svc) = setup().unwrap();
        let sc_id = seed_scenario(&svc);
        let user = user_ctx(&svc);
        let started = svc
            .sessions
            .start(&user, &sc_id, SessionMode::Text)
            .unwrap();
        assert!(svc
            .sessions
            .record_player_turn(&user, &started.session.id, "   ")
            .is_err());
    }

    // ── LLM-квота ──

    #[test]
    fn llm_quota_zero_means_disabled_and_records_usage() {
        let (_db, svc) = setup().unwrap();
        let user = user_ctx(&svc);
        let day = chrono::Utc::now().format("%Y-%m-%d").to_string();

        // limit=0 → квота выключена.
        assert_eq!(
            svc.sessions.llm_daily_token_limit().unwrap(),
            0,
            "default platform.llm_daily_token_limit = 0"
        );
        svc.sessions.ensure_llm_quota(&user.user_id, 0).unwrap();

        // Учёт токенов идёт даже при выключенном лимите.
        svc.sessions.record_llm_usage(&user.user_id, 42);
        assert_eq!(
            svc.repos.llm_usage.tokens_on(&user.user_id, &day).unwrap(),
            42
        );
        svc.sessions.record_llm_usage(&user.user_id, 8);
        assert_eq!(
            svc.repos.llm_usage.tokens_on(&user.user_id, &day).unwrap(),
            50
        );
    }

    #[test]
    fn llm_quota_exhausted_returns_429() {
        let (_db, svc) = setup().unwrap();
        let user = user_ctx(&svc);
        let day = chrono::Utc::now().format("%Y-%m-%d").to_string();

        // Ровно лимит → блок.
        svc.repos
            .llm_usage
            .add_usage(&user.user_id, &day, 100)
            .unwrap();
        let err = svc
            .sessions
            .ensure_llm_quota(&user.user_id, 100)
            .unwrap_err();
        assert!(
            matches!(err, AppError::TooManyRequests(_)),
            "ожидали 429, получили: {err}"
        );

        // Лимит выше израсходованного → можно.
        svc.sessions.ensure_llm_quota(&user.user_id, 101).unwrap();
        // Ноль — квота выключена, всегда можно.
        svc.sessions.ensure_llm_quota(&user.user_id, 0).unwrap();
    }

    #[test]
    fn estimate_tokens_uses_about_four_chars_per_token() {
        assert_eq!(estimate_tokens(""), 1);
        assert_eq!(estimate_tokens("аб"), 1);
        assert_eq!(estimate_tokens("абвг"), 1);
        assert_eq!(estimate_tokens("абвгд"), 2);
        assert_eq!(estimate_tokens(&"x".repeat(40)), 10);
    }

    #[test]
    fn system_prompt_contains_role_and_rules() {
        let sc = Scenario {
            id: "x".into(),
            title: "t".into(),
            description: "d".into(),
            sphere: "s".into(),
            difficulty: Difficulty::Easy,
            player_role: "p".into(),
            player_company: None,
            player_goal: "g".into(),
            player_batna: "b".into(),
            partner_name: "Анна".into(),
            partner_role: "Закупщик".into(),
            partner_company: Some("ООО Х".into()),
            partner_goal: "g2".into(),
            partner_goals: vec!["долгий контракт".into()],
            partner_batna: "b2".into(),
            partner_personality: crate::domain::entities::scenario::PartnerPersonality {
                tone: Some("сдержанный".into()),
                style: None,
                traits: Some("недоверчивый".into()),
            },
            opening_context: "o".into(),
            endings: vec![],
            ai_generated: false,
            is_active: true,
            created_by: None,
            created_at: "now".into(),
            updated_at: None,
        };
        let prompt = system_prompt(&sc);
        assert!(prompt.contains("Закупщик"));
        assert!(prompt.contains("Анна"));
        assert!(prompt.contains("ООО Х"));
        assert!(prompt.contains("недоверчивый"));
        assert!(prompt.contains("BATNA"));
        assert!(prompt.contains("1–4"));
        assert!(prompt.contains("Сложность сценария: Начальная (easy)"));
        assert!(prompt.contains("начальная сложность"));
        assert!(prompt.contains("на том языке"));
    }

    #[test]
    fn system_prompt_reflects_hard_difficulty() {
        let mut sc = Scenario {
            id: "x".into(),
            title: "t".into(),
            description: "d".into(),
            sphere: "s".into(),
            difficulty: Difficulty::Medium,
            player_role: "p".into(),
            player_company: None,
            player_goal: "g".into(),
            player_batna: "b".into(),
            partner_name: "Анна".into(),
            partner_role: "Закупщик".into(),
            partner_company: None,
            partner_goal: "g2".into(),
            partner_goals: vec![],
            partner_batna: "b2".into(),
            partner_personality: Default::default(),
            opening_context: "o".into(),
            endings: vec![],
            ai_generated: false,
            is_active: true,
            created_by: None,
            created_at: "now".into(),
            updated_at: None,
        };
        let medium = system_prompt(&sc);
        assert!(medium.contains("средняя сложность"));
        sc.difficulty = Difficulty::Hard;
        let hard = system_prompt(&sc);
        assert!(hard.contains("Сложность сценария: Сложная (hard)"));
        assert!(hard.contains("сложная сложность"));
        assert!(!hard.contains("начальная сложность"));
    }

    #[test]
    fn metrics_type_used_by_session_is_default() {
        let m = SessionMetrics::default();
        assert_eq!(m.total_score(), 0);
    }
}
