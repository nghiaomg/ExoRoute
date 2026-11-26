use chacha20poly1305::{
    ChaCha20Poly1305, Nonce,
    aead::{Aead, KeyInit, Payload},
};
use rand::{RngCore, rngs::OsRng};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

/// Domain-separation header for v2 ciphertexts. Legacy payloads are raw
/// `nonce || ciphertext` bytes and never start with a 9-byte printable magic,
/// so the two formats cannot be confused in practice; the decrypt path also
/// falls back to legacy decoding if a v2-tagged frame fails authentication.
const ENVELOPE_MAGIC: &[u8; 9] = b"EXOSECRET";
const ENVELOPE_VERSION: u8 = 2;
const AAD_DOMAIN: &[u8] = b"exoroute/secret/v2/";
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;
const LEGACY_MIN_LEN: usize = NONCE_LEN + TAG_LEN;

/// Unambiguous AEAD associated data for one persisted secret.
///
/// Binding `table`, stable `record_id`, and `field` means a ciphertext cannot
/// be transplanted between records, fields, or tables without failing
/// authentication. Component lengths are encoded explicitly, so no delimiter
/// that might appear in a record id can create a context collision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SecretContext {
    table: &'static str,
    record_id: String,
    field: &'static str,
}

impl SecretContext {
    pub fn new(table: &'static str, record_id: impl Into<String>, field: &'static str) -> Self {
        Self {
            table,
            record_id: record_id.into(),
            field,
        }
    }

    pub fn provider_key(record_id: impl Into<String>) -> Self {
        Self::new("provider_api_keys", record_id, "secret")
    }

    pub fn provider(record_id: impl Into<String>, field: &'static str) -> Self {
        Self::new("providers", record_id, field)
    }

    pub fn proxy_endpoint(record_id: impl Into<String>) -> Self {
        Self::new("proxy_endpoints", record_id, "secret")
    }

    pub fn pending_provider_key(record_id: impl Into<String>) -> Self {
        Self::new("oauth_flows", record_id, "encrypted_api_key")
    }

    pub fn oauth_account(record_id: impl Into<String>) -> Self {
        Self::new("provider_api_keys", record_id, "secret")
    }

    fn aad(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(
            AAD_DOMAIN.len() + 12 + self.table.len() + self.record_id.len() + self.field.len(),
        );
        bytes.extend_from_slice(AAD_DOMAIN);
        push_component(&mut bytes, self.table.as_bytes());
        push_component(&mut bytes, self.record_id.as_bytes());
        push_component(&mut bytes, self.field.as_bytes());
        bytes
    }
}

fn push_component(target: &mut Vec<u8>, component: &[u8]) {
    let len = u64::try_from(component.len()).unwrap_or(u64::MAX);
    target.extend_from_slice(&len.to_le_bytes());
    target.extend_from_slice(component);
}

/// True when a stored value is an unversioned legacy ciphertext and should be
/// re-encrypted on the next write.
pub fn secret_needs_migration(stored: Option<&[u8]>) -> bool {
    stored.is_some_and(|bytes| !bytes.starts_with(ENVELOPE_MAGIC))
}

pub fn encrypt_secret(
    master_key: Option<&[u8; 32]>,
    context: &SecretContext,
    value: &str,
) -> Result<Option<Vec<u8>>, String> {
    if value.is_empty() {
        return Ok(None);
    }
    let key = master_key.ok_or("set EXOROUTE_MASTER_KEY before saving provider credentials")?;
    let key = Zeroizing::new(*key);
    let cipher = ChaCha20Poly1305::new((&*key).into());
    let mut nonce = Zeroizing::new([0u8; NONCE_LEN]);
    OsRng.fill_bytes(nonce.as_mut());
    let plaintext = Zeroizing::new(value.as_bytes().to_vec());
    let ciphertext = Zeroizing::new(
        cipher
            .encrypt(
                Nonce::from_slice(&*nonce),
                Payload {
                    msg: &plaintext,
                    aad: &context.aad(),
                },
            )
            .map_err(|_| "could not encrypt provider credential".to_owned())?,
    );
    let mut stored = Vec::with_capacity(ENVELOPE_MAGIC.len() + 1 + NONCE_LEN + ciphertext.len());
    stored.extend_from_slice(ENVELOPE_MAGIC);
    stored.push(ENVELOPE_VERSION);
    stored.extend_from_slice(&*nonce);
    stored.extend_from_slice(&ciphertext);
    Ok(Some(stored))
}

/// Encrypts a non-empty value for storage, returning a flat error when the
/// master key is missing or the value is empty. Used by write transactions
/// that must bind AAD to a record id only known inside the transaction.
pub fn encrypt_for_storage(
    master_key: Option<&[u8; 32]>,
    context: &SecretContext,
    value: &str,
) -> Result<Vec<u8>, String> {
    encrypt_secret(master_key, context, value)?
        .ok_or_else(|| "credential value is empty".to_owned())
}

