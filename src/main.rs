//! Точка входа: сборка зависимостей и запуск сервера.
//!
//! Тонкий composition root: собирает конфигурацию, БД, шифрование и роуты.
//! Вся бизнес-логика живёт в слоях `domain`, `application`, `infrastructure`.
//
// Этапы 0–8 доставляются порциями: контракты домена и схема БД появляются
// раньше их потребителей. Снимаем этот allow на этапе 8 (hardening),
// когда всё задействовано.
#![allow(dead_code)]

mod application;
mod config;
mod domain;
mod error;
mod infrastructure;
mod web;

use std::sync::Arc;

use crate::config::AppConfig;
use crate::error::AppResult;
use crate::infrastructure::{crypto::SecretCipher, db::Database};
use crate::web::{routes, state::AppState};

#[tokio::main]
async fn main() -> AppResult<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,axum=info,tower_http=info".into()),
        )
        .init();

    let config = AppConfig::from_env()?;
    tracing::info!(
        db = %config.storage.db_path.display(),
        port = config.server.port,
        "конфигурация загружена"
    );

    let db = Arc::new(Database::open(&config.storage.db_path)?);
    let applied = db.run_migrations()?;
    if applied > 0 {
        tracing::info!(count = applied, "применены миграции БД");
    }
    infrastructure::db::seeds::run(&db)?;

    let cipher = Arc::new(SecretCipher::from_secret(
        &config.security.encryption_secret,
    )?);

    let repos = Arc::new(infrastructure::db::repos::SqliteRepos::new(db.clone()));
    let services = Arc::new(application::Services::new(&config, repos, cipher.clone())?);

    let state = AppState {
        config: Arc::new(config.clone()),
        db,
        cipher,
        services,
    };

    let app = routes::build(state);

    let addr = format!("{}:{}", config.server.host, config.server.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("сервер слушает http://{addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!("сервер остановлен");
    Ok(())
}

/// Ожидает SIGINT/SIGTERM для graceful shutdown.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("не удалось установить обработчик SIGINT");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("не удалось установить обработчик SIGTERM")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("получен SIGINT"),
        _ = terminate => tracing::info!("получен SIGTERM"),
    }
}
