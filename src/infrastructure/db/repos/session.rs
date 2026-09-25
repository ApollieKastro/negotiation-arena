//! Репозиторий сессий и реплик диалога (SQLite).

use std::sync::Arc;

use rusqlite::{params, OptionalExtension, Row};

use crate::domain::entities::session::{
    MessageRole, Session, SessionBranch, SessionMessage, SessionMode, SessionStatus,
};
use crate::domain::ports::{
    LeaderboardRow, PlatformSessionsAggregate, SessionRepository, UserSessionsAggregate,
};
use crate::error::AppResult;
use crate::infrastructure::db::Database;

pub struct SqliteSessionRepo {
    db: Arc<Database>,
}

impl SqliteSessionRepo {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    fn map_row(row: &Row<'_>) -> rusqlite::Result<Session> {
        let mode: String = row.get(3)?;
        let status: String = row.get(4)?;
        let metrics_raw: String = row.get(7)?;
        let turn_count: i64 = row.get(6)?;

        let metrics = match serde_json::from_str(&metrics_raw) {
            Ok(m) => m,
            Err(err) => {
                // Молчаливая деградация до default теряет счёт — хотя бы логируем.
                tracing::warn!(
                    session_id = %row.get::<_, String>(0).unwrap_or_default(),
                    error = %err,
                    "не удалось разобрать sessions.metrics, используем default"
                );
                Default::default()
            }
        };

        Ok(Session {
            id: row.get(0)?,
            user_id: row.get(1)?,
            scenario_id: row.get(2)?,
            mode: SessionMode::from_slug(&mode),
            status: SessionStatus::from_slug(&status),
            total_score: row.get(5)?,
            turn_count: turn_count.max(0) as u32,
            metrics,
            ending_id: row.get(8)?,
            ending_title: row.get(9)?,
            feedback: row.get(10)?,
            created_at: row.get(11)?,
            finished_at: row.get(12)?,
        })
    }

    fn map_message(row: &Row<'_>) -> rusqlite::Result<SessionMessage> {
        let role: String = row.get(4)?;
        let turn_index: i64 = row.get(3)?;
        Ok(SessionMessage {
            id: row.get(0)?,
            session_id: row.get(1)?,
            branch_id: row.get(2)?,
            turn_index: turn_index.max(0) as u32,
            role: MessageRole::from_slug(&role),
            content: row.get(5)?,
            strategy: row.get(6)?,
            score_delta: row.get(7)?,
            created_at: row.get(8)?,
        })
    }

    fn map_branch(row: &Row<'_>) -> rusqlite::Result<SessionBranch> {
        let metrics_raw: String = row.get(5)?;
        let fork_turn_index: i64 = row.get(4)?;
        let turn_count: i64 = row.get(7)?;
        let metrics = match serde_json::from_str(&metrics_raw) {
            Ok(m) => m,
            Err(err) => {
                tracing::warn!(
                    branch_id = %row.get::<_, String>(0).unwrap_or_default(),
                    error = %err,
                    "не удалось разобрать session_branches.metrics, используем default"
                );
                Default::default()
            }
        };
        Ok(SessionBranch {
            id: row.get(0)?,
            session_id: row.get(1)?,
            parent_id: row.get(2)?,
            label: row.get(3)?,
            fork_turn_index: fork_turn_index.max(0) as u32,
            metrics,
            total_score: row.get(6)?,
            turn_count: turn_count.max(0) as u32,
            is_current: row.get::<_, i64>(8)? != 0,
            created_at: row.get(9)?,
        })
    }
}

const SELECT: &str = "SELECT id, user_id, scenario_id, mode, status, total_score, \
    turn_count, metrics, ending_id, ending_title, feedback, created_at, finished_at \
    FROM sessions";

const SELECT_MESSAGE: &str =
    "SELECT id, session_id, branch_id, turn_index, role, content, strategy, score_delta, created_at \
     FROM session_messages";

const SELECT_BRANCH: &str = "SELECT id, session_id, parent_id, label, fork_turn_index, \
    metrics, total_score, turn_count, is_current, created_at \
    FROM session_branches";

