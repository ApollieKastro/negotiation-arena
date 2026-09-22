//! Статистика: сводка по пользователю, активность, лидерборд, обзор платформы.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::application::auth::AuthContext;
use crate::domain::entities::session::{Session, SessionStatus};
use crate::domain::entities::user::User;
use crate::domain::ports::{ScenarioRepository, SessionRepository, UserRepository};
use crate::error::{AppError, AppResult};
use crate::infrastructure::db::repos::SqliteRepos;

/// Сводка по одному пользователю.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UserStats {
    pub user_id: String,
    pub login: String,
    pub display_name: Option<String>,
    pub total_sessions: u32,
    pub finished: u32,
    pub active: u32,
    pub abandoned: u32,
    pub best_score: i32,
    pub avg_score: i32,
    pub last_session_at: Option<String>,
}

/// Строка лидерборда.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LeaderboardEntry {
    pub rank: u32,
    pub user_id: String,
    pub login: String,
    pub display_name: Option<String>,
    pub finished_sessions: u32,
    pub best_score: i32,
    pub avg_score: i32,
}

/// Обзор платформы для дашборда администратора.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PlatformOverview {
    pub users: u64,
    pub active_users: u64,
    pub sessions: u64,
    pub finished_sessions: u64,
    pub active_sessions: u64,
    pub scenarios: u64,
    pub active_scenarios: u64,
    pub avg_finished_score: i32,
    pub best_score: i32,
}

/// Точка активности за день (`YYYY-MM-DD` из RFC3339).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ActivityPoint {
    pub date: String,
    pub sessions: u32,
}

/// Агрегированная статистика.
pub struct StatsService {
    repos: Arc<SqliteRepos>,
}

impl StatsService {
    pub fn new(repos: Arc<SqliteRepos>) -> Self {
        Self { repos }
    }

    /// Статистика пользователя: своя — всегда; чужая — только `ViewAllStats`.
    pub fn user_stats(&self, actor: &AuthContext, user_id: &str) -> AppResult<UserStats> {
        if user_id != actor.user_id {
            actor.require(super::Permission::ViewAllStats)?;
        }
        let user = self
            .repos
            .users
            .by_id(user_id)?
            .ok_or_else(|| AppError::NotFound("пользователь не найден".into()))?;
        let sessions = self.repos.sessions.list_by_user(user_id, 1000)?;
        Ok(compute_user_stats(&user, &sessions))
    }

    /// Своя сводка.
    pub fn my_stats(&self, actor: &AuthContext) -> AppResult<UserStats> {
        actor.require(super::Permission::ViewOwnStats)?;
        self.user_stats(actor, &actor.user_id)
    }

    /// Лидерборд: только завершённые сессии, по best_score → avg → count.
    pub fn leaderboard(&self, actor: &AuthContext, limit: u32) -> AppResult<Vec<LeaderboardEntry>> {
        actor.require(super::Permission::ViewAllStats)?;
        let users = self.repos.users.list()?;
        let limit = limit.clamp(1, 100);

        let mut rows: Vec<LeaderboardEntry> = Vec::new();
        for user in users.iter().filter(|u| u.is_active) {
            let sessions = self.repos.sessions.list_by_user(&user.id, 1000)?;
            let stats = compute_user_stats(user, &sessions);
            if stats.finished == 0 {
                continue;
            }
            rows.push(LeaderboardEntry {
                rank: 0,
                user_id: user.id.clone(),
                login: user.login.clone(),
                display_name: user.display_name.clone(),
                finished_sessions: stats.finished,
                best_score: stats.best_score,
                avg_score: stats.avg_score,
            });
        }

        rows.sort_by(|a, b| {
            b.best_score
                .cmp(&a.best_score)
                .then(b.avg_score.cmp(&a.avg_score))
                .then(b.finished_sessions.cmp(&a.finished_sessions))
                .then(a.login.cmp(&b.login))
        });
        rows.truncate(limit as usize);
        for (i, row) in rows.iter_mut().enumerate() {
            row.rank = i as u32 + 1;
        }
        Ok(rows)
    }

    /// Сводка по всей платформе.
    pub fn overview(&self, actor: &AuthContext) -> AppResult<PlatformOverview> {
        actor.require(super::Permission::ViewAllStats)?;

        let users = self.repos.users.list()?;
        let scenarios = self.repos.scenarios.list(false)?;
        let active_users = users.iter().filter(|u| u.is_active).count() as u64;
        let active_scenarios = scenarios.iter().filter(|s| s.is_active).count() as u64;

        let mut session_total = 0u64;
        let mut finished = 0u64;
        let mut active = 0u64;
        let mut score_sum = 0i64;
        let mut score_n = 0i64;
        let mut best = i32::MIN;

        for user in &users {
            for s in self.repos.sessions.list_by_user(&user.id, 1000)? {
                session_total += 1;
                match s.status {
                    SessionStatus::Finished => {
                        finished += 1;
                        score_sum += i64::from(s.total_score);
                        score_n += 1;
                        best = best.max(s.total_score);
                    }
                    SessionStatus::Active => active += 1,
                    SessionStatus::Abandoned => {}
                }
            }
        }

        Ok(PlatformOverview {
            users: users.len() as u64,
            active_users,
            sessions: session_total,
            finished_sessions: finished,
            active_sessions: active,
            scenarios: scenarios.len() as u64,
            active_scenarios,
            avg_finished_score: if score_n > 0 {
                (score_sum / score_n) as i32
            } else {
                0
            },
            best_score: if best == i32::MIN { 0 } else { best },
        })
    }

