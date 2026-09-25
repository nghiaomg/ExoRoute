use super::*;

#[tokio::test]
async fn unified_telemetry_writer_increments_api_key_counts() {
    let database = TestDatabase::open().await;
    for id in ["key-a", "key-b"] {
        let id = id.to_owned();
        let record = Record::new()
            .with("id", Field::Text(id.clone()))
            .with("name", Field::Text(id.clone()))
            .with("token_hash", Field::Bytes(id.as_bytes().to_vec()))
            .with("enabled", Field::Bool(true))
            .with("created_at", Field::Text("2026-01-01T00:00:00Z".to_owned()))
            .with("last_used_at", Field::Null)
            .with("request_count", Field::I64(0))
            .with("request_count_generation", Field::I64(0));
        let key = id.clone();
        database
            .db
            .write(move |transaction| transaction.put_if_absent(Table::ApiKeys, &key, &record))
            .await
            .expect("insert API key");
    }
    let state = AppState::new(database.config(), database.db.clone());
    state.record_api_key_request("key-a".to_owned());
    state.record_api_key_request("key-b".to_owned());
    state.record_api_key_request("key-a".to_owned());
    state
        .telemetry
        .flush()
        .await
        .expect("flush telemetry queue");
    let counts = database
        .db
        .read(|transaction| {
            Ok((
                transaction
                    .get::<Record>(Table::ApiKeys, "key-a")?
                    .ok_or(crate::infra::storage::StorageError::NotFound)?
                    .integer("request_count")?,
                transaction
                    .get::<Record>(Table::ApiKeys, "key-b")?
                    .ok_or(crate::infra::storage::StorageError::NotFound)?
                    .integer("request_count")?,
            ))
        })
        .await
        .expect("read API key request counts");
    assert_eq!(counts, (2, 1));
}
