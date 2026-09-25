//! LLM-оценка реплики игрока («судья») поверх эвристики.
//!
//! Отдельный короткий запрос к уже назначенной LLM-модели: модель ставит три
//! оценки 0–10 (стратегия, аргументация, тон) и классифицирует стратегию
//! реплики (`category`), после чего баллы хода смешиваются с ключевыми
//! словами [`analysis::analyze`]:
//! `round((1 - w) * эвристика + w * LLM)`, доля `w` — [`DEFAULT_WEIGHT`]
//! (настраивается `scoring.llm_judge_weight`).
//!
//! Категория стратегии берётся у модели, а не у эвристики: подпись под
//! баллом («Сотрудничество» / «Компромисс» / «Конфронтация») меняется от
//! реплики к реплике по решению судьи.
//!
//! Модуль чистый (без I/O): только промпт, разбор ответа и смешивание.
//! Сетевая ошибка, таймаут, исчерпанная квота и неразборчивый JSON
//! обрабатываются вызывающей стороной — там же решение откатиться
//! на эвристику целиком.

use serde::{Deserialize, Serialize};

use crate::domain::entities::scenario::Scenario;
use crate::domain::entities::session::{MessageRole, SessionMessage};
use crate::domain::services::analysis::Strategy;

/// Доля LLM-оценки в итоге по умолчанию: 40% LLM / 60% эвристика.
pub const DEFAULT_WEIGHT: f32 = 0.4;

/// Оценка судьи: три шкалы 0–10 и классификация стратегии реплики.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct JudgeScores {
    /// Стратегия переговоров: 0 — давление/уход, 10 — работа с интересами.
    pub strategy: u8,
    /// Аргументация: 0 — голое мнение, 10 — конкретика, цифры, критерии.
    pub argument: u8,
    /// Тон: 0 — грубо и давит, 10 — уважительно и делово.
    pub tone: u8,
    /// Какая стратегия **проявлена в реплике** (`collaboration` /
    /// `compromise` / `confrontation`) — этим определяется подпись под баллом.
    /// `None` — модель не назвала категорию, тогда берётся эвристика.
    pub category: Option<Strategy>,
}

/// Сырой ответ модели: числа берём как есть и обрезаем до 0..=10,
/// категория — произвольная строка (распознаём в [`parse_category`]).
#[derive(Debug, Deserialize)]
struct RawJudge {
    strategy: i32,
    argument: i32,
    tone: i32,
    #[serde(default)]
    category: Option<String>,
}

/// Распознаёт категорию стратегии: слаги и русские названия.
fn parse_category(raw: &str) -> Option<Strategy> {
    match raw.trim().to_lowercase().as_str() {
        "collaboration" | "сотрудничество" => Some(Strategy::Collaboration),
        "compromise" | "компромисс" => Some(Strategy::Compromise),
        "confrontation" | "конфронтация" => Some(Strategy::Confrontation),
        _ => None,
    }
}

/// System-prompt судьи: строгий JSON, без пояснений.
pub const SYSTEM_PROMPT: &str = "Ты — судья в тренажёре деловых переговоров. \
Ты оцениваешь ОДНУ последнюю реплику игрока, а не весь диалог.\n\
Оцени по трём шкалам целыми числами от 0 до 10:\n\
- strategy: качество стратегии — 0: давление, ультиматум, уход от диалога; \
5: торг и уступки без выяснения интересов; 10: работа с интересами обеих сторон, \
поиск взаимовыгодных решений, опора на критерии.\n\
- argument: аргументация — 0: голое мнение без обоснования; \
5: есть пояснения, но мало фактов; 10: конкретика, цифры, последствия, \
объективные критерии и данные.\n\
- tone: тон — 0: грубо, давит на собеседника; 5: нейтрально-деловой; \
10: уважительно, с эмпатией, без агрессии.\n\n\
Дополнительно классифицируй, какая стратегия ПРОЯВЛЕНА в этой реплике, \
и укажи её в category строго одним из трёх значений:\n\
- \"collaboration\" — открытые вопросы, интересы, поиск взаимовыгоды, данные и варианты;\n\
- \"compromise\" — делёжка и уступки, частичные договорённости, «пойдём навстречу», без поиска интересов;\n\
- \"confrontation\" — давление, ультиматумы, отрицание, оспаривание, грубость, отказ обсуждать.\n\n\
Верни СТРОГО один JSON-объект и ничего больше, без markdown-блоков и пояснений:\n\
{\"strategy\": 7, \"argument\": 6, \"tone\": 8, \"category\": \"collaboration\"}";

/// Строит user-prompt судьи: краткий контекст сценария + хвост диалога
/// + реплика игрока, которую нужно оценить.
pub fn build_prompt(scenario: &Scenario, history: &[SessionMessage], player_text: &str) -> String {
    let mut p = String::new();
    p.push_str("Контекст сценария:\n");
    p.push_str(&format!("- Роль игрока: {}\n", scenario.player_role));
    p.push_str(&format!("- Цель игрока: {}\n", scenario.player_goal));
    p.push_str(&format!(
        "- Роль собеседника: {} ({})\n",
        scenario.partner_role, scenario.partner_name
    ));
    p.push_str(&format!("- Цель собеседника: {}\n", scenario.partner_goal));

    // Хвост диалога: судье нужно понимать, о чём разговор, но не раздуваем prompt.
    const MAX_TAIL: usize = 8;
    const MAX_CONTENT_CHARS: usize = 300;
    let tail = history.len().saturating_sub(MAX_TAIL);
    if tail > 0 || !history.is_empty() {
        p.push_str("\nПредыдущий диалог:\n");
        for msg in &history[tail..] {
            let who = match msg.role {
                MessageRole::Player => "Игрок",
                MessageRole::Partner => "Собеседник",
            };
            p.push_str(&format!(
                "{}: {}\n",
                who,
                truncate(&msg.content, MAX_CONTENT_CHARS)
            ));
        }
    }

    p.push_str("\nРеплика игрока, которую нужно оценить:\n");
    p.push_str(&truncate(player_text, 2000));
    p.push_str("\n\nОтвет — только JSON.");
    p
}

