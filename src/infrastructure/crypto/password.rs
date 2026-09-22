//! Хеширование паролей (Argon2id) и проверка.

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};

/// Заглушка для выравнивания времени отказа, когда логин не найден:
/// `verify` по несуществующему хешу возвращает `false` мгновенно и
/// выдаёт existence-oracle по времени ответа.
pub fn dummy_verify(password: &str) {
    // Игнорируем результат: важно только потратить примерно столько же
    // времени, сколько на реальную проверку Argon2.
    let _ = verify_password(password, DUMMY_HASH);
}

/// Argon2-хеш фиксированной строки: никогда не совпадает с реальным паролем.
const DUMMY_HASH: &str =
    "$argon2id$v=19$m=19456,t=2,p=1$ZHVtbXktc2FsdA$0000000000000000000000000000000000000000000";

/// Создаёт строку-хеш пароля со случайной солью.
pub fn hash_password(password: &str) -> String {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .expect("Argon2 не может захешировать пароль")
        .to_string()
}

/// Проверяет пароль против хеша. Некорректный хеш = отказ, не паника.
pub fn verify_password(password: &str, hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify_roundtrip() {
        let hash = hash_password("secret-password");
        assert!(verify_password("secret-password", &hash));
        assert!(!verify_password("wrong-password", &hash));
    }

    #[test]
    fn malformed_hash_is_rejected_not_panicking() {
        assert!(!verify_password("any", "not-a-real-hash"));
    }

    #[test]
    fn dummy_verify_never_accepts_any_password() {
        // DUMMY_HASH — фиксированный битый хеш: verify всегда false,
        // dummy_verify лишь тратит время (timing equalizer).
        for candidate in ["", "password", "secret1", "0"] {
            assert!(!verify_password(candidate, DUMMY_HASH));
        }
        // dummy_verify не паникует и не принимает пароль.
        dummy_verify("any-password");
        dummy_verify("");
    }
}
