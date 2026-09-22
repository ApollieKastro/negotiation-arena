//! Сессия прохождения сценария и её метрики.

use serde::{Deserialize, Serialize};

/// Режим общения.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionMode {
    Text,
    Voice,
}

impl SessionMode {
    pub fn slug(&self) -> &'static str {
        match self {
            SessionMode::Text => "text",
            SessionMode::Voice => "voice",
        }
    }

    pub fn from_slug(slug: &str) -> Self {
        match slug {
            "voice" => SessionMode::Voice,
            _ => SessionMode::Text,
        }
    }
}

/// Статус сессии.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Active,
    Finished,
    Abandoned,
}

impl SessionStatus {
    pub fn slug(&self) -> &'static str {
        match self {
            SessionStatus::Active => "active",
            SessionStatus::Finished => "finished",
            SessionStatus::Abandoned => "abandoned",
        }
    }

    pub fn from_slug(slug: &str) -> Self {
        match slug {
            "finished" => SessionStatus::Finished,
            "abandoned" => SessionStatus::Abandoned,
            _ => SessionStatus::Active,
        }
    }
}

/// Кто говорит в реплике.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    Player,
    Partner,
}

impl MessageRole {
    pub fn slug(&self) -> &'static str {
        match self {
            MessageRole::Player => "player",
            MessageRole::Partner => "partner",
        }
    }

    pub fn from_slug(slug: &str) -> Self {
        match slug {
            "partner" => MessageRole::Partner,
            _ => MessageRole::Player,
        }
    }
}

/// Счётчики использования вопросов SPIN.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", default)]
pub struct SpinCounts {
    pub situation: u32,
    pub problem: u32,
    pub implication: u32,
    pub need_payoff: u32,
}

impl SpinCounts {
    pub fn total(&self) -> u32 {
        self.situation + self.problem + self.implication + self.need_payoff
    }
}

/// Накопленные метрики техник за сессию.
///
/// Хранится в `sessions.metrics` как JSON.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", default)]
pub struct SessionMetrics {
    pub strategy_score: i32,
    pub argument_score: i32,
    pub tone_score: i32,
    /// Бонусы за техники: SPIN, фокус на интересах, объективные критерии.
    pub technique_bonus: i32,
    pub spin_counts: SpinCounts,
    pub interest_focused: u32,
    pub objective_criteria_used: u32,
    pub collaboration_count: u32,
    pub compromise_count: u32,
    pub confrontation_count: u32,
}

impl SessionMetrics {
    pub fn total_score(&self) -> i32 {
        self.strategy_score + self.argument_score + self.tone_score + self.technique_bonus
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub user_id: String,
    pub scenario_id: String,
    pub mode: SessionMode,
    pub status: SessionStatus,
    pub total_score: i32,
    pub turn_count: u32,
    pub metrics: SessionMetrics,
    pub ending_id: Option<String>,
    pub ending_title: Option<String>,
    pub feedback: Option<String>,
    pub created_at: String,
    pub finished_at: Option<String>,
}

/// Реплика диалога.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    pub id: String,
    pub session_id: String,
    pub turn_index: u32,
    pub role: MessageRole,
    pub content: String,
    /// Стратегия игрока (`collaboration` / `compromise` / `confrontation`).
    pub strategy: Option<String>,
    pub score_delta: i32,
    pub created_at: String,
}
