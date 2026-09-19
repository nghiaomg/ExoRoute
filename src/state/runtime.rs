use crate::{
    config::{GatewayResourceLimits, OperationalSettings},
    infra::db::OperationalSettingsRecord,
    support::output_styles::{self, OutputStylesSnapshot},
};
use std::sync::{Arc, RwLock as StdRwLock, atomic::AtomicUsize};
use tokio::sync::Mutex;

/// Runtime configuration and the locks that serialize its durable updates.
/// Keeping these handles together makes the settings lifecycle explicit while
/// preserving AppState's existing accessors and persistence ordering.
#[derive(Clone)]
pub(crate) struct RuntimeState {
    pub(crate) operational_settings: Arc<StdRwLock<OperationalSettingsRecord>>,
    pub(crate) operational_env_settings: Arc<Result<OperationalSettings, String>>,
    pub(crate) operational_settings_update_lock: Arc<Mutex<()>>,
    pub(crate) output_styles: Arc<StdRwLock<OutputStylesSnapshot>>,
    pub(crate) output_styles_update_lock: Arc<Mutex<()>>,
    pub(crate) output_styles_metrics: Arc<output_styles::Metrics>,
    pub(crate) gateway_resource_limits: Arc<StdRwLock<GatewayResourceLimits>>,
    pub(crate) gateway_resource_limits_update_lock: Arc<Mutex<()>>,
    pub(crate) gateway_provider_max_concurrency: Arc<AtomicUsize>,
}
