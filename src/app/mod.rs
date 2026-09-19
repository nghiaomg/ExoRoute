//! Application bootstrap, process wiring, and startup output.
use crate::config::{Config, OperationalSettings};
use crate::infra::db::OperationalSettingsRecord;
use crate::state::AppState;
use axum::{
    Router, middleware,
    routing::{get, post},
};
use std::io::IsTerminal;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

pub(crate) mod banner;
pub(crate) mod bootstrap;
pub(crate) mod router;
pub(crate) mod workers;

const SERVER_SHUTDOWN_GRACE_PERIOD: std::time::Duration = std::time::Duration::from_secs(2);
pub(crate) const RUNTIME_SHUTDOWN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

pub(crate) use bootstrap::*;
pub(crate) use router::*;
pub(crate) use workers::*;

#[cfg(test)]
mod tests;
