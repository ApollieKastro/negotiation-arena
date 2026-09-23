//! Анализ реплик игрока: SPIN, стратегия, фокус на интересах, критерии.
//!
//! Эвристика на ключевых словах — работает мгновенно и без обращения к LLM.
//! Даёт обратную связь между ходами; LLM-оценка может накладываться сверху
//! на этапе 3, не меняя контракт [`MessageAnalysis`].

use serde::{Deserialize, Serialize};

/// Стратегия переговорщика.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Strategy {
    Collaboration,
    Compromise,
    Confrontation,
}

impl Strategy {
    pub fn slug(&self) -> &'static str {
        match self {
            Strategy::Collaboration => "collaboration",
            Strategy::Compromise => "compromise",
            Strategy::Confrontation => "confrontation",
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            Strategy::Collaboration => "Сотрудничество",
            Strategy::Compromise => "Компромисс",
            Strategy::Confrontation => "Конфронтация",
        }
    }

    pub fn from_slug(slug: &str) -> Self {
        match slug {
            "compromise" => Strategy::Compromise,
            "confrontation" => Strategy::Confrontation,
            _ => Strategy::Collaboration,
        }
    }
}

/// Тип вопроса по методике SPIN.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpinType {
    Situation,
    Problem,
    Implication,
    NeedPayoff,
}

impl SpinType {
    pub fn code(&self) -> &'static str {
        match self {
            SpinType::Situation => "S",
            SpinType::Problem => "P",
            SpinType::Implication => "I",
            SpinType::NeedPayoff => "N",
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            SpinType::Situation => "Ситуация",
            SpinType::Problem => "Проблема",
            SpinType::Implication => "Последствия",
            SpinType::NeedPayoff => "Ценность решения",
        }
    }
}

/// Результат анализа одной реплики игрока.
#[derive(Debug, Clone, PartialEq)]
pub struct MessageAnalysis {
    pub spin: Option<SpinType>,
    pub strategy: Strategy,
    pub focuses_on_interests: bool,
    pub uses_objective_criteria: bool,
    /// Сила аргументации, 0.0..=1.0.
    pub argument_strength: f32,
    /// Влияние на тон, -1.0..=1.0.
    pub tone_impact: f32,
}

/// Анализирует реплику игрока.
pub fn analyze(text: &str) -> MessageAnalysis {
    let normalized = normalize(text);
    let lowered = normalized.to_lowercase();

    let has_question = normalized.contains('?');
    let spin = detect_spin(&lowered, has_question);
    let strategy = detect_strategy(&lowered, has_question);

    MessageAnalysis {
        spin,
        strategy,
        focuses_on_interests: contains_any(&lowered, INTERESTS),
        uses_objective_criteria: contains_any(&lowered, OBJECTIVE_CRITERIA),
        argument_strength: argument_strength(&normalized, &lowered),
        tone_impact: tone_impact(&lowered),
    }
}

/// Приводит текст к виду для поиска: убирает лишние пробелы, нормализует ё→е.
fn normalize(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('ё', "е")
        .replace('Ё', "Е")
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| haystack.contains(n))
}

fn detect_spin(haystack: &str, has_question: bool) -> Option<SpinType> {
    // Без вопроса это не вопрос по методике, только утверждение.
    if !has_question {
        return None;
    }
    if contains_any(haystack, SPIN_NEED_PAYOFF) {
        return Some(SpinType::NeedPayoff);
    }
    if contains_any(haystack, SPIN_IMPLICATION) {
        return Some(SpinType::Implication);
    }
    if contains_any(haystack, SPIN_PROBLEM) {
        return Some(SpinType::Problem);
    }
    if contains_any(haystack, SPIN_SITUATION) {
        return Some(SpinType::Situation);
    }
    None
}

fn detect_strategy(haystack: &str, has_question: bool) -> Strategy {
    if contains_any(haystack, CONFRONTATION) {
        return Strategy::Confrontation;
    }
    if contains_any(haystack, COMPROMISE) {
        return Strategy::Compromise;
    }
    if contains_any(haystack, COLLABORATION) {
        return Strategy::Collaboration;
    }
    // Открытый вопрос без конфронтации — попытка понять интересы.
    if has_question {
        Strategy::Collaboration
    } else {
        Strategy::Compromise
    }
}

fn argument_strength(normalized: &str, lowered: &str) -> f32 {
    let mut score = 0.3_f32;

    // Цифры и проценты — признак конкретики.
    let digits = normalized.chars().filter(|c| c.is_ascii_digit()).count();
    score += (digits.min(8) as f32) * 0.07;

    if normalized.contains('?') {
        score += 0.1;
    }
    if lowered.contains("потому что") || lowered.contains("поэтому") || lowered.contains("так как")
    {
        score += 0.1;
    }
    if normalized.chars().count() > 120 {
        score += 0.1;
    }
    if normalized.chars().count() < 30 {
        score -= 0.1;
    }

    score.clamp(0.0, 1.0)
}

fn tone_impact(lowered: &str) -> f32 {
    let mut tone = 0.0_f32;

    if contains_any(lowered, POLITE) {
        tone += 0.4;
    }
    if contains_any(lowered, AGGRESSIVE) {
        tone -= 0.6;
    }
    if contains_any(lowered, EMPATHY) {
        tone += 0.3;
    }

    tone.clamp(-1.0, 1.0)
}

// ─────────────────────────────────────────────────────────────
// Словари (нормализованы: без «ё», в нижнем регистре)
// ─────────────────────────────────────────────────────────────

