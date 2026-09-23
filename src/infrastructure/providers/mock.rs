//! Демо-провайдер (mock): офлайн-собеседник и генератор сценариев без API-ключей.
//!
//! Назначен по умолчанию на роль `llm` при первом запуске, чтобы демонстрация
//! и разработка работали сразу (`cargo run` → играть). Как только администратор
//! назначает реальную LLM-модель, mock перестаёт использоваться.
//!
//! Поведение:
//! * **Диалог** — эвристические реплики на языке system-промпта (ru/en),
//!   с учётом сложности сценария (строка `Сложность сценария:` в промпте);
//! * **Генерация сценариев** — валидный JSON из вшитого шаблона (бриф
//!   подставляется в название/описание), чтобы `POST /scenarios/generate`
//!   работал без внешней LLM.

use async_trait::async_trait;
use serde_json::json;

use crate::domain::entities::model::{ModelDescriptor, ModelRole};
use crate::domain::ports::providers::{
    ChatModel, ChatRequest, ChatResponse, ChatRole, ModelCatalog, Usage,
};
use crate::error::AppResult;

/// Уникальный фрагмент промпта генератора сценариев (см. `GENERATOR_SYSTEM`).
const GENERATOR_MARK: &str = "редактор обучающих сценариев";

/// Офлайн-чат: детерминированные, но разнообразные ответы по истории.
#[derive(Debug, Clone, Default)]
pub struct MockChat {
    provider_name: String,
}

impl MockChat {
    pub fn new(provider_name: impl Into<String>) -> Self {
        Self {
            provider_name: provider_name.into(),
        }
    }

    /// Полный ответ: генератор сценариев или реплика собеседника.
    fn complete(&self, request: &ChatRequest) -> String {
        let system = request.system_text().unwrap_or_default();
        if system.to_lowercase().contains(GENERATOR_MARK) {
            return Self::canned_scenario(&system, request);
        }
        Self::partner_reply(&system, request)
    }

    // ── Диалог ──

    fn partner_reply(system: &str, request: &ChatRequest) -> String {
        let en = !system.chars().any(|c| c.is_alphabetic() && is_cyrillic(c));
        let difficulty = detect_difficulty(system);
        let turn = request
            .messages
            .iter()
            .filter(|m| m.role == ChatRole::Assistant)
            .count();
        let last_user = request
            .messages
            .iter()
            .rev()
            .find(|m| m.role == ChatRole::User)
            .map(|m| m.content.as_str())
            .unwrap_or("");

        let polite = matches_word(last_user, POLITE_HINTS);
        let confront = matches_word(last_user, CONFRONT_HINTS);
        let has_number = last_user.chars().any(|c| c.is_ascii_digit());
        let is_question = last_user.contains('?');

        // Ключ выбора шаблона: фаза диалога × реакция игрока.
        let bucket = if turn == 0 {
            0
        } else if confront {
            1
        } else if has_number {
            2
        } else if is_question {
            3
        } else if polite {
            4
        } else {
            5
        };

        let bank = if en { EN_TEMPLATES } else { RU_TEMPLATES };
        let difficulty_idx = match difficulty {
            "easy" => 0,
            "hard" => 2,
            _ => 1,
        };
        let phase = bank[difficulty_idx][bucket];
        // Детерминированный сдвиг внутри фазы — история не зацикливается.
        let idx = (turn + last_user.len()) % phase.len();
        phase[idx].to_string()
    }

    // ── Генерация сценариев ──

