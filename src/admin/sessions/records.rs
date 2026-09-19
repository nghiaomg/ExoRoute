//! Admin session record codec: family/token records and index keys.
//!
//! Pure mapping between session structs and LMDB records; no I/O or HTTP.

use crate::infra::storage::{Field, Record, StorageError, Table, WriteTxn};

pub(crate) const SESSION_CREATED_INDEX: &str = "families/by-created/";
pub(crate) const SESSION_TOKEN_INDEX: &str = "tokens/";
pub(crate) const SESSION_TOKEN_DELETE_BATCH: usize =
    crate::admin::sessions::MAX_REFRESH_GENERATION as usize + 1;

pub(crate) fn session_family_record(
    id: &str,
    created_at: i64,
    last_refresh_at: i64,
    idle_expires_at: i64,
    absolute_expires_at: i64,
    must_change_password: bool,
    revoked_at: Option<i64>,
) -> Record {
    Record::new()
        .with("id", Field::Text(id.to_owned()))
        .with("created_at", Field::I64(created_at))
        .with("last_refresh_at", Field::I64(last_refresh_at))
        .with("idle_expires_at", Field::I64(idle_expires_at))
        .with("absolute_expires_at", Field::I64(absolute_expires_at))
        .with("must_change_password", Field::Bool(must_change_password))
        .with(
            "revoked_at",
            revoked_at.map(Field::I64).unwrap_or(Field::Null),
        )
}

pub(crate) fn refresh_token_record(
    family_id: &str,
    generation: i64,
    created_at: i64,
    consumed_at: Option<i64>,
) -> Record {
    Record::new()
        .with("family_id", Field::Text(family_id.to_owned()))
        .with("generation", Field::I64(generation))
        .with("created_at", Field::I64(created_at))
        .with(
            "consumed_at",
            consumed_at.map(Field::I64).unwrap_or(Field::Null),
        )
}

pub(crate) fn session_created_index_key(
    created_at: i64,
    family_id: &str,
) -> Result<String, StorageError> {
    let key = format!("{SESSION_CREATED_INDEX}{created_at:020}/{family_id}");
    crate::infra::storage::validate_key(&key)?;
    Ok(key)
}

pub(crate) fn session_token_index_key(
    family_id: &str,
    generation: i64,
) -> Result<String, StorageError> {
    let key = format!("{SESSION_TOKEN_INDEX}{family_id}/{generation:010}");
    crate::infra::storage::validate_key(&key)?;
    Ok(key)
}

pub(crate) fn digest_key(digest: &[u8; 32]) -> String {
    let mut key = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(key, "{byte:02x}");
    }
    key
}

pub(crate) fn session_family_active(family: &Record, now: i64) -> Result<bool, StorageError> {
    Ok(family.optional_integer("revoked_at")?.is_none()
        && family.integer("idle_expires_at")? > now
        && family.integer("absolute_expires_at")? > now)
}

pub(crate) fn revoke_family_record_ref(
    transaction: &mut WriteTxn<'_, '_>,
    family_id: &str,
    now: i64,
) -> Result<(), StorageError> {
    let Some(mut family) = transaction.get::<Record>(Table::AdminSessionFamilies, family_id)?
    else {
        return Ok(());
    };
    if family.optional_integer("revoked_at")?.is_none() {
        family.insert("revoked_at", Field::I64(now));
        transaction.put(Table::AdminSessionFamilies, family_id, &family)?;
    }
    Ok(())
}

pub(crate) fn remove_session_family_ref(
    transaction: &mut WriteTxn<'_, '_>,
    family_id: &str,
    created_index: &str,
) -> Result<(), StorageError> {
    let token_prefix = format!("{SESSION_TOKEN_INDEX}{family_id}/");
    let tokens = transaction.scan_prefix::<String>(
        Table::SessionIndex,
        &token_prefix,
        SESSION_TOKEN_DELETE_BATCH.saturating_add(1),
    )?;
    if tokens.len() > SESSION_TOKEN_DELETE_BATCH {
        return Err(StorageError::Invalid(
            "admin session token history exceeds its configured bound".to_owned(),
        ));
    }
    for (index, token_key) in tokens {
        transaction.delete(Table::AdminRefreshTokens, &token_key)?;
        transaction.delete(Table::SessionIndex, &index)?;
    }
    transaction.delete(Table::AdminSessionFamilies, family_id)?;
    transaction.delete(Table::SessionIndex, created_index)?;
    Ok(())
}
