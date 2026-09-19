use rand::{RngCore, rngs::OsRng};
use std::{
    env, fs,
    io::{self, Write},
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};

const KIB: usize = 1024;
const MIB: usize = 1024 * KIB;

mod env_secrets;
mod limits;
mod operational;
mod paths;
#[cfg(windows)]
mod windows;

pub use env_secrets::{ensure_admin_key, ensure_master_key, remove_env_value};
pub use limits::GatewayResourceLimits;
pub use operational::{OperationalSettings, UpstreamSettings};
pub use paths::{
    app_dir, ensure_no_reparse_path_components, ensure_private_dir, ensure_private_file,
    initialize_app_dir,
};

pub(crate) use operational::*;
pub(crate) use paths::*;
#[cfg(windows)]
pub(crate) use windows::*;

#[derive(Clone)]
pub struct Config {
    pub app_dir: PathBuf,
    pub bind: SocketAddr,
    pub data_dir: PathBuf,
    pub database_path: PathBuf,
    pub admin_key: Option<String>,
    pub master_key: Option<[u8; 32]>,
    pub allow_private_provider_urls: bool,
    pub trust_proxy_headers: bool,
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub stream_idle_timeout: Duration,
    pub circuit_breaker_enabled: bool,
    pub circuit_breaker_threshold: u32,
    pub circuit_breaker_cooldown: Duration,
}

impl Config {
    pub fn from_env_deferred_operational(app_dir: PathBuf) -> Result<Self, String> {
        Self::from_env_with_operational_settings(app_dir, false)
    }

    fn from_env_with_operational_settings(
        app_dir: PathBuf,
        parse_operational_settings: bool,
    ) -> Result<Self, String> {
        let host = env::var("EXOROUTE_HOST").unwrap_or_else(|_| "127.0.0.1".to_owned());
        let port = parse_env("EXOROUTE_PORT", 8686u16)?;
        let bind = format!("{host}:{port}")
            .parse()
            .map_err(|e| format!("invalid EXOROUTE_HOST/EXOROUTE_PORT: {e}"))?;
        let data_dir = env::var("EXOROUTE_DATA_DIR")
            .map(|value| resolve_path(&app_dir, value))
            .unwrap_or_else(|_| app_dir.clone());
        let database_path = env::var("EXOROUTE_DATABASE_PATH")
            .map(|value| resolve_path(&app_dir, value))
            .unwrap_or_else(|_| data_dir.join("exoroute.lmdb"));
        let master_key = env::var("EXOROUTE_MASTER_KEY").ok().filter(|v| !v.is_empty()).map(|value| {
            let decoded = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &value).unwrap_or_else(|_| value.as_bytes().to_vec());
            decoded.try_into().map_err(|_| "EXOROUTE_MASTER_KEY must be base64 or raw data representing exactly 32 bytes".to_owned())
        }).transpose()?;
        let operational = if parse_operational_settings {
            OperationalSettings::from_env()?
        } else {
            OperationalSettings::default()
        };
        Ok(Self {
            app_dir,
            bind,
            data_dir,
            database_path,
            admin_key: env::var("EXOROUTE_ADMIN_KEY")
                .ok()
                .filter(|v| !v.is_empty()),
            master_key,
            allow_private_provider_urls: env::var("EXOROUTE_ALLOW_PRIVATE_PROVIDER_URLS")
                .map(|value| value.eq_ignore_ascii_case("true"))
                .unwrap_or(false),
            trust_proxy_headers: env::var("EXOROUTE_TRUST_PROXY_HEADERS")
                .map(|value| value.eq_ignore_ascii_case("true"))
                .unwrap_or(false),
            connect_timeout: operational.connect_timeout,
            request_timeout: operational.request_timeout,
            stream_idle_timeout: operational.stream_idle_timeout,
            circuit_breaker_enabled: operational.circuit_breaker_enabled,
            circuit_breaker_threshold: operational.circuit_breaker_threshold,
            circuit_breaker_cooldown: operational.circuit_breaker_cooldown,
        })
    }
}

/// Returns the configured admin password, creating a random first-run secret
/// when the settings file has no usable value yet.
#[cfg(test)]
mod tests;
