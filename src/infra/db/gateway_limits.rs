//! Gateway resource-limit persistence.
//!
//! Owns the singleton load/save path for `GatewayResourceLimits`; revision
//! compare-and-swap for other settings lives in their own modules.

use super::SINGLETON_KEY;
use crate::{
    config::GatewayResourceLimits,
    infra::storage::{Database, Record, StorageError, Table},
};

pub async fn load_gateway_resource_limits(
    database: &Database,
) -> Result<GatewayResourceLimits, StorageError> {
    let record = database
        .read(|transaction| transaction.get::<Record>(Table::GatewayResourceLimits, SINGLETON_KEY))
        .await?
        .ok_or_else(|| {
            StorageError::Invalid("gateway resource limit record is missing".to_owned())
        })?;
    super::settings_records::gateway_limits_from_record(&record)
}

pub async fn save_gateway_resource_limits(
    database: &Database,
    limits: GatewayResourceLimits,
) -> Result<(), StorageError> {
    limits
        .validate()
        .map_err(|message| StorageError::Invalid(message.to_owned()))?;
    let record = super::settings_records::gateway_limits_record(limits)?;
    database
        .write(move |transaction| {
            transaction.put(Table::GatewayResourceLimits, SINGLETON_KEY, &record)
        })
        .await
}
