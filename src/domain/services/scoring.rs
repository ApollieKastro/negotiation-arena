//! Скоринг сессии: подсчёт баллов, выбор финала, обратная связь.

use serde::{Deserialize, Serialize};

use crate::domain::entities::scenario::{Ending, Scenario};
use crate::domain::entities::session::{SessionMetrics, SpinCounts};
use crate::domain::services::analysis::{MessageAnalysis, SpinType, Strategy};
use crate::domain::services::judge::JudgeScores;

/// Итог прохождения сценария — всё, что нужно для страницы результата.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionReport {
    pub total_score: i32,
    pub strategy_score: i32,
    pub argument_score: i32,
    pub tone_score: i32,
    pub technique_bonus: i32,
    pub spin_counts: SpinCounts,
    pub interest_focused: u32,
    pub objective_criteria_used: u32,
    pub collaboration_count: u32,
    pub compromise_count: u32,
    pub confrontation_count: u32,
    pub turn_count: u32,
    pub ending: Ending,
    pub goal: String,
    pub feedback: String,
    /// Короткие пункты «что улучшить» для карточки результата.
    pub recommendations: Vec<String>,
}

/// Локаль отчёта: `ru` (по умолчанию) или `en`.
fn is_en(locale: &str) -> bool {
    locale.eq_ignore_ascii_case("en")
}

/// Баллы одного хода по шкалам; складываются в [`SessionMetrics`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnPoints {
    /// Стратегия: 15/10/5 по эвристике, 0..=15 после смешивания с LLM.
    pub strategy: i32,
    /// Аргументация: 0..=10.
    pub argument: i32,
    /// Тон: −5..=+5 (отрицательно за грубость).
    pub tone: i32,
    /// Бонус за техники (SPIN, интересы, объективные критерии) —
    /// считает только эвристика, судья его не оценивает.
    pub bonus: i32,
}

impl TurnPoints {
    pub fn total(self) -> i32 {
        self.strategy + self.argument + self.tone + self.bonus
    }
}

/// Баллы хода по ключевым словам ([`analysis::analyze`]) — база скоринга.
pub fn heuristic_points(analysis: &MessageAnalysis) -> TurnPoints {
    let strategy = match analysis.strategy {
        Strategy::Collaboration => 15,
        Strategy::Compromise => 10,
        Strategy::Confrontation => 5,
    };
    let argument = (analysis.argument_strength * 10.0) as i32;
    let tone = (analysis.tone_impact * 5.0).round() as i32;

    let mut bonus = 0;
    if analysis.spin.is_some() {
        bonus += 5;
    }
    if analysis.focuses_on_interests {
        bonus += 5;
    }
    if analysis.uses_objective_criteria {
        bonus += 8;
    }

    TurnPoints {
        strategy,
        argument,
        tone,
        bonus,
    }
}

/// Смешивает эвристику с оценкой LLM-судьи: `weight` — доля модели (0..=1).
///
/// Шкалы судьи (0..=10) приводятся к диапазонам эвристики: стратегия 0..=15,
/// аргумент 0..=10, тон −5..=+5 (оценка 5 — нейтральный тон). Бонус за
/// техники остаётся эвристическим. `weight = 0` — чистая эвристика,
/// `weight = 1` — чистая LLM-оценка.
pub fn blend(heuristic: TurnPoints, judge: JudgeScores, weight: f32) -> TurnPoints {
    let w = weight.clamp(0.0, 1.0);
    let mix = |base: i32, other: f32| ((base as f32) * (1.0 - w) + other * w).round() as i32;
    TurnPoints {
        strategy: mix(heuristic.strategy, f32::from(judge.strategy) * 15.0 / 10.0),
        argument: mix(heuristic.argument, f32::from(judge.argument)),
        tone: mix(heuristic.tone, f32::from(judge.tone) - 5.0),
        bonus: heuristic.bonus,
    }
}

