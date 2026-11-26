//! Zero-restart provider catalog reloading.
//!
//! Two independent entry points share one validation path:
//!   * [`watch_provider_catalog`] polls the YAML file and reloads only when its
//!     fingerprint changes, so an invalid file left on disk is reported once.
//!   * [`terminal_reload_listener`] offers a manual `[r]` + Enter hotkey when a
//!     real terminal is attached.
//!
//! Both call `AppState::reload_provider_presets`, which validates the whole file
//! before swapping the live snapshot. A failed reload therefore leaves the last
//! known-good catalog active.

use crate::config::provider_catalog::ProviderCatalogError;
use crate::state::{AppState, ReloadSummary};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tokio::sync::watch;

/// Default polling cadence for the user-editable provider catalog.
pub(crate) const DEFAULT_WATCH_POLL_INTERVAL: Duration = Duration::from_millis(1500);
/// Minimum accepted polling cadence; also prevents a zero interval from
/// panicking inside `tokio::time::interval`.
const MIN_WATCH_POLL_INTERVAL: Duration = Duration::from_millis(1);
const SOURCE_LABEL: &str = "providers.yaml";

/// What one metadata probe observed. `symlink_metadata` deliberately does not
/// follow links: a link is a distinct state and `reload_provider_presets`
/// rejects it cleanly.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FileFingerprint {
    modified: Option<SystemTime>,
    len: u64,
    exists: bool,
    is_symlink: bool,
}

fn fingerprint(path: &Path) -> FileFingerprint {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => FileFingerprint {
            modified: metadata.modified().ok(),
            len: metadata.len(),
            exists: true,
            is_symlink: metadata.file_type().is_symlink(),
        },
        Err(_) => FileFingerprint {
            modified: None,
            len: 0,
            exists: false,
            is_symlink: false,
        },
    }
}

/// Runs the validating swap off a Tokio worker: YAML parsing and the two
/// `std::sync::RwLock` writes are synchronous and bounded by the catalog limit.
async fn reload_in_background(state: &AppState) -> Result<ReloadSummary, ProviderCatalogError> {
    let state = state.clone();
    tokio::task::spawn_blocking(move || state.reload_provider_presets())
        .await
        .unwrap_or_else(|error| {
            Err(ProviderCatalogError::Io(std::io::Error::other(format!(
                "provider catalog reload task failed: {error}"
            ))))
        })
}

/// Builds the exact terminal line for a reload outcome. Keeping this pure makes
/// the success/error wording directly testable without capturing stdout.
fn reload_notice(
    source: &str,
    result: &Result<ReloadSummary, ProviderCatalogError>,
) -> (String, bool) {
    match result {
        Ok(summary) => {
            let provider_count = summary.provider_count;
            let overlaid = if summary.overlaid_providers.is_empty() {
                "none".to_owned()
            } else {
                summary.overlaid_providers.join(", ")
            };
            (
                format!("[{source}] Reloaded: {provider_count} providers (overlaid: {overlaid})"),
                true,
            )
        }
        Err(error) => (
            format!("[{source}] Reload failed ({error}) — keeping previous cached catalog"),
            false,
        ),
    }
}

fn report_reload(result: Result<ReloadSummary, ProviderCatalogError>) {
    let (notice, succeeded) = reload_notice(SOURCE_LABEL, &result);
    match &result {
        Ok(summary) => tracing::info!(
            provider_count = summary.provider_count,
            overlaid = ?summary.overlaid_providers,
            "provider catalog reloaded from YAML"
        ),
        Err(error) => {
            tracing::error!(%error, "provider catalog reload failed; keeping previous cached catalog")
        }
    }
    if succeeded {
        println!("{notice}");
    } else {
        eprintln!("{notice}");
    }
}

/// Normalizes both a real shutdown signal and a closed channel into "stop".
async fn shutdown_changed(shutdown: &mut watch::Receiver<bool>) -> bool {
    match shutdown.changed().await {
        Ok(()) => *shutdown.borrow(),
        Err(_) => true,
    }
}

