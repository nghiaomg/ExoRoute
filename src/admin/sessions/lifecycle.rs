//! Admin session token lifecycle: issue, rotate, and revoke.
//!
//! Owns the refresh-token state machine and family-cap eviction. Record
//! encoding lives in `records.rs`; HTTP handlers live in `handlers.rs`.

use super::records::{
    SESSION_CREATED_INDEX, digest_key, refresh_token_record, remove_session_family_ref,
    revoke_family_record_ref, session_created_index_key, session_family_active,
    session_family_record, session_token_index_key,
};
use super::{IssuedAdminSession, MAX_REFRESH_GENERATION, MAX_SESSION_FAMILIES, RefreshResult};
use crate::{
    infra::storage::{Field, Record, StorageError, Table, WriteTxn},
    state::AppState,
};

pub(crate) async fn issue_admin_session(
    state: &AppState,
    must_change_password: bool,
) -> Result<IssuedAdminSession, StorageError> {
    issue_admin_session_at(state, must_change_password, super::db_now_unix_seconds()?).await
}

pub(crate) async fn issue_admin_session_at(
    state: &AppState,
    must_change_password: bool,
    now: i64,
) -> Result<IssuedAdminSession, StorageError> {
    let family_id = uuid::Uuid::new_v4().simple().to_string();
    let refresh_token = super::new_token()?;
    let access_token = state
        .generate_admin_access_token()
        .map_err(|_| StorageError::Invalid("secure random source is unavailable".into()))?;
    let refresh_hash = super::token_digest(&refresh_token);
    let idle_expiry = now.saturating_add(super::IDLE_TTL_SECONDS);
    let absolute_expiry = now.saturating_add(super::ABSOLUTE_TTL_SECONDS);
    let refresh_hash_key = digest_key(&refresh_hash);
    let family = session_family_record(
        &family_id,
        now,
        now,
        idle_expiry,
        absolute_expiry,
        must_change_password,
        None,
    );
    let token_record = refresh_token_record(&family_id, 0, now, None);
    let family_for_write = family_id.clone();
    let evicted_family = state
        .db
        .write(move |transaction| {
            cleanup_one_expired_session_family(transaction, now)?;
            let family_index = transaction.scan_prefix::<String>(
                Table::SessionIndex,
                SESSION_CREATED_INDEX,
                usize::try_from(MAX_SESSION_FAMILIES)
                    .unwrap_or(128)
                    .saturating_add(1),
            )?;
            if family_index.len() > usize::try_from(MAX_SESSION_FAMILIES).unwrap_or(128) {
                return Err(StorageError::Invalid(
                    "admin session family cap invariant is violated".to_owned(),
                ));
            }
            let mut active = Vec::new();
            for (index, existing_id) in &family_index {
                let Some(record) =
                    transaction.get::<Record>(Table::AdminSessionFamilies, existing_id)?
                else {
                    return Err(StorageError::Invalid(
                        "admin session index is stale".to_owned(),
                    ));
                };
                if session_family_active(&record, now)? {
                    active.push((index.clone(), existing_id.clone()));
                }
            }
            let evicted = if active.len() >= usize::try_from(MAX_SESSION_FAMILIES).unwrap_or(128) {
                let (index, id) = active.first().cloned().ok_or_else(|| {
                    StorageError::Invalid(
                        "admin session family cap invariant is violated".to_owned(),
                    )
                })?;
                remove_session_family_ref(transaction, &id, &index)?;
                Some(id)
            } else {
                None
            };
            transaction.put_if_absent(Table::AdminSessionFamilies, &family_for_write, &family)?;
            let created_index = session_created_index_key(now, &family_for_write)?;
            transaction.put_if_absent(Table::SessionIndex, &created_index, &family_for_write)?;
            transaction.put_if_absent(
                Table::AdminRefreshTokens,
                &refresh_hash_key,
                &token_record,
            )?;
            let token_index = session_token_index_key(&family_for_write, 0)?;
            transaction.put_if_absent(Table::SessionIndex, &token_index, &refresh_hash_key)?;
            Ok(evicted)
        })
        .await?;

    if let Some(evicted_family) = evicted_family {
        state.revoke_admin_family(&evicted_family).await;
    }
    state
        .cache_admin_access_token(access_token.clone(), family_id, must_change_password)
        .await;
    Ok(IssuedAdminSession {
        access_token,
        refresh_token,
        must_change_password,
        refresh_max_age: super::IDLE_TTL_SECONDS,
    })
}

pub(crate) fn cleanup_one_expired_session_family(
    transaction: &mut WriteTxn<'_, '_>,
    now: i64,
) -> Result<(), StorageError> {
    let families = transaction.scan_prefix::<String>(
        Table::SessionIndex,
        SESSION_CREATED_INDEX,
        usize::try_from(MAX_SESSION_FAMILIES)
            .unwrap_or(128)
            .saturating_add(1),
    )?;
    for (index, family_id) in families {
        let record = transaction
            .get::<Record>(Table::AdminSessionFamilies, &family_id)?
            .ok_or_else(|| StorageError::Invalid("admin session index is stale".to_owned()))?;
        if !session_family_active(&record, now)? {
            remove_session_family_ref(transaction, &family_id, &index)?;
            break;
        }
    }
    Ok(())
}

pub(crate) async fn rotate_refresh_token(
    state: &AppState,
    presented_token: &str,
) -> Result<RefreshResult, StorageError> {
    rotate_refresh_token_at(state, presented_token, super::db_now_unix_seconds()?).await
}