/// Вставляет одну строку `session_messages` (используется внутри транзакций).
fn insert_message(
    conn: &rusqlite::Connection,
    message: &SessionMessage,
) -> rusqlite::Result<usize> {
    conn.execute(
        "INSERT INTO session_messages (id, session_id, branch_id, turn_index, role, content, strategy, score_delta, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            message.id,
            message.session_id,
            message.branch_id,
            message.turn_index as i64,
            message.role.slug(),
            message.content,
            message.strategy,
            message.score_delta,
            message.created_at,
        ],
    )
}

/// main-ветка сессии: `id` ветки равен `id` сессии.
fn main_branch(session: &Session) -> SessionBranch {
    SessionBranch {
        id: session.id.clone(),
        session_id: session.id.clone(),
        parent_id: None,
        label: "main".into(),
        fork_turn_index: 0,
        metrics: session.metrics.clone(),
        total_score: session.total_score,
        turn_count: session.turn_count,
        is_current: true,
        created_at: session.created_at.clone(),
    }
}

/// Вставляет строку `session_branches` (используется внутри транзакций).
fn insert_branch(conn: &rusqlite::Connection, branch: &SessionBranch) -> rusqlite::Result<usize> {
    let metrics = serde_json::to_string(&branch.metrics)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    conn.execute(
        "INSERT INTO session_branches (id, session_id, parent_id, label, fork_turn_index,
            metrics, total_score, turn_count, is_current, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            branch.id,
            branch.session_id,
            branch.parent_id,
            branch.label,
            branch.fork_turn_index as i64,
            metrics,
            branch.total_score,
            branch.turn_count as i64,
            branch.is_current as i64,
            branch.created_at,
        ],
    )
}

/// Обновляет снапшот метрик ветки (вызывается внутри транзакции хода).
fn update_branch_state(
    conn: &rusqlite::Connection,
    branch: &SessionBranch,
) -> rusqlite::Result<usize> {
    let metrics = serde_json::to_string(&branch.metrics)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    conn.execute(
        "UPDATE session_branches SET metrics = ?1, total_score = ?2, turn_count = ?3
         WHERE id = ?4",
        params![
            metrics,
            branch.total_score,
            branch.turn_count as i64,
            branch.id
        ],
    )
}

/// Вставляет строку `sessions` (используется внутри транзакций).
fn insert_session(conn: &rusqlite::Connection, session: &Session) -> rusqlite::Result<usize> {
    let metrics = serde_json::to_string(&session.metrics)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    conn.execute(
        "INSERT INTO sessions (id, user_id, scenario_id, mode, status, total_score,
            turn_count, metrics, ending_id, ending_title, feedback, created_at, finished_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            session.id,
            session.user_id,
            session.scenario_id,
            session.mode.slug(),
            session.status.slug(),
            session.total_score,
            session.turn_count as i64,
            metrics,
            session.ending_id,
            session.ending_title,
            session.feedback,
            session.created_at,
            session.finished_at,
        ],
    )
}

/// Обновляет строку `sessions` (используется внутри транзакций).
fn update_session(conn: &rusqlite::Connection, session: &Session) -> rusqlite::Result<usize> {
    let metrics = serde_json::to_string(&session.metrics)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    conn.execute(
        "UPDATE sessions SET status = ?1, total_score = ?2, turn_count = ?3,
            metrics = ?4, ending_id = ?5, ending_title = ?6, feedback = ?7, finished_at = ?8
         WHERE id = ?9",
        params![
            session.status.slug(),
            session.total_score,
            session.turn_count as i64,
            metrics,
            session.ending_id,
            session.ending_title,
            session.feedback,
            session.finished_at,
            session.id,
        ],
    )
}

impl SessionRepository for SqliteSessionRepo {
    fn create(&self, session: &Session) -> AppResult<()> {
        let conn = self.db.conn();
        let tx = conn.unchecked_transaction()?;
        insert_session(&tx, session)?;
        insert_branch(&tx, &main_branch(session))?;
        tx.commit()?;
        Ok(())
    }

    fn create_with_opening(&self, session: &Session, opening: &SessionMessage) -> AppResult<()> {
        let conn = self.db.conn();
        let tx = conn.unchecked_transaction()?;
        insert_session(&tx, session)?;
        let branch = main_branch(session);
        insert_branch(&tx, &branch)?;
        // Opening всегда принадлежит main-ветке сессии.
        let mut opening = opening.clone();
        opening.branch_id = Some(branch.id.clone());
        insert_message(&tx, &opening)?;
        tx.commit()?;
        Ok(())
    }