/// Записывает баллы хода в метрики сессии.
///
/// Счётчики (стратегии, SPIN, интересы, критерии) ведутся по эвристике —
/// они питают обратную связь и не зависят от оценки судьи.
///
/// Возвращает прибавку к баллу за ход (может быть отрицательной
/// за грубый тон).
pub fn apply_points(
    metrics: &mut SessionMetrics,
    analysis: &MessageAnalysis,
    points: TurnPoints,
) -> i32 {
    match analysis.strategy {
        Strategy::Collaboration => metrics.collaboration_count += 1,
        Strategy::Compromise => metrics.compromise_count += 1,
        Strategy::Confrontation => metrics.confrontation_count += 1,
    }

    if let Some(spin) = analysis.spin {
        match spin {
            SpinType::Situation => metrics.spin_counts.situation += 1,
            SpinType::Problem => metrics.spin_counts.problem += 1,
            SpinType::Implication => metrics.spin_counts.implication += 1,
            SpinType::NeedPayoff => metrics.spin_counts.need_payoff += 1,
        }
    }
    if analysis.focuses_on_interests {
        metrics.interest_focused += 1;
    }
    if analysis.uses_objective_criteria {
        metrics.objective_criteria_used += 1;
    }

    metrics.strategy_score += points.strategy;
    metrics.argument_score += points.argument;
    metrics.tone_score += points.tone;
    metrics.technique_bonus += points.bonus;

    points.total()
}

/// Применяет анализ реплики к метрикам сессии (эвристика, без LLM-судьи).
///
/// Возвращает прибавку к баллу за этот ход (может быть отрицательной
/// за грубый тон).
pub fn apply_analysis(metrics: &mut SessionMetrics, analysis: &MessageAnalysis) -> i32 {
    apply_points(metrics, analysis, heuristic_points(analysis))
}

/// Строит итоговый отчёт по сессии.
///
/// `locale` — `ru` | `en` (из настроек пользователя); иначе — русский.
pub fn build_report(
    scenario: &Scenario,
    metrics: &SessionMetrics,
    turn_count: u32,
    locale: &str,
) -> SessionReport {
    let total_score = metrics.total_score();
    let ending = determine_ending(scenario, total_score, locale);

    SessionReport {
        total_score,
        strategy_score: metrics.strategy_score,
        argument_score: metrics.argument_score,
        tone_score: metrics.tone_score,
        technique_bonus: metrics.technique_bonus,
        spin_counts: metrics.spin_counts.clone(),
        interest_focused: metrics.interest_focused,
        objective_criteria_used: metrics.objective_criteria_used,
        collaboration_count: metrics.collaboration_count,
        compromise_count: metrics.compromise_count,
        confrontation_count: metrics.confrontation_count,
        turn_count,
        ending,
        goal: scenario.player_goal.clone(),
        feedback: build_feedback(metrics, turn_count, locale),
        recommendations: build_recommendations(metrics, locale),
    }
}

/// Выбирает финал с наивысшим `min_score`, не превышающим фактический балл.
///
/// Дефолтный финал (когда ни один threshold не подошёл) локализуется.
pub fn determine_ending(scenario: &Scenario, score: i32, locale: &str) -> Ending {
    let mut sorted = scenario.endings.clone();
    sorted.sort_by_key(|e| std::cmp::Reverse(e.min_score));

    if let Some(ending) = sorted.iter().find(|e| score >= e.min_score) {
        return ending.clone();
    }

    if is_en(locale) {
        sorted.last().cloned().unwrap_or_else(|| Ending {
            id: "none".to_string(),
            title: "Negotiation finished".to_string(),
            text: "The negotiation ended without a clear outcome.".to_string(),
            outcome: "Outcome undetermined".to_string(),
            min_score: 0,
        })
    } else {
        sorted.last().cloned().unwrap_or_else(|| Ending {
            id: "none".to_string(),
            title: "Переговоры завершены".to_string(),
            text: "Переговоры завершены без явного результата.".to_string(),
            outcome: "Результат не определён".to_string(),
            min_score: 0,
        })
    }
}

/// Разборная обратная связь (markdown), локаль `ru` | `en`.
fn build_feedback(metrics: &SessionMetrics, turn_count: u32, locale: &str) -> String {
    if is_en(locale) {
        build_feedback_en(metrics, turn_count)
    } else {
        build_feedback_ru(metrics, turn_count)
    }
}