/// One poll step. Returns `None` when the file fingerprint is unchanged from
/// the last *attempted* reload, which is the anti-log-spam guarantee. The
/// fingerprint is recorded before the reload runs, so an invalid file that
/// stays untouched is attempted (and reported) exactly once.
async fn poll_step(
    state: &AppState,
    path: &Path,
    last: &mut Option<FileFingerprint>,
) -> Option<Result<ReloadSummary, ProviderCatalogError>> {
    let current = fingerprint(path);
    if last.as_ref() == Some(&current) {
        return None;
    }
    *last = Some(current);
    Some(reload_in_background(state).await)
}

/// Polls `path` until the application shutdown signal fires.
///
/// `path` must be the same file `AppState` loaded at construction
/// (`config.app_dir.join("providers.yaml")`). Callers pass it explicitly so
/// the watcher and the state can never disagree about which file is live.
pub(crate) async fn watch_provider_catalog(
    state: AppState,
    path: PathBuf,
    poll_interval: Duration,
    mut shutdown: watch::Receiver<bool>,
) {
    let poll_interval = poll_interval.max(MIN_WATCH_POLL_INTERVAL);
    let mut ticker = tokio::time::interval(poll_interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // Start from the current state so a boot-time reload is not reported as a
    // change; only edits made while the process is running are announced.
    let mut last = Some(fingerprint(&path));
    loop {
        tokio::select! {
            biased;
            stop = shutdown_changed(&mut shutdown) => {
                if stop { return; }
            }
            _ = ticker.tick() => {
                if let Some(result) = poll_step(&state, &path, &mut last).await {
                    report_reload(result);
                }
            }
        }
    }
}

/// Result of interpreting one terminal input line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineAction {
    Reloaded,
    Ignored,
    Closed,
}

async fn handle_terminal_line(state: &AppState, line: Option<&str>) -> LineAction {
    let Some(input) = line else {
        return LineAction::Closed;
    };
    if !input.trim().eq_ignore_ascii_case("r") {
        return LineAction::Ignored;
    }
    report_reload(reload_in_background(state).await);
    LineAction::Reloaded
}

/// Reads terminal lines on a dedicated OS thread and forwards them over a
/// bounded channel. A plain thread (rather than `spawn_blocking`) means runtime
/// shutdown never waits on a thread parked in `read_line`; the channel receiver
/// is dropped on shutdown, and the next send fails and ends the thread.
fn spawn_terminal_input_reader() -> tokio::sync::mpsc::Receiver<Option<String>> {
    let (sender, receiver) = tokio::sync::mpsc::channel(8);
    let spawned = std::thread::Builder::new()
        .name("exoroute-provider-reload-input".to_owned())
        .spawn(move || {
            let stdin = std::io::stdin();
            let mut line = String::new();
            loop {
                line.clear();
                match stdin.read_line(&mut line) {
                    Ok(0) => {
                        let _ = sender.blocking_send(None);
                        return;
                    }
                    Ok(_) => {
                        if sender.blocking_send(Some(line.clone())).is_err() {
                            return;
                        }
                    }
                    Err(error) => {
                        tracing::warn!(%error, "could not read terminal input for manual provider reload");
                        return;
                    }
                }
            }
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "could not start terminal provider reload reader");
    }
    receiver
}

