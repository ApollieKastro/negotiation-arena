use crate::models::*;
use std::collections::HashMap;

// ─────────────────────────────────────────────────────────────
// Загрузка всех сценариев
// ─────────────────────────────────────────────────────────────

pub fn load_scenarios() -> HashMap<String, Scenario> {
    let mut scenarios = HashMap::new();

    // ── 1. Продажи: Скидка для клиента (Начальная) ──
    let mut sales_tree = HashMap::new();
    sales_tree.insert("start".to_string(), DialogueNode {
        id: "start".to_string(),
        speaker: "partner".to_string(),
        text: "Здравствуйте! Я рассматривал ваше предложение, но цена слишком высока. Нам нужны скидки, иначе придётся искать других поставщиков.".to_string(),
        responses: vec![
            ResponseOption {
                id: "s1_collab".to_string(),
                text: "Понимаю вашу позицию. Давайте обсудим, какие объёмы вы планируете и какие условия сотрудничества для вас приоритетны?".to_string(),
                strategy: "Сотрудничество".to_string(),
                spin_type: "S".to_string(),
                uses_objective_criteria: false,
                focuses_on_interests: true,
                tone_impact: 0.3, argument_strength: 0.9, next_node_id: "s1_r2".to_string(), score_delta: 15,
            },
            ResponseOption {
                id: "s1_comp".to_string(),
                text: "Мы можем снизить цену на 10%, если вы заключите долгосрочный контракт на 12 месяцев.".to_string(),
                strategy: "Компромисс".to_string(),
                spin_type: "N".to_string(),
                uses_objective_criteria: true,
                focuses_on_interests: false,
                tone_impact: 0.1, argument_strength: 0.7, next_node_id: "s1_r2c".to_string(), score_delta: 10,
            },
            ResponseOption {
                id: "s1_conf".to_string(),
                text: "Цена обоснована качеством продукта. Мы не можем её снизить — посмотрите на рыночные аналоги.".to_string(),
                strategy: "Конфронтация".to_string(),
                spin_type: "".to_string(),
                uses_objective_criteria: true,
                focuses_on_interests: false,
                tone_impact: -0.1, argument_strength: 0.5, next_node_id: "s1_r2f".to_string(), score_delta: 5,
            },
        ],
        is_ending: false, score: 0,
    });

    sales_tree.insert("s1_r2".to_string(), DialogueNode {
        id: "s1_r2".to_string(),
        speaker: "partner".to_string(),
        text: "Хорошо, давайте обсудим объёмы. Какой минимальный заказ вы готовы гарантировать при скидке?".to_string(),
        responses: vec![
            ResponseOption {
                id: "s1_r2a".to_string(),
                text: "При объёме от 1000 штук мы готовы к сотрудничеству. Какие условия поставки для вас оптимальны?".to_string(),
                strategy: "Сотрудничество".to_string(),
                spin_type: "S".to_string(),
                uses_objective_criteria: false,
                focuses_on_interests: true,
                tone_impact: 0.2, argument_strength: 0.8, next_node_id: "s1_r3".to_string(), score_delta: 15,
            },
            ResponseOption {
                id: "s1_r2b".to_string(),
                text: "500 штук — это наш стандарт. При таком объёме скидка 10% справедлива.".to_string(),
                strategy: "Компромисс".to_string(),
                spin_type: "".to_string(),
                uses_objective_criteria: true,
                focuses_on_interests: false,
                tone_impact: 0.0, argument_strength: 0.6, next_node_id: "s1_r3".to_string(), score_delta: 10,
            },
        ],
        is_ending: false, score: 0,
    });

    sales_tree.insert("s1_r2c".to_string(), DialogueNode {
        id: "s1_r2c".to_string(),
        speaker: "partner".to_string(),
        text: "10% — это недостаточно. Наши конкуренты предлагают минимум 15%. Что вы можете предложить дополнительно?".to_string(),
        responses: vec![
            ResponseOption {
                id: "s1_r2ca".to_string(),
                text: "Помимо скидки, мы предлагаем бесплатную доставку и техническую поддержку. Это повысит вашу рентабельность. Как это влияет на ваше решение?".to_string(),
                strategy: "Сотрудничество".to_string(),
                spin_type: "I".to_string(),
                uses_objective_criteria: false,
                focuses_on_interests: true,
                tone_impact: 0.3, argument_strength: 0.85, next_node_id: "s1_r3".to_string(), score_delta: 15,
            },
            ResponseOption {
                id: "s1_r2cb".to_string(),
                text: "15% возможно при объёме от 2000 штук. Это наши стандартные условия для крупных клиентов.".to_string(),
                strategy: "Компромисс".to_string(),
                spin_type: "N".to_string(),
                uses_objective_criteria: true,
                focuses_on_interests: false,
                tone_impact: 0.1, argument_strength: 0.7, next_node_id: "s1_r3".to_string(), score_delta: 10,
            },
        ],
        is_ending: false, score: 0,
    });

    sales_tree.insert("s1_r2f".to_string(), DialogueNode {
        id: "s1_r2f".to_string(),
        speaker: "partner".to_string(),
        text: "Понимаю вашу позицию, но тогда я буду вынужден обратиться к другим поставщикам. У них условия мягче.".to_string(),
        responses: vec![
            ResponseOption {
                id: "s1_r2fa".to_string(),
                text: "Прежде чем принять решение, давайте сравним не только цену, но и общую стоимость владения. Какие критерии важны для вас при выборе поставщика?".to_string(),
                strategy: "Сотрудничество".to_string(),
                spin_type: "P".to_string(),
                uses_objective_criteria: true,
                focuses_on_interests: true,
                tone_impact: 0.2, argument_strength: 0.8, next_node_id: "s1_end_partial".to_string(), score_delta: 10,
            },
            ResponseOption {
                id: "s1_r2fb".to_string(),
                text: "Хорошо, давайте договоримся — 12% скидки и мы включим сервисное обслуживание.".to_string(),
                strategy: "Компромисс".to_string(),
                spin_type: "".to_string(),
                uses_objective_criteria: false,
                focuses_on_interests: false,
                tone_impact: 0.0, argument_strength: 0.5, next_node_id: "s1_end_partial".to_string(), score_delta: 5,
            },
        ],
        is_ending: false, score: 0,
    });

    sales_tree.insert("s1_r3".to_string(), DialogueNode {
        id: "s1_r3".to_string(),
        speaker: "partner".to_string(),
        text: "Отлично! А как насчёт сроков поставки и гарантийных обязательств?".to_string(),
        responses: vec![
            ResponseOption {
                id: "s1_r3a".to_string(),
                text: "Поставка — 2 недели, гарантия 24 месяца. Это стандарт для наших клиентов. Как эти условия повлияют на ваш выбор?".to_string(),
                strategy: "Сотрудничество".to_string(),
                spin_type: "N".to_string(),
                uses_objective_criteria: true,
                focuses_on_interests: false,
                tone_impact: 0.3, argument_strength: 0.9, next_node_id: "s1_r4".to_string(), score_delta: 20,
            },
            ResponseOption {
                id: "s1_r3b".to_string(),
                text: "Сроки — 3 недели, гарантия 12 месяцев. Это стандартно для такого объёма.".to_string(),
                strategy: "Компромисс".to_string(),
                spin_type: "".to_string(),
                uses_objective_criteria: true,
                focuses_on_interests: false,
                tone_impact: 0.1, argument_strength: 0.6, next_node_id: "s1_r4".to_string(), score_delta: 10,
            },
        ],
        is_ending: false, score: 0,
    });

    sales_tree.insert("s1_r3f".to_string(), DialogueNode {
        id: "s1_r3f".to_string(),
        speaker: "partner".to_string(),
        text: "Хм, неплохие аргументы. Но мне нужно убедить руководство. Что вы скажете о бонусах за лояльность?".to_string(),
        responses: vec![
            ResponseOption {
                id: "s1_r3fa".to_string(),
                text: "Для постоянных клиентов у нас программа лояльности: ретроскидки 5% после 3 контрактов. Это важно для долгосрочного сотрудничества. Что скажете?".to_string(),
                strategy: "Сотрудничество".to_string(),
                spin_type: "I".to_string(),
                uses_objective_criteria: false,
                focuses_on_interests: true,
                tone_impact: 0.2, argument_strength: 0.8, next_node_id: "s1_end_partial".to_string(), score_delta: 10,
            },
        ],
        is_ending: false, score: 0,
    });

    sales_tree.insert("s1_r3c".to_string(), DialogueNode {
        id: "s1_r3c".to_string(),
        speaker: "partner".to_string(),
        text: "Неплохо. А что насчёт гарантии и технической поддержки?".to_string(),
        responses: vec![
            ResponseOption {
                id: "s1_r3ca".to_string(),
                text: "Гарантия 18 месяцев, техподдержка 24/7. Это наш стандарт для всех клиентов. Как эти условия соотносятся с вашими потребностями?".to_string(),
                strategy: "Сотрудничество".to_string(),
                spin_type: "S".to_string(),
                uses_objective_criteria: true,
                focuses_on_interests: false,
                tone_impact: 0.2, argument_strength: 0.7, next_node_id: "s1_r4c".to_string(), score_delta: 10,
            },
        ],
        is_ending: false, score: 0,
    });

    sales_tree.insert("s1_r4".to_string(), DialogueNode {
        id: "s1_r4".to_string(),
        speaker: "partner".to_string(),
        text: "Отлично! Давайте зафиксируем условия. Я готов подписать контракт.".to_string(),
        responses: vec![],
        is_ending: true, score: 20,
    });

    sales_tree.insert("s1_r4c".to_string(), DialogueNode {
        id: "s1_r4c".to_string(),
        speaker: "partner".to_string(),
        text: "Хорошо, давайте попробуем сотрудничать в рамках этих условий.".to_string(),
        responses: vec![],
        is_ending: true, score: 10,
    });

    sales_tree.insert("s1_end_partial".to_string(), DialogueNode {
        id: "s1_end_partial".to_string(),
        speaker: "partner".to_string(),
        text: "Спасибо за предложение. Я передам руководству на рассмотрение.".to_string(),
        responses: vec![],
        is_ending: true, score: 5,
    });

    scenarios.insert("sales_easy".to_string(), Scenario {
        id: "sales_easy".to_string(),
        title: "Продажи: Скидка для клиента".to_string(),
        description: "Клиент запрашивает скидку на продукт. Найдите баланс между сохранением маржи и удовлетворением клиента.".to_string(),
        sphere: "Продажи".to_string(),
        difficulty: "Начальная".to_string(),
        partner_name: "Дмитрий Козлов".to_string(),
        partner_role: "Директор по закупкам".to_string(),
        partner_goals: vec!["Получить скидку 15-20%".to_string(), "Обеспечить долгосрочное сотрудничество".to_string(), "Получить гарантию и сервис".to_string()],
        initial_context: "Вы — менеджер по продажам. Клиент — крупная оптовая компания, заинтересованная в вашем продукте, но считает цену завышенной. Ваш BATNA: найти другого клиента на аналогичный объём. BATNA клиента: обратиться к конкурентам.".to_string(),
        dialogue_tree: sales_tree,
        endings: vec![
            Ending { id: "win".to_string(), title: "Отличный результат".to_string(), text: "Вы нашли общее язык с клиентом и заключили выгодную сделку!".to_string(), outcome: "Клиент подписал контракт на выгодных условиях".to_string(), min_score: 50 },
            Ending { id: "partial".to_string(), title: "Частичный успех".to_string(), text: "Переговоры завершились, но не все цели достигнуты.".to_string(), outcome: "Клиент уходит, но оставляет заявку".to_string(), min_score: 20 },
            Ending { id: "fail".to_string(), title: "Неудача".to_string(), text: "Не удалось найти компромисс.".to_string(), outcome: "Клиент уходит к конкурентам".to_string(), min_score: 0 },
        ],
        partner_batna: "Обратиться к конкурентам (˄ цена, но ˅ условия)".to_string(),
        player_batna: "Найти другого клиента на аналогичный объём".to_string(),
    });

    // ── 2. HR: Собеседование (Средняя) ──
    let mut hr_tree = HashMap::new();
    hr_tree.insert("start".to_string(), DialogueNode {
        id: "start".to_string(),
        speaker: "partner".to_string(),
        text: "Расскажите о вашем опыте управления командой. Почему вы считаете, что справитесь с этой ролью?".to_string(),
        responses: vec![
            ResponseOption {
                id: "hr1_collab".to_string(),
                text: "Я заинтересован в вашей миссии. Мой опыт в управлении проектами поможет команде расти. Какие задачи стоят перед новым руководителем?".to_string(),
                strategy: "Сотрудничество".to_string(),
                spin_type: "S".to_string(),
                uses_objective_criteria: false,
                focuses_on_interests: true,
                tone_impact: 0.3, argument_strength: 0.9, next_node_id: "hr_r2".to_string(), score_delta: 20,
            },
            ResponseOption {
                id: "hr1_comp".to_string(),
                text: "Я готов адаптироваться под ваши процессы. У меня 5 лет опыта в управлении IT-проектами.".to_string(),
                strategy: "Компромисс".to_string(),
                spin_type: "".to_string(),
                uses_objective_criteria: true,
                focuses_on_interests: false,
                tone_impact: 0.1, argument_strength: 0.6, next_node_id: "hr_r2c".to_string(), score_delta: 15,
            },
        ],
        is_ending: false, score: 0,
    });

    hr_tree.insert("hr_r2".to_string(), DialogueNode {
        id: "hr_r2".to_string(),
        speaker: "partner".to_string(),
        text: "Основная проблема — высокая текучка. Как вы планируете это решать?".to_string(),
        responses: vec![
            ResponseOption {
                id: "hr_r2a".to_string(),
                text: "Давайте разберём причины. Какие отделы страдают больше всего? Спрашивать — ли проблема в оплате, корпоративной культуре или росте?".to_string(),
                strategy: "Сотрудничество".to_string(),
                spin_type: "P".to_string(),
                uses_objective_criteria: false,
                focuses_on_interests: true,
                tone_impact: 0.3, argument_strength: 0.85, next_node_id: "hr_r3".to_string(), score_delta: 20,
            },
            ResponseOption {
                id: "hr_r2b".to_string(),
                text: "Внедрить систему мотивации и регулярные reviews. Это стандартный подход.".to_string(),
                strategy: "Компромисс".to_string(),
                spin_type: "".to_string(),
                uses_objective_criteria: false,
                focuses_on_interests: false,
                tone_impact: 0.0, argument_strength: 0.5, next_node_id: "hr_r3".to_string(), score_delta: 10,
            },
        ],
        is_ending: false, score: 0,
    });

    hr_tree.insert("hr_r2c".to_string(), DialogueNode {
        id: "hr_r2c".to_string(),
        speaker: "partner".to_string(),
        text: "Опыт — это хорошо, но как именно вы будете мотивировать команду в кризис?".to_string(),
        responses: vec![
            ResponseOption {
                id: "hr_r2ca".to_string(),
                text: "В кризис важно сохранять прозрачность. Как сейчас выглядит ситуация в команде? Что они думают о перспективах?".to_string(),
                strategy: "Сотрудничество".to_string(),
                spin_type: "I".to_string(),
                uses_objective_criteria: false,
                focuses_on_interests: true,
                tone_impact: 0.2, argument_strength: 0.8, next_node_id: "hr_r3".to_string(), score_delta: 15,
            },
        ],
        is_ending: false, score: 0,
    });

    hr_tree.insert("hr_r3".to_string(), DialogueNode {
        id: "hr_r3".to_string(),
        speaker: "partner".to_string(),
        text: "Хороший подход. А как вы видите первый месяц на этой должности?".to_string(),
        responses: vec![
            ResponseOption {
                id: "hr_r3a".to_string(),
                text: "Первый месяц — встреча со всеми ключевыми сотрудниками. Я хочу понять их видение. С чего бы вы начали?".to_string(),
                strategy: "Сотрудничество".to_string(),
                spin_type: "N".to_string(),
                uses_objective_criteria: false,
                focuses_on_interests: true,
                tone_impact: 0.3, argument_strength: 0.9, next_node_id: "hr_end".to_string(), score_delta: 20,
            },
            ResponseOption {
                id: "hr_r3b".to_string(),
                text: "Аудит процессов, выявление слабых мест, план действий на 90 дней.".to_string(),
                strategy: "Компромисс".to_string(),
                spin_type: "".to_string(),
                uses_objective_criteria: true,
                focuses_on_interests: false,
                tone_impact: 0.1, argument_strength: 0.6, next_node_id: "hr_end".to_string(), score_delta: 10,
            },
        ],
        is_ending: false, score: 0,
    });

    hr_tree.insert("hr_end".to_string(), DialogueNode {
        id: "hr_end".to_string(),
        speaker: "partner".to_string(),
        text: "Спасибо за ответы. Мы свяжемся с вами в течение недели.".to_string(),
        responses: vec![],
        is_ending: true, score: 10,
    });

    scenarios.insert("hr_medium".to_string(), Scenario {
        id: "hr_medium".to_string(),
        title: "HR: Собеседование на руководителя".to_string(),
        description: "Вы проходите собеседование на должность руководителя отдела. HR-директор оценивает ваши компетенции.".to_string(),
        sphere: "HR".to_string(),
        difficulty: "Средняя".to_string(),
        partner_name: "Елена Петрова".to_string(),
        partner_role: "HR-директор".to_string(),
        partner_goals: vec!["Оценить лидерские качества".to_string(), "Понять мотивацию кандидата".to_string(), "Проверить соответствие культуре".to_string()],
        initial_context: "Вы — кандидат на должность руководителя IT-отдела. BATNA: оставаться на текущей позиции или принять предложение от другой компании. BATNA компании: найти кандидата изнутри или через рекрутинговое агентство.".to_string(),
        dialogue_tree: hr_tree,
        endings: vec![
            Ending { id: "hire".to_string(), title: "Оффер получен".to_string(), text: "Вас рекомендуют на должность!".to_string(), outcome: "Оффер с ожидаемой зарплатой".to_string(), min_score: 45 },
            Ending { id: "wait".to_string(), title: "Лист ожидания".to_string(), text: "Решение отложено — сравнивают с другими кандидатами.".to_string(), outcome: "Звонок через неделю".to_string(), min_score: 20 },
            Ending { id: "reject".to_string(), title: "Отказ".to_string(), text: "К сожалению, вы не подходите.".to_string(), outcome: "Письмо с отказом".to_string(), min_score: 0 },
        ],
        partner_batna: "Найти кандидата изнутри компании или через агентство".to_string(),
        player_batna: "Остаться на текущей позиции или принять предложение от конкурента".to_string(),
    });

    // ── 3. Продажи: Продление контракта (Начальная) ──
    let mut renewal_tree = HashMap::new();
    renewal_tree.insert("start".to_string(), DialogueNode {
        id: "start".to_string(),
        speaker: "partner".to_string(),
        text: "Наш контракт истекает через месяц. Мы довольны сервисом, но цена выросла на 20%. Нужно обсудить условия продления.".to_string(),
        responses: vec![
            ResponseOption {
                id: "r1_collab".to_string(),
                text: "Спасибо за обратную связь! Давайте разберём, какие аспекты сервиса ценятся больше всего и как мы можем обосновать стоимость.".to_string(),
                strategy: "Сотрудничество".to_string(),
                spin_type: "S".to_string(),
                uses_objective_criteria: false,
                focuses_on_interests: true,
                tone_impact: 0.3, argument_strength: 0.9, next_node_id: "r_r2".to_string(), score_delta: 15,
            },
            ResponseOption {
                id: "r1_comp".to_string(),
                text: "Мы готовы обсудить скидку при продлении на 2 года. Какие объёмы вы планируете?".to_string(),
                strategy: "Компромисс".to_string(),
                spin_type: "".to_string(),
                uses_objective_criteria: true,
                focuses_on_interests: false,
                tone_impact: 0.1, argument_strength: 0.7, next_node_id: "r_end".to_string(), score_delta: 10,
            },
        ],
        is_ending: false, score: 0,
    });

    renewal_tree.insert("r_r2".to_string(), DialogueNode {
        id: "r_r2".to_string(),
        speaker: "partner".to_string(),
        text: "Нам важно стабильность. Как рост цен повлияет на наш бюджет?".to_string(),
        responses: vec![
            ResponseOption {
                id: "r_r2a".to_string(),
                text: "Понимаю. Какой процент бюджета выделяется на нашу услугу? Если рост критичен, давайте найдём решение.".to_string(),
                strategy: "Сотрудничество".to_string(),
                spin_type: "I".to_string(),
                uses_objective_criteria: false,
                focuses_on_interests: true,
                tone_impact: 0.2, argument_strength: 0.85, next_node_id: "r_end".to_string(), score_delta: 15,
            },
        ],
        is_ending: false, score: 0,
    });

    renewal_tree.insert("r_end".to_string(), DialogueNode {
        id: "r_end".to_string(),
        speaker: "partner".to_string(),
        text: "Хорошо, давайте попробуем найти решение.".to_string(),
        responses: vec![],
        is_ending: true, score: 10,
    });

    scenarios.insert("renewal_easy".to_string(), Scenario {
        id: "renewal_easy".to_string(),
        title: "Продажи: Продление контракта".to_string(),
        description: "Клиент продлевает контракт, но недоволен ростом цен. Сохраните отношения и предложите решение.".to_string(),
        sphere: "Продажи".to_string(),
        difficulty: "Начальная".to_string(),
        partner_name: "Андрей Волков".to_string(),
        partner_role: "Коммерческий директор".to_string(),
        partner_goals: vec!["Снизить рост расходов".to_string(), "Сохранить качество сервиса".to_string(), "Получить предсказуемость бюджета".to_string()],
        initial_context: "Вы — менеджер по работе с клиентами. Клиент — действующий партнёр, контракт истекает. Ваш BATNA: предложить скидку на продление. BATNA клиента: перейти к конкуренту или сократить объём закупок.".to_string(),
        dialogue_tree: renewal_tree,
        endings: vec![
            Ending { id: "renew".to_string(), title: "Контракт продлён".to_string(), text: "Клиент согласен на новые условия!".to_string(), outcome: "Продление на 2 года".to_string(), min_score: 30 },
            Ending { id: "partial".to_string(), title: "Частичное согласие".to_string(), text: "Клиент продлевает, но с уменьшенным объёмом.".to_string(), outcome: "Снижение объёма на 30%".to_string(), min_score: 10 },
            Ending { id: "lost".to_string(), title: "Клиент уходит".to_string(), text: "Не удалось договориться.".to_string(), outcome: "Расторжение контракта".to_string(), min_score: 0 },
        ],
        partner_batna: "Перейти к конкуренту или сократить объём закупок".to_string(),
        player_batna: "Предложить скидку на продление или найти нового клиента".to_string(),
    });

    // ── 4. Закупки: Поставщик (Средняя) ──
    let mut procurement_tree = HashMap::new();
    procurement_tree.insert("start".to_string(), DialogueNode {
        id: "start".to_string(),
        speaker: "partner".to_string(),
        text: "Мы получили более выгодное предложение от вашего конкурента. Готовы ли вы пересмотреть условия?".to_string(),
        responses: vec![
            ResponseOption {
                id: "p1_collab".to_string(),
                text: "Давайте сравним условия. Что именно предложил конкурент? Мы хотим понять ваши приоритеты, чтобы предложить оптимальное решение.".to_string(),
                strategy: "Сотрудничество".to_string(),
                spin_type: "S".to_string(),
                uses_objective_criteria: true,
                focuses_on_interests: true,
                tone_impact: 0.3, argument_strength: 0.9, next_node_id: "p_r2".to_string(), score_delta: 15,
            },
            ResponseOption {
                id: "p1_comp".to_string(),
                text: "Мы готовы пересмотреть цены на 10%. Это наша максимальная скидка.".to_string(),
                strategy: "Компромисс".to_string(),
                spin_type: "".to_string(),
                uses_objective_criteria: true,
                focuses_on_interests: false,
                tone_impact: 0.0, argument_strength: 0.6, next_node_id: "p_r2c".to_string(), score_delta: 10,
            },
        ],
        is_ending: false, score: 0,
    });

    procurement_tree.insert("p_r2".to_string(), DialogueNode {
        id: "p_r2".to_string(),
        speaker: "partner".to_string(),
        text: "Конкурент предлагает на 15% дешевле. Но у них сроки дольше. Что вы можете предложить?".to_string(),
        responses: vec![
            ResponseOption {
                id: "p_r2a".to_string(),
                text: "Разница в 15% — существенна. Давайте посчитаем TCO (Total Cost of Ownership). Учитывая наши сроки и качество, разница может быть меньше. Считаете ли вы время простоев?".to_string(),
                strategy: "Сотрудничество".to_string(),
                spin_type: "I".to_string(),
                uses_objective_criteria: true,
                focuses_on_interests: true,
                tone_impact: 0.2, argument_strength: 0.85, next_node_id: "p_end".to_string(), score_delta: 15,
            },
        ],
        is_ending: false, score: 0,
    });

    procurement_tree.insert("p_r2c".to_string(), DialogueNode {
        id: "p_r2c".to_string(),
        speaker: "partner".to_string(),
        text: "10% — это мало. Конкурент даёт 15%. Нужно больше.".to_string(),
        responses: vec![
            ResponseOption {
                id: "p_r2ca".to_string(),
                text: "Понимаю. Помимо цены, давайте обсудим условия оплаты и доставки. Возможно, гибкий график сократит ваши расходы. Что для вас важнее — цена или условия?".to_string(),
                strategy: "Сотрудничество".to_string(),
                spin_type: "P".to_string(),
                uses_objective_criteria: false,
                focuses_on_interests: true,
                tone_impact: 0.2, argument_strength: 0.8, next_node_id: "p_end".to_string(), score_delta: 10,
            },
        ],
        is_ending: false, score: 0,
    });

    procurement_tree.insert("p_end".to_string(), DialogueNode {
        id: "p_end".to_string(),
        speaker: "partner".to_string(),
        text: "Хорошо, давайте подумаем и свяжемся.".to_string(),
        responses: vec![],
        is_ending: true, score: 10,
    });

    scenarios.insert("procurement_medium".to_string(), Scenario {
        id: "procurement_medium".to_string(),
        title: "Закупки: Выбор поставщика".to_string(),
        description: "Вы — закупщик, competing поставщики предлагают разные условия. Оцените каждое предложение.".to_string(),
        sphere: "Закупки".to_string(),
        difficulty: "Средняя".to_string(),
        partner_name: "Сергей Иванов".to_string(),
        partner_role: "Менеджер по продажам конкурента".to_string(),
        partner_goals: vec!["Получить контракт".to_string(), "Предложить лучшую цену".to_string(), "Обеспечить долгосрочное сотрудничество".to_string()],
        initial_context: "Вы — менеджер по закупкам. Нужно выбрать поставщика. Ваш BATNA: выбрать конкурента или провести тендер. BATNA конкурента: предложить скидку или улучшить условия.".to_string(),
        dialogue_tree: procurement_tree,
        endings: vec![
            Ending { id: "deal".to_string(), title: "Сделка состоялась".to_string(), text: "Вы выбрали лучшее предложение!".to_string(), outcome: "Подписание контракта".to_string(), min_score: 25 },
            Ending { id: "delay".to_string(), title: "Отсрочка".to_string(), text: "Решение отложено — нужно дополнительное сравнение.".to_string(), outcome: "Повторный тендер через месяц".to_string(), min_score: 10 },
            Ending { id: "cancel".to_string(), title: "Тендер отменён".to_string(), text: "Условия не устроили ни одну сторону.".to_string(), outcome: "Поиск альтернативных решений".to_string(), min_score: 0 },
        ],
        partner_batna: "Предложить скидку или улучшить условия для другого клиента".to_string(),
        player_batna: "Выбрать конкурента или провести тендер".to_string(),
    });

    // ── 5. Продажи: Партнёрство (Сложная) ──
    let mut partnership_tree = HashMap::new();
    partnership_tree.insert("start".to_string(), DialogueNode {
        id: "start".to_string(),
        speaker: "partner".to_string(),
        text: "Мы заинтересованы в партнёрстве, но хотим эксклюзивность в нашем регионе. Это принципиально.".to_string(),
        responses: vec![
            ResponseOption {
                id: "pp1_collab".to_string(),
                text: "Эксклюзивность — серьёзный вопрос. Давайте обсудим, какие обязательства вы готовы взять на себя и как это повлияет на наши взаимные выгоды.".to_string(),
                strategy: "Сотрудничество".to_string(),
                spin_type: "S".to_string(),
                uses_objective_criteria: false,
                focuses_on_interests: true,
                tone_impact: 0.3, argument_strength: 0.9, next_node_id: "pp_r2".to_string(), score_delta: 20,
            },
            ResponseOption {
                id: "pp1_comp".to_string(),
                text: "Эксклюзивность возможна при минимальном объёме закупок. Какой минимум вы готовы гарантировать?".to_string(),
                strategy: "Компромисс".to_string(),
                spin_type: "".to_string(),
                uses_objective_criteria: true,
                focuses_on_interests: false,
                tone_impact: 0.1, argument_strength: 0.7, next_node_id: "pp_r2c".to_string(), score_delta: 10,
            },
        ],
        is_ending: false, score: 0,
    });

    partnership_tree.insert("pp_r2".to_string(), DialogueNode {
        id: "pp_r2".to_string(),
        speaker: "partner".to_string(),
        text: "Мы готовы гарантировать объём от 5000 штук в год. Как это соотносится с вашими планами?".to_string(),
        responses: vec![
            ResponseOption {
                id: "pp_r2a".to_string(),
                text: "5000 — это серьёзный объём. Давайте обсудим, как рост продаж в вашем регионе повлияет на наш общий бизнес. Какие каналы продаж вы используете?".to_string(),
                strategy: "Сотрудничество".to_string(),
                spin_type: "I".to_string(),
                uses_objective_criteria: false,
                focuses_on_interests: true,
                tone_impact: 0.2, argument_strength: 0.85, next_node_id: "pp_r3".to_string(), score_delta: 15,
            },
        ],
        is_ending: false, score: 0,
    });

    partnership_tree.insert("pp_r2c".to_string(), DialogueNode {
        id: "pp_r2c".to_string(),
        speaker: "partner".to_string(),
        text: "Минимум 3000 штук. Но мы хотим скидку за объём. Сколько вы готовы дать?".to_string(),
        responses: vec![
            ResponseOption {
                id: "pp_r2ca".to_string(),
                text: "При 3000 штук скидка 10%. Но давайте рассмотрим не только цену — какую добавленную стоимость мы можем предложить? Техподдержка, маркетинг, обучение?".to_string(),
                strategy: "Сотрудничество".to_string(),
                spin_type: "P".to_string(),
                uses_objective_criteria: true,
                focuses_on_interests: true,
                tone_impact: 0.2, argument_strength: 0.8, next_node_id: "pp_r3".to_string(), score_delta: 15,
            },
        ],
        is_ending: false, score: 0,
    });

    partnership_tree.insert("pp_r3".to_string(), DialogueNode {
        id: "pp_r3".to_string(),
        speaker: "partner".to_string(),
        text: "Хорошо. А как вы видите развитие партнёрства через 2-3 года?".to_string(),
        responses: vec![
            ResponseOption {
                id: "pp_r3a".to_string(),
                text: "Давайте построим дорожную карту. Через год — выход на 80% региона, через 3 года — полное покрытие. Какие цели ставите вы?".to_string(),
                strategy: "Сотрудничество".to_string(),
                spin_type: "N".to_string(),
                uses_objective_criteria: false,
                focuses_on_interests: true,
                tone_impact: 0.3, argument_strength: 0.9, next_node_id: "pp_end".to_string(), score_delta: 20,
            },
        ],
        is_ending: false, score: 0,
    });

    partnership_tree.insert("pp_end".to_string(), DialogueNode {
        id: "pp_end".to_string(),
        speaker: "partner".to_string(),
        text: "Отлично! Давайте подготовим соглашение о партнёрстве.".to_string(),
        responses: vec![],
        is_ending: true, score: 15,
    });

    scenarios.insert("partnership_hard".to_string(), Scenario {
        id: "partnership_hard".to_string(),
        title: "Продажи: Долгосрочное партнёрство".to_string(),
        description: "Обсуждение условий партнёрства с эксклюзивностью. Сложные переговоры с высокими ставками.".to_string(),
        sphere: "Продажи".to_string(),
        difficulty: "Сложная".to_string(),
        partner_name: "Ольга Сидорова".to_string(),
        partner_role: "Коммерческий директор".to_string(),
        partner_goals: vec!["Получить эксклюзивность".to_string(), "Обеспечить долгосрочное сотрудничество".to_string(), "Получить конкурентные цены".to_string()],
        initial_context: "Вы — руководитель отдела продаж. Партнёр хочет эксклюзивность в регионе. Ваш BATNA: найти другого партнёра без эксклюзива. BATNA партнёра: работать с вашим конкурентом или расширить собственное производство.".to_string(),
        dialogue_tree: partnership_tree,
        endings: vec![
            Ending { id: "exclusive".to_string(), title: "Эксклюзивное партнёрство".to_string(), text: "Заключено соглашение на 3 года!".to_string(), outcome: "Эксклюзив в регионе + план роста".to_string(), min_score: 45 },
            Ending { id: "partial".to_string(), title: "Частичное партнёрство".to_string(), text: "Договорились о сотрудничестве без эксклюзива.".to_string(), outcome: "Работа на общих условиях".to_string(), min_score: 20 },
            Ending { id: "fail".to_string(), title: "Переговоры сорваны".to_string(), text: "Не удалось найти общего языка.".to_string(), outcome: "Партнёр уходит к конкуренту".to_string(), min_score: 0 },
        ],
        partner_batna: "Работать с конкурентом или расширить собственное производство".to_string(),
        player_batna: "Найти другого партнёра без эксклюзивных обязательств".to_string(),
    });

    // ── 6. Управление: Конфликт ресурсов (Сложная) ──
    let mut mgmt_tree = HashMap::new();
    mgmt_tree.insert("start".to_string(), DialogueNode {
        id: "start".to_string(),
        speaker: "partner".to_string(),
        text: "Мой отдел не получает достаточно ресурсов. Вы забираете людей и бюджет. Это несправедливо.".to_string(),
        responses: vec![
            ResponseOption {
                id: "m1_collab".to_string(),
                text: "Понимаю вашу обеспокоенность. Давайте разберём ситуацию: какие задачи стоят перед вашим отделом и почему ресурсы критичны именно сейчас?".to_string(),
                strategy: "Сотрудничество".to_string(),
                spin_type: "S".to_string(),
                uses_objective_criteria: false,
                focuses_on_interests: true,
                tone_impact: 0.3, argument_strength: 0.9, next_node_id: "mgmt_r2".to_string(), score_delta: 20,
            },
            ResponseOption {
                id: "m1_comp".to_string(),
                text: "Давайте пересмотрим распределение. Готов выделить 2 человек из моей команды временно.".to_string(),
                strategy: "Компромисс".to_string(),
                spin_type: "".to_string(),
                uses_objective_criteria: false,
                focuses_on_interests: false,
                tone_impact: 0.1, argument_strength: 0.6, next_node_id: "mgmt_r2c".to_string(), score_delta: 10,
            },
        ],
        is_ending: false, score: 0,
    });

    mgmt_tree.insert("mgmt_r2".to_string(), DialogueNode {
        id: "mgmt_r2".to_string(),
        speaker: "partner".to_string(),
        text: "У нас дедлайн через 2 недели. Нет людей — сорвём проект. Это повлияет на всю компанию.".to_string(),
        responses: vec![
            ResponseOption {
                id: "m2_collab".to_string(),
                text: "Понимаю срочность. Давайте совместно оценим приоритеты: что критично для дедлайна, а что можно отложить? Как это повлияет на общий план?".to_string(),
                strategy: "Сотрудничество".to_string(),
                spin_type: "I".to_string(),
                uses_objective_criteria: false,
                focuses_on_interests: true,
                tone_impact: 0.3, argument_strength: 0.85, next_node_id: "mgmt_r3".to_string(), score_delta: 15,
            },
        ],
        is_ending: false, score: 0,
    });

    mgmt_tree.insert("mgmt_r2c".to_string(), DialogueNode {
        id: "mgmt_r2c".to_string(),
        speaker: "partner".to_string(),
        text: "2 человек — мало. Нужно минимум 4. Иначе проект сорвётся.".to_string(),
        responses: vec![
            ResponseOption {
                id: "m2ca".to_string(),
                text: "Давайте посмотрим на это шире: какие задачи вашего проекта критичны для компании? Если дедлайн действительно важен, возможно, стоит пересмотреть приоритеты на уровне руководства.".to_string(),
                strategy: "Сотрудничество".to_string(),
                spin_type: "P".to_string(),
                uses_objective_criteria: false,
                focuses_on_interests: true,
                tone_impact: 0.2, argument_strength: 0.8, next_node_id: "mgmt_r3".to_string(), score_delta: 10,
            },
        ],
        is_ending: false, score: 0,
    });

    mgmt_tree.insert("mgmt_r3".to_string(), DialogueNode {
        id: "mgmt_r3".to_string(),
        speaker: "partner".to_string(),
        text: "Хорошо. Давайте договоримся: вы помогаете сейчас, потом я помогу вам. Как зафиксировать?".to_string(),
        responses: vec![
            ResponseOption {
                id: "m3_collab".to_string(),
                text: "Отлично! Давайте составим формальное соглашение: временные ресурсы в обмен на поддержку в следующем квартале. Какие сроки вам удобны?".to_string(),
                strategy: "Сотрудничество".to_string(),
                spin_type: "N".to_string(),
                uses_objective_criteria: true,
                focuses_on_interests: true,
                tone_impact: 0.3, argument_strength: 0.9, next_node_id: "mgmt_end".to_string(), score_delta: 20,
            },
        ],
        is_ending: false, score: 0,
    });

    mgmt_tree.insert("mgmt_end".to_string(), DialogueNode {
        id: "mgmt_end".to_string(),
        speaker: "partner".to_string(),
        text: "Спасибо за сотрудничество! Давайте обсудим детали.".to_string(),
        responses: vec![],
        is_ending: true, score: 15,
    });

    scenarios.insert("management_hard".to_string(), Scenario {
        id: "management_hard".to_string(),
        title: "Управление: Конфликт ресурсов".to_string(),
        description: "Конфликт между двумя руководителями из-за ресурсов. Нужно найти решение на уровне компании.".to_string(),
        sphere: "Управление".to_string(),
        difficulty: "Сложная".to_string(),
        partner_name: "Алексей Морозов".to_string(),
        partner_role: "Руководитель проектного офиса".to_string(),
        partner_goals: vec!["Получить людей для дедлайна".to_string(), "Сорвать проект коллеги".to_string(), "Добиться справедливого распределения".to_string()],
        initial_context: "Вы — руководитель IT-отдела. Коллега требует ресурсы для его проекта. Ваш BATNA: обратиться к генеральному директору. BATNA коллеги: сорвать дедлайн и обвинить вас в провале.".to_string(),
        dialogue_tree: mgmt_tree,
        endings: vec![
            Ending { id: "deal".to_string(), title: "Соглашение reached".to_string(), text: "Вы договорились о взаимной помощи!".to_string(), outcome: "Формальное соглашение между отделами".to_string(), min_score: 35 },
            Ending { id: "escalate".to_string(), title: "Эскалация".to_string(), text: "Конфликт передан руководству.".to_string(), outcome: "Совещание с генеральным директором".to_string(), min_score: 15 },
            Ending { id: "conflict".to_string(), title: "Конфликт обострился".to_string(), text: "Отношения испорчены, оба отдела страдают.".to_string(), outcome: "Кадровые перестановки".to_string(), min_score: 0 },
        ],
        partner_batna: "Сорвать дедлайн и обвинить вас в провале проекта".to_string(),
        player_batna: "Обратиться к генеральному директору для разрешения конфликта".to_string(),
    });

    scenarios
}

