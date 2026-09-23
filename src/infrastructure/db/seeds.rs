//! Начальные данные: роли, администратор, базовые сценарии.
//!
//! Запускается на старте, идемпотентно: повторный запуск ничего не меняет.

use std::sync::Arc;

use crate::domain::entities::model::{ModelRole, ProviderKind};
use crate::domain::entities::provider::{ModelRecord, Provider};
use crate::domain::entities::scenario::{Difficulty, Ending, PartnerPersonality, Scenario};
use crate::domain::entities::user::UserRole;
use crate::domain::ports::{
    AuditRepository, ProviderRepository, ScenarioRepository, SettingsRepository, UserRepository,
};
use crate::error::AppResult;
use crate::infrastructure::crypto::password::hash_password;
use crate::infrastructure::db::{repos::SqliteRepos, Database};

/// Фиксированные id демо-провайдера (идемпотентный сид).
const DEMO_PROVIDER_ID: &str = "demo-mock";
const DEMO_MODEL_ID: &str = "demo-mock-llm";

/// Фиксированные id локального Ollama (сид включается `OLLAMA_BASE_URL`).
const OLLAMA_PROVIDER_ID: &str = "ollama";
const OLLAMA_MODEL_ID: &str = "ollama-llm";

/// Применяет сиды: администратор (при отсутствии), базовые сценарии,
/// демо LLM (mock) без API-ключей; при заданном `OLLAMA_BASE_URL` — Ollama.
///
/// Роли и настройки создаются миграциями, здесь — только то,
/// что требует логики (хеш пароля, структуры сценариев, назначение demo).
pub fn run(db: &Arc<Database>) -> AppResult<()> {
    let repos = SqliteRepos::new(db.clone());
    seed_admin(&repos)?;
    seed_scenarios(&repos)?;
    seed_mock_llm(&repos)?;
    seed_ollama(&repos)?;
    Ok(())
}

fn seed_admin(repos: &SqliteRepos) -> AppResult<()> {
    if repos.users.by_login("admin")?.is_some() {
        return Ok(());
    }

    let password = std::env::var("ADMIN_PASSWORD").unwrap_or_else(|_| "admin123".to_string());
    let hash = hash_password(&password);
    let user = repos
        .users
        .create("admin", &hash, UserRole::Admin, Some("Администратор"))?;

    repos.audit.append(
        Some(&user.id),
        "seed.admin_created",
        Some("users"),
        Some(&user.id),
        None,
    )?;

    tracing::info!(login = %user.login, "создан начальный администратор");
    Ok(())
}

fn seed_scenarios(repos: &SqliteRepos) -> AppResult<()> {
    if repos.scenarios.count()? > 0 {
        return Ok(());
    }

    for scenario in default_scenarios() {
        repos.scenarios.upsert(&scenario)?;
    }
    tracing::info!(
        count = default_scenarios().len(),
        "загружены сценарии по умолчанию"
    );
    Ok(())
}