/// Listens for `[r]` + Enter on an attached terminal until shutdown.
///
/// Line input is intentional: no raw mode means no termios state to restore on
/// `Ctrl+C`, and no new Cargo dependencies. When stdin is not a TTY (headless,
/// systemd, tests) the task returns without touching stdin at all.
pub(crate) async fn terminal_reload_listener(
    state: AppState,
    path: PathBuf,
    mut shutdown: watch::Receiver<bool>,
) {
    if !std::io::stdin().is_terminal() {
        return;
    }
    println!("Press [r] + Enter to reload {} manually", path.display());
    let mut input = spawn_terminal_input_reader();
    loop {
        tokio::select! {
            biased;
            stop = shutdown_changed(&mut shutdown) => {
                if stop { return; }
            }
            line = input.recv() => {
                match line {
                    Some(Some(line)) => {
                        if handle_terminal_line(&state, Some(&line)).await == LineAction::Closed {
                            return;
                        }
                    }
                    Some(None) | None => return,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::support::test_support::TestDatabase;
    use std::time::Instant;

    async fn fixture() -> (TestDatabase, AppState, PathBuf) {
        let database = TestDatabase::open().await;
        let path = database.config().app_dir.join("providers.yaml");
        let state = AppState::new(database.config(), database.db.clone());
        (database, state, path)
    }

    fn write_overlay(path: &Path, body: &str) {
        std::fs::write(path, body).expect("write provider catalog");
    }

    fn overlay(model: &str) -> String {
        format!("providers:\n  custom:\n    default_models: [{model}]\n")
    }

    fn snapshot_models(state: &AppState) -> Vec<String> {
        state
            .provider_presets_snapshot()
            .find("custom")
            .expect("built-in custom preset is always present")
            .default_models
            .clone()
    }

    async fn wait_for_models(state: &AppState, model: &str) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !snapshot_models(state).iter().any(|entry| entry == model) {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for model {model} in live snapshot"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    #[tokio::test]
    async fn watcher_applies_a_valid_edit_without_a_restart() {
        let (database, state, path) = fixture().await;
        let watcher_shutdown = state.shutdown_receiver();
        let mut observer_shutdown = state.shutdown_receiver();
        let task = tokio::spawn(watch_provider_catalog(
            state.clone(),
            path.clone(),
            Duration::from_millis(50),
            watcher_shutdown,
        ));
        // Let the spawned task take its startup fingerprint before editing;
        // otherwise the edit is indistinguishable from the initial file state.
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!snapshot_models(&state).iter().any(|m| m == "watched-a"));
        write_overlay(&path, &overlay("watched-a"));
        wait_for_models(&state, "watched-a").await;
        state.request_shutdown();
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .expect("watcher must stop on shutdown")
            .expect("watcher task must not panic");
        assert!(observer_shutdown.changed().await.is_ok());
        drop(database);
    }

    #[tokio::test]
    async fn invalid_edit_keeps_the_cached_snapshot_and_is_not_retried_until_changed() {
        let (_database, state, path) = fixture().await;
        write_overlay(&path, &overlay("kept-a"));
        let mut last = None;
        let valid = poll_step(&state, &path, &mut last)
            .await
            .expect("valid first edit is attempted");
        assert!(valid.is_ok());
        assert!(snapshot_models(&state).iter().any(|m| m == "kept-a"));

        write_overlay(&path, "providers:\n  custom:\n    mystery: 1\n");
        let invalid = poll_step(&state, &path, &mut last)
            .await
            .expect("invalid edit is attempted once");
        assert!(invalid.is_err());
        assert!(snapshot_models(&state).iter().any(|m| m == "kept-a"));
        let (notice, succeeded) = reload_notice(SOURCE_LABEL, &invalid);
        assert!(!succeeded);
        assert!(notice.contains("keeping previous cached catalog"));

        assert!(
            poll_step(&state, &path, &mut last).await.is_none(),
            "an unchanged invalid file must not be reported repeatedly"
        );
        assert!(snapshot_models(&state).iter().any(|m| m == "kept-a"));
    }

    #[tokio::test]
    async fn a_valid_edit_recovers_after_an_invalid_one() {
        let (_database, state, path) = fixture().await;
        write_overlay(&path, &overlay("before-a"));
        let mut last = None;
        assert!(poll_step(&state, &path, &mut last).await.unwrap().is_ok());
        assert!(snapshot_models(&state).iter().any(|m| m == "before-a"));

        write_overlay(&path, "providers:\n  custom:\n    mystery: 1\n");
        assert!(poll_step(&state, &path, &mut last).await.unwrap().is_err());
        assert!(snapshot_models(&state).iter().any(|m| m == "before-a"));

        write_overlay(&path, &overlay("recovered-a"));
        let recovered = poll_step(&state, &path, &mut last).await.unwrap();
        assert!(recovered.is_ok());
        assert!(snapshot_models(&state).iter().any(|m| m == "recovered-a"));
    }

    #[tokio::test]
    async fn terminal_hotkey_reloads_on_r_and_r_and_ignores_other_input() {
        let (_database, state, path) = fixture().await;
        write_overlay(&path, &overlay("manual-a"));
        assert_eq!(
            handle_terminal_line(&state, Some("r")).await,
            LineAction::Reloaded
        );
        assert!(snapshot_models(&state).iter().any(|m| m == "manual-a"));

        write_overlay(&path, "providers:\n  custom:\n    mystery: 1\n");
        assert_eq!(
            handle_terminal_line(&state, Some("R")).await,
            LineAction::Reloaded
        );
        assert!(snapshot_models(&state).iter().any(|m| m == "manual-a"));

        write_overlay(&path, &overlay("manual-b"));
        assert_eq!(
            handle_terminal_line(&state, Some("other")).await,
            LineAction::Ignored
        );
        assert!(!snapshot_models(&state).iter().any(|m| m == "manual-b"));
        assert_eq!(handle_terminal_line(&state, None).await, LineAction::Closed);
    }

    #[tokio::test]
    async fn reload_notice_reports_counts_and_overlays() {
        let summary = ReloadSummary {
            reloaded: true,
            provider_count: 7,
            overlaid_providers: vec!["custom".to_owned(), "openai".to_owned()],
        };
        let (notice, succeeded) = reload_notice(SOURCE_LABEL, &Ok(summary));
        assert!(succeeded);
        assert_eq!(
            notice,
            "[providers.yaml] Reloaded: 7 providers (overlaid: custom, openai)"
        );
        let error = ProviderCatalogError::Invalid("bad key".to_owned());
        let expected = error.to_string();
        let (notice, succeeded) = reload_notice(SOURCE_LABEL, &Err(error));
        assert!(!succeeded);
        assert_eq!(
            notice,
            format!(
                "[providers.yaml] Reload failed ({expected}) — keeping previous cached catalog"
            )
        );
    }

    #[tokio::test]
    async fn non_tty_terminal_listener_returns_without_waiting_for_input() {
        if std::io::stdin().is_terminal() {
            return; // this test harness cannot simulate a detached stdin on a TTY
        }
        let (database, state, path) = fixture().await;
        let (_sender, shutdown) = watch::channel(false);
        tokio::time::timeout(
            Duration::from_millis(500),
            terminal_reload_listener(state, path, shutdown),
        )
        .await
        .expect("non-TTY listener must return immediately");
        drop(database);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn fingerprint_distinguishes_deletion_and_symlinks() {
        let (database, _state, path) = fixture().await;
        write_overlay(&path, &overlay("probe-a"));
        let present = fingerprint(&path);
        assert!(present.exists);
        assert!(!present.is_symlink);
        assert_eq!(present.len, std::fs::metadata(&path).expect("stat").len());

        std::fs::remove_file(&path).expect("remove catalog");
        let missing = fingerprint(&path);
        assert!(!missing.exists);
        assert_ne!(missing, present);

        let link_target = path.with_extension("target.yaml");
        write_overlay(&link_target, &overlay("probe-a"));
        std::os::unix::fs::symlink(&link_target, &path).expect("create catalog symlink");
        let linked = fingerprint(&path);
        assert!(linked.exists);
        assert!(linked.is_symlink);
        assert_ne!(linked, missing);
        drop(database);
    }
}