/// Обрезает текст до `max` символов, добавляя многоточие.
fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max).collect();
    format!("{cut}…")
}

/// Разбирает ответ судьи: первый JSON-объект с тремя оценками и категорией.
///
/// Понимает ответ с префиксом («Оценка: {...}») и markdown-блоками; числа
/// вне 0..=10 обрезаются до границ, категория распознаётся по слагам и
/// русским названиям (неизвестная → `None`, тогда категория берётся с
/// эвристики). Не нашёл JSON или поля — `None`, вызывающая сторона
/// считает ход на эвристике.
pub fn parse(raw: &str) -> Option<JudgeScores> {
    let start = raw.find('{')?;
    let rest = &raw[start..];
    let end = rest.find('}')?;
    let parsed: RawJudge = serde_json::from_str(&rest[..=end]).ok()?;
    Some(JudgeScores {
        strategy: parsed.strategy.clamp(0, 10) as u8,
        argument: parsed.argument.clamp(0, 10) as u8,
        tone: parsed.tone.clamp(0, 10) as u8,
        category: parsed.category.as_deref().and_then(parse_category),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entities::scenario::Difficulty;
    use crate::domain::entities::session::SessionMessage;

    fn scenario() -> Scenario {
        Scenario {
            id: "sc".into(),
            title: "Т".into(),
            description: "Т".into(),
            sphere: "Т".into(),
            difficulty: Difficulty::Easy,
            player_role: "Продавец".into(),
            player_company: None,
            player_goal: "Заключить договор".into(),
            player_batna: "Уйти к конкуренту".into(),
            partner_name: "Пётр".into(),
            partner_role: "Клиент".into(),
            partner_company: None,
            partner_goal: "Скидку 20%".into(),
            partner_goals: vec![],
            partner_batna: "Конкурент".into(),
            partner_personality: Default::default(),
            opening_context: "Здравствуйте".into(),
            endings: crate::application::scenario::default_endings(),
            ai_generated: false,
            is_active: true,
            created_by: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: None,
        }
    }

    #[test]
    fn parse_plain_json_defaults_category_to_none() {
        let s = parse(r#"{"strategy": 7, "argument": 6, "tone": 8}"#).unwrap();
        assert_eq!(
            s,
            JudgeScores {
                strategy: 7,
                argument: 6,
                tone: 8,
                category: None,
            }
        );
    }

    #[test]
    fn parse_category_accepts_slugs_and_russian() {
        let s = parse(r#"{"strategy":7,"argument":6,"tone":8,"category":"compromise"}"#).unwrap();
        assert_eq!(s.category, Some(Strategy::Compromise));

        let s = parse(r#"{"strategy":7,"argument":6,"tone":8,"category":"Конфронтация"}"#).unwrap();
        assert_eq!(s.category, Some(Strategy::Confrontation));

        let s =
            parse(r#"{"strategy":7,"argument":6,"tone":8,"category":"collaboration"}"#).unwrap();
        assert_eq!(s.category, Some(Strategy::Collaboration));
    }

    #[test]
    fn parse_unknown_category_is_none_but_scores_survive() {
        let s = parse(r#"{"strategy":3,"argument":4,"tone":5,"category":"агрессия"}"#).unwrap();
        assert_eq!(
            s.category, None,
            "неизвестная категория не должна валить оценку"
        );
        assert_eq!((s.strategy, s.argument, s.tone), (3, 4, 5));
    }

    #[test]
    fn parse_json_with_prefix_and_markdown_fence() {
        let raw = "Вот оценка:\n```json\n{\"strategy\":3,\"argument\":10,\"tone\":0}\n```";
        let s = parse(raw).unwrap();
        assert_eq!((s.strategy, s.argument, s.tone), (3, 10, 0));
    }

    #[test]
    fn parse_clamps_out_of_range() {
        let s = parse(r#"{"strategy": -5, "argument": 42, "tone": 10}"#).unwrap();
        assert_eq!((s.strategy, s.argument, s.tone), (0, 10, 10));
    }

    #[test]
    fn parse_rejects_non_json_and_missing_fields() {
        assert!(parse("Отличная реплика!").is_none());
        assert!(parse(r#"{"strategy": 5}"#).is_none());
        assert!(parse("").is_none());
    }

    #[test]
    fn prompt_contains_context_and_player_text() {
        let mut msg = SessionMessage {
            id: "m1".into(),
            session_id: "s1".into(),
            turn_index: 1,
            role: MessageRole::Player,
            content: "Сколько вы готовы заплатить?".into(),
            strategy: Some("collaboration".into()),
            score_delta: 5,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        msg.role = MessageRole::Partner;
        let p = build_prompt(&scenario(), &[msg], "Цена 100₽, это рынок.");
        assert!(p.contains("Цель игрока"));
        assert!(p.contains("Собеседник: Сколько вы готовы заплатить?"));
        assert!(p.contains("Цена 100₽, это рынок."));
    }
}