    fn get(&self, id: &str) -> AppResult<Option<Session>> {
        let conn = self.db.conn();
        let sql = format!("{SELECT} WHERE id = ?1");
        conn.query_row(&sql, params![id], Self::map_row)
            .optional()
            .map_err(Into::into)
    }

    fn list_by_user(
        &self,
        user_id: &str,
        limit: u32,
        offset: u32,
    ) -> AppResult<(Vec<Session>, u64)> {
        let conn = self.db.conn();
        let total: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE user_id = ?1",
            params![user_id],
            |r| r.get(0),
        )?;
        let sql =
            format!("{SELECT} WHERE user_id = ?1 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![user_id, limit as i64, offset as i64], Self::map_row)?;
        let items = rows.collect::<Result<Vec<_>, _>>()?;
        Ok((items, total.max(0) as u64))
    }

    fn update(&self, session: &Session) -> AppResult<()> {
        let conn = self.db.conn();
        update_session(&conn, session)?;
        Ok(())
    }

    fn commit_turn(
        &self,
        session: &Session,
        branch: &SessionBranch,
        player: &SessionMessage,
        partner: Option<&SessionMessage>,
    ) -> AppResult<()> {
        let conn = self.db.conn();
        let tx = conn.unchecked_transaction()?;
        update_session(&tx, session)?;
        // Снапшот ветки должен отражать те же метрики, что и сессия.
        update_branch_state(&tx, branch)?;
        insert_message(&tx, player)?;
        if let Some(p) = partner {
            insert_message(&tx, p)?;
        }
        tx.commit()?;
        Ok(())
    }

    fn append_message(&self, message: &SessionMessage) -> AppResult<()> {
        let conn = self.db.conn();
        insert_message(&conn, message)?;
        Ok(())
    }

    fn messages(&self, session_id: &str) -> AppResult<Vec<SessionMessage>> {
        let conn = self.db.conn();
        let sql = format!(
            "{SELECT_MESSAGE} WHERE branch_id = (
                 SELECT id FROM session_branches
                 WHERE session_id = ?1 AND is_current = 1
             ) ORDER BY turn_index ASC, created_at ASC"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![session_id], Self::map_message)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn messages_in_branch(
        &self,
        session_id: &str,
        branch_id: &str,
    ) -> AppResult<Vec<SessionMessage>> {
        let conn = self.db.conn();
        let sql = format!(
            "{SELECT_MESSAGE} WHERE session_id = ?1 AND branch_id = ?2 \
             ORDER BY turn_index ASC, created_at ASC"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![session_id, branch_id], Self::map_message)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn message(&self, id: &str) -> AppResult<Option<SessionMessage>> {
        let conn = self.db.conn();
        let sql = format!("{SELECT_MESSAGE} WHERE id = ?1");
        conn.query_row(&sql, params![id], Self::map_message)
            .optional()
            .map_err(Into::into)
    }

    fn branches(&self, session_id: &str) -> AppResult<Vec<SessionBranch>> {
        let conn = self.db.conn();
        let sql = format!("{SELECT_BRANCH} WHERE session_id = ?1 ORDER BY created_at ASC, id ASC");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![session_id], Self::map_branch)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn branch(&self, id: &str) -> AppResult<Option<SessionBranch>> {
        let conn = self.db.conn();
        let sql = format!("{SELECT_BRANCH} WHERE id = ?1");
        conn.query_row(&sql, params![id], Self::map_branch)
            .optional()
            .map_err(Into::into)
    }

    fn current_branch(&self, session_id: &str) -> AppResult<Option<SessionBranch>> {
        let conn = self.db.conn();
        let sql = format!("{SELECT_BRANCH} WHERE session_id = ?1 AND is_current = 1");
        conn.query_row(&sql, params![session_id], Self::map_branch)
            .optional()
            .map_err(Into::into)
    }

    fn count_branches(&self, session_id: &str) -> AppResult<u64> {
        let conn = self.db.conn();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM session_branches WHERE session_id = ?1",
            params![session_id],
            |r| r.get(0),
        )?;
        Ok(count.max(0) as u64)
    }

