use super::*;

pub(crate) type RestoreTaskResult = Result<(), (bool, &'static str)>;

pub(crate) fn spawn_restore_reconciliation_task(
    state: AppState,
    import_path: PathBuf,
    confirm_unlimited: bool,
    previous_required: bool,
    auth_guard: tokio::sync::OwnedMutexGuard<()>,
    import_permit: tokio::sync::OwnedSemaphorePermit,
    cleanup: TempDirectory,
) -> tokio::task::JoinHandle<RestoreTaskResult> {
    tokio::spawn(async move {
        let _auth_guard = auth_guard;
        let _import_permit = import_permit;
        let _cleanup = cleanup;
        let result =
            restore_application_data_with_runtime(&state, &import_path, confirm_unlimited).await;
        if let Err(message) = result {
            state
                .admin
                .admin_password_change_required
                .store(previous_required, std::sync::atomic::Ordering::Release);
            return Err((false, message));
        }

        // Keep reconciliation in this owned task: dropping the HTTP handler
        // must not leave restored credentials/settings paired with old RAM
        // sessions or a stale forced-password-change flag.
        state.clear_admin_sessions().await;
        let database_requires_change = match crate::infra::db::admin_password_change_required(
            &state.db,
        )
        .await
        {
            Ok(value) => value,
            Err(error) => {
                tracing::error!(%error, "could not reconcile admin password state after backup restore");
                return Err((
                    true,
                    "The backup was restored, but authentication state could not be reconciled. Restart ExoRoute before logging in.",
                ));
            }
        };
        let requires_change = database_requires_change.unwrap_or_else(|| {
            state
                .admin_key
                .try_read()
                .ok()
                .and_then(|key| key.clone())
                .as_deref()
                .is_some_and(super::auth::password_needs_change)
        });
        state
            .admin
            .admin_password_change_required
            .store(requires_change, std::sync::atomic::Ordering::Release);
        Ok(())
    })
}

#[cfg(test)]
pub(crate) async fn restore_application_data(
    database: &Database,
    import_path: &Path,
) -> Result<(), &'static str> {
    restore_application_data_inner(database, import_path, None, false)
        .await
        .map(|_| ())
}

pub(crate) async fn restore_application_data_with_runtime(
    state: &AppState,
    import_path: &Path,
    confirm_unlimited: bool,
) -> Result<(), &'static str> {
    let _limits_guard = state
        .runtime
        .gateway_resource_limits_update_lock
        .lock()
        .await;
    let _operational_guard = state.runtime.operational_settings_update_lock.lock().await;
    let _output_styles_guard = state.runtime.output_styles_update_lock.lock().await;
    restore_application_data_inner(&state.db, import_path, Some(state), confirm_unlimited)
        .await
        .map(|_| ())
}

async fn restore_application_data_inner(
    database: &Database,
    import_path: &Path,
    runtime_state: Option<&AppState>,
    confirm_unlimited: bool,
) -> Result<RestoredRuntimeSettings, &'static str> {
    let path = import_path.to_path_buf();
    let allow_private_provider_urls =
        runtime_state.is_some_and(|state| state.config.allow_private_provider_urls);
    let (mut entries, mut restored) = tokio::task::spawn_blocking(move || {
        let file_len = fs::metadata(&path)
            .map_err(|error| {
                tracing::warn!(%error, "could not inspect LMDB backup file");
                "The uploaded file could not be read."
            })?
            .len();
        if file_len > MAX_DATABASE_IMPORT_BYTES {
            return Err("The database file exceeds the 512 MB upload limit.");
        }
        let payload = decode_backup_file(&path)?;
        let restored =
            validate_backup_payload(&payload, allow_private_provider_urls, confirm_unlimited)?;
        let mut entries = payload.entries;
        normalize_provider_defaults(&mut entries)?;
        rebuild_backup_secondary_indexes(&mut entries)?;
        Ok::<_, &'static str>((entries, restored))
    })
    .await
    .map_err(|error| {
        tracing::error!(%error, "LMDB backup validation task failed");
        "Database import failed. The current database was left unchanged."
    })??;
    let current_output_styles = database
        .read(|transaction| transaction.get::<Record>(Table::OutputStyles, "singleton"))
        .await
        .map_err(|error| {
            tracing::warn!(%error, "could not read current output style settings before restore");
            "Database import failed. The current database was left unchanged."
        })?
        .ok_or_else(|| {
            tracing::warn!("current output style settings record is missing before restore");
            "Database import failed. The current database was left unchanged."
        })?;
    let current_output_styles = crate::infra::db::output_styles_from_record(&current_output_styles)
        .map_err(|error| {
            tracing::warn!(%error, "current output style settings are invalid before restore");
            "Database import failed. The current database was left unchanged."
        })?;
    let next_revision = current_output_styles
        .revision
        .checked_add(1)
        .ok_or("Database import failed. The current database was left unchanged.")?;
    if let Some(imported) = restored.output_styles.as_mut() {
        imported.revision = next_revision;
    } else {
        let mut preserved = current_output_styles.clone();
        preserved.revision = next_revision;
        restored.output_styles = Some(preserved);
    }
    let output_styles = restored
        .output_styles
        .as_ref()
        .ok_or("Database import failed. The current database was left unchanged.")?;
    let record = crate::infra::db::output_styles_record(output_styles).map_err(|error| {
        tracing::warn!(%error, "could not encode restored output style settings");
        "Database import failed. The current database was left unchanged."
    })?;
    if let Some(entry) = entries
        .iter_mut()
        .find(|entry| entry.table == Table::OutputStyles && entry.key == "singleton")
    {
        entry.value = bincode::serialize(&record)
            .map_err(|_| "Database import failed. The current database was left unchanged.")?;
    } else {
        entries.push(SnapshotEntry {
            table: Table::OutputStyles,
            key: "singleton".to_owned(),
            value: bincode::serialize(&record)
                .map_err(|_| "Database import failed. The current database was left unchanged.")?,
        });
    }
    database
        .replace_snapshot_entries(entries)
        .await
        .map_err(|error| {
            tracing::warn!(%error, "LMDB backup transaction failed; current data was retained");
            "Database import failed. The current database was left unchanged."
        })?;
    if let Some(state) = runtime_state {
        // Publish synchronously after the durable transaction commits, before
        // the next await can leave database and runtime settings inconsistent.
        state.apply_gateway_resource_limits(restored.gateway_resource_limits);
        state.apply_operational_settings(restored.operational_settings);
        if let Some(output_styles) = restored.output_styles.as_ref() {
            state.apply_output_styles(output_styles.clone());
        }
        state.clear_restored_provider_runtime_state().await;
        state.provider_deletion_notify.notify_one();
        if let Err(error) = super::providers::refresh_enabled_provider_count(state).await {
            tracing::warn!(%error, "could not refresh the overview upstream count after database import");
        }
    }
    Ok(restored)
}
