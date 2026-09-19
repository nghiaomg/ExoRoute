use crate::infra::storage::{Database, Record, StorageError, Table};

/// Bounded-memory cursor over one provider's usable API keys. Keys are visited
/// in the provider's fallback order, starting with the preferred key, so the
/// gateway tries the ordered keys before moving on to the next one. Each step
/// reads one short LMDB read transaction and returns one encrypted credential.
///
/// A rotating cursor ([`Self::start_rotating`]) begins at an operator-visible
/// position instead of the preferred key and keeps the rest of the provider's
/// keys reachable by wrapping around once.
pub struct ProviderApiKeyCursor {
    provider_id: String,
    after: Option<String>,
    finished: bool,
    /// Position that ends a wrapped rotating scan. `None` keeps the scan
    /// forward-only.
    stop_before: Option<String>,
    /// Whether a rotating scan already wrapped around the last key.
    wrapped: bool,
}

/// How many ordering-index entries one read transaction may inspect while it
/// looks for the next usable key. The window bounds memory and is only larger
/// than one when a provider keeps disabled or invalid keys in its order.
const KEY_ORDER_SCAN_WINDOW: usize = 32;

#[derive(Debug, PartialEq)]
pub struct ProviderApiKeyRow {
    pub id: String,
    pub encrypted_secret: Vec<u8>,
    pub credential_type: ProviderCredentialType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderCredentialType {
    ApiKey,
    OAuth,
}

impl ProviderCredentialType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ApiKey => "api_key",
            Self::OAuth => "oauth",
        }
    }
}

impl ProviderApiKeyCursor {
    pub async fn start(
        db: &Database,
        provider_id: &str,
    ) -> Result<Option<(Self, ProviderApiKeyRow)>, StorageError> {
        Self::advance_to(db, Self::new(provider_id, None)).await
    }

    /// Starts a rotating scan at the first usable key strictly after `after`.
    /// When that position is the provider's last key the scan wraps around and
    /// continues from the first key, so a rejected key still has fallbacks.
    pub async fn start_rotating(
        db: &Database,
        provider_id: &str,
        after: Option<&str>,
    ) -> Result<Option<(Self, ProviderApiKeyRow)>, StorageError> {
        let Some((mut cursor, key)) = Self::advance_to(db, Self::new(provider_id, after)).await?
        else {
            return Ok(None);
        };
        // Remember the position this rotation started at: the wrapped pass
        // stops there so one rotation attempts every key at most once.
        cursor.stop_before = cursor.after.clone();
        Ok(Some((cursor, key)))
    }

    /// The ordering position of the last returned key, if any.
    pub fn position(&self) -> Option<&str> {
        self.after.as_deref()
    }

    fn new(provider_id: &str, after: Option<&str>) -> Self {
        Self {
            provider_id: provider_id.to_owned(),
            after: after.map(str::to_owned),
            finished: false,
            stop_before: None,
            wrapped: false,
        }
    }

    async fn advance_to(
        db: &Database,
        mut cursor: Self,
    ) -> Result<Option<(Self, ProviderApiKeyRow)>, StorageError> {
        match cursor.next(db).await? {
            Some(key) => Ok(Some((cursor, key))),
            None => Ok(None),
        }
    }