    fn fork_branch(
        &self,
        branch: &SessionBranch,
        copies: &[SessionMessage],
        session: &Session,
    ) -> AppResult<()> {
        let conn = self.db.conn();
        let tx = conn.unchecked_transaction()?;
        // Сначала снимаем current у всех веток сессии (уникальный индекс
        // разрешает ровно одну is_current = 1).
        tx.execute(
            "UPDATE session_branches SET is_current = 0 WHERE session_id = ?1",
            params![branch.session_id],
        )?;
        insert_branch(&tx, branch)?;
        for copy in copies {
            insert_message(&tx, copy)?;
        }
        // Сессия откатывается к состоянию точки ветвления.
        update_session(&tx, session)?;
        tx.commit()?;
        Ok(())
    }

    fn switch_branch(&self, branch_id: &str, session: &Session) -> AppResult<()> {
        let conn = self.db.conn();
        let tx = conn.unchecked_transaction()?;
        let cleared = tx.execute(
            "UPDATE session_branches SET is_current = 0 WHERE session_id = ?1",
            params![session.id],
        )?;
        if cleared == 0 {
            return Err(rusqlite::Error::QueryReturnedNoRows.into());
        }
        let flipped = tx.execute(
            "UPDATE session_branches SET is_current = 1 WHERE id = ?1 AND session_id = ?2",
            params![branch_id, session.id],
        )?;
        if flipped != 1 {
            return Err(rusqlite::Error::QueryReturnedNoRows.into());
        }
        update_session(&tx, session)?;
        tx.commit()?;
        Ok(())
    }