pub(crate) async fn rotate_refresh_token_at(
    state: &AppState,
    presented_token: &str,
    now: i64,
) -> Result<RefreshResult, StorageError> {
    let presented_hash = super::token_digest(presented_token);
    let next_token = super::new_token()?;
    let access_token = state
        .generate_admin_access_token()
        .map_err(|_| StorageError::Invalid("secure random source is unavailable".into()))?;
    let next_hash = super::token_digest(&next_token);
    let presented_hash_key = digest_key(&presented_hash);
    let next_hash_key = digest_key(&next_hash);
    let rotation = state
        .db
        .write(move |transaction| {
            cleanup_one_expired_session_family(transaction, now)?;
            let Some(mut token_record) =
                transaction.get::<Record>(Table::AdminRefreshTokens, &presented_hash_key)?
            else {
                return Ok(super::RotationCommit::Invalid(None));
            };
            let family_id = token_record.text("family_id")?.to_owned();
            let generation = token_record.integer("generation")?;
            if token_record.optional_integer("consumed_at")?.is_some() {
                revoke_family_record_ref(transaction, &family_id, now)?;
                return Ok(super::RotationCommit::Invalid(Some(family_id)));
            }
            let Some(mut family) =
                transaction.get::<Record>(Table::AdminSessionFamilies, &family_id)?
            else {
                return Ok(super::RotationCommit::Invalid(None));
            };
            let absolute_expiry = family.integer("absolute_expires_at")?;
            let must_change_password = family.boolean("must_change_password")?;
            if !session_family_active(&family, now)? {
                revoke_family_record_ref(transaction, &family_id, now)?;
                return Ok(super::RotationCommit::Invalid(Some(family_id)));
            }
            let Some(next_generation) = generation
                .checked_add(1)
                .filter(|next| *next <= MAX_REFRESH_GENERATION)
            else {
                revoke_family_record_ref(transaction, &family_id, now)?;
                return Ok(super::RotationCommit::Invalid(Some(family_id)));
            };
            token_record.insert("consumed_at", Field::I64(now));
            transaction.put(
                Table::AdminRefreshTokens,
                &presented_hash_key,
                &token_record,
            )?;
            let next_idle_expiry = now
                .saturating_add(super::IDLE_TTL_SECONDS)
                .min(absolute_expiry);
            family.insert("last_refresh_at", Field::I64(now));
            family.insert("idle_expires_at", Field::I64(next_idle_expiry));
            transaction.put(Table::AdminSessionFamilies, &family_id, &family)?;
            let next_record = refresh_token_record(&family_id, next_generation, now, None);
            transaction.put_if_absent(Table::AdminRefreshTokens, &next_hash_key, &next_record)?;
            let next_index = session_token_index_key(&family_id, next_generation)?;
            transaction.put_if_absent(Table::SessionIndex, &next_index, &next_hash_key)?;
            Ok(super::RotationCommit::Rotated {
                family_id,
                must_change_password,
                next_idle_expiry,
            })
        })
        .await?;

    let (family_id, must_change_password, next_idle_expiry) = match rotation {
        super::RotationCommit::Invalid(Some(family_id)) => {
            state.revoke_admin_family(&family_id).await;
            return Ok(RefreshResult::Invalid);
        }
        super::RotationCommit::Invalid(None) => return Ok(RefreshResult::Invalid),
        super::RotationCommit::Rotated {
            family_id,
            must_change_password,
            next_idle_expiry,
        } => (family_id, must_change_password, next_idle_expiry),
    };

    state
        .cache_admin_access_token(access_token.clone(), family_id, must_change_password)
        .await;
    Ok(RefreshResult::Rotated(IssuedAdminSession {
        access_token,
        refresh_token: next_token,
        must_change_password,
        refresh_max_age: next_idle_expiry.saturating_sub(now).max(0),
    }))
}

pub(crate) async fn revoke_family_in_database(
    state: &AppState,
    family_id: &str,
) -> Result<(), StorageError> {
    let now = super::db_now_unix_seconds()?;
    let family_id_owned = family_id.to_owned();
    state
        .db
        .write(move |transaction| revoke_family_record_ref(transaction, &family_id_owned, now))
        .await?;
    state.revoke_admin_family(family_id).await;
    Ok(())
}

pub(crate) async fn family_is_active(
    state: &AppState,
    family_id: &str,
) -> Result<bool, StorageError> {
    let now = super::db_now_unix_seconds()?;
    let family_id = family_id.to_owned();
    state
        .db
        .read(move |transaction| {
            let Some(family) =
                transaction.get::<Record>(Table::AdminSessionFamilies, &family_id)?
            else {
                return Ok(false);
            };
            session_family_active(&family, now)
        })
        .await
}

pub(crate) fn revoke_all_sessions_in_transaction(
    transaction: &mut WriteTxn<'_, '_>,
) -> Result<(), StorageError> {
    let now = super::db_now_unix_seconds()?;
    let families = transaction.scan_prefix::<Record>(
        Table::AdminSessionFamilies,
        "",
        usize::try_from(MAX_SESSION_FAMILIES)
            .unwrap_or(128)
            .saturating_add(1),
    )?;
    if families.len() > usize::try_from(MAX_SESSION_FAMILIES).unwrap_or(128) {
        return Err(StorageError::Invalid(
            "admin session family cap invariant is violated".to_owned(),
        ));
    }
    for (id, mut family) in families {
        family.insert("revoked_at", Field::I64(now));
        transaction.put(Table::AdminSessionFamilies, &id, &family)?;
    }
    Ok(())
}