/// Обратная связь на русском (markdown).
fn build_feedback_ru(metrics: &SessionMetrics, turn_count: u32) -> String {
    let mut out = String::new();

    // ── Стратегия ──
    out.push_str("## Стратегия\n\n");
    if metrics.collaboration_count > metrics.compromise_count
        && metrics.collaboration_count > metrics.confrontation_count
    {
        out.push_str(
            "Вы вели переговоры через **сотрудничество** — это лучший способ создать ценность для обеих сторон.\n\n",
        );
    } else if metrics.compromise_count > metrics.collaboration_count {
        out.push_str(
            "Вы чаще выбирали **компромисс**. Он двигает переговоры вперёд, но часто ограничивает потенциал: уступка снимает вопрос раньше, чем вы выяснили интересы.\n\n",
        );
    } else if metrics.confrontation_count > 0 {
        out.push_str(
            "Вы использовали **конфронтацию**. Она защищает вашу позицию, но снижает доверие и ужесточает собеседника.\n\n",
        );
    } else {
        out.push_str("Диалог шёл в спокойном деловом русле.\n\n");
    }

    // ── SPIN ──
    out.push_str("## Вопросы (SPIN)\n\n");
    let spin = &metrics.spin_counts;
    if spin.total() > 0 {
        out.push_str(&format!(
            "- **S** (ситуация): {}\n- **P** (проблема): {}\n- **I** (последствия): {}\n- **N** (ценность решения): {}\n\n",
            spin.situation, spin.problem, spin.implication, spin.need_payoff
        ));
        if spin.implication == 0 {
            out.push_str(
                "⚠️ Вопросов о **последствиях** не было. «Как это повлияет на ваш бизнес?» создаёт ощущение срочности и заставляет собеседника признать масштаб проблемы.\n\n",
            );
        }
        if spin.need_payoff == 0 {
            out.push_str(
                "⚠️ Не было вопросов о **ценности решения**. Дайте собеседнику самому озвучить выгоду от решения — это сильнее любого вашего утверждения.\n\n",
            );
        }
    } else {
        out.push_str("Вопросы по методике SPIN не использовались. Попробуйте:\n");
        out.push_str("- **S**: «Как вы сейчас решаете эту задачу?»\n");
        out.push_str("- **P**: «С какими проблемами сталкиваетесь?»\n");
        out.push_str("- **I**: «Как это влияет на ваши результаты?»\n");
        out.push_str("- **N**: «Что изменится, если это будет решено?»\n\n");
    }

    // ── Интересы ──
    out.push_str("## Интересы против позиций\n\n");
    if metrics.interest_focused > 0 {
        out.push_str(&format!(
            "В {} из {} реплик вы работали с **интересами** собеседника, а не с его позицией. Это ключевой принцип Гарвардского метода.\n\n",
            metrics.interest_focused, turn_count
        ));
    } else {
        out.push_str(
            "⚠️ Вы не выходили на интересы. Вместо «цена слишком высока» спрашивайте: «Почему это важно для вас?» — под позицией почти всегда лежит потребность, которую можно закрыть иначе.\n\n",
        );
    }

    // ── Объективные критерии ──
    out.push_str("## Объективные критерии\n\n");
    if metrics.objective_criteria_used > 0 {
        out.push_str(&format!(
            "Вы {} раз(а) опирались на **объективные критерии**: рыночные данные, стандарты, договорные условия. Это снижает эмоциональность и укрепляет вашу позицию.\n\n",
            metrics.objective_criteria_used
        ));
    } else {
        out.push_str(
            "⚠️ Аргументы опирались на ваше мнение. Опирайтесь на рыночную стоимость, профессиональные стандарты, прецеденты — это переводит спор из плоскости «мнение против мнения».\n\n",
        );
    }

    out
}

