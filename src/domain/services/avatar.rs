//! Валидация аватара пользователя: размер, MIME, сигнатура файла.
//!
//! Чистая логика без I/O — используется `AuthService` перед записью в БД.
//! Ложный `Content-Type` отбрасывается проверкой магических байт: заявленный
//! MIME обязан соответствовать реальному формату файла. SVG намеренно
//! запрещён — при открытии ответа напрямую он исполняется на нашем origin.

use crate::error::{AppError, AppResult};

/// Максимальный размер аватара, байт (1 МиБ) — миниатюра, не галерея.
pub const MAX_AVATAR_BYTES: usize = 1_048_576;

/// Длина стороны, до которой клиенту рекомендуется ужимать картинку.
pub const AVATAR_RECOMMENDED_PX: u32 = 256;

/// Проверенный аватар: MIME гарантированно совпадает с содержимым.
#[derive(Debug, Clone)]
pub struct AvatarImage {
    pub mime: &'static str,
    pub data: Vec<u8>,
}

/// Нормализует `Content-Type` картинки к `'static`-канону.
///
/// Отбрасывает параметры (`; charset=…`), регистр, алиасы `image/jpg` /
/// `image/pjpeg`. Возвращает `None` для неразрешённых типов (в т.ч. SVG).
fn canonical_mime(raw: &str) -> Option<&'static str> {
    let base = raw
        .split(';')
        .next()
        .unwrap_or(raw)
        .trim()
        .to_ascii_lowercase();
    Some(match base.as_str() {
        "image/png" => "image/png",
        "image/jpeg" | "image/jpg" | "image/pjpeg" => "image/jpeg",
        "image/webp" => "image/webp",
        "image/gif" => "image/gif",
        _ => return None,
    })
}

/// Определяет формат по магическим байтам. `None` — не картинка/не поддержан.
fn sniff_mime(data: &[u8]) -> Option<&'static str> {
    const PNG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if data.starts_with(&PNG) {
        return Some("image/png");
    }
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    // WEBP: RIFF <4 байта размера> WEBP
    if data.len() >= 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    None
}

/// Проверяет размер и содержимое аватара.
///
/// * размер ≤ [`MAX_AVATAR_BYTES`];
/// * заявленный MIME из [`ALLOWED_MIMES`] (после нормализации);
/// * магические байты совпадают с заявленным семейством.
pub fn validate_avatar(declared_mime: &str, data: &[u8]) -> AppResult<AvatarImage> {
    if data.is_empty() {
        return Err(AppError::BadRequest("файл аватара пуст".into()));
    }
    if data.len() > MAX_AVATAR_BYTES {
        return Err(AppError::BadRequest(format!(
            "аватар больше {} КБ — уменьшите изображение",
            MAX_AVATAR_BYTES / 1024
        )));
    }

    let Some(declared) = canonical_mime(declared_mime) else {
        return Err(AppError::BadRequest(
            "поддерживаются только PNG, JPEG, WebP и GIF".into(),
        ));
    };

    let Some(actual) = sniff_mime(data) else {
        return Err(AppError::BadRequest(
            "файл не распознан как изображение".into(),
        ));
    };
    if actual != declared {
        return Err(AppError::BadRequest(format!(
            "Content-Type «{declared}» не соответствует содержимому файла («{actual}»)"
        )));
    }

    Ok(AvatarImage {
        mime: declared,
        data: data.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(len: usize) -> Vec<u8> {
        let mut v = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        v.resize(len.max(8), 0);
        v
    }

    #[test]
    fn accepts_valid_png() {
        let img = validate_avatar("image/png", &png(64)).unwrap();
        assert_eq!(img.mime, "image/png");
    }

    #[test]
    fn normalizes_jpg_aliases_and_parameters() {
        // Алиасы и параметры Content-Type должны сойтись к image/jpeg —
        // проверяем на настоящих JPEG-магических байтах.
        let jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 0, 0, 0];
        for raw in ["image/jpg", "IMAGE/JPEG", "image/jpeg; charset=binary"] {
            let img = validate_avatar(raw, &jpeg).unwrap_or_else(|e| panic!("{raw}: {e}"));
            assert_eq!(img.mime, "image/jpeg", "{raw}");
        }
    }

    #[test]
    fn rejects_oversized_file() {
        // Валидная сигнатура не спасает: сначала проверяем размер.
        let mut signed = png(8);
        signed.resize(MAX_AVATAR_BYTES + 1, 0);
        let err = validate_avatar("image/png", &signed)
            .unwrap_err()
            .to_string();
        assert!(err.contains("больше"), "{err}");
    }

    #[test]
    fn rejects_lie_about_mime() {
        // PNG-байты под видом JPEG.
        let err = validate_avatar("image/jpeg", &png(32))
            .unwrap_err()
            .to_string();
        assert!(err.contains("не соответствует"), "{err}");
    }

    #[test]
    fn rejects_svg_and_unknown_bytes() {
        let svg = b"<svg xmlns='http://www.w3.org/2000/svg'></svg>";
        assert!(validate_avatar("image/svg+xml", svg).is_err());
        assert!(validate_avatar("text/html", svg).is_err());
        // Даже заявленный image/png не проходит без сигнатуры.
        assert!(validate_avatar("image/png", b"<html>boom</html>").is_err());
    }

    #[test]
    fn detects_webp_by_riff_container() {
        let mut data = b"RIFF".to_vec();
        data.extend_from_slice(&[0, 0, 0, 0]);
        data.extend_from_slice(b"WEBP");
        data.extend_from_slice(b"VP8 ");
        assert_eq!(sniff_mime(&data), Some("image/webp"));
        assert!(validate_avatar("image/webp", &data).is_ok());
    }

    #[test]
    fn empty_file_is_rejected() {
        assert!(validate_avatar("image/png", b"").is_err());
    }
}