    fn canned_scenario(system: &str, request: &ChatRequest) -> String {
        let brief_raw = request
            .messages
            .iter()
            .find(|m| m.role == ChatRole::User)
            .map(|m| m.content.clone())
            .unwrap_or_else(|| "Новый сценарий".into());
        let en = !system.chars().any(|c| c.is_alphabetic() && is_cyrillic(c));
        // Сложность — из строки «Сложность: … (slug)» в user-сообщении, не из брифа.
        let difficulty = detect_difficulty(&brief_raw);
        let brief = extract_brief(&brief_raw);

        let scenario = if en {
            json!({
                "id": "",
                "title": title_from_brief(&brief, true),
                "description": format!("Role-play negotiation based on: {brief}. Both sides have real interests and BATNA."),
                "sphere": "Business",
                "difficulty": difficulty,
                "player_role": "Negotiator",
                "player_company": null,
                "player_goal": "Reach an agreement that meets the core goal",
                "player_batna": "Walk away and use an alternative partner",
                "partner_name": "Alex Morgan",
                "partner_role": "Counterpart",
                "partner_company": null,
                "partner_goal": "Secure favorable terms for their side",
                "partner_goals": ["Favorable terms", "Long-term cooperation"],
                "partner_batna": "Switch to another supplier",
                "partner_personality": {"tone": "pragmatic", "style": "measured", "traits": "cautious"},
                "opening_context": "Thanks for meeting. Let's discuss the terms for the upcoming deal.",
                "endings": [
                    {"id": "win", "title": "Great outcome", "text": "You reached a deal close to your goal.", "outcome": "Agreement signed", "min_score": 60},
                    {"id": "partial", "title": "Partial success", "text": "Partial agreement with trade-offs.", "outcome": "Terms agreed in part", "min_score": 25},
                    {"id": "fail", "title": "Talks collapsed", "text": "No agreement was reached.", "outcome": "No deal", "min_score": 0}
                ],
                "ai_generated": true,
                "is_active": false,
                "created_by": null,
                "created_at": "",
                "updated_at": null
            })
        } else {
            json!({
                "id": "",
                "title": title_from_brief(&brief, false),
                "description": format!("Ролевая переговорная игра по брифу: {brief}. У обеих сторон реальные интересы и BATNA."),
                "sphere": "Бизнес",
                "difficulty": difficulty,
                "player_role": "Переговорщик",
                "player_company": null,
                "player_goal": "Достичь соглашения, закрывающего ключевую цель",
                "player_batna": "Уйти и найти альтернативного партнёра",
                "partner_name": "Алексей Морозов",
                "partner_role": "Контрагент",
                "partner_company": null,
                "partner_goal": "Выторговать выгодные условия для своей стороны",
                "partner_goals": ["Выгодные условия", "Долгосрочное сотрудничество"],
                "partner_batna": "Перейти к конкуренту",
                "partner_personality": {"tone": "деловой", "style": "сдержанный", "traits": "осторожный"},
                "opening_context": "Здравствуйте! Давайте обсудим условия предстоящей сделки.",
                "endings": [
                    {"id": "win", "title": "Отличный результат", "text": "Вы договорились на условиях, близких к вашей цели.", "outcome": "Сделка заключена", "min_score": 60},
                    {"id": "partial", "title": "Частичный успех", "text": "Достигнута частичная договорённость с уступками.", "outcome": "Условия согласованы частично", "min_score": 25},
                    {"id": "fail", "title": "Переговоры сорвались", "text": "Соглашение достичь не удалось.", "outcome": "Сделки нет", "min_score": 0}
                ],
                "ai_generated": true,
                "is_active": false,
                "created_by": null,
                "created_at": "",
                "updated_at": null
            })
        };

        serde_json::to_string(&scenario).unwrap_or_else(|_| "{}".into())
    }

    /// Каталог моделей demo-провайдера.
    pub fn catalog(&self) -> MockCatalog {
        MockCatalog {
            provider_name: self.provider_name.clone(),
        }
    }
}

#[async_trait]
impl ChatModel for MockChat {
    async fn chat(&self, request: ChatRequest) -> AppResult<ChatResponse> {
        let content = self.complete(&request);
        let prompt_tokens: u32 = request
            .messages
            .iter()
            .map(|m| m.content.chars().count())
            .sum::<usize>()
            .div_ceil(4)
            .min(u32::MAX as usize) as u32;
        let completion_tokens = content.chars().count().div_ceil(4).min(u32::MAX as usize) as u32;
        Ok(ChatResponse {
            content,
            model: request.model,
            finish_reason: Some("stop".into()),
            usage: Usage {
                prompt_tokens: Some(prompt_tokens),
                completion_tokens: Some(completion_tokens),
                total_tokens: Some(prompt_tokens + completion_tokens),
            },
        })
    }
}

