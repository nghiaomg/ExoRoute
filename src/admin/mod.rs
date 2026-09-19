//! Admin dashboard API barrel.
//!
//! This module only declares feature submodules and re-exports their public
//! surface. Handler logic lives in the submodules: `errors` for shared
//! error constructors, `middleware` for auth gates, `overview`/`metrics`
//! for process-level reads, and one module per dashboard domain.
//!
//! Handlers validate and serialize HTTP contracts, then delegate persistence
//! and runtime changes to the owning domain/state modules. Provider adapters,
//! LMDB transaction details, and gateway orchestration do not belong here.

use crate::{
    protocol::UpstreamProtocol,
    provider_adapters::{self},
    security::egress,
    security::{decrypt_secret, encrypt_secret, secure_eq, token_hash},
    state::{AppState, PendingProviderAuthFlow, ProviderAuthFlowStatus},
};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};

use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, HeaderValue, Request, StatusCode, Uri, header},
    middleware::{self},
    response::{Html, IntoResponse, Response},
    routing::{delete, get, post, put},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use futures_util::StreamExt;
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{io, net::SocketAddr, sync::atomic::Ordering};
use tokio::{net::TcpListener, sync::watch, task::JoinSet, time::Duration};

mod api_keys;
mod auth;
mod combos;
mod cookie_policy;
mod database;
mod errors;
mod gates;
mod metrics;
mod model_probes;
mod overview;
mod provider_api_key_auth;
mod provider_auth;
pub(crate) mod providers;
mod request_logs;
mod router;
mod sessions;
mod settings;
mod statistics;
#[cfg(test)]
mod tests;
mod update_check;

pub(crate) use errors::{ApiResult, fail, internal, internal_message, resource_id};
pub(crate) use gates::{rate_limit_admin_login, require_admin};
pub(crate) use metrics::prometheus_metrics;
pub(crate) use overview::overview;
pub(crate) use provider_auth::provider_auth_status;
pub(crate) use provider_auth::{
    MAX_PROVIDER_AUTH_CALLBACK_BODY_BYTES, complete_provider_auth_callback,
    complete_provider_auth_callback_for_flow,
};
pub(crate) use provider_auth::{list_provider_presets, start_provider_auth};
pub(crate) use router::router;

pub(crate) fn cleanup_stale_database_temp_dirs(app_dir: &std::path::Path) -> io::Result<usize> {
    database::cleanup_stale_temp_directories(app_dir)
}

#[cfg(test)]
pub(crate) use auth::hash_admin_password;
pub(crate) use auth::password_needs_change;
#[cfg(test)]
use auth::{MAX_ADMIN_PASSWORD_HASH_BYTES, is_supported_admin_password_hash};
#[cfg(test)]
pub(crate) use providers::{
    ProviderInput, normalized_provider_model_prefix, normalized_provider_model_prefix_for_adapter,
    validate_provider,
};