    pub async fn next(&mut self, db: &Database) -> Result<Option<ProviderApiKeyRow>, StorageError> {
        loop {
            if self.finished {
                if self.wrapped || self.stop_before.is_none() {
                    return Ok(None);
                }
                // Continue with the keys that precede the wrapped position.
                self.wrapped = true;
                self.after = None;
                self.finished = false;
            }
            let provider_id = self.provider_id.clone();
            let after = self.after.clone();
            // The forward pass keeps going to the end of the fallback order.
            // The wrapped pass stops at the position the rotation started from,
            // so no key is attempted twice in one rotation.
            let stop_before = if self.wrapped {
                self.stop_before.clone()
            } else {
                None
            };
            let prefix = crate::infra::db::provider_api_key_created_prefix(&provider_id)?;
            let (found, last_position, exhausted) = db
                .read(move |transaction| {
                    let entries = transaction.scan_prefix_after::<String>(
                        Table::ProviderApiKeyCreatedIndex,
                        &prefix,
                        after.as_deref(),
                        KEY_ORDER_SCAN_WINDOW,
                    )?;
                    let exhausted = entries.len() < KEY_ORDER_SCAN_WINDOW;
                    let mut last_position = None;
                    for (position, id) in entries {
                        if stop_before
                            .as_deref()
                            .is_some_and(|stop| position.as_str() >= stop)
                        {
                            return Ok((None, last_position, true));
                        }
                        last_position = Some(position.clone());
                        let Some(record) =
                            transaction.get::<Record>(Table::ProviderApiKeys, &id)?
                        else {
                            continue;
                        };
                        if record.text("provider_id")? != provider_id
                            || !record.boolean("enabled")?
                            || record.boolean("invalid")?
                        {
                            continue;
                        }
                        let adapter_id = transaction
                            .get::<Record>(Table::Providers, &provider_id)?
                            .map(|provider| provider.text("adapter_id").map(str::to_owned))
                            .transpose()?;
                        return Ok((
                            Some((position, id, record, adapter_id)),
                            last_position,
                            exhausted,
                        ));
                    }
                    Ok((None, last_position, exhausted))
                })
                .await?;
            if let Some((position, id, record, adapter_id)) = found {
                self.after = Some(position);
                return Ok(Some(provider_api_key_row(
                    id,
                    record,
                    adapter_id.as_deref(),
                )?));
            }
            match last_position {
                Some(position) => self.after = Some(position),
                None => self.finished = true,
            }
            if exhausted {
                self.finished = true;
            }
        }
    }
}

