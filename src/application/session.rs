//! Сессии: старт, ход диалога (LLM + скоринг), финализация с отчётом.

use std::sync::Arc;

use crate::application::auth::AuthContext;
use crate::application::provider::ProviderService;
use crate::domain::entities::scenario::Scenario;
use crate::domain::entities::session::{
    MessageRole, Session, SessionMessage, SessionMode, SessionStatus,
};
use crate::domain::ports::{
    AuditRepository, ChatMessage, ChatRequest, ScenarioRepository, SessionRepository,
};
use crate::domain::services::scoring::SessionReport;
use crate::domain::services::{analysis, scoring};
use crate::error::{AppError, AppResult};
use crate::infrastructure::db::repos::SqliteRepos;

/// Максимум ходов игрока в сессии, чтобы LLM-затраты были ограниченными.
const MAX_TURNS: u32 = 40;

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
}

impl SessionService {
    pub fn new(repos: Arc<SqliteRepos>, providers: Arc<ProviderService>) -> Self {
        Self { repos, providers }
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

    pub fn history(&self, actor: &AuthContext, limit: u32) -> AppResult<Vec<Session>> {
        self.repos.sessions.list_by_user(&actor.user_id, limit)
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
        self.repos.sessions.create(&session)?;

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
        self.repos.sessions.append_message(&partner_msg)?;
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

        let mut session = self.load(session_id)?;
        self.ensure_owner(actor, &session)?;
        if session.status != SessionStatus::Active {
            return Err(AppError::BadRequest("сессия уже завершена".into()));
        }
        if session.turn_count >= MAX_TURNS {
            return Err(AppError::BadRequest(format!(
                "достигнут лимит ходов ({MAX_TURNS}); завершите сессию"
            )));
        }

        let scenario = self
            .repos
            .scenarios
            .get(&session.scenario_id)?
            .ok_or_else(|| AppError::NotFound("сценарий сессии не найден".into()))?;

        let history = self.repos.sessions.messages(session_id)?;

        // 1) Ответ собеседника (LLM) — до записи в БД.
        let partner_reply = self.partner_reply(&scenario, &history, player_text).await?;

        // 2) Анализ и скоринг реплики игрока.
        let analysis = analysis::analyze(player_text);
        let score_delta = scoring::apply_analysis(&mut session.metrics, &analysis);
        session.total_score = session.metrics.total_score();
        session.turn_count += 1;

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

        self.repos.sessions.append_message(&player_msg)?;
        self.repos.sessions.append_message(&partner_msg)?;
        self.repos.sessions.update(&session)?;

        Ok(TurnOutcome {
            session,
            partner_reply,
            player_score_delta: score_delta,
            strategy_slug: analysis.strategy.slug(),
            spin_code: analysis.spin.map(|s| s.code()),
            total_score: score_delta, // legacy-поле совместимости; итог в session
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
        let mut session = self.load(session_id)?;
        self.ensure_owner(actor, &session)?;
        if session.status != SessionStatus::Active {
            return Err(AppError::BadRequest("сессия уже завершена".into()));
        }

        let analysis = analysis::analyze(player_text);
        let score_delta = scoring::apply_analysis(&mut session.metrics, &analysis);
        session.total_score = session.metrics.total_score();
        session.turn_count += 1;

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
        self.repos.sessions.append_message(&msg)?;
        self.repos.sessions.update(&session)?;

        Ok(TurnOutcome {
            session,
            partner_reply: String::new(),
            player_score_delta: score_delta,
            strategy_slug: analysis.strategy.slug(),
            spin_code: analysis.spin.map(|s| s.code()),
            total_score: score_delta,
        })
    }

    // ── Финал ──

    pub fn finish(
        &self,
        actor: &AuthContext,
        session_id: &str,
    ) -> AppResult<(Session, SessionReport)> {
        let mut session = self.load(session_id)?;
        self.ensure_owner(actor, &session)?;
        if session.status == SessionStatus::Finished {
            return Err(AppError::BadRequest("сессия уже завершена".into()));
        }

        let scenario = self
            .repos
            .scenarios
            .get(&session.scenario_id)?
            .ok_or_else(|| AppError::NotFound("сценарий сессии не найден".into()))?;

        let report = scoring::build_report(&scenario, &session.metrics, session.turn_count);
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
        self.ensure_owner(actor, &session)?;
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
        Ok(scoring::build_report(
            &scenario,
            &session.metrics,
            session.turn_count,
        ))
    }

    // ── Внутреннее ──

    async fn partner_reply(
        &self,
        scenario: &Scenario,
        history: &[SessionMessage],
        player_text: &str,
    ) -> AppResult<String> {
        let (chat, model_key) = self.providers.resolve_chat().await?;

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
    p.push_str(&format!("BATNA игрока: {}.\n", scenario.player_batna));

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

    p.push_str(
        "\nПравила:\n\
         - Говори как живой человек, 1–4 коротких предложения, без списков и маркеров.\n\
         - Не раскрывай явно свои тайные цели, пока игрок не выяснит их вопросами.\n\
         - Учитывай его аргументы; не уступай мгновенно без сопротивления.\n\
         - Только текст реплики, без преамбул вроде «Собеседник:».\n",
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

        // Админ (owner-доступ) — может смотреть.
        let admin = ctx("admin-1", UserRole::Admin);
        assert!(svc.sessions.get(&admin, &started.session.id).is_ok());
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
    }

    #[test]
    fn metrics_type_used_by_session_is_default() {
        let m = SessionMetrics::default();
        assert_eq!(m.total_score(), 0);
    }
}
