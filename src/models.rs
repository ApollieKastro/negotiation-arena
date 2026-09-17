use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─────────────────────────────────────────────────────────────
// Модель сценария
// ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scenario {
    pub id: String,
    pub title: String,
    pub description: String,
    pub sphere: String,
    pub difficulty: String,
    pub partner_name: String,
    pub partner_role: String,
    pub partner_goals: Vec<String>,
    pub initial_context: String,
    pub dialogue_tree: HashMap<String, DialogueNode>,
    pub endings: Vec<Ending>,
    /// BATNA собеседника (что он сделает если переговоры не удастся)
    #[serde(default)]
    pub partner_batna: String,
    /// BATNA игрока
    #[serde(default)]
    pub player_batna: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DialogueNode {
    pub id: String,
    pub speaker: String,
    pub text: String,
    pub responses: Vec<ResponseOption>,
    #[serde(default)]
    pub is_ending: bool,
    #[serde(default)]
    pub score: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseOption {
    pub id: String,
    pub text: String,
    pub strategy: String,
    /// Тип вопроса по SPIN: "S", "P", "I", "N" или "" если не SPIN
    #[serde(default)]
    pub spin_type: String,
    /// Использует ли ответ объективные критерии (рыночная стоимость, прецедент и т.д.)
    #[serde(default)]
    pub uses_objective_criteria: bool,
    /// Фокус на интересах, а не на позициях
    #[serde(default)]
    pub focuses_on_interests: bool,
    #[serde(default)]
    pub tone_impact: f32,
    #[serde(default)]
    pub argument_strength: f32,
    pub next_node_id: String,
    #[serde(default)]
    pub score_delta: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ending {
    pub id: String,
    pub title: String,
    pub text: String,
    pub outcome: String,
    pub min_score: i32,
}

// ─────────────────────────────────────────────────────────────
// Конфигурация сессии
// ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct NegotiationConfig {
    pub scenario_id: String,
    #[serde(default = "default_difficulty")]
    pub difficulty: String,
    #[serde(default = "default_tone")]
    pub partner_tone: String,
    #[serde(default)]
    pub sphere: String,
    #[serde(default = "default_mode")]
    pub mode: String,
}

fn default_difficulty() -> String { "Средняя".to_string() }
fn default_tone() -> String { "нейтральный".to_string() }
fn default_mode() -> String { "text".to_string() }

// ─────────────────────────────────────────────────────────────
// Ответ игрока
// ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct PlayerResponse {
    pub response_id: String,
    pub selected_strategy: String,
    #[serde(default)]
    pub is_final: bool,
}

// ─────────────────────────────────────────────────────────────
// Ответ сервера
// ─────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct StartResponse {
    pub session_id: String,
    pub initial_node: DialogueNode,
    pub partner_name: String,
    pub partner_role: String,
}

#[derive(Debug, Serialize)]
pub struct DialogueResponse {
    pub partner_text: String,
    pub score_delta: i32,
    pub is_final: bool,
    pub current_score: i32,
    pub next_node: Option<DialogueNode>,
    pub feedback: Option<String>,
}

#[derive(Debug, Serialize)]
#[allow(dead_code)]
pub struct ResultData {
    pub total_score: i32,
    pub max_score: i32,
    pub strategy_score: i32,
    pub argument_score: i32,
    pub tone_score: i32,
    pub feedback: String,
    pub recommendations: Vec<String>,
    pub ending: Ending,
    pub history: Vec<HistoryEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HistoryEntry {
    pub partner_text: String,
    pub player_text: String,
    pub strategy: String,
    pub score_delta: i32,
}

// ─────────────────────────────────────────────────────────────
// Сессия (in-memory)
// ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct NegotiationSession {
    pub id: String,
    pub scenario: Scenario,
    pub config: NegotiationConfig,
    pub current_node_id: String,
    pub current_score: i32,
    pub strategy_score: i32,
    pub argument_score: i32,
    pub tone_score: i32,
    pub history: Vec<HistoryEntry>,
    pub turn_count: u32,
    pub mode: String, // "text" or "voice"
    // ─── Трекинг переговорных техник ───
    pub spin_counts: SpinCounts,       // Сколько раз использован каждый тип SPIN
    pub interest_focused: u32,         // Сколько ответов фокусируются на интересах
    pub objective_criteria_used: u32,  // Сколько раз использованы объективные критерии
    pub collaboration_count: u32,      // Сколько раз выбрано сотрудничество
    pub compromise_count: u32,         // Сколько раз выбран компромисс
    pub confrontation_count: u32,      // Сколько раз выбрана конфронтация
}

