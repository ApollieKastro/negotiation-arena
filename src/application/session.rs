//! Сессии: старт, ход диалога (LLM + скоринг), финализация с отчётом.

use std::sync::Arc;
use std::time::Duration;

use crate::application::auth::AuthContext;
use crate::application::provider::ProviderService;
use crate::application::settings::{global_keys, user_keys, SettingsService};
use crate::domain::entities::scenario::{Difficulty, Scenario};
use crate::domain::entities::session::{
    MessageRole, Session, SessionBranch, SessionMessage, SessionMode, SessionStatus,
};
use crate::domain::ports::{
    AuditRepository, ChatMessage, ChatRequest, LlmUsageRepository, ScenarioRepository,
    SessionRepository,
};
use crate::domain::services::judge::{self, JudgeScores};
use crate::domain::services::scoring::SessionReport;
use crate::domain::services::{analysis, scoring};
use crate::error::{AppError, AppResult};
use crate::infrastructure::db::repos::SqliteRepos;

/// Резервный максимум ходов, если настройка `platform.max_turns` не задана
/// или не является числом. Правда — в настройках ([`global_keys::MAX_TURNS`]).
const FALLBACK_MAX_TURNS: u32 = 40;

/// Таймаут запроса к LLM-судье. Превышение → ход считается на эвристике:
/// оценка модели не должна задерживать диалог. Больше ответа собеседника:
/// судья идёт параллельно, поэтому итоговая задержка хода — максимум из двух.
const JUDGE_TIMEOUT: Duration = Duration::from_secs(20);

/// Максимальная длина реплики игрока в символах (защита от DoS и раздувания prompt).
const MAX_PLAYER_TEXT_CHARS: usize = 4000;

/// Верхняя граница `limit` для истории сессий.
const MAX_HISTORY_LIMIT: u32 = 200;

/// Максимум веток на сессию (включая main) — защита от раздувания дерева.
pub const MAX_BRANCHES: u64 = 16;

/// Результат создания ветки: новая ветка + сессия, откатнутая к точке ветвления.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BranchCreated {
    pub branch: SessionBranch,
    pub session: Session,
}