// ─────────────────────────────────────────────────────────────
// Генерация ответа собеседника (AI)
// ─────────────────────────────────────────────────────────────

fn groq_api_key() -> String {
    std::env::var("GROQ_API_KEY").expect("GROQ_API_KEY must be set in .env")
}

pub async fn generate_partner_response_ai(
    partner_name: &str,
    partner_role: &str,
    scenario_context: &str,
    player_text: &str,
    history: &[(String, String)],
    _spin_type: &str,
) -> Result<String, String> {
    let client = reqwest::Client::new();

    // Собираем историю диалога
    let mut dialogue_history = String::new();
    for (partner_says, player_says) in history {
        dialogue_history.push_str(&format!("{}: {}\nИгрок: {}\n", partner_name, partner_says, player_says));
    }

    let prompt = format!(
        r#"Ты играешь роль собеседника в деловых переговорах.

РОЛЬ: {partner_name}, {partner_role}
КОНТЕКСТ: {scenario_context}

СТИЛЬ ОТВЕТА:
- Говори только на РУССКОМ языке. Никаких иностранных символов, китайского, английского.
- Отвечай кратко: 1-3 предложения, как в реальном разговоре.
- Будь живым, естественным собеседником, а не роботом.
- Реагируй на конкретные слова собеседника.
- Сохраняй позицию и интересы своей роли.
- Не соглашайся слишком быстро — веди переговоры.

{dialogue_history}Игрок говорит: «{player_text}»

Ответь как {partner_name}. ТОЛЬКО текст ответа, без кавычек и пояснений:"#,
        partner_name = partner_name,
        partner_role = partner_role,
        scenario_context = scenario_context,
        dialogue_history = if dialogue_history.is_empty() { String::new() } else { format!("Предыдущий диалог:\n{}", dialogue_history) },
        player_text = player_text,
    );

    let body = serde_json::json!({
        "model": "qwen/qwen3.8-27b",
        "messages": [
            {"role": "system", "content": "Ты — профессиональный актёр, играющий роль делового собеседника. Отвечай ТОЛЬКО на русском языке. Никаких других языков. Будь живым и естественным."},
            {"role": "user", "content": prompt}
        ],
        "temperature": 0.8,
        "max_tokens": 200
    });

    let resp = client.post("https://api.groq.com/openai/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", groq_api_key()))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    let groq_resp: serde_json::Value = resp.json()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;

    let content = groq_resp["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("Продолжайте, пожалуйста.")
        .trim()
        .replace('\n', " ")
        .replace('\r', "")
        .to_string();

    // Убираем кавычки если модель обернула ответ
    let cleaned = content
        .trim_start_matches('"')
        .trim_end_matches('"')
        .trim_start_matches('«')
        .trim_end_matches('»')
        .to_string();

    Ok(cleaned)
}

