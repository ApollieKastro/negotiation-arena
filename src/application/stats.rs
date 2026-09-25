//! Статистика: сводка по пользователю, активность, лидерборд, обзор платформы.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::application::auth::AuthContext;
use crate::domain::ports::{ScenarioRepository, SessionRepository, UserRepository};
use crate::domain::services::progress::{progress_for_xp, Progress};
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
    /// Прогрессия: XP, уровень, прогресс до следующего уровня.
    #[serde(flatten)]
    pub progress: Progress,
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
    pub xp: i64,
    pub level: u32,
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

    /// Статистика пользователя: своя — по `ViewOwnStats`; чужая — `ViewAllStats`.
    ///
    /// Без явного `require` для своего id любой авторизованный контекст
    /// (в т.ч. сервисный) обходил бы таблицу прав. Агрегаты считаются в SQL
    /// одной строкой — без выборки всех сессий в память (N+1 устранён).
    pub fn user_stats(&self, actor: &AuthContext, user_id: &str) -> AppResult<UserStats> {
        if user_id == actor.user_id {
            actor.require(super::Permission::ViewOwnStats)?;
        } else {
            actor.require(super::Permission::ViewAllStats)?;
        }
        let user = self
            .repos
            .users
            .by_id(user_id)?
            .ok_or_else(|| AppError::NotFound("пользователь не найден".into()))?;
        let agg = self.repos.sessions.user_stats_aggregate(user_id)?;
        Ok(UserStats {
            user_id: user.id.clone(),
            login: user.login.clone(),
            display_name: user.display_name.clone(),
            total_sessions: agg.total,
            finished: agg.finished,
            active: agg.active,
            abandoned: agg.abandoned,
            best_score: agg.best_score,
            avg_score: agg.avg_score,
            last_session_at: agg.last_session_at,
            progress: progress_for_xp(agg.xp),
        })
    }

    /// Своя сводка.
    pub fn my_stats(&self, actor: &AuthContext) -> AppResult<UserStats> {
        actor.require(super::Permission::ViewOwnStats)?;
        self.user_stats(actor, &actor.user_id)
    }

    /// Лидерборд: только завершённые сессии, по best_score → avg → count.
    ///
    /// Один SQL-запрос (JOIN + GROUP BY), без цикла по пользователям.
    pub fn leaderboard(&self, actor: &AuthContext, limit: u32) -> AppResult<Vec<LeaderboardEntry>> {
        actor.require(super::Permission::ViewAllStats)?;
        let limit = limit.clamp(1, 100);

        let mut rows: Vec<LeaderboardEntry> = self
            .repos
            .sessions
            .leaderboard_rows()?
            .into_iter()
            .filter(|r| r.is_active && r.finished > 0)
            .map(|r| {
                let level = progress_for_xp(r.xp).level;
                LeaderboardEntry {
                    rank: 0,
                    user_id: r.user_id,
                    login: r.login,
                    display_name: r.display_name,
                    finished_sessions: r.finished,
                    best_score: r.best_score,
                    avg_score: r.avg_score,
                    xp: r.xp,
                    level,
                }
            })
            .collect();

        rows.sort_by(|a, b| {
            b.xp.cmp(&a.xp)
                .then(b.best_score.cmp(&a.best_score))
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
    ///
    /// Сессии — один SQL-агрегат; users/scenarios — по одному list (не N+1).
    pub fn overview(&self, actor: &AuthContext) -> AppResult<PlatformOverview> {
        actor.require(super::Permission::ViewAllStats)?;

        let users = self.repos.users.list()?;
        let scenarios = self.repos.scenarios.list(false)?;
        let active_users = users.iter().filter(|u| u.is_active).count() as u64;
        let active_scenarios = scenarios.iter().filter(|s| s.is_active).count() as u64;

        let sess = self.repos.sessions.platform_sessions_aggregate()?;

        Ok(PlatformOverview {
            users: users.len() as u64,
            active_users,
            sessions: sess.total,
            finished_sessions: sess.finished,
            active_sessions: sess.active,
            scenarios: scenarios.len() as u64,
            active_scenarios,
            avg_finished_score: sess.avg_finished_score,
            best_score: sess.best_score,
        })
    }

    /// Сессии по дням за последние `days` суток (дни без сессий — 0).
    ///
    /// Счётчики по дням — один SQL GROUP BY; пустые дни достраиваются в памяти.
    pub fn activity(&self, actor: &AuthContext, days: u32) -> AppResult<Vec<ActivityPoint>> {
        actor.require(super::Permission::ViewAllStats)?;
        let days = days.clamp(1, 365);

        let mut by_day: BTreeMap<String, u32> = BTreeMap::new();
        let mut since = None;
        for offset in 0..i64::from(days) {
            if let Some(day) = chrono::Utc::now()
                .date_naive()
                .checked_sub_signed(chrono::Duration::days(offset))
            {
                let key = day.format("%Y-%m-%d").to_string();
                if since.as_deref().is_none_or(|s| key.as_str() < s) {
                    since = Some(key.clone());
                }
                by_day.entry(key).or_insert(0);
            }
        }
        let since = since.unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%d").to_string());

        for (date, count) in self.repos.sessions.activity_by_day(&since)? {
            if let Some(slot) = by_day.get_mut(&date) {
                *slot = count;
            }
        }

        Ok(by_day
            .into_iter()
            .map(|(date, sessions)| ActivityPoint { date, sessions })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::testsupport::{ctx, setup};
    use crate::domain::entities::scenario::{Difficulty, Scenario};
    use crate::domain::entities::session::{
        MessageRole, Session, SessionMessage, SessionMetrics, SessionMode, SessionStatus,
    };
    use crate::domain::entities::user::{User, UserRole};

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
                session_id: sid.clone(),
                // Реплика в main-ветке (id ветки = id сессии).
                branch_id: Some(sid),
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
        // XP = 80 + 40 = 120 → уровень 2 (порог 100).
        assert_eq!(stats.progress.xp, 120);
        assert_eq!(stats.progress.level, 2);
        assert_eq!(stats.progress.next_level_xp, 250);
        assert!(stats.progress.progress_pct > 0 && stats.progress.progress_pct < 100);
    }

    #[test]
    fn user_cannot_read_foreign_stats_but_admin_can() {
        let (_db, svc) = setup().unwrap();
        let a = seed_user(&svc, "alice", UserRole::User);
        let b = seed_user(&svc, "bob", UserRole::User);
        let alice_ctx = actor_of(&a);
        let admin = ctx("admin-1", UserRole::Admin);

        assert!(svc.stats.my_stats(&alice_ctx).is_ok());
        // Своя сводка через user_stats тоже требует ViewOwnStats.
        assert!(svc.stats.user_stats(&alice_ctx, &a.id).is_ok());
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
        // high: 90+70=160 XP → уровень 2; low: 50 XP → уровень 1.
        assert_eq!(board[0].xp, 160);
        assert_eq!(board[0].level, 2);
        assert_eq!(board[1].login, "low");
        assert_eq!(board[1].rank, 2);
        assert_eq!(board[1].xp, 50);
        assert_eq!(board[1].level, 1);
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
