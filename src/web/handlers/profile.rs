//! Свой профиль: логин/имя, пароль, аватар (self-service, любой вошедший).
//!
//! Эндпоинты трогают только учётную запись вызывающего (`actor.user_id`):
//! чужой профиль через этот модуль недоступен — RBAC внутри `AuthService`.

use axum::extract::State;
use axum::http::{header, HeaderValue};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;

use crate::domain::entities::user::User;
use crate::error::{AppError, AppResult};
use crate::web::middleware::{AppJson, AppMultipart, AuthUser};
use crate::web::state::AppState;

/// Лимит тела под аватар: 1 МиB файла + запас на multipart-обвязку.
/// Навешивается на `/profile/avatar` в `routes` (см. [`AVATAR_BODY_LIMIT`]).
pub const AVATAR_BODY_LIMIT: usize = 2 * 1024 * 1024;

#[derive(Debug, Deserialize)]
pub struct UpdateProfileRequest {
    pub login: String,
    /// Явное поле: `null` — очистить имя, строка — записать.
    /// Отсутствие поля — ошибка валидации (клиент обязан быть явным).
    pub display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

/// `GET /api/v1/profile` — свежий профиль текущего пользователя.
pub async fn get(State(state): State<AppState>, user: AuthUser) -> AppResult<Json<User>> {
    let profile = state.services.auth.own_profile(user.context())?;
    Ok(Json(profile))
}

/// `PATCH /api/v1/profile` — смена логина и/или отображаемого имени.
pub async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    AppJson(req): AppJson<UpdateProfileRequest>,
) -> AppResult<Json<User>> {
    // `Option<String>` из тела: null → None (очистить), строка → Some.
    // Сервис ждёт двойной Option: Some(...) — действие, None — «не трогать»;
    // здесь поле всегда присутствует → всегда Some.
    let display_name = Some(req.display_name.as_deref());
    let profile =
        state
            .services
            .auth
            .update_own_profile(user.context(), &req.login, display_name)?;
    Ok(Json(profile))
}

/// `PUT /api/v1/profile/password` — смена пароля (нужен текущий).
///
/// Argon2 синхронный и дорогой: уводим в `spawn_blocking`.
pub async fn change_password(
    State(state): State<AppState>,
    user: AuthUser,
    AppJson(req): AppJson<ChangePasswordRequest>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = state.services.auth.clone();
    let ctx = user.context().clone();
    let current = req.current_password;
    let next = req.new_password;
    tokio::task::spawn_blocking(move || auth.change_own_password(&ctx, &current, &next))
        .await
        .map_err(|e| AppError::internal(format!("смена пароля прервана: {e}")))??;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `PUT /api/v1/profile/avatar` — загрузка аватара (`multipart`, поле `file`).
pub async fn upload_avatar(
    State(state): State<AppState>,
    user: AuthUser,
    AppMultipart(mut multipart): AppMultipart,
) -> AppResult<Json<User>> {
    let mut file: Option<(String, Vec<u8>)> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(format!("чтение multipart: {e}")))?
    {
        if field.name() != Some("file") {
            continue;
        }
        let mime = field.content_type().map(str::to_string).unwrap_or_default();
        let bytes = field
            .bytes()
            .await
            .map_err(|e| AppError::BadRequest(format!("чтение файла: {e}")))?;
        file = Some((mime, bytes.to_vec()));
        break;
    }

    let (mime, data) =
        file.ok_or_else(|| AppError::BadRequest("в форме нет поля file с изображением".into()))?;
    let profile = state
        .services
        .auth
        .set_own_avatar(user.context(), &mime, &data)?;
    Ok(Json(profile))
}

/// `DELETE /api/v1/profile/avatar` — удалить аватар (вернуть инициалы).
pub async fn remove_avatar(State(state): State<AppState>, user: AuthUser) -> AppResult<Json<User>> {
    let profile = state.services.auth.clear_own_avatar(user.context())?;
    Ok(Json(profile))
}

/// `GET /api/v1/users/:id/avatar` — картинка аватара (бинарный ответ).
///
/// Любой вошедший: шапка, лидерборд и список юзеров рисуют чужие аватары.
/// Кэш `private`: картинка может меняться, но в рамках сессии актуальна.
pub async fn avatar_response(
    State(state): State<AppState>,
    user: AuthUser,
    id: String,
) -> AppResult<Response> {
    let _ = user; // аутентификация уже выполнена extractor'ом AuthUser
    let Some((mime, data)) = state.services.auth.get_avatar(&id)? else {
        return Err(AppError::NotFound("аватар не найден".into()));
    };
    let content_type = HeaderValue::from_str(&mime)
        .map_err(|_| AppError::internal("некорректный MIME аватара"))?;
    // 200 OK — статус по умолчанию у IntoResponse для кортежа (headers, body).
    Ok((
        [
            (header::CONTENT_TYPE, content_type),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static("private, max-age=300"),
            ),
        ],
        data,
    )
        .into_response())
}
