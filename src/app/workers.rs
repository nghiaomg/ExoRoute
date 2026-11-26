use super::*;

pub(crate) async fn request_log_retention_worker(state: AppState) {
    let mut shutdown = state.shutdown_receiver();
    loop {
        let mut failed = false;
        loop {
            if *shutdown.borrow() {
                return;
            }
            let settings = state.operational_settings().settings;
            match crate::infra::db::prune_request_logs_batch(
                &state.db,
                settings.request_log_retention_days,
                settings.request_log_max_rows,
            )
            .await
            {
                Ok(true) => tokio::task::yield_now().await,
                Ok(false) => break,
                Err(error) => {
                    tracing::warn!(%error, "request-log retention failed; will retry");
                    failed = true;
                    break;
                }
            }
        }
        if failed {
            tokio::select! {
                _ = shutdown.changed() => return,
                _ = state.request_log_retention_notify.notified() => {},
                _ = tokio::time::sleep(std::time::Duration::from_secs(60)) => {},
            }
        } else {
            tokio::select! {
                _ = shutdown.changed() => return,
                _ = state.request_log_retention_notify.notified() => {},
                _ = tokio::time::sleep(std::time::Duration::from_secs(60 * 60)) => {},
            }
        }
    }
}
