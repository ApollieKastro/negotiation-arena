//! Скоринг сессии: подсчёт баллов, выбор финала, обратная связь.

use serde::{Deserialize, Serialize};

use crate::domain::entities::scenario::{Ending, Scenario};
use crate::domain::entities::session::{SessionMetrics, SpinCounts};
use crate::domain::services::analysis::{MessageAnalysis, SpinType, Strategy};

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

/// Применяет анализ реплики к метрикам сессии.
///
/// Возвращает прибавку к баллу за этот ход (может быть отрицательной
/// за грубый тон).
pub fn apply_analysis(metrics: &mut SessionMetrics, analysis: &MessageAnalysis) -> i32 {
    let strategy_points = match analysis.strategy {
        Strategy::Collaboration => {
            metrics.strategy_score += 15;
            metrics.collaboration_count += 1;
            15
        }
        Strategy::Compromise => {
            metrics.strategy_score += 10;
            metrics.compromise_count += 1;
            10
        }
        Strategy::Confrontation => {
            metrics.strategy_score += 5;
            metrics.confrontation_count += 1;
            5
        }
    };

    let argument_points = (analysis.argument_strength * 10.0) as i32;
    metrics.argument_score += argument_points;

    let tone_points = (analysis.tone_impact * 5.0).round() as i32;
    metrics.tone_score += tone_points;

    let mut bonus = 0;
    if let Some(spin) = analysis.spin {
        bonus += 5;
        match spin {
            SpinType::Situation => metrics.spin_counts.situation += 1,
            SpinType::Problem => metrics.spin_counts.problem += 1,
            SpinType::Implication => metrics.spin_counts.implication += 1,
            SpinType::NeedPayoff => metrics.spin_counts.need_payoff += 1,
        }
    }
    if analysis.focuses_on_interests {
        bonus += 5;
        metrics.interest_focused += 1;
    }
    if analysis.uses_objective_criteria {
        bonus += 8;
        metrics.objective_criteria_used += 1;
    }
    metrics.technique_bonus += bonus;

    strategy_points + argument_points + tone_points + bonus
}

/// Строит итоговый отчёт по сессии.
pub fn build_report(
    scenario: &Scenario,
    metrics: &SessionMetrics,
    turn_count: u32,
) -> SessionReport {
    let total_score = metrics.total_score();
    let ending = determine_ending(scenario, total_score);

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
        feedback: build_feedback(metrics, turn_count),
        recommendations: build_recommendations(metrics),
    }
}

/// Выбирает финал с наивысшим `min_score`, не превышающим фактический балл.
pub fn determine_ending(scenario: &Scenario, score: i32) -> Ending {
    let mut sorted = scenario.endings.clone();
    sorted.sort_by_key(|e| std::cmp::Reverse(e.min_score));

    if let Some(ending) = sorted.iter().find(|e| score >= e.min_score) {
        return ending.clone();
    }

    sorted.last().cloned().unwrap_or_else(|| Ending {
        id: "none".to_string(),
        title: "Переговоры завершены".to_string(),
        text: "Переговоры завершены без явного результата.".to_string(),
        outcome: "Результат не определён".to_string(),
        min_score: 0,
    })
}

/// Разборная обратная связь на русском (markdown).
fn build_feedback(metrics: &SessionMetrics, turn_count: u32) -> String {
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

/// Короткие пункты «что улучшить» для карточки результата.
fn build_recommendations(metrics: &SessionMetrics) -> Vec<String> {
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
        assert_eq!(determine_ending(&scenario, 70).id, "win");
        assert_eq!(determine_ending(&scenario, 30).id, "partial");
        assert_eq!(determine_ending(&scenario, 5).id, "fail");
        assert_eq!(determine_ending(&scenario, -10).id, "fail");
    }

    #[test]
    fn report_contains_goal_and_recommendations() {
        let scenario = scenario_with_endings();
        let analysis = crate::domain::services::analysis::analyze("Просто так.");
        let mut metrics = SessionMetrics::default();
        apply_analysis(&mut metrics, &analysis);

        let report = build_report(&scenario, &metrics, 1);
        assert_eq!(report.goal, "Заключить сделку");
        assert!(!report.feedback.is_empty());
        assert!(
            !report.recommendations.is_empty(),
            "должны быть пункты улучшения"
        );
        assert_eq!(report.total_score, metrics.total_score());
    }
}