/// Обратная связь на английском (markdown).
fn build_feedback_en(metrics: &SessionMetrics, turn_count: u32) -> String {
    let mut out = String::new();

    out.push_str("## Strategy\n\n");
    if metrics.collaboration_count > metrics.compromise_count
        && metrics.collaboration_count > metrics.confrontation_count
    {
        out.push_str(
            "You ran the negotiation through **collaboration** — the best way to create value for both sides.\n\n",
        );
    } else if metrics.compromise_count > metrics.collaboration_count {
        out.push_str(
            "You chose **compromise** most often. It moves talks forward but often caps the upside: you concede before uncovering interests.\n\n",
        );
    } else if metrics.confrontation_count > 0 {
        out.push_str(
            "You used **confrontation**. It protects your position but lowers trust and hardens the counterpart.\n\n",
        );
    } else {
        out.push_str("The dialogue stayed in a calm business tone.\n\n");
    }

    out.push_str("## Questions (SPIN)\n\n");
    let spin = &metrics.spin_counts;
    if spin.total() > 0 {
        out.push_str(&format!(
            "- **S** (situation): {}\n- **P** (problem): {}\n- **I** (implication): {}\n- **N** (need-payoff): {}\n\n",
            spin.situation, spin.problem, spin.implication, spin.need_payoff
        ));
        if spin.implication == 0 {
            out.push_str(
                "⚠️ No **implication** questions. “How does this affect your business?” creates urgency and makes the counterpart feel the scale of the problem.\n\n",
            );
        }
        if spin.need_payoff == 0 {
            out.push_str(
                "⚠️ No **need-payoff** questions. Let the counterpart voice the benefit themselves — it beats any claim you make.\n\n",
            );
        }
    } else {
        out.push_str("You did not use SPIN questions. Try:\n");
        out.push_str("- **S**: “How do you handle this today?”\n");
        out.push_str("- **P**: “What problems do you run into?”\n");
        out.push_str("- **I**: “How does that affect your results?”\n");
        out.push_str("- **N**: “What changes once this is solved?”\n\n");
    }

    out.push_str("## Interests vs positions\n\n");
    if metrics.interest_focused > 0 {
        out.push_str(&format!(
            "In {} of {} replies you worked with **interests**, not positions. That is the core of the Harvard method.\n\n",
            metrics.interest_focused, turn_count
        ));
    } else {
        out.push_str(
            "⚠️ You stayed on positions. Instead of “the price is too high”, ask “Why does this matter to you?” — under a position there is almost always a need you can meet differently.\n\n",
        );
    }

    out.push_str("## Objective criteria\n\n");
    if metrics.objective_criteria_used > 0 {
        out.push_str(&format!(
            "You leaned on **objective criteria** {} time(s): market data, standards, contract terms. That reduces emotion and strengthens your case.\n\n",
            metrics.objective_criteria_used
        ));
    } else {
        out.push_str(
            "⚠️ Your arguments rested on opinion. Ground them in market value, professional standards, precedents — it moves the debate out of “opinion vs opinion”.\n\n",
        );
    }

    out
}

/// Короткие пункты «что улучшить» для карточки результата, локаль `ru` | `en`.
fn build_recommendations(metrics: &SessionMetrics, locale: &str) -> Vec<String> {
    if is_en(locale) {
        build_recommendations_en(metrics)
    } else {
        build_recommendations_ru(metrics)
    }
}

fn build_recommendations_ru(metrics: &SessionMetrics) -> Vec<String> {
    let mut recs = Vec::new();

    if metrics.spin_counts.implication < 2 {
        recs.push(
            "Задавайте больше вопросов о последствиях («чем это обернется, если не решить?»)"
                .to_string(),
        );
    }
    if metrics.spin_counts.need_payoff < 2 {
        recs.push("Дайте собеседнику самому назвать выгоду от решения (Need-payoff)".to_string());
    }
    if metrics.interest_focused < 2 {
        recs.push(
            "Выходите на интересы: «почему это важно для вас?» вместо спора о позициях".to_string(),
        );
    }
    if metrics.objective_criteria_used < 1 {
        recs.push(
            "Подкрепляйте позицию объективными критериями: рынок, стандарты, данные".to_string(),
        );
    }
    if metrics.confrontation_count > metrics.collaboration_count {
        recs.push("Меньше давления, больше совместного поиска решения".to_string());
    }
    if metrics.collaboration_count == 0 && metrics.compromise_count == 0 {
        recs.push("Ищите варианты выгодные обеим сторонам, а не только свою уступку".to_string());
    }

    recs
}

