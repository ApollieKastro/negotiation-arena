//! In-memory rate-limit для публичных auth-эндпоинтов (по IP).
//!
//! Фиксированное окно: не более `max` запросов на ключ за `window`.
//! `max = 0` — лимит отключён. Никакой персистентности: после рестарта
//! счётчики сбрасываются — для MVP это осознанный компромисс.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, Request, State};
use axum::http::header;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use parking_lot::Mutex;
use serde_json::json;

use crate::config::RateLimitConfig;

/// Счётчик одного ключа в текущем окне.
#[derive(Debug, Clone)]
struct Bucket {
    count: u32,
    reset_at: Instant,
}

/// Потокобезопасный лимитер: ключ → счётчик в фиксированном окне.
#[derive(Clone)]
pub struct RateLimiter {
    inner: Arc<Mutex<HashMap<String, Bucket>>>,
    max: u32,
    window: Duration,
}

impl RateLimiter {
    pub fn new(config: &RateLimitConfig) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            max: config.auth_max,
            window: Duration::from_secs(config.auth_window_secs.max(1)),
        }
    }

    /// true — запрос разрешён (счётчик увеличен); false — лимит исчерпан.
    /// `max == 0` — отключено, всегда true.
    pub fn check(&self, key: &str) -> bool {
        if self.max == 0 {
            return true;
        }
        let now = Instant::now();
        let mut map = self.inner.lock();

        // Простая уборка протухших ключей, чтобы карта не росла бесконечно.
        if map.len() > 10_000 {
            map.retain(|_, b| b.reset_at > now);
        }

        let bucket = map.entry(key.to_string()).or_insert(Bucket {
            count: 0,
            reset_at: now + self.window,
        });
        if now >= bucket.reset_at {
            bucket.count = 0;
            bucket.reset_at = now + self.window;
        }
        if bucket.count >= self.max {
            return false;
        }
        bucket.count += 1;
        true
    }

    /// Секунды до сброса окна для ключа (для `Retry-After`); минимум 1.
    pub fn retry_after_secs(&self, key: &str) -> u64 {
        let now = Instant::now();
        let map = self.inner.lock();
        map.get(key)
            .map(|b| b.reset_at.saturating_duration_since(now).as_secs().max(1))
            .unwrap_or(1)
    }
}

/// Клиентский IP: `X-Forwarded-For` (первый hop) → `ConnectInfo` → заглушка.
///
/// В oneshot-тестах `ConnectInfo` нет — ключ `"local"`, поэтому в тестовом
/// конфиге лимит по умолчанию отключён (`auth_max = 0`).
fn client_key(req: &Request) -> String {
    if let Some(xff) = req
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
    {
        if let Some(first) = xff.split(',').next() {
            let t = first.trim();
            if !t.is_empty() {
                return t.to_string();
            }
        }
    }
    if let Some(ConnectInfo(addr)) = req.extensions().get::<ConnectInfo<SocketAddr>>() {
        return addr.ip().to_string();
    }
    "local".into()
}

/// Middleware: rate-limit по IP, при превышении — 429 + `Retry-After`.
pub async fn rate_limit_auth(
    State(limiter): State<RateLimiter>,
    req: Request,
    next: Next,
) -> Response {
    let key = client_key(&req);
    if limiter.check(&key) {
        return next.run(req).await;
    }

    let retry = limiter.retry_after_secs(&key);
    tracing::warn!(ip = %key, retry_after = retry, "rate-limit: auth-запрос отклонён");
    (
        axum::http::StatusCode::TOO_MANY_REQUESTS,
        [(header::RETRY_AFTER, retry.to_string())],
        Json(json!({
            "error": format!("Слишком много попыток. Повторите через {retry} с.")
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limiter(max: u32, secs: u64) -> RateLimiter {
        RateLimiter::new(&RateLimitConfig {
            auth_max: max,
            auth_window_secs: secs,
        })
    }

    #[test]
    fn zero_max_always_allows() {
        let l = limiter(0, 60);
        for _ in 0..100 {
            assert!(l.check("ip1"));
        }
    }

    #[test]
    fn blocks_after_max_then_keys_are_independent() {
        let l = limiter(2, 60);
        assert!(l.check("a"));
        assert!(l.check("a"));
        assert!(!l.check("a"), "третий запрос в окне должен быть отклонён");
        assert!(l.check("b"), "другой ключ не зависит от первого");
        assert!(l.retry_after_secs("a") >= 1);
    }

    #[test]
    fn window_resets_after_expiry() {
        let l = RateLimiter::new(&RateLimitConfig {
            auth_max: 1,
            auth_window_secs: 1,
        });
        assert!(l.check("k"));
        assert!(!l.check("k"));
        std::thread::sleep(Duration::from_millis(1100));
        assert!(l.check("k"), "после окна счётчик должен сброситься");
    }
}