// ─────────────────────────────────────────────────────────────
// Определение финала
// ─────────────────────────────────────────────────────────────

pub fn determine_ending(scenario: &Scenario, score: i32) -> Ending {
    let mut sorted = scenario.endings.clone();
    sorted.sort_by(|a, b| b.min_score.cmp(&a.min_score));

    for ending in &sorted {
        if score >= ending.min_score {
            return ending.clone();
        }
    }

    scenario.endings.last().cloned().unwrap_or(Ending {
        id: "none".to_string(),
        title: "Неопределённый исход".to_string(),
        text: "Переговоры завершились без результата.".to_string(),
        outcome: "Нет результата".to_string(),
        min_score: 0,
    })
}

// ─────────────────────────────────────────────────────────────
// Генерация обратной связи с техниками
// ─────────────────────────────────────────────────────────────

pub fn generate_feedback(session: &NegotiationSession, _ending: &Ending) -> String {
    let tech = session.technique_summary();
    let mut feedback = String::new();

    // ─── Оценка стратегии ───
    feedback.push_str("## Стратегия\n\n");
    if tech.collaboration_count > tech.compromise_count && tech.collaboration_count > tech.confrontation_count {
        feedback.push_str("Вы активно использовали **стратегию сотрудничества** — это лучший подход для создания ценности.\n\n");
    } else if tech.compromise_count > tech.collaboration_count {
        feedback.push_str("Вы чаще выбирали **компромисс**. Это помогает двигаться вперёд, но может ограничить потенциал создания ценности.\n\n");
    } else if tech.confrontation_count > 0 {
        feedback.push_str("Вы использовали **конфронтационные подходы**. Это может быть эффективно для защиты интересов, но снижает доверие.\n\n");
    }

    // ─── SPIN анализ ───
    feedback.push_str("## Техника вопросов (SPIN)\n\n");
    let total_spin = tech.spin_counts.situation + tech.spin_counts.problem
        + tech.spin_counts.implication + tech.spin_counts.need_payoff;

    if total_spin > 0 {
        feedback.push_str(&format!(
            "- **S** (ситуация): {} вопросов\n- **P** (проблема): {} вопросов\n- **I** (последствия): {} вопросов\n- **N** (ценность решения): {} вопросов\n\n",
            tech.spin_counts.situation, tech.spin_counts.problem,
            tech.spin_counts.implication, tech.spin_counts.need_payoff
        ));

        if tech.spin_counts.implication == 0 {
            feedback.push_str("⚠️ Вы не задавали вопросов о **последствиях** (Implication). ");
            feedback.push_str("Вопросы типа «Как это влияет на ваш бизнес?» создают осознание срочности.\n\n");
        }
        if tech.spin_counts.need_payoff == 0 {
            feedback.push_str("⚠️ Вы не задавали вопросов о **ценности решения** (Need-payoff). ");
            feedback.push_str("Позвольте собеседнику самому озвучить benefits от решения проблемы.\n\n");
        }
    } else {
        feedback.push_str("Вы не использовали вопросы SPIN. Попробуйте:\n");
        feedback.push_str("- **S**: «Как вы сейчас решаете эту задачу?»\n");
        feedback.push_str("- **P**: «Какие проблемы возникают?»\n");
        feedback.push_str("- **I**: «Как это влияет на ваш бизнес?»\n");
        feedback.push_str("- **N**: «Что бы изменилось, если это было решено?»\n\n");
    }

    // ─── Интересы vs позиции ───
    feedback.push_str("## Фокус на интересах\n\n");
    if tech.interest_focused > 0 {
        feedback.push_str(&format!(
            "В {} из {} ответов вы фокусировались на интересах собеседника. ",
            tech.interest_focused, tech.total_turns
        ));
        feedback.push_str("Это ключевой принцип Гарвардского метода.\n\n");
    } else {
        feedback.push_str("⚠️ Вы не фокусировались на интересах. Спросите: «Почему это важно для вас?»\n\n");
    }

    // ─── Объективные критерии ───
    feedback.push_str("## Объективные критерии\n\n");
    if tech.objective_criteria_used > 0 {
        feedback.push_str(&format!(
            "Вы {} раз ссылались на объективные критеры (рыночная стоимость, прецедент). ",
            tech.objective_criteria_used
        ));
        feedback.push_str("Это укрепляет вашу позицию и снижает эмоциональность.\n\n");
    } else {
        feedback.push_str("⚠️ Попробуйте опереться на объективные критерии: рыночную стоимость, профессиональные стандарты, прецеденты.\n\n");
    }

    // ─── Рекомендации ───
    feedback.push_str("## Рекомендации\n\n");
    let mut recs: Vec<&str> = Vec::new();

    if tech.spin_counts.implication < 2 {
        recs.push("Задавайте больше вопросов о последствиях (Implication)");
    }
    if tech.spin_counts.need_payoff < 2 {
        recs.push("Используйте Need-payoff вопросы для выявления ценности");
    }
    if tech.interest_focused < 2 {
        recs.push("Фокусируйтесь на интересах, а не на позициях");
    }
    if tech.objective_criteria_used < 1 {
        recs.push("Опирайтесь на объективные критерии");
    }
    if tech.collaboration_count < 2 {
        recs.push("Больше сотрудничества — меньше компромиссов");
    }

    for (i, rec) in recs.iter().enumerate() {
        feedback.push_str(&format!("{}. {}\n", i + 1, rec));
    }

    feedback
}