fn build_recommendations_en(metrics: &SessionMetrics) -> Vec<String> {
    let mut recs = Vec::new();

    if metrics.spin_counts.implication < 2 {
        recs.push(
            "Ask more implication questions (“what happens if this stays unsolved?”)".to_string(),
        );
    }
    if metrics.spin_counts.need_payoff < 2 {
        recs.push(
            "Let the counterpart name the benefit of the solution themselves (Need-payoff)"
                .to_string(),
        );
    }
    if metrics.interest_focused < 2 {
        recs.push(
            "Go for interests: “why does this matter to you?” instead of arguing positions"
                .to_string(),
        );
    }
    if metrics.objective_criteria_used < 1 {
        recs.push("Back your case with objective criteria: market, standards, data".to_string());
    }
    if metrics.confrontation_count > metrics.collaboration_count {
        recs.push("Less pressure, more joint problem-solving".to_string());
    }
    if metrics.collaboration_count == 0 && metrics.compromise_count == 0 {
        recs.push("Look for options that help both sides, not only your concession".to_string());
    }

    recs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entities::scenario::{Difficulty, Ending, Scenario};

    fn scenario_with_endings() -> Scenario {
        Scenario {
            id: "test".into(),
            title: "Тест".into(),
            description: "Тестовый сценарий".into(),
            sphere: "Продажи".into(),
            difficulty: Difficulty::Easy,
            player_role: "Менеджер".into(),
            player_company: None,
            player_goal: "Заключить сделку".into(),
            player_batna: "Найти другого клиента".into(),
            partner_name: "Иван".into(),
            partner_role: "Закупщик".into(),
            partner_company: None,
            partner_goal: "Снизить цену".into(),
            partner_goals: vec![],
            partner_batna: "Перейти к конкуренту".into(),
            partner_personality: Default::default(),
            opening_context: "Здравствуйте!".into(),
            endings: vec![
                Ending {
                    id: "win".into(),
                    title: "Отличный результат".into(),
                    text: "Сделка!".into(),
                    outcome: "Контракт подписан".into(),
                    min_score: 50,
                },
                Ending {
                    id: "partial".into(),
                    title: "Частичный успех".into(),
                    text: "Частично".into(),
                    outcome: "Есть заявка".into(),
                    min_score: 20,
                },
                Ending {
                    id: "fail".into(),
                    title: "Неудача".into(),
                    text: "Нет сделки".into(),
                    outcome: "Клиент ушёл".into(),
                    min_score: 0,
                },
            ],
            ai_generated: false,
            is_active: true,
            created_by: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: None,
        }
    }

    #[test]
    fn collaboration_scores_higher_than_confrontation() {
        let collab = crate::domain::services::analysis::analyze(
            "Какие условия поставки для вас оптимальны? Давайте найдём решение, выгодное для обеих сторон.",
        );
        let confront = crate::domain::services::analysis::analyze(
            "Мы не можем снизить цену, это неприемлемо.",
        );

        let mut m_collab = SessionMetrics::default();
        let mut m_confront = SessionMetrics::default();
        let d_collab = apply_analysis(&mut m_collab, &collab);
        let d_confront = apply_analysis(&mut m_confront, &confront);

        assert!(
            d_collab > d_confront,
            "сотрудничество должно давать больше баллов: {d_collab} vs {d_confront}"
        );
    }

    #[test]
    fn blend_at_zero_weight_keeps_heuristic() {
        let analysis = crate::domain::services::analysis::analyze(
            "Какие условия поставки для вас оптимальны? Давайте найдём решение, выгодное для обеих сторон.",
        );
        let heuristic = heuristic_points(&analysis);
        let judge = JudgeScores {
            strategy: 0,
            argument: 0,
            tone: 0,
        };

        assert_eq!(blend(heuristic, judge, 0.0), heuristic);
    }

    #[test]
    fn blend_at_full_weight_uses_judge_scales() {
        let analysis = crate::domain::services::analysis::analyze(
            "Мы не можем снизить цену, это неприемлемо.",
        );
        let heuristic = heuristic_points(&analysis);
        let judge = JudgeScores {
            strategy: 10,
            argument: 4,
            tone: 0,
        };

        let blended = blend(heuristic, judge, 1.0);
        assert_eq!(blended.strategy, 15, "10/10 → 15 баллов стратегии");
        assert_eq!(blended.argument, 4);
        assert_eq!(blended.tone, -5, "тон 0 → шкала −5..+5");
        assert_eq!(blended.bonus, heuristic.bonus, "бонус считает эвристика");
    }

    #[test]
    fn blend_splits_between_heuristic_and_judge() {
        let analysis = crate::domain::services::analysis::analyze(
            "Мы не можем снизить цену, это неприемлемо.",
        );
        let heuristic = heuristic_points(&analysis);
        let judge = JudgeScores {
            strategy: 10,
            argument: 10,
            tone: 10,
        };

        let blended = blend(heuristic, judge, 0.4);
        // Стратегия: 0.6*5 + 0.4*15 = 9.
        assert_eq!(blended.strategy, 9);
        // Тон: 0.6*0 + 0.4*(10−5) = 2.
        assert_eq!(blended.tone, 2);
        assert!(blended.total() > heuristic.total());
    }

    #[test]
    fn apply_points_writes_blended_scores_but_counts_from_analysis() {
        let analysis = crate::domain::services::analysis::analyze(
            "Давайте найдём решение, выгодное для обеих сторон.",
        );
        let judge = JudgeScores {
            strategy: 2,
            argument: 2,
            tone: 2,
        };
        let points = blend(heuristic_points(&analysis), judge, 0.4);

        let mut metrics = SessionMetrics::default();
        let delta = apply_points(&mut metrics, &analysis, points);

        assert_eq!(delta, points.total());
        assert_eq!(metrics.strategy_score, points.strategy);
        assert_eq!(metrics.argument_score, points.argument);
        assert_eq!(metrics.tone_score, points.tone);
        assert_eq!(
            metrics.collaboration_count, 1,
            "счётчики стратегий остаются эвристическими"
        );
        assert_eq!(metrics.total_score(), delta);
    }

    #[test]
    fn spin_and_techniques_add_bonus() {
        let analysis = crate::domain::services::analysis::analyze(
            "Как вы сейчас решаете эту задачу? Что для вас важнее — цена или качество? По данным рынка мы в среднем на 15% дешевле.",
        );
        let mut metrics = SessionMetrics::default();
        apply_analysis(&mut metrics, &analysis);

        assert!(
            metrics.spin_counts.total() >= 1,
            "SPIN должен быть засчитан"
        );
        assert!(metrics.interest_focused >= 1);
        assert!(metrics.objective_criteria_used >= 1);
        assert!(metrics.technique_bonus > 0);
    }

    #[test]
    fn ending_picked_by_threshold() {
        let scenario = scenario_with_endings();
        assert_eq!(determine_ending(&scenario, 70, "ru").id, "win");
        assert_eq!(determine_ending(&scenario, 30, "ru").id, "partial");
        assert_eq!(determine_ending(&scenario, 5, "ru").id, "fail");
        assert_eq!(determine_ending(&scenario, -10, "ru").id, "fail");
    }

    #[test]
    fn report_contains_goal_and_recommendations() {
        let scenario = scenario_with_endings();
        let analysis = crate::domain::services::analysis::analyze("Просто так.");
        let mut metrics = SessionMetrics::default();
        apply_analysis(&mut metrics, &analysis);

        let report = build_report(&scenario, &metrics, 1, "ru");
        assert_eq!(report.goal, "Заключить сделку");
        assert!(!report.feedback.is_empty());
        assert!(
            !report.recommendations.is_empty(),
            "должны быть пункты улучшения"
        );
        assert_eq!(report.total_score, metrics.total_score());
    }

    #[test]
    fn report_localized_to_english() {
        let scenario = scenario_with_endings();
        let analysis = crate::domain::services::analysis::analyze("Just testing.");
        let mut metrics = SessionMetrics::default();
        apply_analysis(&mut metrics, &analysis);

        let report = build_report(&scenario, &metrics, 1, "en");
        assert!(report.feedback.contains("## Strategy"), "EN feedback");
        assert!(
            !report.recommendations.is_empty(),
            "EN recommendations present"
        );
        // Русский раздел не должен мелькать в EN-отчёте.
        assert!(!report.feedback.contains("## Стратегия"));
    }

    #[test]
    fn default_ending_respects_locale() {
        let empty = Scenario {
            endings: vec![],
            ..scenario_with_endings()
        };
        let ru = determine_ending(&empty, 10, "ru");
        let en = determine_ending(&empty, 10, "en");
        assert_eq!(ru.id, "none");
        assert_eq!(en.id, "none");
        assert!(ru.title.contains("Переговоры"));
        assert!(en.title.contains("Negotiation"));
    }
}