/// Демо-режим: mock-провайдер + LLM-модель, назначение роли `llm`,
/// если ещё ничего не назначено. Работает без API-ключей (`cargo run` → играть).
fn seed_mock_llm(repos: &SqliteRepos) -> AppResult<()> {
    let now = chrono::Utc::now().to_rfc3339();

    if repos.providers.get(DEMO_PROVIDER_ID)?.is_none() {
        repos.providers.upsert(&Provider {
            id: DEMO_PROVIDER_ID.into(),
            name: "Демо (офлайн)".into(),
            kind: ProviderKind::Mock,
            base_url: None,
            api_key_encrypted: None,
            api_key_hint: None,
            is_enabled: true,
            created_at: now.clone(),
            updated_at: None,
        })?;
    }

    if repos.providers.get_model(DEMO_MODEL_ID)?.is_none() {
        repos.providers.upsert_model(&ModelRecord {
            id: DEMO_MODEL_ID.into(),
            provider_id: DEMO_PROVIDER_ID.into(),
            role: ModelRole::Llm,
            model_key: "arena-mock".into(),
            display_name: "Демо-собеседник (офлайн)".into(),
            is_enabled: true,
            metadata: serde_json::json!({
                "notes": "Встроенный ответный движок без внешних API"
            }),
            created_at: now,
        })?;
    }

    // Назначаем только если роли llm ещё нет: админ может снять назначение,
    // и мы не будем силой возвращать mock при каждом рестарте после снятия
    // (первый запуск — assign; далее — только если пусто и не снимали вручную
    // до фиксации... см. флаг ниже).
    let assignments = repos.providers.role_assignments()?;
    let has_llm = assignments.iter().any(|a| a.role == ModelRole::Llm);
    if !has_llm {
        // Флаг: demo-назначение делали мы и снимали вручную — не восстанавливаем.
        let took_over = repos.settings.get("platform.demo_llm_assigned")?.is_some();
        if !took_over {
            repos.providers.set_role_assignment("llm", DEMO_MODEL_ID)?;
            repos.settings.set("platform.demo_llm_assigned", "1")?;
            tracing::info!(
                model = DEMO_MODEL_ID,
                "назначена демо LLM (mock) на роль llm"
            );
        }
    } else if repos.settings.get("platform.demo_llm_assigned")?.is_none() {
        // Уже есть чужое/реальное назначение — просто фиксируем, что seed идёт.
        repos.settings.set("platform.demo_llm_assigned", "1")?;
    }
    Ok(())
}

