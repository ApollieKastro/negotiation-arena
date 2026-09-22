//! Сценарий переговоров: стартовый контекст диалога.
//!
//! Сценарий описывает, кто игрок, кто собеседник, какая у обеих сторон цель
//! и BATNA, с какой реплики начинается диалог и какие финалы возможны.
//! Сам ход диалога ведёт LLM (роль `llm`), дерево ответов не хранится.

use serde::{Deserialize, Serialize};

/// Сложность сценария.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Difficulty {
    Easy,
    Medium,
    Hard,
}

impl Difficulty {
    /// Значение для БД и URL (принимает и слаг, и русское название).
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "easy" | "начальная" | "низкая" => Difficulty::Easy,
            "hard" | "сложная" | "высокая" => Difficulty::Hard,
            _ => Difficulty::Medium,
        }
    }

    pub fn slug(&self) -> &'static str {
        match self {
            Difficulty::Easy => "easy",
            Difficulty::Medium => "medium",
            Difficulty::Hard => "hard",
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            Difficulty::Easy => "Начальная",
            Difficulty::Medium => "Средняя",
            Difficulty::Hard => "Сложная",
        }
    }
}

/// Финал сценария, выбирается по итоговому баллу.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ending {
    pub id: String,
    pub title: String,
    pub text: String,
    /// Что конкретно произошло (исход для карточки результата).
    pub outcome: String,
    /// Минимальный итоговый балл для этого финала.
    pub min_score: i32,
}

/// Манера общения оппонента (передаётся в system-prompt LLM).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", default)]
pub struct PartnerPersonality {
    pub tone: Option<String>,
    pub style: Option<String>,
    /// Черты характера через запятую, например «суетливый, недоверчивый».
    pub traits: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scenario {
    pub id: String,
    pub title: String,
    pub description: String,
    pub sphere: String,
    pub difficulty: Difficulty,

    // ── Игрок ──
    pub player_role: String,
    pub player_company: Option<String>,
    pub player_goal: String,
    pub player_batna: String,

    // ── Собеседник ──
    pub partner_name: String,
    pub partner_role: String,
    pub partner_company: Option<String>,
    pub partner_goal: String,
    pub partner_goals: Vec<String>,
    pub partner_batna: String,
    pub partner_personality: PartnerPersonality,

    /// Реплика собеседника, которой начинается диалог.
    pub opening_context: String,

    pub endings: Vec<Ending>,
    pub ai_generated: bool,
    pub is_active: bool,
    pub created_by: Option<String>,
    pub created_at: String,
    pub updated_at: Option<String>,
}