    fn user_stats_aggregate(&self, user_id: &str) -> AppResult<UserSessionsAggregate> {
        let conn = self.db.conn();
        let row = conn.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN status = 'finished' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN status = 'active' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN status = 'abandoned' THEN 1 ELSE 0 END), 0),
                    COALESCE(MAX(CASE WHEN status = 'finished' THEN total_score END), 0),
                    COALESCE(AVG(CASE WHEN status = 'finished' THEN total_score END), 0),
                    MAX(created_at),
                    COALESCE(SUM(CASE WHEN status = 'finished' AND total_score > 0 THEN total_score ELSE 0 END), 0)
             FROM sessions WHERE user_id = ?1",
            params![user_id],
            |r| {
                let total: i64 = r.get(0)?;
                let finished: i64 = r.get(1)?;
                let active: i64 = r.get(2)?;
                let abandoned: i64 = r.get(3)?;
                let best: i64 = r.get(4)?;
                let avg: f64 = r.get(5)?;
                let last: Option<String> = r.get(6)?;
                let xp: i64 = r.get(7)?;
                Ok((total, finished, active, abandoned, best, avg, last, xp))
            },
        )?;
        let (total, finished, active, abandoned, best, avg, last, xp) = row;
        Ok(UserSessionsAggregate {
            total: total.max(0) as u32,
            finished: finished.max(0) as u32,
            active: active.max(0) as u32,
            abandoned: abandoned.max(0) as u32,
            best_score: best as i32,
            avg_score: avg as i32,
            last_session_at: last,
            xp: xp.max(0),
        })
    }

    fn leaderboard_rows(&self) -> AppResult<Vec<LeaderboardRow>> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare(
            "SELECT s.user_id, u.login, u.display_name, u.is_active,
                    COUNT(*) AS finished,
                    MAX(s.total_score) AS best_score,
                    COALESCE(AVG(s.total_score), 0) AS avg_score,
                    COALESCE(SUM(CASE WHEN s.total_score > 0 THEN s.total_score ELSE 0 END), 0) AS xp
             FROM sessions s
             JOIN users u ON u.id = s.user_id
             WHERE s.status = 'finished' AND u.is_active = 1
             GROUP BY s.user_id, u.login, u.display_name, u.is_active",
        )?;
        let rows = stmt.query_map([], |r| {
            // Колонки: 0=user_id, 1=login, 2=display_name, 3=is_active,
            // 4=finished, 5=best_score, 6=avg_score, 7=xp.
            let finished: i64 = r.get(4)?;
            let best: i64 = r.get(5)?;
            let avg: f64 = r.get(6)?;
            let xp: i64 = r.get(7)?;
            Ok(LeaderboardRow {
                user_id: r.get(0)?,
                login: r.get(1)?,
                display_name: r.get(2)?,
                is_active: r.get::<_, i64>(3)? != 0,
                finished: finished.max(0) as u32,
                best_score: best as i32,
                avg_score: avg as i32,
                xp: xp.max(0),
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn platform_sessions_aggregate(&self) -> AppResult<PlatformSessionsAggregate> {
        let conn = self.db.conn();
        let row = conn.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN status = 'finished' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN status = 'active' THEN 1 ELSE 0 END), 0),
                    COALESCE(AVG(CASE WHEN status = 'finished' THEN total_score END), 0),
                    COALESCE(MAX(CASE WHEN status = 'finished' THEN total_score END), 0)
             FROM sessions",
            [],
            |r| {
                let total: i64 = r.get(0)?;
                let finished: i64 = r.get(1)?;
                let active: i64 = r.get(2)?;
                let avg: f64 = r.get(3)?;
                let best: i64 = r.get(4)?;
                Ok((total, finished, active, avg, best))
            },
        )?;
        let (total, finished, active, avg, best) = row;
        Ok(PlatformSessionsAggregate {
            total: total.max(0) as u64,
            finished: finished.max(0) as u64,
            active: active.max(0) as u64,
            avg_finished_score: avg as i32,
            best_score: best as i32,
        })
    }

    fn activity_by_day(&self, since: &str) -> AppResult<Vec<(String, u32)>> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare(
            "SELECT substr(created_at, 1, 10) AS day, COUNT(*)
             FROM sessions WHERE created_at >= ?1
             GROUP BY day ORDER BY day ASC",
        )?;
        let rows = stmt.query_map(params![since], |r| {
            let day: String = r.get(0)?;
            let n: i64 = r.get(1)?;
            Ok((day, n.max(0) as u32))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn count_by_scenario(&self, scenario_id: &str) -> AppResult<u64> {
        let conn = self.db.conn();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE scenario_id = ?1",
            params![scenario_id],
            |r| r.get(0),
        )?;
        Ok(count.max(0) as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entities::scenario::Scenario;
    use crate::domain::entities::session::SessionMetrics;
    use crate::domain::entities::user::UserRole;
    use crate::domain::ports::{ScenarioRepository, SessionRepository, UserRepository};
    use crate::infrastructure::db::repos::SqliteRepos;
    use crate::infrastructure::db::Database;
    use std::sync::Arc;

    fn make_session(id: &str, user_id: &str) -> Session {
        Session {
            id: id.to_string(),
            user_id: user_id.to_string(),
            scenario_id: "sc-1".to_string(),
            mode: SessionMode::Text,
            status: SessionStatus::Active,
            total_score: 0,
            turn_count: 0,
            metrics: SessionMetrics::default(),
            ending_id: None,
            ending_title: None,
            feedback: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            finished_at: None,
        }
    }

    /// Создаёт родительские записи и возвращает id созданного пользователя.
    fn seed_parents(db: &Arc<Database>) -> String {
        let repos = SqliteRepos::new(db.clone());
        let user = repos
            .users
            .create("tester", "hash", UserRole::User, None)
            .unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        let scenario = Scenario {
            id: "sc-1".to_string(),
            title: "Тест".to_string(),
            description: "Тест".to_string(),
            sphere: "Тест".to_string(),
            difficulty: crate::domain::entities::scenario::Difficulty::Easy,
            player_role: "Игрок".to_string(),
            player_company: None,
            player_goal: "Цель".to_string(),
            player_batna: "BATNA".to_string(),
            partner_name: "Партнёр".to_string(),
            partner_role: "Роль".to_string(),
            partner_company: None,
            partner_goal: "Цель оппонента".to_string(),
            partner_goals: vec![],
            partner_batna: "BATNA оппонента".to_string(),
            partner_personality: crate::domain::entities::scenario::PartnerPersonality::default(),
            opening_context: "Привет".to_string(),
            endings: vec![],
            ai_generated: false,
            is_active: true,
            created_by: None,
            created_at: now,
            updated_at: None,
        };
        repos.scenarios.upsert(&scenario).unwrap();
        user.id
    }

    #[test]
    fn session_roundtrip_with_messages() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        db.run_migrations().unwrap();
        let repo = SqliteSessionRepo::new(db.clone());
        let user_id = seed_parents(&db);

        let session = make_session("s1", &user_id);
        repo.create(&session).unwrap();

        let loaded = repo.get("s1").unwrap().expect("сессия должна найтись");
        assert_eq!(loaded.id, "s1");
        assert_eq!(loaded.status, SessionStatus::Active);

        repo.append_message(&SessionMessage {
            id: "m1".into(),
            session_id: "s1".into(),
            // create() создаёт main-ветку с id = id сессии.
            branch_id: Some("s1".into()),
            turn_index: 0,
            role: MessageRole::Partner,
            content: "Здравствуйте!".into(),
            strategy: None,
            score_delta: 0,
            created_at: chrono::Utc::now().to_rfc3339(),
        })
        .unwrap();

        let messages = repo.messages("s1").unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, MessageRole::Partner);
    }

    fn make_message(session_id: &str, turn: u32, role: MessageRole) -> SessionMessage {
        SessionMessage {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            // main-ветка сессии (id ветки = id сессии).
            branch_id: Some(session_id.to_string()),
            turn_index: turn,
            role,
            content: "реплика".into(),
            strategy: Some("collaboration".into()),
            score_delta: 1,
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    /// Сессия + opening — одна транзакция: обе записи либо обе, либо ни одной.
    #[test]
    fn create_with_opening_is_atomic() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        db.run_migrations().unwrap();
        let repo = SqliteSessionRepo::new(db.clone());
        let user_id = seed_parents(&db);

        let session = make_session("tx1", &user_id);
        let opening = make_message("tx1", 0, MessageRole::Partner);
        repo.create_with_opening(&session, &opening).unwrap();

        assert!(repo.get("tx1").unwrap().is_some());
        assert_eq!(repo.messages("tx1").unwrap().len(), 1);
    }

    /// Ход: UPDATE сессии + сообщения одной транзакцией.
    #[test]
    fn commit_turn_writes_session_and_messages_atomically() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        db.run_migrations().unwrap();
        let repo = SqliteSessionRepo::new(db.clone());
        let user_id = seed_parents(&db);

        let mut session = make_session("tx2", &user_id);
        repo.create(&session).unwrap();
        let player = make_message("tx2", 1, MessageRole::Player);
        let partner = make_message("tx2", 1, MessageRole::Partner);
        session.turn_count = 1;
        session.total_score = 5;
        // Ход обновляет и снапшот текущей (main) ветки.
        let mut branch = main_branch(&session);
        branch.metrics = session.metrics.clone();
        branch.total_score = session.total_score;
        branch.turn_count = session.turn_count;

        repo.commit_turn(&session, &branch, &player, Some(&partner))
            .unwrap();

        let loaded = repo.get("tx2").unwrap().expect("session");
        assert_eq!(loaded.turn_count, 1);
        assert_eq!(loaded.total_score, 5);
        assert_eq!(repo.messages("tx2").unwrap().len(), 2);
        let branch_loaded = repo.current_branch("tx2").unwrap().expect("ветка");
        assert_eq!(branch_loaded.total_score, 5);
        assert_eq!(branch_loaded.turn_count, 1);
    }

    /// LIMIT/OFFSET + общий счётчик: total не зависит от окна.
    #[test]
    fn list_by_user_respects_offset_and_returns_total() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        db.run_migrations().unwrap();
        let repo = SqliteSessionRepo::new(db.clone());
        let user_id = seed_parents(&db);

        for i in 0..5 {
            let mut s = make_session(&format!("p{i}"), &user_id);
            // Убываем по created_at (RFC3339-строки лексикографически):
            // p0 = самый новый → первым в ORDER BY DESC.
            s.created_at = format!("2026-01-0{}T00:00:00+00:00", 5 - i);
            repo.create(&s).unwrap();
        }

        let (page1, total) = repo.list_by_user(&user_id, 2, 0).unwrap();
        assert_eq!(total, 5);
        assert_eq!(page1.len(), 2);
        assert_eq!(page1[0].id, "p0"); // newest first

        let (page3, total3) = repo.list_by_user(&user_id, 2, 4).unwrap();
        assert_eq!(total3, 5);
        assert_eq!(page3.len(), 1);
        assert_eq!(page3[0].id, "p4");

        let (empty, _) = repo.list_by_user(&user_id, 2, 99).unwrap();
        assert!(empty.is_empty());
    }
}
