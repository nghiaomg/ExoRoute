use super::*;

pub(super) async fn authenticate_client(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<String, Response> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .or_else(|| headers.get("x-api-key").and_then(|h| h.to_str().ok()));
    let Some(token) = token else {
        return Err(gateway_error(
            StatusCode::UNAUTHORIZED,
            "gateway API key required",
        ));
    };
    if token.is_empty() || token.len() > 4096 {
        return Err(gateway_error(
            StatusCode::UNAUTHORIZED,
            "invalid gateway API key",
        ));
    }
    let candidate = token_hash(token);
    let token_index = format!("token/{}", hex_bytes(&candidate));
    let lookup_hash = candidate.to_vec();
    let key_id = state
        .db
        .read(move |transaction| {
            let Some(id) = transaction.get::<String>(Table::ApiKeyTokenIndex, &token_index)? else {
                return Ok(None);
            };
            let Some(record) = transaction.get::<Record>(Table::ApiKeys, &id)? else {
                return Err(StorageError::Invalid(
                    "API key token index points to a missing key".to_owned(),
                ));
            };
            if !record.boolean("enabled")? || !secure_eq(record.bytes("token_hash")?, &lookup_hash)
            {
                return Ok(None);
            }
            Ok(Some(id))
        })
        .await
        .map_err(gateway_database_error)?;
    let Some(key_id) = key_id else {
        return Err(gateway_error(
            StatusCode::UNAUTHORIZED,
            "invalid gateway API key",
        ));
    };
    // `last_used_at` is an administrative hint; throttle writes so it does not
    // turn every accepted gateway request into a serialized LMDB writer.
    if API_KEY_LAST_USED_UPDATES.check(&key_id).allowed {
        let update_id = key_id.clone();
        let timestamp = crate::infra::db::utc_timestamp_now().map_err(gateway_database_error)?;
        if let Err(error) = state
            .db
            .write(move |transaction| {
                let Some(mut record) = transaction.get::<Record>(Table::ApiKeys, &update_id)?
                else {
                    return Ok(());
                };
                record.insert("last_used_at", Field::Text(timestamp.clone()));
                transaction.put(Table::ApiKeys, &update_id, &record)
            })
            .await
        {
            tracing::debug!(%error, api_key_id = %key_id, "could not persist API key usage timestamp");
        }
    }
    Ok(key_id)
}

fn hex_bytes(value: &[u8]) -> String {
    let mut encoded = String::with_capacity(value.len().saturating_mul(2));
    for byte in value {
        use std::fmt::Write as _;
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}