/// Discovery/`ping` demo-провайдера: один стабильный ключ модели.
#[derive(Debug, Clone)]
pub struct MockCatalog {
    provider_name: String,
}

#[async_trait]
impl ModelCatalog for MockCatalog {
    async fn list_models(&self, role: ModelRole) -> AppResult<Vec<ModelDescriptor>> {
        if role != ModelRole::Llm {
            return Ok(Vec::new());
        }
        Ok(vec![ModelDescriptor {
            model_key: "arena-mock".into(),
            display_name: "Демо-собеседник (офлайн)".into(),
            role,
            supports_streaming: false,
            notes: Some("Встроенный ответный движок без внешних API".into()),
        }])
    }

    async fn ping(&self) -> AppResult<()> {
        Ok(())
    }
}

// ── Вспомогательное ──

fn is_cyrillic(c: char) -> bool {
    matches!(c, '\u{0400}'..='\u{04FF}')
}

fn detect_difficulty(text: &str) -> &'static str {
    let lower = text.to_ascii_lowercase();
    if lower.contains("(hard)") || lower.contains("сложная") || lower.contains("hard") {
        "hard"
    } else if lower.contains("(easy)") || lower.contains("начальная") || lower.contains("easy")
    {
        "easy"
    } else {
        "medium"
    }
}

fn matches_word(haystack: &str, needles: &[&str]) -> bool {
    let lower = haystack.to_lowercase();
    needles.iter().any(|n| lower.contains(n))
}

const POLITE_HINTS: &[&str] = &[
    "пожалуйста",
    "давайте",
    "договор",
    "компромисс",
    "выгодн",
    "взаимн",
    "please",
    "let's",
    "deal",
    "compromise",
    "benefit",
    "mutual",
];
const CONFRONT_HINTS: &[&str] = &[
    "неприемлем",
    "не можем",
    "никогда",
    "уже нет",
    "уходим",
    "конкурент",
    "unacceptable",
    "never",
    "cannot",
    "can't",
    "leave",
    "competitor",
];

/// Банк реплик: [difficulty][bucket][variant].
/// bucket: 0=открытие, 1=давление, 2=цифры, 3=вопрос, 4=вежливо, 5=иначе.
type Phase = &'static [&'static str];
type DifficultyBank = [Phase; 6];
type ReplyBank = [DifficultyBank; 3];