/// Счётчик использований типов SPIN
#[derive(Debug, Clone, Default, Serialize)]
pub struct SpinCounts {
    pub situation: u32,   // S — ситуационные вопросы
    pub problem: u32,     // P — вопросы о проблемах
    pub implication: u32, // I — вопросы о последствиях
    pub need_payoff: u32, // N — вопросы о ценности решения
}

impl NegotiationSession {
    pub fn new(id: String, scenario: Scenario, config: NegotiationConfig, mode: String) -> Self {
        let start_node = scenario.dialogue_tree.get("start")
            .map(|n| n.id.clone())
            .unwrap_or_default();

        Self {
            id,
            scenario,
            config,
            current_node_id: start_node,
            current_score: 0,
            strategy_score: 0,
            argument_score: 0,
            tone_score: 0,
            history: Vec::new(),
            turn_count: 0,
            mode,
            spin_counts: SpinCounts::default(),
            interest_focused: 0,
            objective_criteria_used: 0,
            collaboration_count: 0,
            compromise_count: 0,
            confrontation_count: 0,
        }
    }

    pub fn get_current_node(&self) -> Option<&DialogueNode> {
        self.scenario.dialogue_tree.get(&self.current_node_id)
    }

    pub fn advance(&mut self, response: &PlayerResponse, node: &DialogueNode) -> Option<&DialogueNode> {
        let selected = node.responses.iter().find(|r| r.id == response.response_id)?;
        let next_id = &selected.next_node_id;

        self.current_score += selected.score_delta;
        self.current_node_id = next_id.clone();
        self.turn_count += 1;

        match selected.strategy.as_str() {
            "Сотрудничество" => {
                self.strategy_score += selected.score_delta;
                self.argument_score += (selected.argument_strength * 10.0) as i32;
                self.tone_score += (selected.tone_impact * 10.0).max(0.0) as i32;
                self.collaboration_count += 1;
            }
            "Компромисс" => {
                self.strategy_score += (selected.score_delta as f32 * 0.7) as i32;
                self.argument_score += (selected.argument_strength * 7.0) as i32;
                self.tone_score += ((selected.tone_impact + 0.1) * 5.0).max(0.0) as i32;
                self.compromise_count += 1;
            }
            "Конфронтация" => {
                self.strategy_score += (selected.score_delta as f32 * 0.3) as i32;
                self.argument_score += (selected.argument_strength * 5.0) as i32;
                self.tone_score += (selected.tone_impact * 5.0).max(-5.0) as i32;
                self.confrontation_count += 1;
            }
            _ => {
                self.strategy_score += selected.score_delta / 2;
                self.argument_score += (selected.argument_strength * 3.0) as i32;
            }
        }

        // ─── Трекинг SPIN ───
        match selected.spin_type.as_str() {
            "S" => self.spin_counts.situation += 1,
            "P" => self.spin_counts.problem += 1,
            "I" => self.spin_counts.implication += 1,
            "N" => self.spin_counts.need_payoff += 1,
            _ => {}
        }

        // ─── Трекинг переговорных техник ───
        if selected.focuses_on_interests {
            self.interest_focused += 1;
        }
        if selected.uses_objective_criteria {
            self.objective_criteria_used += 1;
        }

        self.scenario.dialogue_tree.get(next_id)
    }

    /// Сводка по техникам для feedback
    pub fn technique_summary(&self) -> TechniqueSummary {
        TechniqueSummary {
            spin_counts: self.spin_counts.clone(),
            interest_focused: self.interest_focused,
            objective_criteria_used: self.objective_criteria_used,
            collaboration_count: self.collaboration_count,
            compromise_count: self.compromise_count,
            confrontation_count: self.confrontation_count,
            total_turns: self.turn_count,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TechniqueSummary {
    pub spin_counts: SpinCounts,
    pub interest_focused: u32,
    pub objective_criteria_used: u32,
    pub collaboration_count: u32,
    pub compromise_count: u32,
    pub confrontation_count: u32,
    pub total_turns: u32,
}