/// Локальный Ollama: провайдер `openai_compatible` + модель, без API-ключа.
///
/// Включается только при заданном `OLLAMA_BASE_URL` (пусто/нет — сид пропускается).
/// Модель: `OLLAMA_MODEL` (default `qwen3.5:4b`).
/// Назначение роли `llm`:
/// * `OLLAMA_SEED_ASSIGN=1` — всегда переключать на Ollama при старте;
/// * иначе — только если роли `llm` ещё нет.
fn seed_ollama(repos: &SqliteRepos) -> AppResult<()> {
    let base = match std::env::var("OLLAMA_BASE_URL") {
        Ok(v) if !v.trim().is_empty() => v.trim().trim_end_matches('/').to_string(),
        _ => return Ok(()),
    };
    let model_key = std::env::var("OLLAMA_MODEL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "qwen3.5:4b".into())
        .trim()
        .to_string();
    let force = matches!(
        std::env::var("OLLAMA_SEED_ASSIGN").as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    );
    seed_ollama_with(repos, &base, &model_key, force)
}

/// См. [`seed_ollama`]: параметризованная часть (тестируется без env).
fn seed_ollama_with(
    repos: &SqliteRepos,
    base: &str,
    model_key: &str,
    force_assign: bool,
) -> AppResult<()> {
    let base = base.trim().trim_end_matches('/');
    let model_key = model_key.trim();
    if base.is_empty() || model_key.is_empty() {
        return Ok(());
    }

    // OpenAI-совместимый endpoint Ollama: `/v1` + пустой ключ (локальный base_url).
    let now = chrono::Utc::now().to_rfc3339();
    let existing = repos.providers.get(OLLAMA_PROVIDER_ID)?;
    let needs_fix = existing.as_ref().is_some_and(|p| {
        p.kind != ProviderKind::OpenAiCompatible
            || p.base_url.as_deref() != Some(base)
            || !p.is_enabled
    });
    if existing.is_none() || needs_fix {
        repos.providers.upsert(&Provider {
            id: OLLAMA_PROVIDER_ID.into(),
            name: "Ollama".into(),
            kind: ProviderKind::OpenAiCompatible,
            base_url: Some(base.to_string()),
            api_key_encrypted: None,
            api_key_hint: None,
            is_enabled: true,
            created_at: existing
                .as_ref()
                .map(|p| p.created_at.clone())
                .unwrap_or_else(|| now.clone()),
            updated_at: Some(now.clone()),
        })?;
        tracing::info!(base = %base, "сид: провайдер Ollama (openai_compatible, без ключа)");
    }

    if let Some(mut model) = repos.providers.get_model(OLLAMA_MODEL_ID)? {
        // Модель уже есть — синхронизируем model_key, если сменили OLLAMA_MODEL.
        if model.model_key != model_key {
            model.model_key = model_key.to_string();
            model.display_name = format!("Ollama · {model_key}");
            repos.providers.upsert_model(&model)?;
        }
    } else {
        repos.providers.upsert_model(&ModelRecord {
            id: OLLAMA_MODEL_ID.into(),
            provider_id: OLLAMA_PROVIDER_ID.into(),
            role: ModelRole::Llm,
            model_key: model_key.to_string(),
            display_name: format!("Ollama · {model_key}"),
            is_enabled: true,
            metadata: serde_json::json!({
                "notes": "Локальный Ollama (OpenAI-совместимый /v1, без API-ключа)"
            }),
            created_at: now,
        })?;
    }

    let assignments = repos.providers.role_assignments()?;
    let has_llm = assignments.iter().any(|a| a.role == ModelRole::Llm);
    let already = assignments
        .iter()
        .any(|a| a.role == ModelRole::Llm && a.model_id == OLLAMA_MODEL_ID);

    if already {
        return Ok(());
    }
    if force_assign || !has_llm {
        // force: админ явно просит Ollama; иначе — только пустая роль.
        repos
            .providers
            .set_role_assignment("llm", OLLAMA_MODEL_ID)?;
        tracing::info!(
            model = OLLAMA_MODEL_ID,
            key = %model_key,
            force = force_assign,
            "назначена Ollama LLM на роль llm"
        );
    }
    Ok(())
}

fn default_scenarios() -> Vec<Scenario> {
    let now = chrono::Utc::now().to_rfc3339();

    let make = |id: &str,
                title: &str,
                description: &str,
                sphere: &str,
                difficulty: Difficulty,
                player_role: &str,
                player_goal: &str,
                player_batna: &str,
                partner_name: &str,
                partner_role: &str,
                partner_goal: &str,
                partner_goals: Vec<&str>,
                partner_batna: &str,
                opening_context: &str,
                endings: Vec<(&str, &str, &str, &str, i32)>|
     -> Scenario {
        Scenario {
            id: id.to_string(),
            title: title.to_string(),
            description: description.to_string(),
            sphere: sphere.to_string(),
            difficulty,
            player_role: player_role.to_string(),
            player_company: None,
            player_goal: player_goal.to_string(),
            player_batna: player_batna.to_string(),
            partner_name: partner_name.to_string(),
            partner_role: partner_role.to_string(),
            partner_company: None,
            partner_goal: partner_goal.to_string(),
            partner_goals: partner_goals.into_iter().map(str::to_string).collect(),
            partner_batna: partner_batna.to_string(),
            partner_personality: PartnerPersonality::default(),
            opening_context: opening_context.to_string(),
            endings: endings
                .into_iter()
                .map(|(id, title, text, outcome, min_score)| Ending {
                    id: id.to_string(),
                    title: title.to_string(),
                    text: text.to_string(),
                    outcome: outcome.to_string(),
                    min_score,
                })
                .collect(),
            ai_generated: false,
            is_active: true,
            created_by: None,
            created_at: now.clone(),
            updated_at: None,
        }
    };

    vec![
        make(
            "sales_easy",
            "Продажи: Скидка для клиента",
            "Клиент запрашивает скидку на продукт. Найдите баланс между сохранением маржи и удовлетворением клиента.",
            "Продажи",
            Difficulty::Easy,
            "Менеджер по продажам",
            "Заключить контракт на приемлемых условиях, сохранив клиента",
            "Найти другого клиента на аналогичный объём",
            "Дмитрий Козлов",
            "Директор по закупкам",
            "Получить скидку 15-20%",
            vec!["Получить скидку 15-20%", "Обеспечить долгосрочное сотрудничество", "Получить гарантию и сервис"],
            "Обратиться к конкурентам (˄ цена, но ˅ условия)",
            "Здравствуйте! Я рассматривал ваше предложение, но цена слишком высока. Нам нужны скидки, иначе придётся искать других поставщиков.",
            vec![
                ("win", "Отличный результат", "Вы нашли общий язык с клиентом и заключили выгодную сделку!", "Клиент подписывает контракт на выгодных условиях", 50),
                ("partial", "Частичный успех", "Переговоры завершились, но не все цели достигнуты.", "Клиент уходит, но оставляет заявку", 20),
                ("fail", "Неудача", "Не удалось найти компромисс.", "Клиент уходит к конкурентам", 0),
            ],
        ),
        make(
            "hr_medium",
            "HR: Собеседование на руководителя",
            "Вы проходите собеседование на должность руководителя отдела. HR-директор оценивает ваши компетенции.",
            "HR",
            Difficulty::Medium,
            "Кандидат на руководящую должность",
            "Получить оффер на должность руководителя IT-отдела",
            "Остаться на текущей позиции или принять предложение от конкурента",
            "Елена Петрова",
            "HR-директор",
            "Найти руководителя, который закроет текучку в отделе",
            vec!["Оценить лидерские качества", "Понять мотивацию кандидата", "Проверить соответствие культуре"],
            "Найти кандидата изнутри компании или через агентство",
            "Расскажите о вашем опыте управления командой. Почему вы считаете, что справитесь с этой ролью?",
            vec![
                ("hire", "Оффер получен", "Вас рекомендуют на должность!", "Оффер с ожидаемой зарплатой", 45),
                ("wait", "Лист ожидания", "Решение отложено — сравнивают с другими кандидатами.", "Звонок через неделю", 20),
                ("reject", "Отказ", "К сожалению, вы не подходите.", "Письмо с отказом", 0),
            ],
        ),
        make(
            "renewal_easy",
            "Продажи: Продление контракта",
            "Клиент продлевает контракт, но недоволен ростом цен. Сохраните отношения и предложите решение.",
            "Продажи",
            Difficulty::Easy,
            "Менеджер по работе с клиентами",
            "Продлить контракт, сохранив объём и отношения",
            "Предложить скидку на продление или найти нового клиента",
            "Андрей Волков",
            "Коммерческий директор",
            "Снизить рост расходов при продлении",
            vec!["Снизить рост расходов", "Сохранить качество сервиса", "Получить предсказуемость бюджета"],
            "Перейти к конкуренту или сократить объём закупок",
            "Наш контракт истекает через месяц. Мы довольны сервисом, но цена выросла на 20%. Нужно обсудить условия продления.",
            vec![
                ("renew", "Контракт продлён", "Клиент согласен на новые условия!", "Продление на 2 года", 30),
                ("partial", "Частичное согласие", "Клиент продлевает, но с уменьшенным объёмом.", "Снижение объёма на 30%", 10),
                ("lost", "Клиент уходит", "Не удалось договориться.", "Расторжение контракта", 0),
            ],
        ),
        make(
            "procurement_medium",
            "Закупки: Выбор поставщика",
            "Вы — закупщик, конкурирующие поставщики предлагают разные условия. Оцените каждое предложение.",
            "Закупки",
            Difficulty::Medium,
            "Менеджер по закупкам",
            "Выбрать поставщика с лучшей совокупной стоимостью владения",
            "Выбрать конкурента или провести тендер",
            "Сергей Иванов",
            "Менеджер по продажам конкурента",
            "Получить контракт ценой и условиями",
            vec!["Получить контракт", "Предложить лучшую цену", "Обеспечить долгосрочное сотрудничество"],
            "Предложить скидку или улучшить условия для другого клиента",
            "Мы получили более выгодное предложение от вашего конкурента. Готовы ли вы пересмотреть условия?",
            vec![
                ("deal", "Сделка состоялась", "Вы выбрали лучшее предложение!", "Подписание контракта", 25),
                ("delay", "Отсрочка", "Решение отложено — нужно дополнительное сравнение.", "Повторный тендер через месяц", 10),
                ("cancel", "Тендер отменён", "Условия не устроили ни одну сторону.", "Поиск альтернативных решений", 0),
            ],
        ),
        make(
            "partnership_hard",
            "Продажи: Долгосрочное партнёрство",
            "Обсуждение условий партнёрства с эксклюзивностью. Сложные переговоры с высокими ставками.",
            "Продажи",
            Difficulty::Hard,
            "Руководитель отдела продаж",
            "Заключить партнёрство без чрезмерных эксклюзивных ограничений",
            "Найти другого партнёра без эксклюзивных обязательств",
            "Ольга Сидорова",
            "Коммерческий директор",
            "Получить эксклюзивность в регионе",
            vec!["Получить эксклюзивность", "Обеспечить долгосрочное сотрудничество", "Получить конкурентные цены"],
            "Работать с конкурентом или расширить собственное производство",
            "Мы заинтересованы в партнёрстве, но хотим эксклюзивность в нашем регионе. Это принципиально.",
            vec![
                ("exclusive", "Эксклюзивное партнёрство", "Заключено соглашение на 3 года!", "Эксклюзив в регионе + план роста", 45),
                ("partial", "Частичное партнёрство", "Договорились о сотрудничестве без эксклюзива.", "Работа на общих условиях", 20),
                ("fail", "Переговоры сорваны", "Не удалось найти общего языка.", "Партнёр уходит к конкуренту", 0),
            ],
        ),
        make(
            "management_hard",
            "Управление: Конфликт ресурсов",
            "Конфликт между двумя руководителями из-за ресурсов. Нужно найти решение на уровне компании.",
            "Управление",
            Difficulty::Hard,
            "Руководитель IT-отдела",
            "Договориться о распределении ресурсов без эскалации",
            "Обратиться к генеральному директору для разрешения конфликта",
            "Алексей Морозов",
            "Руководитель проектного офиса",
            "Получить людей под свой дедлайн",
            vec!["Получить людей для дедлайна", "Добиться справедливого распределения", "Защитить свой проект"],
            "Сорвать дедлайн и обвинить вас в провале проекта",
            "Мой отдел не получает достаточно ресурсов. Вы забираете людей и бюджет. Это несправедливо.",
            vec![
                ("deal", "Соглашение достигнуто", "Вы договорились о взаимной помощи!", "Формальное соглашение между отделами", 35),
                ("escalate", "Эскалация", "Конфликт передан руководству.", "Совещание с генеральным директором", 15),
                ("conflict", "Конфликт обострился", "Отношения испорчены, оба отдела страдают.", "Кадровые перестановки", 0),
            ],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ports::ScenarioRepository;

    #[test]
    fn seeds_are_idempotent() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        db.run_migrations().unwrap();

        run(&db).unwrap();
        let first = repos_count(&db);
        assert_eq!(first, 6, "должно быть 6 сценариев");

        run(&db).unwrap();
        assert_eq!(
            repos_count(&db),
            first,
            "повторный сид не должен дублировать"
        );
    }

    #[test]
    fn seeds_create_demo_mock_llm_and_assign_once() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        db.run_migrations().unwrap();
        run(&db).unwrap();

        let repos = SqliteRepos::new(db.clone());
        let provider = repos.providers.get(DEMO_PROVIDER_ID).unwrap();
        assert!(provider.is_some(), "demo-mock provider должен существовать");
        let model = repos.providers.get_model(DEMO_MODEL_ID).unwrap();
        assert!(model.is_some(), "demo-mock model должна существовать");
        let assignments = repos.providers.role_assignments().unwrap();
        assert!(
            assignments
                .iter()
                .any(|a| a.role == ModelRole::Llm && a.model_id == DEMO_MODEL_ID),
            "llm должен быть назначен на demo-mock: {assignments:?}"
        );

        // Повторный запуск не дублирует назначение и не ломает id.
        run(&db).unwrap();
        let assignments2 = repos.providers.role_assignments().unwrap();
        assert_eq!(assignments.len(), assignments2.len());
        // demo-mock + (опционально) Ollama: OLLAMA_BASE_URL в тестовом окружении пуст → только demo.
        assert_eq!(
            repos.providers.list().unwrap().len(),
            1,
            "без OLLAMA_BASE_URL — только demo-mock"
        );
    }

    #[test]
    fn ollama_seed_skipped_without_env() {
        // Без OLLAMA_BASE_URL сид Ollama не создаёт провайдер (env в тестах не задаём).
        let db = Arc::new(Database::open_in_memory().unwrap());
        db.run_migrations().unwrap();
        run(&db).unwrap();
        let repos = SqliteRepos::new(db.clone());
        assert!(
            repos.providers.get(OLLAMA_PROVIDER_ID).unwrap().is_none(),
            "без OLLAMA_BASE_URL провайдер ollama не должен появиться"
        );
    }

    #[test]
    fn ollama_seed_creates_keyless_provider_and_assigns_when_forced() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        db.run_migrations().unwrap();
        run(&db).unwrap(); // demo-mock назначен на llm

        let repos = SqliteRepos::new(db.clone());
        seed_ollama_with(&repos, "http://127.0.0.1:11434/v1/", "qwen3.5:4b", true).unwrap();

        let p = repos.providers.get(OLLAMA_PROVIDER_ID).unwrap().unwrap();
        assert_eq!(p.kind, ProviderKind::OpenAiCompatible);
        assert_eq!(p.base_url.as_deref(), Some("http://127.0.0.1:11434/v1"));
        assert!(p.api_key_encrypted.is_none(), "Ollama без API-ключа в БД");
        assert!(!p.requires_api_key(), "локальный base_url — keyless");

        let m = repos.providers.get_model(OLLAMA_MODEL_ID).unwrap().unwrap();
        assert_eq!(m.model_key, "qwen3.5:4b");
        assert_eq!(m.role, ModelRole::Llm);

        let assignments = repos.providers.role_assignments().unwrap();
        assert!(
            assignments
                .iter()
                .any(|a| a.role == ModelRole::Llm && a.model_id == OLLAMA_MODEL_ID),
            "force_assign должен переключить llm: {assignments:?}"
        );

        // Идемпотентно: повторный сид не ломает и не дублирует.
        seed_ollama_with(&repos, "http://127.0.0.1:11434/v1", "qwen3.5:4b", false).unwrap();
        assert_eq!(
            repos.providers.list().unwrap().len(),
            2,
            "demo-mock + ollama"
        );
    }

    #[test]
    fn ollama_seed_without_force_keeps_existing_llm() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        db.run_migrations().unwrap();
        run(&db).unwrap(); // demo-mock на llm

        let repos = SqliteRepos::new(db.clone());
        seed_ollama_with(&repos, "http://127.0.0.1:11434/v1", "qwen3.5:4b", false).unwrap();

        let assignments = repos.providers.role_assignments().unwrap();
        assert!(
            assignments
                .iter()
                .any(|a| a.role == ModelRole::Llm && a.model_id == DEMO_MODEL_ID),
            "без force_assign не трогаем чужое назначение: {assignments:?}"
        );
        assert!(
            repos.providers.get(OLLAMA_PROVIDER_ID).unwrap().is_some(),
            "провайдер при этом создан"
        );
    }

    #[test]
    fn seed_does_not_restore_cleared_demo_assignment() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        db.run_migrations().unwrap();
        run(&db).unwrap();

        let repos = SqliteRepos::new(db.clone());
        repos.providers.clear_role_assignment("llm").unwrap();
        run(&db).unwrap();
        let assignments = repos.providers.role_assignments().unwrap();
        assert!(
            assignments.iter().all(|a| a.role != ModelRole::Llm),
            "после ручного снятия seed не должен восстанавливать llm"
        );
    }

    fn repos_count(db: &Arc<Database>) -> usize {
        SqliteRepos::new(db.clone()).scenarios.count().unwrap() as usize
    }
}
