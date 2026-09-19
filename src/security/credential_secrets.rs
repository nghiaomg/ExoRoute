use chacha20poly1305::{
    ChaCha20Poly1305, Nonce,
    aead::{Aead, KeyInit},
};
use rand::{RngCore, rngs::OsRng};
use sha2::{Digest, Sha256};

pub fn encrypt_secret(
    master_key: Option<&[u8; 32]>,
    value: &str,
) -> Result<Option<Vec<u8>>, String> {
    if value.is_empty() {
        return Ok(None);
    }
    let key = master_key.ok_or("set EXOROUTE_MASTER_KEY before saving provider credentials")?;
    let cipher = ChaCha20Poly1305::new(key.into());
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce);
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce), value.as_bytes())
        .map_err(|_| "could not encrypt provider credential")?;
    let mut stored = nonce.to_vec();
    stored.extend_from_slice(&ciphertext);
    Ok(Some(stored))
}

pub fn decrypt_secret(
    master_key: Option<&[u8; 32]>,
    stored: Option<&[u8]>,
) -> Result<Option<String>, String> {
    let Some(stored) = stored else {
        return Ok(None);
    };
    if stored.len() < 13 {
        return Err("stored provider credential is invalid".to_owned());
    }
    let key = master_key.ok_or("set EXOROUTE_MASTER_KEY to use saved provider credentials")?;
    let cipher = ChaCha20Poly1305::new(key.into());
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&stored[..12]), &stored[12..])
        .map_err(|_| "could not decrypt provider credential; check EXOROUTE_MASTER_KEY")?;
    String::from_utf8(plaintext)
        .map(Some)
        .map_err(|_| "stored provider credential is not valid UTF-8".to_owned())
}

pub fn token_hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

pub fn secure_eq(left: &[u8], right: &[u8]) -> bool {
    use subtle::ConstantTimeEq;
    left.len() == right.len() && bool::from(left.ct_eq(right))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_secret_round_trips_and_requires_master_key() {
        let key = [19u8; 32];
        assert!(encrypt_secret(None, "secret").is_err());
        let encrypted = encrypt_secret(Some(&key), "secret-value")
            .expect("encryption works")
            .expect("secret stored");
        assert_ne!(encrypted, b"secret-value");
        assert_eq!(
            decrypt_secret(Some(&key), Some(&encrypted))
                .expect("decryption works")
                .as_deref(),
            Some("secret-value")
        );
        assert!(decrypt_secret(Some(&[20u8; 32]), Some(&encrypted)).is_err());
    }
}