const RU_TEMPLATES: ReplyBank = [
    // easy
    [
        &["Здравствуйте! Готов обсудить условия — надеюсь, найдём общий язык."],
        &[
            "Понимаю вашу позицию, но так резко уходить рано. Давайте посмотрим на варианты.",
            "Не спешите с выводами — я открыт к разговору, если аргументы весомые.",
        ],
        &[
            "Цифры выглядят интересными. Если подтвердите их данными, я готов смягчить условия.",
            "Хороший расчёт. При таких вводных я могу пойти навстречу.",
        ],
        &[
            "Хороший вопрос. По нашей стороне приоритет — стабильность объёмов, а не только цена.",
            "Давайте уточним: что для вас важнее — сроки или стоимость?",
        ],
        &[
            "Спасибо, так гораздо приятнее работать. Предложите конкретику — рассмотрю.",
            "Да, давайте ищем решение выгодное обоим. Что вы предлагаете?",
        ],
        &["Продолжайте — я слушаю и готов обсуждать детали."],
    ],
    // medium
    [
        &["Здравствуйте. Изложите ваше предложение — оценим по существу."],
        &[
            "Такие формулировки меня не убеждают. Нужны аргументы, а не давление.",
            "Я слышу вашу позицию, но пока оснований менять свою — мало.",
        ],
        &[
            "Если цифры подкреплены рынком, это меняет дело. Объясните методику расчёта.",
            "При обоснованных числах возможна корректировка, но не сразу и не полностью.",
        ],
        &[
            "Отвечу так: важнее предсказуемость поставок и понятный горизонт договора.",
            "Сначала выясню ваши интересы: почему именно это условие критично?",
        ],
        &[
            "Деловой тон — здорово. Предложите вариант, который устраивает обе стороны.",
            "Компромисс возможен, если каждая сторона понимает, чем жертвует.",
        ],
        &["Пока не вижу оснований для уступок. Убедьте меня фактами."],
    ],
    // hard
    [
        &["Добрый день. Сразу предупрежу: условия у нас жёсткие, разговор будет непростым."],
        &[
            "Ультиматумы не работают. Либо аргументы, либо мы на этом остановимся.",
            "Давление только ужесточает мою позицию. У меня есть альтернативы.",
        ],
        &[
            "Цифры есть — но этого мало. Нужен расчёт рисков и для моей стороны.",
            "Покажите, как именно это закрывает мою цель, иначе считайте разговор закрытым.",
        ],
        &[
            "Отвечу неудобно: без гарантий объёма мне невыгодно идти на уступки.",
            "Мой интерес — защитить позицию компании. Что вы предлагаете взамен?",
        ],
        &[
            "Сдержанно, но мало содержания. Мне нужны конкретные механизмы, не обещания.",
            "Вежливость приветствую, но уступок без выгоды для меня не будет.",
        ],
        &["Не вижу причин смягчаться. Работайте с моими условиями."],
    ],
];

const EN_TEMPLATES: ReplyBank = [
    // easy
    [
        &["Hello! I'm ready to discuss the terms — I'm sure we can find common ground."],
        &[
            "I hear you, but walking away so quickly seems premature. Let's look at options.",
            "Let's not rush to conclusions — I'm open if your arguments hold water.",
        ],
        &[
            "Those numbers look workable. If you back them with data, I can soften my terms.",
            "Good math. On those inputs I'm willing to meet you halfway.",
        ],
        &[
            "Fair question. On my side the priority is volume stability, not just price.",
            "Quick clarification: what matters more to you — timing or cost?",
        ],
        &[
            "Thanks — much easier to work this way. Give me something concrete and I'll consider it.",
            "Yes, let's find a solution that works for both of us. What do you propose?",
        ],
        &["Go on — I'm listening and ready to discuss the details."],
    ],
    // medium
    [
        &["Hello. Make your offer — we'll evaluate it on the merits."],
        &[
            "That framing doesn't convince me. I need arguments, not pressure.",
            "I hear your position, but I don't yet have grounds to change mine.",
        ],
        &[
            "If the figures are market-backed, that changes things. Explain the method.",
            "With solid numbers an adjustment is possible — but not all at once.",
        ],
        &[
            "My answer: predictability of supply and a clear contract horizon come first.",
            "Let's surface interests first: why exactly is this condition critical for you?",
        ],
        &[
            "Professional tone — good. Propose something that serves both sides.",
            "Compromise works when each side understands what it is giving up.",
        ],
        &["I still see no reason to concede. Convince me with facts."],
    ],
    // hard
    [
        &["Good day. Fair warning: our terms are firm and this will not be an easy talk."],
        &[
            "Ultimatums don't work. Either real arguments, or we stop here.",
            "Pressure only hardens my position. I have alternatives.",
        ],
        &[
            "Numbers are a start — not enough. I need risk calculus for my side as well.",
            "Show how this actually serves my goal, or consider the conversation closed.",
        ],
        &[
            "Uncomfortable answer: without volume guarantees, concessions are not in my interest.",
            "My interest is protecting my company's position. What do you offer in return?",
        ],
        &[
            "Polite, but thin on substance. I need concrete mechanisms, not promises.",
            "I appreciate the courtesy, but there will be no concessions without benefit to me.",
        ],
        &["I see no reason to soften. Work with my terms."],
    ],
];

