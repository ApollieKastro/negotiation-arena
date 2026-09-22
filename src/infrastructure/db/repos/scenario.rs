//! Репозиторий сценариев (SQLite).

use std::sync::Arc;

use rusqlite::{params, OptionalExtension, Row};

use crate::domain::entities::scenario::{Difficulty, Scenario};
use crate::domain::ports::ScenarioRepository;
use crate::error::AppResult;
use crate::infrastructure::db::Database;

pub struct SqliteScenarioRepo {
    db: Arc<Database>,
}

impl SqliteScenarioRepo {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    fn map_row(row: &Row<'_>) -> rusqlite::Result<Scenario> {
        let difficulty: String = row.get(3)?;
        let partner_personality_raw: String = row.get(15)?;
        let endings_raw: String = row.get(17)?;
        let partner_goals_raw: String = row.get(13)?;
        let ai_generated: i32 = row.get(18)?;
        let is_active: i32 = row.get(19)?;

        let partner_personality =
            serde_json::from_str(&partner_personality_raw).unwrap_or_default();
        let endings = serde_json::from_str(&endings_raw).unwrap_or_default();
        let partner_goals = serde_json::from_str(&partner_goals_raw).unwrap_or_default();

        Ok(Scenario {
            id: row.get(0)?,
            title: row.get(1)?,
            description: row.get(2)?,
            sphere: row.get(4)?,
            difficulty: Difficulty::parse(&difficulty),
            player_role: row.get(5)?,
            player_company: row.get(6)?,
            player_goal: row.get(7)?,
            player_batna: row.get(8)?,
            partner_name: row.get(9)?,
            partner_role: row.get(10)?,
            partner_company: row.get(11)?,
            partner_goal: row.get(12)?,
            partner_goals,
            partner_batna: row.get(14)?,
            partner_personality,
            opening_context: row.get(16)?,
            endings,
            ai_generated: ai_generated != 0,
            is_active: is_active != 0,
            created_by: row.get(20)?,
            created_at: row.get(21)?,
            updated_at: row.get(22)?,
        })
    }
}

// Порядок колонок задаёт индексы в map_row — держите в синхроне.
const SELECT_SQL: &str = "SELECT id, title, description, difficulty, sphere, player_role, \
    player_company, player_goal, player_batna, partner_name, partner_role, partner_company, \
    partner_goal, partner_goals, partner_batna, partner_personality, opening_context, endings, \
    ai_generated, is_active, created_by, created_at, updated_at FROM scenarios";

impl ScenarioRepository for SqliteScenarioRepo {
    fn list(&self, active_only: bool) -> AppResult<Vec<Scenario>> {
        let conn = self.db.conn();
        let sql = if active_only {
            format!("{SELECT_SQL} WHERE is_active = 1 ORDER BY created_at DESC")
        } else {
            format!("{SELECT_SQL} ORDER BY created_at DESC")
        };
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], Self::map_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn get(&self, id: &str) -> AppResult<Option<Scenario>> {
        let conn = self.db.conn();
        let sql = format!("{SELECT_SQL} WHERE id = ?1");
        conn.query_row(&sql, params![id], Self::map_row)
            .optional()
            .map_err(Into::into)
    }

    fn upsert(&self, scenario: &Scenario) -> AppResult<()> {
        let conn = self.db.conn();
        let personality = serde_json::to_string(&scenario.partner_personality)?;
        let endings = serde_json::to_string(&scenario.endings)?;
        let partner_goals = serde_json::to_string(&scenario.partner_goals)?;
        // ?23 берётся из entity: create-путь держит `updated_at = None`,
        // update-путь выставляет таймстамп в сервисе.
        let updated_at = scenario.updated_at.clone();

        conn.execute(
            "INSERT INTO scenarios (
                id, title, description, sphere, difficulty,
                player_role, player_company, player_goal, player_batna,
                partner_name, partner_role, partner_company, partner_goal, partner_goals,
                partner_batna, partner_personality, opening_context, endings,
                ai_generated, is_active, created_by, created_at, updated_at
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5,
                ?6, ?7, ?8, ?9,
                ?10, ?11, ?12, ?13, ?14,
                ?15, ?16, ?17, ?18,
                ?19, ?20, ?21, ?22, ?23
             )
             ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                description = excluded.description,
                sphere = excluded.sphere,
                difficulty = excluded.difficulty,
                player_role = excluded.player_role,
                player_company = excluded.player_company,
                player_goal = excluded.player_goal,
                player_batna = excluded.player_batna,
                partner_name = excluded.partner_name,
                partner_role = excluded.partner_role,
                partner_company = excluded.partner_company,
                partner_goal = excluded.partner_goal,
                partner_goals = excluded.partner_goals,
                partner_batna = excluded.partner_batna,
                partner_personality = excluded.partner_personality,
                opening_context = excluded.opening_context,
                endings = excluded.endings,
                ai_generated = excluded.ai_generated,
                is_active = excluded.is_active,
                updated_at = excluded.updated_at",
            params![
                scenario.id,
                scenario.title,
                scenario.description,
                scenario.sphere,
                scenario.difficulty.slug(),
                scenario.player_role,
                scenario.player_company,
                scenario.player_goal,
                scenario.player_batna,
                scenario.partner_name,
                scenario.partner_role,
                scenario.partner_company,
                scenario.partner_goal,
                partner_goals,
                scenario.partner_batna,
                personality,
                scenario.opening_context,
                endings,
                scenario.ai_generated as i32,
                scenario.is_active as i32,
                scenario.created_by,
                scenario.created_at,
                updated_at,
            ],
        )?;
        Ok(())
    }

    fn delete(&self, id: &str) -> AppResult<()> {
        let conn = self.db.conn();
        conn.execute("DELETE FROM scenarios WHERE id = ?1", params![id])?;
        Ok(())
    }

    fn count(&self) -> AppResult<u64> {
        let conn = self.db.conn();
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM scenarios", [], |r| r.get(0))?;
        Ok(count.max(0) as u64)
    }
}