/// Пересчитывает метрики ветки по её репликам — детерминированный fold
/// (тот же `analyze` + `apply_analysis`, что и в ходе).
///
/// Нужен при форке: снапшот в точке ветвления восстанавливается из
/// скопированного префикса, а не хранится в каждой реплике.
fn replay_metrics(
    messages: &[SessionMessage],
) -> (crate::domain::entities::session::SessionMetrics, u32) {
    let mut metrics = crate::domain::entities::session::SessionMetrics::default();
    let mut turns = 0u32;
    for msg in messages.iter().filter(|m| m.role == MessageRole::Player) {
        let analysis = analysis::analyze(&msg.content);
        scoring::apply_analysis(&mut metrics, &analysis);
        turns += 1;
    }
    (metrics, turns)
}

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
    /// Оценка LLM-судьи (0..=10 по трём шкалам), `None` — судья не работал
    /// (выключен, сбой, квота) и баллы счищены на эвристике целиком.
    pub judge: Option<JudgeScores>,
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

    /// Реплики диалога: по умолчанию — текущая ветка, с `branch_id` — указанная.
    pub fn messages(
        &self,
        actor: &AuthContext,
        session_id: &str,
        branch_id: Option<&str>,
    ) -> AppResult<Vec<SessionMessage>> {
        let session = self.load(session_id)?;
        self.ensure_owner(actor, &session)?;
        match branch_id {
            None => self.repos.sessions.messages(session_id),
            Some(bid) => {
                let branch = self.load_branch_for(&session, bid)?;
                self.repos
                    .sessions
                    .messages_in_branch(session_id, &branch.id)
            }
        }
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

    // ── Ветвление диалога ──

    /// Ветки сессии в порядке создания (main — первая).
    pub fn branches(&self, actor: &AuthContext, session_id: &str) -> AppResult<Vec<SessionBranch>> {
        let session = self.load(session_id)?;
        self.ensure_owner(actor, &session)?;
        self.repos.sessions.branches(session_id)
    }

    /// Ветвится **после** реплики собеседника: копирует префикс реплик до
    /// точки ветвления в новую ветку, пересчитывает метрики в этой точке
    /// и делает ветку текущей (сессия «откатывается» к ней).
    ///
    /// Ограничения: только активная сессия, только partner-реплика,
    /// не больше [`MAX_BRANCHES`] веток.
    pub fn create_branch(
        &self,
        actor: &AuthContext,
        session_id: &str,
        after_message_id: &str,
    ) -> AppResult<BranchCreated> {
        let mut session = self.load(session_id)?;
        self.ensure_owner_strict(actor, &session)?;
        if session.status != SessionStatus::Active {
            return Err(AppError::BadRequest(
                "ветвление доступно только для активной сессии".into(),
            ));
        }
        if self.repos.sessions.count_branches(session_id)? >= MAX_BRANCHES {
            return Err(AppError::BadRequest(format!(
                "достигнут лимит веток ({MAX_BRANCHES})"
            )));
        }

        // Точка ветвления — реплика этой сессии.
        let point = self
            .repos
            .sessions
            .message(after_message_id)?
            .filter(|m| m.session_id == session.id)
            .ok_or_else(|| AppError::NotFound("реплика не найдена".into()))?;
        // Модель дерева: форк только после реплики собеседника — тогда новая
        // ветка всегда заканчивается partner-репликой и ходы не «двойнятся».
        if point.role != MessageRole::Partner {
            return Err(AppError::BadRequest(
                "ветвление возможно только после реплики собеседника".into(),
            ));
        }

        let source =
            self.load_branch_for(&session, point.branch_id.as_deref().unwrap_or_default())?;
        // Префикс: реплики ветки-источника до точки ветвления включительно.
        let prefix: Vec<SessionMessage> = self
            .repos
            .sessions
            .messages_in_branch(session_id, &source.id)?
            .into_iter()
            .filter(|m| m.turn_index <= point.turn_index)
            .collect();

        // Снапшот метрик в точке ветвления — replay по player-репликам префикса.
        let (metrics, turn_count) = replay_metrics(&prefix);
        let total_score = metrics.total_score();

        let branch = SessionBranch {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session.id.clone(),
            parent_id: Some(source.id.clone()),
            label: "fork".into(),
            fork_turn_index: point.turn_index,
            metrics: metrics.clone(),
            total_score,
            turn_count,
            is_current: true,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        // Копии префикса: свои id, та же последовательность ходов.
        let copies: Vec<SessionMessage> = prefix
            .iter()
            .map(|m| SessionMessage {
                id: uuid::Uuid::new_v4().to_string(),
                branch_id: Some(branch.id.clone()),
                ..m.clone()
            })
            .collect();

        // Сессия откатывается к точке ветвления (status/feedback не трогаем).
        session.metrics = metrics;
        session.total_score = total_score;
        session.turn_count = turn_count;

        self.repos
            .sessions
            .fork_branch(&branch, &copies, &session)?;
        self.audit(actor, "session.branch", &session.id);
        Ok(BranchCreated { branch, session })
    }

    /// Переключает текущую ветку: состояние сессии (метрики/счёт/ходы)
    /// подставляется из снапшота ветки. No-op, если ветка уже текущая.
    pub fn switch_branch(
        &self,
        actor: &AuthContext,
        session_id: &str,
        branch_id: &str,
    ) -> AppResult<(Session, SessionBranch)> {
        let mut session = self.load(session_id)?;
        self.ensure_owner_strict(actor, &session)?;
        if session.status != SessionStatus::Active {
            return Err(AppError::BadRequest(
                "переключение веток доступно только для активной сессии".into(),
            ));
        }
        let mut branch = self.load_branch_for(&session, branch_id)?;
        if !branch.is_current {
            session.metrics = branch.metrics.clone();
            session.total_score = branch.total_score;
            session.turn_count = branch.turn_count;
            self.repos.sessions.switch_branch(&branch.id, &session)?;
            // Возвращаем актуальное состояние: ветка стала текущей.
            branch.is_current = true;
            self.audit(actor, "session.switch_branch", &session.id);
        }
        Ok((session, branch))
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
            // Opening принадлежит main-ветке (id ветки = id сессии).
            branch_id: Some(session.id.clone()),
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
    /// Порядок: сетевые вызовы LLM (в репозитории ничего не трогаем при
    /// ошибке), затем сохранение обоих сообщений и метрик. Ответ собеседника
    /// и оценка LLM-судьи выполняются параллельно; сбой судьи не валит ход —
    /// баллы тогда считаются на эвристике целиком.
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

        // Ход идёт в текущую ветку; её снапшот обновляем вместе с сессией.
        let mut branch = self.current_branch(&session.id)?;
        let history = self.repos.sessions.messages(session_id)?;

        // 1) Ответ собеседника (LLM) и оценка судьи — параллельно, до записи в БД.
        // Резолв модели: предпочтение пользователя → глобальное назначение роли.
        let (partner_reply, judge_scores) = tokio::join!(
            self.partner_reply(&scenario, &history, player_text, &session.user_id),
            self.judge(&scenario, &history, player_text, &session.user_id),
        );
        let partner_reply = partner_reply?;

        // 2) Анализ и скоринг реплики игрока: эвристика, смешанная с LLM-оценкой.
        let analysis = analysis::analyze(player_text);
        let heuristic = scoring::heuristic_points(&analysis);
        let points = match judge_scores {
            Some(scores) => scoring::blend(heuristic, scores, self.judge_weight()),
            None => heuristic,
        };
        // Категория стратегии: решение судьи, иначе эвристика. По ней — и
        // подпись под баллом в ленте, и счётчики в итоговом отчёте.
        let strategy = judge_scores
            .and_then(|scores| scores.category)
            .unwrap_or(analysis.strategy);
        let score_before = session.total_score;
        let score_delta = scoring::apply_points(&mut session.metrics, &analysis, points, strategy);
        session.total_score = session.metrics.total_score();
        session.turn_count += 1;
        let total_score = session.total_score;
        debug_assert_eq!(total_score, score_before + score_delta);
        branch.metrics = session.metrics.clone();
        branch.total_score = session.total_score;
        branch.turn_count = session.turn_count;

        let now = chrono::Utc::now().to_rfc3339();
        let player_msg = SessionMessage {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session.id.clone(),
            branch_id: Some(branch.id.clone()),
            turn_index: (history.len() as u32).max(1),
            role: MessageRole::Player,
            content: player_text.to_string(),
            strategy: Some(strategy.slug().to_string()),
            score_delta,
            created_at: now.clone(),
        };
        let partner_msg = SessionMessage {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session.id.clone(),
            branch_id: Some(branch.id.clone()),
            turn_index: player_msg.turn_index + 1,
            role: MessageRole::Partner,
            content: partner_reply.clone(),
            strategy: None,
            score_delta: 0,
            created_at: now,
        };

        // Счёт + 2 реплики + UPDATE сессии и ветки — одна транзакция:
        // иначе при сбое середины ход «повисит» (счёт без сообщений или наоборот).
        self.repos
            .sessions
            .commit_turn(&session, &branch, &player_msg, Some(&partner_msg))?;

        Ok(TurnOutcome {
            session,
            partner_reply,
            player_score_delta: score_delta,
            strategy_slug: strategy.slug(),
            spin_code: analysis.spin.map(|s| s.code()),
            total_score, // накопленный счёт сессии (не дельта этого хода)
            judge: judge_scores,
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

        let mut branch = self.current_branch(&session.id)?;
        let analysis = analysis::analyze(player_text);
        let score_before = session.total_score;
        let score_delta = scoring::apply_analysis(&mut session.metrics, &analysis);
        session.total_score = session.metrics.total_score();
        session.turn_count += 1;
        let total_score = session.total_score;
        debug_assert_eq!(total_score, score_before + score_delta);
        branch.metrics = session.metrics.clone();
        branch.total_score = session.total_score;
        branch.turn_count = session.turn_count;

        let history_len = self.repos.sessions.messages(session_id)?.len();
        let msg = SessionMessage {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session.id.clone(),
            branch_id: Some(branch.id.clone()),
            turn_index: (history_len as u32).max(1),
            role: MessageRole::Player,
            content: player_text.to_string(),
            strategy: Some(analysis.strategy.slug().to_string()),
            score_delta,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        self.repos
            .sessions
            .commit_turn(&session, &branch, &msg, None)?;

        Ok(TurnOutcome {
            session,
            partner_reply: String::new(),
            player_score_delta: score_delta,
            strategy_slug: analysis.strategy.slug(),
            spin_code: analysis.spin.map(|s| s.code()),
            total_score,
            // Без сети — только эвристика.
            judge: None,
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
            // Запас под русские реплики; reasoning-модели съедали 400 на «thinking».
            .with_max_tokens(1024);
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
            // Частая причина у reasoning-моделей: весь max_tokens ушёл в thinking.
            let hint = if response.finish_reason.as_deref() == Some("length") {
                "ответ пуст: модель не уложилась в лимит токенов (возможно, reasoning-модель)"
            } else {
                "пустой ответ собеседника, попробуйте ещё раз"
            };
            return Err(AppError::upstream("LLM", hint));
        }
        Ok(content)
    }

    // ── LLM-судья ──

    /// Оценивает последнюю реплику игрока моделью (0..=10 по трём шкалам).
    ///
    /// Судья ничего не ломает: при выключенной настройке, исчерпанной квоте,
    /// ненастроенной модели, таймауте или неразборчивом ответе возвращается
    /// `None` — вызывающая сторона считает ход на эвристике целиком.
    async fn judge(
        &self,
        scenario: &Scenario,
        history: &[SessionMessage],
        player_text: &str,
        user_id: &str,
    ) -> Option<JudgeScores> {
        if !self.judge_enabled() {
            return None;
        }
        // Квота: судья — тоже расход токенов, при исчерпании молча на эвристику.
        if let Err(err) = self.ensure_llm_quota(user_id, self.llm_daily_token_limit().ok()?) {
            tracing::warn!(error = %err, "судья: квота исчерпана, считаем на эвристике");
            return None;
        }
        let (chat, model_key) = match self.providers.resolve_chat_for(user_id).await {
            Ok(resolved) => resolved,
            Err(err) => {
                tracing::warn!(error = %err, "судья: LLM не настроена, считаем на эвристике");
                return None;
            }
        };

        let request = ChatRequest::new(
            model_key,
            vec![
                ChatMessage::system(judge::SYSTEM_PROMPT),
                ChatMessage::user(judge::build_prompt(scenario, history, player_text)),
            ],
        )
        .with_temperature(0.0)
        .with_max_tokens(120);

        let response = match tokio::time::timeout(JUDGE_TIMEOUT, chat.chat(request)).await {
            Ok(Ok(response)) => response,
            Ok(Err(err)) => {
                tracing::warn!(error = %err, "судья: ошибка запроса, считаем на эвристике");
                return None;
            }
            Err(_) => {
                tracing::warn!(
                    timeout = JUDGE_TIMEOUT.as_secs(),
                    "судья: таймаут, считаем на эвристике"
                );
                return None;
            }
        };

        let tokens = response
            .usage
            .total_tokens
            .map(u64::from)
            .unwrap_or_else(|| estimate_tokens(&response.content));
        self.record_llm_usage(user_id, tokens);

        let scores = judge::parse(&response.content);
        if scores.is_none() {
            tracing::warn!(
                content = %response.content,
                "судья: не удалось разобрать ответ, считаем на эвристике"
            );
        }
        scores
    }

    /// LLM-судья включён: `scoring.llm_judge_enabled` (по умолчанию `true`).
    fn judge_enabled(&self) -> bool {
        let value = self
            .settings
            .get_global(global_keys::LLM_JUDGE_ENABLED)
            .unwrap_or_else(|_| "true".to_string());
        !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "" | "0" | "false" | "off" | "no"
        )
    }

    /// Доля LLM-оценки в баллах хода: `scoring.llm_judge_weight` (0..=1),
    /// иначе [`judge::DEFAULT_WEIGHT`] (0.4).
    fn judge_weight(&self) -> f32 {
        self.settings
            .get_global(global_keys::LLM_JUDGE_WEIGHT)
            .ok()
            .and_then(|v| v.trim().parse::<f32>().ok())
            .filter(|w| (0.0..=1.0).contains(w))
            .unwrap_or(judge::DEFAULT_WEIGHT)
    }

    fn load(&self, session_id: &str) -> AppResult<Session> {
        self.repos
            .sessions
            .get(session_id)?
            .ok_or_else(|| AppError::NotFound("сессия не найдена".into()))
    }

    /// Текущая ветка сессии; отсутствие — нарушение инварианта
    /// (ветка создаётся вместе с сессией, бэкфилл — миграцией 0011).
    fn current_branch(&self, session_id: &str) -> AppResult<SessionBranch> {
        self.repos
            .sessions
            .current_branch(session_id)?
            .ok_or_else(|| AppError::internal(format!("у сессии {session_id} нет текущей ветки")))
    }

    /// Ветка, принадлежащая конкретной сессии; иначе 404.
    fn load_branch_for(&self, session: &Session, branch_id: &str) -> AppResult<SessionBranch> {
        self.repos
            .sessions
            .branch(branch_id)?
            .filter(|b| b.session_id == session.id)
            .ok_or_else(|| AppError::NotFound("ветка не найдена".into()))
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
    p.push_str(&format!(
        "Твоя лучшая альтернатива (BATNA): {}.\n",
        scenario.partner_batna
    ));
    p.push_str(&format!(
        "Цель игрока (не называй её вслух, но учитывай): {}.\n",
        scenario.player_goal
    ));
    p.push_str(&format!(
        "Лучшая альтернатива игрока (BATNA): {}\n",
        scenario.player_batna
    ));
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

        let msgs = svc
            .sessions
            .messages(&user, &started.session.id, None)
            .unwrap();
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

        let msgs = svc
            .sessions
            .messages(&user, &started.session.id, None)
            .unwrap();
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

    // ── Ветвление диалога ──

    /// Добавляет partner-реплику в текущую ветку (тесты идут без LLM).
    fn append_partner(
        svc: &crate::application::Services,
        session_id: &str,
        turn_index: u32,
        content: &str,
    ) -> String {
        let branch = svc
            .repos
            .sessions
            .current_branch(session_id)
            .unwrap()
            .unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        svc.repos
            .sessions
            .append_message(&SessionMessage {
                id: id.clone(),
                session_id: session_id.to_string(),
                branch_id: Some(branch.id),
                turn_index,
                role: MessageRole::Partner,
                content: content.to_string(),
                strategy: None,
                score_delta: 0,
                created_at: chrono::Utc::now().to_rfc3339(),
            })
            .unwrap();
        id
    }

    const POSITIVE_REPLY: &str = "Какие условия поставки для вас оптимальны? \
        Давайте найдём решение, выгодное для обеих сторон. \
        По данным рынка скидка 10% обоснована.";

    #[test]
    fn fork_copies_prefix_and_becomes_current_branch() {
        let (_db, svc) = setup().unwrap();
        let sc_id = seed_scenario(&svc);
        let user = user_ctx(&svc);
        let started = svc
            .sessions
            .start(&user, &sc_id, SessionMode::Text)
            .unwrap();
        let sid = started.session.id;

        svc.sessions
            .record_player_turn(&user, &sid, POSITIVE_REPLY)
            .unwrap();
        let partner_id = append_partner(&svc, &sid, 2, "Обсудим условия.");

        let created = svc
            .sessions
            .create_branch(&user, &sid, &partner_id)
            .unwrap();

        // Новая ветка: форк от main (id main = id сессии), стала текущей.
        assert_eq!(created.branch.label, "fork");
        assert!(created.branch.is_current);
        assert_eq!(created.branch.parent_id.as_deref(), Some(sid.as_str()));
        assert_eq!(created.branch.fork_turn_index, 2);

        // Префикс скопирован: opening + player + partner.
        let msgs = svc.sessions.messages(&user, &sid, None).unwrap();
        assert_eq!(msgs.len(), 3, "префикс из 3 реплик");
        assert!(msgs
            .iter()
            .all(|m| m.branch_id.as_deref() == Some(created.branch.id.as_str())));

        // Дерево: main + fork, current — форк.
        let branches = svc.sessions.branches(&user, &sid).unwrap();
        assert_eq!(branches.len(), 2);
        assert!(!branches[0].is_current, "main больше не текущая");
        let current = svc.repos.sessions.current_branch(&sid).unwrap().unwrap();
        assert_eq!(current.id, created.branch.id);

        // Сессия откатилась к точке ветвления: 1 ход.
        assert_eq!(created.session.turn_count, 1);
    }

    #[test]
    fn fork_replays_metrics_to_point_and_switch_restores() {
        let (_db, svc) = setup().unwrap();
        let sc_id = seed_scenario(&svc);
        let user = user_ctx(&svc);
        let sid = svc
            .sessions
            .start(&user, &sc_id, SessionMode::Text)
            .unwrap()
            .session
            .id;

        // Ход 1 → partner1 → ход 2: в main 2 хода, счёт растёт.
        svc.sessions
            .record_player_turn(&user, &sid, POSITIVE_REPLY)
            .unwrap();
        let partner1 = append_partner(&svc, &sid, 2, "Обсудим условия.");
        svc.sessions
            .record_player_turn(&user, &sid, POSITIVE_REPLY)
            .unwrap();
        let score_two_turns = svc.sessions.get(&user, &sid).unwrap().total_score;
        assert_eq!(svc.sessions.get(&user, &sid).unwrap().turn_count, 2);
        assert!(score_two_turns > 0);

        // Форк после partner1 (1 ход) — метрики пересчитаны в этой точке.
        let created = svc.sessions.create_branch(&user, &sid, &partner1).unwrap();
        assert_eq!(created.session.turn_count, 1, "откат к 1 ходу");
        assert_eq!(created.session.total_score, created.branch.total_score);
        assert!(
            created.session.total_score < score_two_turns,
            "счёт откатился: {} < {score_two_turns}",
            created.session.total_score
        );

        // Main сохранила своё состояние (2 хода).
        let branches = svc.sessions.branches(&user, &sid).unwrap();
        let main = &branches[0];
        assert_eq!(main.turn_count, 2);
        assert_eq!(main.total_score, score_two_turns);

        // Переключение обратно восстанавливает снапшот main.
        let (restored, _) = svc.sessions.switch_branch(&user, &sid, &main.id).unwrap();
        assert_eq!(restored.turn_count, 2);
        assert_eq!(restored.total_score, score_two_turns);
        // Ходы после переключения идут в main (4 реплики нетронуты).
        assert_eq!(svc.sessions.messages(&user, &sid, None).unwrap().len(), 4);
    }

    #[test]
    fn fork_from_player_message_is_rejected() {
        let (_db, svc) = setup().unwrap();
        let sc_id = seed_scenario(&svc);
        let user = user_ctx(&svc);
        let sid = svc
            .sessions
            .start(&user, &sc_id, SessionMode::Text)
            .unwrap()
            .session
            .id;
        svc.sessions
            .record_player_turn(&user, &sid, POSITIVE_REPLY)
            .unwrap();

        let player_msg = svc
            .sessions
            .messages(&user, &sid, None)
            .unwrap()
            .into_iter()
            .find(|m| m.role == MessageRole::Player)
            .unwrap();
        let err = svc
            .sessions
            .create_branch(&user, &sid, &player_msg.id)
            .unwrap_err();
        assert!(
            matches!(err, AppError::BadRequest(_)),
            "форк от реплики игрока должен отклоняться: {err}"
        );
    }

    #[test]
    fn fork_restricted_to_active_session_and_owner() {
        let (_db, svc) = setup().unwrap();
        let sc_id = seed_scenario(&svc);
        let user = user_ctx(&svc);
        let sid = svc
            .sessions
            .start(&user, &sc_id, SessionMode::Text)
            .unwrap()
            .session
            .id;
        let opening_id = svc.sessions.messages(&user, &sid, None).unwrap()[0]
            .id
            .clone();

        // Чужой пользователь — не может ветвить.
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
        let err = svc
            .sessions
            .create_branch(&stranger_ctx, &sid, &opening_id)
            .unwrap_err();
        assert!(matches!(err, AppError::Forbidden(_)), "{err}");

        // Админ читает чужие сессии, но не ветвит (strict owner).
        let admin = ctx("admin-1", UserRole::Admin);
        assert!(svc.sessions.branches(&admin, &sid).is_ok());
        let err = svc
            .sessions
            .create_branch(&admin, &sid, &opening_id)
            .unwrap_err();
        assert!(matches!(err, AppError::Forbidden(_)), "{err}");

        // Завершённая сессия — ветвление и переключение запрещены.
        svc.sessions.finish(&user, &sid).unwrap();
        let err = svc
            .sessions
            .create_branch(&user, &sid, &opening_id)
            .unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)), "{err}");

        let branches = svc.sessions.branches(&user, &sid).unwrap();
        let err = svc
            .sessions
            .switch_branch(&user, &sid, &branches[0].id)
            .unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)), "{err}");
    }

    #[test]
    fn fork_unknown_message_or_branch_is_not_found() {
        let (_db, svc) = setup().unwrap();
        let sc_id = seed_scenario(&svc);
        let user = user_ctx(&svc);
        let sid = svc
            .sessions
            .start(&user, &sc_id, SessionMode::Text)
            .unwrap()
            .session
            .id;

        let err = svc
            .sessions
            .create_branch(&user, &sid, "no-such-message")
            .unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)), "{err}");

        // Реплика чужой сессии не подходит (filter по session_id).
        let other = svc
            .sessions
            .start(&user, &sc_id, SessionMode::Text)
            .unwrap()
            .session;
        let other_opening = svc.sessions.messages(&user, &other.id, None).unwrap()[0]
            .id
            .clone();
        let err = svc
            .sessions
            .create_branch(&user, &sid, &other_opening)
            .unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)), "{err}");

        // Читаем несуществующую ветку → 404.
        let err = svc
            .sessions
            .messages(&user, &sid, Some("no-such-branch"))
            .unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)), "{err}");
    }

    #[test]
    fn fork_limit_is_enforced() {
        let (_db, svc) = setup().unwrap();
        let sc_id = seed_scenario(&svc);
        let user = user_ctx(&svc);
        let sid = svc
            .sessions
            .start(&user, &sc_id, SessionMode::Text)
            .unwrap()
            .session
            .id;
        let opening_id = svc.sessions.messages(&user, &sid, None).unwrap()[0]
            .id
            .clone();

        // 1 main + (MAX_BRANCHES - 1) форков от opening.
        for i in 0..(MAX_BRANCHES - 1) {
            let created = svc
                .sessions
                .create_branch(&user, &sid, &opening_id)
                .unwrap_or_else(|e| panic!("форк #{i} должен пройти: {e}"));
            assert!(created.branch.is_current);
        }
        assert_eq!(
            svc.sessions.branches(&user, &sid).unwrap().len() as u64,
            MAX_BRANCHES
        );

        // Следующий форк — отказ по лимиту.
        let err = svc
            .sessions
            .create_branch(&user, &sid, &opening_id)
            .unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)), "{err}");
    }

    #[test]
    fn messages_by_branch_are_isolated() {
        let (_db, svc) = setup().unwrap();
        let sc_id = seed_scenario(&svc);
        let user = user_ctx(&svc);
        let sid = svc
            .sessions
            .start(&user, &sc_id, SessionMode::Text)
            .unwrap()
            .session
            .id;
        let main_id = sid.clone();
        let opening_id = svc.sessions.messages(&user, &sid, None).unwrap()[0]
            .id
            .clone();

        svc.sessions
            .record_player_turn(&user, &sid, POSITIVE_REPLY)
            .unwrap();
        let created = svc
            .sessions
            .create_branch(&user, &sid, &opening_id)
            .unwrap();

        // Текущая (fork): opening-копия, 1 реплика.
        let fork_msgs = svc.sessions.messages(&user, &sid, None).unwrap();
        assert_eq!(fork_msgs.len(), 1);
        assert_ne!(fork_msgs[0].id, opening_id, "копия с новым id");

        // main осталась с двумя репликами.
        let main_msgs = svc.sessions.messages(&user, &sid, Some(&main_id)).unwrap();
        assert_eq!(main_msgs.len(), 2);
        assert_eq!(main_msgs[0].id, opening_id);

        // Явный запрос ветки форка — тот же набор.
        let same = svc
            .sessions
            .messages(&user, &sid, Some(&created.branch.id))
            .unwrap();
        assert_eq!(same.len(), 1);
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
