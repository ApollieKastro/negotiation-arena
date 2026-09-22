//! Репозиторий сессий и реплик диалога (SQLite).

use std::sync::Arc;

use rusqlite::{params, OptionalExtension, Row};

use crate::domain::entities::session::{
    MessageRole, Session, SessionMessage, SessionMode, SessionStatus,
};
use crate::domain::ports::SessionRepository;
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

        let metrics = serde_json::from_str(&metrics_raw).unwrap_or_default();

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
        let role: String = row.get(3)?;
        let turn_index: i64 = row.get(2)?;
        Ok(SessionMessage {
            id: row.get(0)?,
            session_id: row.get(1)?,
            turn_index: turn_index.max(0) as u32,
            role: MessageRole::from_slug(&role),
            content: row.get(4)?,
            strategy: row.get(5)?,
            score_delta: row.get(6)?,
            created_at: row.get(7)?,
        })
    }
}

const SELECT: &str = "SELECT id, user_id, scenario_id, mode, status, total_score, \
    turn_count, metrics, ending_id, ending_title, feedback, created_at, finished_at \
    FROM sessions";

impl SessionRepository for SqliteSessionRepo {
    fn create(&self, session: &Session) -> AppResult<()> {
        let conn = self.db.conn();
        let metrics = serde_json::to_string(&session.metrics)?;
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
        )?;
        Ok(())
    }

    fn get(&self, id: &str) -> AppResult<Option<Session>> {
        let conn = self.db.conn();
        let sql = format!("{SELECT} WHERE id = ?1");
        conn.query_row(&sql, params![id], Self::map_row)
            .optional()
            .map_err(Into::into)
    }

    fn list_by_user(&self, user_id: &str, limit: u32) -> AppResult<Vec<Session>> {
        let conn = self.db.conn();
        let sql = format!("{SELECT} WHERE user_id = ?1 ORDER BY created_at DESC LIMIT ?2");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![user_id, limit as i64], Self::map_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn update(&self, session: &Session) -> AppResult<()> {
        let conn = self.db.conn();
        let metrics = serde_json::to_string(&session.metrics)?;
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
        )?;
        Ok(())
    }

    fn append_message(&self, message: &SessionMessage) -> AppResult<()> {
        let conn = self.db.conn();
        conn.execute(
            "INSERT INTO session_messages (id, session_id, turn_index, role, content, strategy, score_delta, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                message.id,
                message.session_id,
                message.turn_index as i64,
                message.role.slug(),
                message.content,
                message.strategy,
                message.score_delta,
                message.created_at,
            ],
        )?;
        Ok(())
    }

    fn messages(&self, session_id: &str) -> AppResult<Vec<SessionMessage>> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare(
            "SELECT id, session_id, turn_index, role, content, strategy, score_delta, created_at
             FROM session_messages WHERE session_id = ?1 ORDER BY turn_index ASC, created_at ASC",
        )?;
        let rows = stmt.query_map(params![session_id], Self::map_message)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
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
}