pub fn decrypt_secret(
    master_key: Option<&[u8; 32]>,
    context: &SecretContext,
    stored: Option<&[u8]>,
) -> Result<Option<String>, String> {
    let Some(stored) = stored else {
        return Ok(None);
    };
    let key = master_key.ok_or("set EXOROUTE_MASTER_KEY to use saved provider credentials")?;
    let key = Zeroizing::new(*key);
    let cipher = ChaCha20Poly1305::new((&*key).into());

    if stored.starts_with(ENVELOPE_MAGIC) {
        let header_len = ENVELOPE_MAGIC.len() + 1;
        if stored.len() < header_len {
            return Err("stored provider credential is invalid".to_owned());
        }
        let version = stored[ENVELOPE_MAGIC.len()];
        if version != ENVELOPE_VERSION {
            return Err(format!(
                "unsupported credential encryption version {version}"
            ));
        }
        let body = &stored[header_len..];
        if body.len() < LEGACY_MIN_LEN {
            return Err("stored provider credential is invalid".to_owned());
        }
        let plaintext = cipher
            .decrypt(
                Nonce::from_slice(&body[..NONCE_LEN]),
                Payload {
                    msg: &body[NONCE_LEN..],
                    aad: &context.aad(),
                },
            )
            .map_err(|_| DECRYPT_ERROR.to_owned())?;
        return decode_plaintext(plaintext);
    }

    // Legacy v1 frames were encrypted with empty AAD. The fallback also makes
    // the format detection collision-safe for the impossible case of a legacy
    // ciphertext that happens to begin with the v2 magic bytes.
    if secret_needs_migration(Some(stored)) {
        tracing::debug!(
            "decrypting a legacy AAD-free credential; it will migrate on the next write"
        );
    }
    if stored.len() < LEGACY_MIN_LEN {
        return Err("stored provider credential is invalid".to_owned());
    }
    let plaintext = cipher
        .decrypt(
            Nonce::from_slice(&stored[..NONCE_LEN]),
            Payload {
                msg: &stored[NONCE_LEN..],
                aad: b"",
            },
        )
        .map_err(|_| DECRYPT_ERROR.to_owned())?;
    decode_plaintext(plaintext)
}

const DECRYPT_ERROR: &str =
    "could not decrypt provider credential; check EXOROUTE_MASTER_KEY and credential ownership";

fn decode_plaintext(plaintext: Vec<u8>) -> Result<Option<String>, String> {
    let plaintext = Zeroizing::new(plaintext);
    String::from_utf8((*plaintext).clone())
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

    const KEY: [u8; 32] = [19u8; 32];

    #[test]
    fn provider_secret_round_trips_and_requires_master_key() {
        let context = SecretContext::provider_key("key-1");
        assert!(encrypt_secret(None, &context, "secret").is_err());
        let encrypted = encrypt_secret(Some(&KEY), &context, "secret-value")
            .expect("encryption works")
            .expect("secret stored");
        assert_ne!(&encrypted[..ENVELOPE_MAGIC.len()], b"secret-va");
        assert_eq!(
            decrypt_secret(Some(&KEY), &context, Some(&encrypted))
                .expect("decryption works")
                .as_deref(),
            Some("secret-value")
        );
        assert!(decrypt_secret(Some(&[20u8; 32]), &context, Some(&encrypted)).is_err());
    }

    #[test]
    fn swapping_context_fails_authentication() {
        let context = SecretContext::provider_key("key-1");
        let encrypted = encrypt_secret(Some(&KEY), &context, "secret-value")
            .expect("encrypt")
            .expect("stored");
        for other in [
            SecretContext::provider_key("key-2"),
            SecretContext::provider("key-1", "secret"),
            SecretContext::proxy_endpoint("key-1"),
        ] {
            assert!(
                decrypt_secret(Some(&KEY), &other, Some(&encrypted)).is_err(),
                "a ciphertext must not decrypt under a different context"
            );
        }
    }

    #[test]
    fn legacy_ciphertext_decrypts_and_can_be_reencrypted() {
        // Build a legacy frame exactly as the previous implementation did.
        let cipher = ChaCha20Poly1305::new((&KEY).into());
        let mut nonce = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce);
        let mut legacy = nonce.to_vec();
        legacy.extend_from_slice(
            &cipher
                .encrypt(Nonce::from_slice(&nonce), b"legacy-value".as_slice())
                .expect("legacy encrypt"),
        );
        assert!(secret_needs_migration(Some(&legacy)));
        let context = SecretContext::provider_key("legacy-key");
        assert_eq!(
            decrypt_secret(Some(&KEY), &context, Some(&legacy))
                .expect("legacy decrypt")
                .as_deref(),
            Some("legacy-value")
        );
        let migrated = encrypt_secret(Some(&KEY), &context, "legacy-value")
            .expect("migrate")
            .expect("stored");
        assert!(!secret_needs_migration(Some(&migrated)));
        assert_eq!(
            decrypt_secret(Some(&KEY), &context, Some(&migrated))
                .expect("v2 decrypt")
                .as_deref(),
            Some("legacy-value")
        );
    }

    #[test]
    fn malformed_and_unknown_version_frames_fail_cleanly() {
        let context = SecretContext::provider_key("key-1");
        assert!(decrypt_secret(Some(&KEY), &context, Some(b"short")).is_err());
        assert!(decrypt_secret(Some(&KEY), &context, Some(&[0u8; 40])).is_err());
        let mut unknown = ENVELOPE_MAGIC.to_vec();
        unknown.push(99);
        unknown.extend_from_slice(&[0u8; 40]);
        let error = decrypt_secret(Some(&KEY), &context, Some(&unknown))
            .expect_err("unknown version is rejected");
        assert!(error.contains("version"), "{error}");
        assert!(!error.contains("secret-value"));
    }

    #[test]
    fn empty_values_stay_absent() {
        let context = SecretContext::provider_key("key-1");
        assert!(
            encrypt_secret(Some(&KEY), &context, "")
                .expect("empty input is valid")
                .is_none()
        );
        assert!(
            decrypt_secret(Some(&KEY), &context, None)
                .expect("absent input is valid")
                .is_none()
        );
    }
}
