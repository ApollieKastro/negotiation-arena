//! Хеширование паролей (Argon2id) и проверка.

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};

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
}