const SPIN_SITUATION: &[&str] = &[
    "как вы сейчас",
    "какая ситуация",
    "что у вас сейчас",
    "какой объем",
    "сколько",
    "кто ",
    "где ",
    "когда ",
    "расскажите",
    "опишите",
    "какие у вас",
    "как проходит",
    "с чего начнем",
    "ваша ситуация",
    "как вы решаете",
];

const SPIN_PROBLEM: &[&str] = &[
    "проблем",
    "трудност",
    "не устраивает",
    "больше всего",
    "сейчас возникает",
    "слабое место",
    "недостаток",
    "вас беспокоит",
    "не получается",
    "не хватает",
];

const SPIN_IMPLICATION: &[&str] = &[
    "влияет",
    "влияние",
    "последстви",
    "сколько это стоит",
    "во что обходится",
    "потер",
    "приведет",
    "чем это грозит",
    "если не решить",
    "цена промедления",
    "дорого обойдется",
    "чем обернется",
];

const SPIN_NEED_PAYOFF: &[&str] = &[
    "что изменится",
    "если решить",
    "если это решить",
    "ценность",
    "выгод",
    "почему это важно",
    "как это поможет",
    "что это даст",
    "какой эффект",
    "представьте",
    "что вы получите",
];

const INTERESTS: &[&str] = &[
    "для вас",
    "вам важно",
    "ваша цель",
    "ваши цели",
    "ваша задача",
    "ваша проблема",
    "что для вас",
    "ваш бюджет",
    "ваши планы",
    "какие у вас ожидания",
    "почему это важно для вас",
    "вам критично",
    "ваши приоритет",
];

const OBJECTIVE_CRITERIA: &[&str] = &[
    "рынок",
    "рыночн",
    "конкурент",
    "аналог",
    "данные",
    "цифры",
    "статистик",
    "среднерыноч",
    "средне рыноч",
    "стандарт",
    "прецедент",
    "контракт",
    "техзадание",
    "гаранти",
    "по данным",
    "%",
    "процент",
];

const COLLABORATION: &[&str] = &[
    "давайте найдем",
    "выгодн",
    "для обеих сторон",
    "вместе",
    "сотруднич",
    "общий интерес",
    "взаимн",
    "поймем друг друга",
];

const COMPROMISE: &[&str] = &[
    "компромисс",
    "давайте так",
    "навстречу",
    "половин",
    "50 на 50",
    "если вы",
    "уступить",
    "частично",
    "обмен",
];

const CONFRONTATION: &[&str] = &[
    "не можем",
    "невозможно",
    "неприемлем",
    "ультиматум",
    "требуем",
    "никаких",
    "никогда",
    "не согласны",
    "это абсурд",
    "отклоняем",
];

const POLITE: &[&str] = &["спасибо", "пожалуйста", "уважаем", "благодарю", "приветств"];

const EMPATHY: &[&str] = &["понимаю", "вижу", "согласен", "правы", "ваша точка зрения"];

const AGGRESSIVE: &[&str] = &[
    "немедленно",
    "возмущ",
    "нагл",
    "бессмысленн",
    "хватит",
    "не стану",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_situation_question() {
        let a = analyze("Как вы сейчас решаете эту задачу?");
        assert_eq!(a.spin, Some(SpinType::Situation));
    }

    #[test]
    fn detects_need_payoff_question() {
        let a = analyze("Что изменится, если это будет решено?");
        assert_eq!(a.spin, Some(SpinType::NeedPayoff));
    }

    #[test]
    fn detects_implication_question() {
        let a = analyze("Как это влияет на ваш бизнес?");
        assert_eq!(a.spin, Some(SpinType::Implication));
    }

    #[test]
    fn statement_is_not_spin_question() {
        let a = analyze("Мы решили увеличить объём поставок.");
        assert_eq!(a.spin, None);
    }

    #[test]
    fn detects_interest_focus() {
        let a = analyze("Что для вас важнее — цена или сроки?");
        assert!(a.focuses_on_interests);
    }

    #[test]
    fn detects_objective_criteria() {
        let a = analyze("По рыночным данным рост 20% ниже среднерыночного.");
        assert!(a.uses_objective_criteria);
    }

    #[test]
    fn detects_confrontation() {
        let a = analyze("Мы не можем снизить цену, это неприемлемо.");
        assert_eq!(a.strategy, Strategy::Confrontation);
    }

    #[test]
    fn detects_compromise() {
        let a = analyze("Давайте так: скидка 10%, но на два года.");
        assert_eq!(a.strategy, Strategy::Compromise);
    }

    #[test]
    fn open_question_defaults_to_collaboration() {
        let a = analyze("Какие условия поставки для вас оптимальны?");
        assert_eq!(a.strategy, Strategy::Collaboration);
    }

    #[test]
    fn concrete_numbers_raise_argument_strength() {
        let weak = analyze("Давайте так.");
        let strong = analyze(
            "При объёме от 1000 штук цена за единицу падает на 12%, \
             поэтому скидка 10% экономически обоснована.",
        );
        assert!(
            strong.argument_strength > weak.argument_strength,
            "сильный аргумент должен оцениваться выше: {} vs {}",
            strong.argument_strength,
            weak.argument_strength
        );
    }

    #[test]
    fn aggressive_tone_is_negative() {
        let a = analyze("Это возмутительно, немедленно пересмотрите условия!");
        assert!(a.tone_impact < 0.0);
    }

    #[test]
    fn polite_tone_is_positive() {
        let a = analyze("Спасибо, понимаю вашу позицию. Давайте обсудим объёмы.");
        assert!(a.tone_impact > 0.0);
    }
}