    /// Сессии по дням за последние `days` суток (дни без сессий — 0).
    pub fn activity(&self, actor: &AuthContext, days: u32) -> AppResult<Vec<ActivityPoint>> {
        actor.require(super::Permission::ViewAllStats)?;
        let days = days.clamp(1, 365);
        let users = self.repos.users.list()?;

        let mut by_day: BTreeMap<String, u32> = BTreeMap::new();
        for offset in 0..i64::from(days) {
            if let Some(day) = chrono::Utc::now()
                .date_naive()
                .checked_sub_signed(chrono::Duration::days(offset))
            {
                by_day
                    .entry(day.format("%Y-%m-%d").to_string())
                    .or_insert(0);
            }
        }

        for user in &users {
            for s in self.repos.sessions.list_by_user(&user.id, 1000)? {
                let date: String = s.created_at.chars().take(10).collect();
                if let Some(slot) = by_day.get_mut(&date) {
                    *slot += 1;
                }
            }
        }

        Ok(by_day
            .into_iter()
            .map(|(date, sessions)| ActivityPoint { date, sessions })
            .collect())
    }
}

fn compute_user_stats(user: &User, sessions: &[Session]) -> UserStats {
    let mut finished = 0u32;
    let mut active = 0u32;
    let mut abandoned = 0u32;
    let mut best = 0i32;
    let mut sum = 0i64;
    let mut last: Option<String> = None;

    for s in sessions {
        match s.status {
            SessionStatus::Finished => {
                finished += 1;
                best = best.max(s.total_score);
                sum += i64::from(s.total_score);
            }
            SessionStatus::Active => active += 1,
            SessionStatus::Abandoned => abandoned += 1,
        }
        let newer = match &last {
            None => true,
            Some(l) => s.created_at.as_str() > l.as_str(),
        };
        if newer {
            last = Some(s.created_at.clone());
        }
    }

    UserStats {
        user_id: user.id.clone(),
        login: user.login.clone(),
        display_name: user.display_name.clone(),
        total_sessions: sessions.len() as u32,
        finished,
        active,
        abandoned,
        best_score: best,
        avg_score: if finished > 0 {
            (sum / i64::from(finished)) as i32
        } else {
            0
        },
        last_session_at: last,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::testsupport::{ctx, setup};
    use crate::domain::entities::scenario::{Difficulty, Scenario};
    use crate::domain::entities::session::{
        MessageRole, SessionMessage, SessionMetrics, SessionMode,
    };
    use crate::domain::entities::user::UserRole;

    fn seed_user(svc: &crate::application::Services, login: &str, role: UserRole) -> User {
        svc.repos.users.create(login, "hash", role, None).unwrap()
    }

    fn seed_scenario(svc: &crate::application::Services) {
        let scenario = Scenario {
            id: "sc".into(),
            title: "t".into(),
            description: "d".into(),
            sphere: "s".into(),
            difficulty: Difficulty::Easy,
            player_role: "p".into(),
            player_company: None,
            player_goal: "g".into(),
            player_batna: "b".into(),
            partner_name: "n".into(),
            partner_role: "r".into(),
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
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: None,
        };
        svc.repos.scenarios.upsert(&scenario).unwrap();
    }

    fn insert_session(
        svc: &crate::application::Services,
        user_id: &str,
        status: SessionStatus,
        score: i32,
        created_at: &str,
    ) {
        let session = Session {
            id: uuid::Uuid::new_v4().to_string(),
            user_id: user_id.to_string(),
            scenario_id: "sc".into(),
            mode: SessionMode::Text,
            status,
            total_score: score,
            turn_count: 1,
            metrics: SessionMetrics::default(),
            ending_id: None,
            ending_title: None,
            feedback: None,
            created_at: created_at.to_string(),
            finished_at: if status == SessionStatus::Finished {
                Some(created_at.to_string())
            } else {
                None
            },
        };
        let sid = session.id.clone();
        svc.repos.sessions.create(&session).unwrap();
        svc.repos
            .sessions
            .append_message(&SessionMessage {
                id: uuid::Uuid::new_v4().to_string(),
                session_id: sid,
                turn_index: 0,
                role: MessageRole::Partner,
                content: "hi".into(),
                strategy: None,
                score_delta: 0,
                created_at: created_at.to_string(),
            })
            .unwrap();
    }

    fn actor_of(user: &User) -> AuthContext {
        AuthContext {
            user_id: user.id.clone(),
            login: user.login.clone(),
            role: user.role,
        }
    }

    #[test]
    fn my_stats_aggregates_finished_scores() {
        let (_db, svc) = setup().unwrap();
        seed_scenario(&svc);
        let user = seed_user(&svc, "alice", UserRole::User);
        let actor = actor_of(&user);

        let today = chrono::Utc::now().to_rfc3339();
        insert_session(&svc, &user.id, SessionStatus::Finished, 80, &today);
        insert_session(&svc, &user.id, SessionStatus::Finished, 40, &today);
        insert_session(&svc, &user.id, SessionStatus::Active, 0, &today);

        let stats = svc.stats.my_stats(&actor).unwrap();
        assert_eq!(stats.finished, 2);
        assert_eq!(stats.active, 1);
        assert_eq!(stats.total_sessions, 3);
        assert_eq!(stats.best_score, 80);
        assert_eq!(stats.avg_score, 60);
        assert!(stats.last_session_at.is_some());
    }

    #[test]
    fn user_cannot_read_foreign_stats_but_admin_can() {
        let (_db, svc) = setup().unwrap();
        let a = seed_user(&svc, "alice", UserRole::User);
        let b = seed_user(&svc, "bob", UserRole::User);
        let alice_ctx = actor_of(&a);
        let admin = ctx("admin-1", UserRole::Admin);

        assert!(svc.stats.my_stats(&alice_ctx).is_ok());
        assert!(svc.stats.user_stats(&alice_ctx, &b.id).is_err());
        assert!(svc.stats.user_stats(&admin, &a.id).is_ok());
        assert!(svc.stats.leaderboard(&alice_ctx, 10).is_err());
        assert!(svc.stats.overview(&alice_ctx).is_err());
        assert!(svc.stats.activity(&alice_ctx, 7).is_err());
    }

    #[test]
    fn leaderboard_ranks_by_best_score() {
        let (_db, svc) = setup().unwrap();
        seed_scenario(&svc);
        let low = seed_user(&svc, "low", UserRole::User);
        let high = seed_user(&svc, "high", UserRole::User);
        let none = seed_user(&svc, "none", UserRole::User);
        let admin = ctx("admin-1", UserRole::Admin);

        let today = chrono::Utc::now().to_rfc3339();
        insert_session(&svc, &low.id, SessionStatus::Finished, 50, &today);
        insert_session(&svc, &high.id, SessionStatus::Finished, 90, &today);
        insert_session(&svc, &high.id, SessionStatus::Finished, 70, &today);
        insert_session(&svc, &none.id, SessionStatus::Active, 0, &today);

        let board = svc.stats.leaderboard(&admin, 10).unwrap();
        assert_eq!(board.len(), 2);
        assert_eq!(board[0].login, "high");
        assert_eq!(board[0].rank, 1);
        assert_eq!(board[0].best_score, 90);
        assert_eq!(board[1].login, "low");
        assert_eq!(board[1].rank, 2);
    }

    #[test]
    fn overview_counts_entities() {
        let (_db, svc) = setup().unwrap();
        seed_scenario(&svc);
        let u = seed_user(&svc, "alice", UserRole::User);
        let admin = ctx("admin-1", UserRole::Admin);
        let today = chrono::Utc::now().to_rfc3339();
        insert_session(&svc, &u.id, SessionStatus::Finished, 10, &today);

        let ov = svc.stats.overview(&admin).unwrap();
        assert_eq!(ov.users, 1);
        assert_eq!(ov.active_users, 1);
        assert_eq!(ov.scenarios, 1);
        assert_eq!(ov.active_scenarios, 1);
        assert_eq!(ov.finished_sessions, 1);
        assert_eq!(ov.avg_finished_score, 10);
        assert_eq!(ov.best_score, 10);
    }

    #[test]
    fn activity_returns_requested_days() {
        let (_db, svc) = setup().unwrap();
        seed_scenario(&svc);
        let u = seed_user(&svc, "alice", UserRole::User);
        let admin = ctx("admin-1", UserRole::Admin);
        let today = chrono::Utc::now().to_rfc3339();
        insert_session(&svc, &u.id, SessionStatus::Finished, 1, &today);

        let points = svc.stats.activity(&admin, 7).unwrap();
        assert_eq!(points.len(), 7);
        let today_key: String = today.chars().take(10).collect();
        let today_point = points.iter().find(|p| p.date == today_key).unwrap();
        assert_eq!(today_point.sessions, 1);
    }
}