fn extract_brief(user_message: &str) -> String {
    // Формат промпта генератора: «…\nБриф:\n<текст>».
    user_message
        .rsplit_once("Бриф:")
        .map(|(_, tail)| tail.trim().to_string())
        .or_else(|| {
            user_message
                .rsplit_once("Brief:")
                .map(|(_, t)| t.trim().to_string())
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| user_message.trim().to_string())
}

fn title_from_brief(brief: &str, en: bool) -> String {
    let clean: String = brief
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == ' ' {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let mut title: String = clean.chars().take(80).collect();
    if title.trim().is_empty() {
        title = if en {
            "New negotiation scenario"
        } else {
            "Новый переговорный сценарий"
        }
        .into();
    }
    title
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ports::providers::ChatMessage;

    fn dialogue_request(system: &str, user: &str, assistant_turns: usize) -> ChatRequest {
        let mut messages = vec![ChatMessage::system(system)];
        for i in 0..assistant_turns {
            let _ = i;
            messages.push(ChatMessage::assistant("ранее"));
            messages.push(ChatMessage::user("ладно"));
        }
        messages.push(ChatMessage::user(user));
        ChatRequest::new("arena-mock", messages)
    }

    #[tokio::test]
    async fn mock_chat_answers_ru_dialogue() {
        let chat = MockChat::new("Demo");
        let req = dialogue_request(
            "Ты — собеседник в тренажёре. Сложность сценария: Средняя (medium).",
            "Давайте найдём решение, выгодное для обеих сторон?",
            1,
        );
        let resp = chat.chat(req).await.unwrap();
        assert!(!resp.content.is_empty());
        assert!(
            resp.content.chars().any(is_cyrillic),
            "ожидали русский ответ"
        );
        assert!(resp.usage.total_tokens.unwrap() > 0);
    }

    #[tokio::test]
    async fn mock_chat_answers_en_dialogue() {
        let chat = MockChat::new("Demo");
        let req = dialogue_request(
            "You are a negotiation partner. Difficulty: medium.",
            "Can we find a mutual benefit?",
            0,
        );
        let resp = chat.chat(req).await.unwrap();
        assert!(!resp.content.is_empty());
        assert!(
            !resp.content.chars().any(is_cyrillic),
            "ожидали английский ответ: {}",
            resp.content
        );
    }

    #[tokio::test]
    async fn mock_chat_generates_valid_scenario_json() {
        let chat = MockChat::new("Demo");
        let system =
            "Ты — редактор обучающих сценариев тренажёра переговоров (SPIN, Гарвардский метод).";
        let req = ChatRequest::new(
            "arena-mock",
            vec![
                ChatMessage::system(system),
                ChatMessage::user(
                    "Сложность: Сложная (hard)\nСфера: Продажи\nБриф:\nПереговоры о зарплате",
                ),
            ],
        );
        let resp = chat.chat(req).await.unwrap();
        let value: serde_json::Value = serde_json::from_str(&resp.content).expect("валидный JSON");
        assert!(value["player_goal"].as_str().is_some());
        assert!(value["endings"].as_array().is_some_and(|e| e.len() == 3));
        assert_eq!(value["difficulty"], "hard");
        let title = value["title"].as_str().unwrap_or_default();
        assert!(title.to_lowercase().contains("зарплат") || !title.is_empty());
    }

    #[tokio::test]
    async fn mock_catalog_lists_llm_model_only() {
        let catalog = MockChat::new("Demo").catalog();
        assert!(catalog.ping().await.is_ok());
        let llm = catalog.list_models(ModelRole::Llm).await.unwrap();
        assert_eq!(llm.len(), 1);
        assert_eq!(llm[0].model_key, "arena-mock");
        let tts = catalog.list_models(ModelRole::Tts).await.unwrap();
        assert!(tts.is_empty());
    }
}