fn provider_api_key_row(
    id: String,
    record: Record,
    adapter_id: Option<&str>,
) -> Result<ProviderApiKeyRow, StorageError> {
    let credential_type = match record.optional_text("credential_type")? {
        Some("api_key") => ProviderCredentialType::ApiKey,
        Some("oauth") => ProviderCredentialType::OAuth,
        Some(_) => {
            return Err(StorageError::Invalid(
                "provider credential type is invalid".to_owned(),
            ));
        }
        None if adapter_id == Some(crate::provider_adapters::CODEX_ADAPTER_ID) => {
            ProviderCredentialType::OAuth
        }
        None => ProviderCredentialType::ApiKey,
    };
    Ok(ProviderApiKeyRow {
        id,
        encrypted_secret: record.bytes("secret")?.to_vec(),
        credential_type,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::storage::{Field, Record};
    use std::fs;

    fn key_record(
        id: &str,
        provider_id: &str,
        created_at: &str,
        enabled: bool,
        invalid: bool,
    ) -> Record {
        Record::new()
            .with("id", Field::Text(id.to_owned()))
            .with("provider_id", Field::Text(provider_id.to_owned()))
            .with("secret", Field::Bytes(vec![1]))
            .with("enabled", Field::Bool(enabled))
            .with("invalid", Field::Bool(invalid))
            .with("created_at", Field::Text(created_at.to_owned()))
    }

    fn seed_key(
        transaction: &mut crate::infra::storage::WriteTxn<'_, '_>,
        record: &Record,
    ) -> Result<(), StorageError> {
        let id = record.text("id")?.to_owned();
        let provider_id = record.text("provider_id")?.to_owned();
        let created_at = record.text("created_at")?.to_owned();
        transaction.put(Table::ProviderApiKeys, &id, record)?;
        transaction.put(
            Table::ProviderApiKeyCreatedIndex,
            &crate::infra::db::provider_api_key_order_key(
                &provider_id,
                &format!("{created_at}/{id}"),
            )?,
            &id,
        )?;
        Ok(())
    }

    #[tokio::test]
    async fn returns_usable_keys_in_fallback_order_without_materializing_them() {
        let path =
            std::env::temp_dir().join(format!("exoroute-provider-keys-{}", uuid::Uuid::new_v4()));
        let db = crate::infra::db::connect(&path).await.expect("open LMDB");
        let expected = (0..137)
            .map(|index| format!("key-{index:03}"))
            .collect::<Vec<_>>();
        let seeded = expected
            .iter()
            .enumerate()
            .map(|(index, id)| (id.clone(), format!("2026-01-01T00:00:00.{index:03}Z")))
            .collect::<Vec<_>>();
        db.write(move |transaction| {
            for (id, created_at) in &seeded {
                seed_key(
                    transaction,
                    &key_record(id, "provider", created_at, true, false),
                )?;
            }
            seed_key(
                transaction,
                &key_record(
                    "disabled-key",
                    "provider",
                    "2026-01-02T00:00:00.000Z",
                    false,
                    false,
                ),
            )?;
            seed_key(
                transaction,
                &key_record(
                    "invalid-key",
                    "provider",
                    "2026-01-03T00:00:00.000Z",
                    true,
                    true,
                ),
            )?;
            seed_key(
                transaction,
                &key_record(
                    "other-provider-key",
                    "other",
                    "2026-01-04T00:00:00.000Z",
                    true,
                    false,
                ),
            )?;
            Ok(())
        })
        .await
        .expect("seed provider keys");

        let (mut cursor, first) = ProviderApiKeyCursor::start(&db, "provider")
            .await
            .expect("query first key")
            .expect("provider has usable keys");
        assert_eq!(first.id, expected[0]);
        let mut ids = vec![first.id];
        while let Some(key) = cursor.next(&db).await.expect("query next key") {
            ids.push(key.id);
        }
        assert_eq!(ids, expected);
        assert!(cursor.next(&db).await.expect("exhausted cursor").is_none());
        drop(db);
        let _ = fs::remove_dir_all(path);
    }

    #[tokio::test]
    async fn skips_unusable_keys_that_share_a_creation_timestamp() {
        let path =
            std::env::temp_dir().join(format!("exoroute-provider-keys-{}", uuid::Uuid::new_v4()));
        let db = crate::infra::db::connect(&path).await.expect("open LMDB");
        let created_at = "2026-03-01T12:00:00.000Z";
        db.write(|transaction| {
            for (id, enabled, invalid) in [
                ("key-a", false, false),
                ("key-b", true, true),
                ("key-c", true, false),
            ] {
                seed_key(
                    transaction,
                    &key_record(id, "provider", created_at, enabled, invalid),
                )?;
            }
            Ok(())
        })
        .await
        .expect("seed provider keys");

        let (mut cursor, first) = ProviderApiKeyCursor::start(&db, "provider")
            .await
            .expect("query first key")
            .expect("provider has a usable key");
        assert_eq!(first.id, "key-c");
        assert!(cursor.next(&db).await.expect("query next key").is_none());
        drop(db);
        let _ = fs::remove_dir_all(path);
    }

    #[tokio::test]
    async fn reports_no_keys_for_a_provider_without_usable_credentials() {
        let path =
            std::env::temp_dir().join(format!("exoroute-provider-keys-{}", uuid::Uuid::new_v4()));
        let db = crate::infra::db::connect(&path).await.expect("open LMDB");
        db.write(|transaction| {
            seed_key(
                transaction,
                &key_record("key-a", "provider", "2026-04-01T00:00:00.000Z", true, true),
            )
        })
        .await
        .expect("seed provider keys");

        assert!(
            ProviderApiKeyCursor::start(&db, "provider")
                .await
                .expect("query keys")
                .is_none()
        );
        drop(db);
        let _ = fs::remove_dir_all(path);
    }

    fn ordered_keys() -> Vec<(String, String)> {
        (1..=4)
            .map(|index| {
                (
                    format!("key-{index}"),
                    format!("2026-05-01T00:00:00.00{index}Z"),
                )
            })
            .collect()
    }

    fn order_position(id: &str, created_at: &str) -> String {
        crate::infra::db::provider_api_key_order_key("provider", &format!("{created_at}/{id}"))
            .expect("ordering position")
    }

    async fn seed_ordered_keys(db: &Database, keys: &[(String, String)]) {
        let keys = keys.to_vec();
        db.write(move |transaction| {
            for (id, created_at) in &keys {
                seed_key(
                    transaction,
                    &key_record(id, "provider", created_at, true, false),
                )?;
            }
            Ok(())
        })
        .await
        .expect("seed provider keys");
    }

    async fn cursor_ids(
        cursor: &mut ProviderApiKeyCursor,
        db: &Database,
        first: String,
    ) -> Vec<String> {
        let mut ids = vec![first];
        while let Some(key) = cursor.next(db).await.expect("query next key") {
            ids.push(key.id);
        }
        ids
    }

    #[tokio::test]
    async fn rotating_cursor_continues_after_a_stored_position_and_wraps_once() {
        let path =
            std::env::temp_dir().join(format!("exoroute-provider-keys-{}", uuid::Uuid::new_v4()));
        let db = crate::infra::db::connect(&path).await.expect("open LMDB");
        let keys = ordered_keys();
        seed_ordered_keys(&db, &keys).await;

        let stored = order_position(&keys[1].0, &keys[1].1);
        let (mut cursor, first) =
            ProviderApiKeyCursor::start_rotating(&db, "provider", Some(&stored))
                .await
                .expect("query rotating key")
                .expect("provider has a key after the stored position");
        assert_eq!(first.id, "key-3");
        assert_eq!(
            cursor_ids(&mut cursor, &db, first.id).await,
            ["key-3", "key-4", "key-1", "key-2"]
        );
        assert!(cursor.next(&db).await.expect("exhausted cursor").is_none());
        drop(db);
        let _ = fs::remove_dir_all(path);
    }

    #[tokio::test]
    async fn rotating_cursor_reports_no_key_past_the_last_position_so_the_caller_restarts() {
        let path =
            std::env::temp_dir().join(format!("exoroute-provider-keys-{}", uuid::Uuid::new_v4()));
        let db = crate::infra::db::connect(&path).await.expect("open LMDB");
        let keys = ordered_keys();
        seed_ordered_keys(&db, &keys).await;

        let last = order_position(&keys[3].0, &keys[3].1);
        assert!(
            ProviderApiKeyCursor::start_rotating(&db, "provider", Some(&last))
                .await
                .expect("query rotating key")
                .is_none()
        );

        let (mut cursor, first) = ProviderApiKeyCursor::start_rotating(&db, "provider", None)
            .await
            .expect("query first key")
            .expect("provider has usable keys");
        assert_eq!(first.id, "key-1");
        assert_eq!(
            cursor_ids(&mut cursor, &db, first.id).await,
            ["key-1", "key-2", "key-3", "key-4"]
        );
        drop(db);
        let _ = fs::remove_dir_all(path);
    }

    #[tokio::test]
    async fn sequential_cursor_never_wraps_around_the_fallback_order() {
        let path =
            std::env::temp_dir().join(format!("exoroute-provider-keys-{}", uuid::Uuid::new_v4()));
        let db = crate::infra::db::connect(&path).await.expect("open LMDB");
        let keys = ordered_keys();
        seed_ordered_keys(&db, &keys).await;

        let (mut cursor, first) = ProviderApiKeyCursor::start(&db, "provider")
            .await
            .expect("query first key")
            .expect("provider has usable keys");
        assert_eq!(first.id, "key-1");
        assert_eq!(
            cursor_ids(&mut cursor, &db, first.id).await,
            ["key-1", "key-2", "key-3", "key-4"]
        );
        assert!(cursor.next(&db).await.expect("exhausted cursor").is_none());
        drop(db);
        let _ = fs::remove_dir_all(path);
    }
}
