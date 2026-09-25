//! Шифрование секретов провайдеров (API-ключи) — AES-256-GCM.
//!
//! Ключ хранения выводится из `ENCRYPTION_KEY`/`JWT_SECRET` через Argon2id
//! с фиксированной солью приложения. Формат шифротекста: `base64(nonce || ciphertext)`,
//! nonce — 12 байт, tag включён в ciphertext.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Key, Nonce,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use rand::{rngs::OsRng, RngCore};

use crate::error::{AppError, AppResult};

const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;
/// Фиксированная соль приложения для вывода ключа хранения.
const KEY_SALT: &[u8] = b"negotiation-arena:v1:secrets";

pub mod password;

/// Шифровальщик секретов. Создаётся один раз на старте, разделяется через `Arc`.
///
/// `Debug` реализован вручную, чтобы не раскрывать ключевой материал.
#[derive(Clone)]
pub struct SecretCipher {
    cipher: Aes256Gcm,
}

impl std::fmt::Debug for SecretCipher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretCipher([REDACTED])")
    }
}

impl SecretCipher {
    /// Создаёт шифровальщик из 32-байтового ключа.
    pub fn new(key: &[u8; KEY_LEN]) -> AppResult<Self> {
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
        Ok(Self { cipher })
    }

    /// Создаёт шифровальщик, выводя ключ из произвольного секрета (Argon2id).
    pub fn from_secret(secret: &str) -> AppResult<Self> {
        if secret.trim().is_empty() {
            return Err(AppError::Config(
                "секрет шифрования не задан (ENCRYPTION_KEY)".into(),
            ));
        }
        Self::new(&derive_key(secret))
    }

    /// Шифрует строку, возвращает `base64(nonce || ciphertext)`.
    pub fn encrypt(&self, plaintext: &str) -> AppResult<String> {
        let mut nonce_bytes = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce_bytes);

        let ciphertext = self
            .cipher
            .encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_bytes())
            .map_err(|_| AppError::internal("не удалось зашифровать секрет"))?;

        let mut payload = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        payload.extend_from_slice(&nonce_bytes);
        payload.extend_from_slice(&ciphertext);
        Ok(BASE64.encode(payload))
    }

    /// Расшифровывает значение, созданное [`SecretCipher::encrypt`].
    pub fn decrypt(&self, encoded: &str) -> AppResult<String> {
        let payload = BASE64
            .decode(encoded.trim())
            .map_err(|_| AppError::internal("некорректный формат шифротекста"))?;

        if payload.len() < NONCE_LEN + 16 {
            return Err(AppError::internal("слишком короткий шифротекст"));
        }

        let (nonce, ciphertext) = payload.split_at(NONCE_LEN);
        let plaintext = self
            .cipher
            .decrypt(Nonce::from_slice(nonce), ciphertext)
            .map_err(|_| {
                AppError::internal("не удалось расшифровать секрет (секрет шифрования изменился?)")
            })?;

        String::from_utf8(plaintext)
            .map_err(|_| AppError::internal("расшифрованный секрет не является UTF-8"))
    }
}

/// Выводит 32-байтовый ключ хранения из секрета через Argon2id.
///
/// Явный `ENCRYPTION_KEY` в проде обязателен: фиксированная соль означает,
/// что стойкость равна стойкости самого секрета.
pub fn derive_key(secret: &str) -> [u8; KEY_LEN] {
    use argon2::{Algorithm, Argon2, Params, Version};

    let params = Params::new(19_456, 2, 1, Some(KEY_LEN)).expect("валидные параметры Argon2");
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

    let mut key = [0u8; KEY_LEN];
    argon
        .hash_password_into(secret.as_bytes(), KEY_SALT, &mut key)
        .expect("Argon2 не может вывести ключ");
    key
}

/// Маскировка ключа для показа в UI: `gsk_••••abcd`.
pub fn mask_secret(secret: &str) -> String {
    let secret = secret.trim();
    // Работаем по границам символов: срез по байтам паникует на многобайтовом UTF-8.
    let mut chars = secret.chars();
    let head: String = chars
        .by_ref()
        .take(secret.chars().count().saturating_sub(4))
        .collect();
    if head.is_empty() {
        return "••••".to_string();
    }
    let tail: String = chars.collect();
    format!("••••{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_encrypt_decrypt() {
        let cipher = SecretCipher::from_secret("test-secret").unwrap();
        let encrypted = cipher.encrypt("sk-super-secret-key").unwrap();
        assert_ne!(encrypted, "sk-super-secret-key");
        assert_eq!(cipher.decrypt(&encrypted).unwrap(), "sk-super-secret-key");
    }

    #[test]
    fn different_nonces_produce_different_ciphertexts() {
        let cipher = SecretCipher::from_secret("test-secret").unwrap();
        let a = cipher.encrypt("same").unwrap();
        let b = cipher.encrypt("same").unwrap();
        assert_ne!(a, b, "nonce обязан быть случайным");
    }

    #[test]
    fn wrong_secret_cannot_decrypt() {
        let cipher = SecretCipher::from_secret("secret-a").unwrap();
        let other = SecretCipher::from_secret("secret-b").unwrap();
        let encrypted = cipher.encrypt("value").unwrap();
        assert!(other.decrypt(&encrypted).is_err());
    }

    #[test]
    fn empty_secret_is_rejected() {
        assert!(SecretCipher::from_secret("   ").is_err());
    }

    #[test]
    fn mask_hides_middle() {
        assert_eq!(mask_secret("gsk_abcdefgh"), "••••efgh");
        assert_eq!(mask_secret("abc"), "••••");
    }

    #[test]
    fn mask_is_utf8_safe() {
        // Хвост из многобайтовых символов не должен паниковать.
        let key = format!("sk_{}", "ключ".repeat(10));
        assert!(mask_secret(&key).starts_with('•'));
        assert_eq!(mask_secret("аия"), "••••"); // 3 символа → whole secret
        assert_eq!(mask_secret("абвгд"), "••••бвгд"); // 5 символов → хвост 4
        assert_eq!(mask_secret("абвгде"), "••••вгде"); // 6 символов → хвост 4
    }
}
