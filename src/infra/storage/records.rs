//! Typed record primitives for the LMDB store: table identifiers, the
//! dynamically typed `Record`/`Field` model, snapshot entries, and the shared
//! storage error type.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// Errors surfaced by the storage layer. Variants are intentionally coarse so
/// callers can react to retryable, conflict, and permanent failures without
/// inspecting LMDB internals.
#[derive(Debug)]
pub enum StorageError {
    /// The write could not be admitted because another writer holds the
    /// single-writer gate or the transaction budget is exhausted.
    Busy,
    /// The environment map is full and could not grow further.
    MapFull,
    /// A write-if-absent or compare-and-swap expectation did not hold.
    Conflict,
    /// A required record or table is missing.
    NotFound,
    /// Input failed structural validation.
    Invalid(String),
    /// A record failed to decode or encode.
    Codec(String),
    /// A spawned blocking task failed or was cancelled before completing.
    Task(String),
    /// The storage engine reported an unrecoverable backend error.
    Backend(String),
    /// An I/O error outside the LMDB environment.
    Io(std::io::Error),
}

impl StorageError {
    pub fn is_busy(&self) -> bool {
        matches!(self, StorageError::Busy)
    }

    pub fn is_conflict(&self) -> bool {
        matches!(self, StorageError::Conflict)
    }
}

impl fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StorageError::Busy => write!(formatter, "storage is busy"),
            StorageError::MapFull => write!(formatter, "storage map is full"),
            StorageError::Conflict => write!(formatter, "conflicting concurrent write"),
            StorageError::NotFound => write!(formatter, "record not found"),
            StorageError::Invalid(message) => write!(formatter, "invalid record: {message}"),
            StorageError::Codec(message) => write!(formatter, "record codec failure: {message}"),
            StorageError::Task(message) => write!(formatter, "storage task failed: {message}"),
            StorageError::Backend(message) => {
                write!(formatter, "storage backend error: {message}")
            }
            StorageError::Io(error) => write!(formatter, "storage I/O error: {error}"),
        }
    }
}

impl std::error::Error for StorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StorageError::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for StorageError {
    fn from(error: std::io::Error) -> Self {
        StorageError::Io(error)
    }
}

impl From<heed::Error> for StorageError {
    fn from(error: heed::Error) -> Self {
        match error {
            heed::Error::Mdb(MdbError::MapFull) => StorageError::MapFull,
            heed::Error::Mdb(MdbError::NotFound) => StorageError::NotFound,
            heed::Error::Mdb(_) => StorageError::Backend(error.to_string()),
            _ => StorageError::Backend(error.to_string()),
        }
    }
}

use heed::MdbError;

/// The named LMDB databases backing the gateway. `ALL` is the authoritative
/// order used for environment setup and full-snapshot iteration; index tables
/// are named with an `Index` suffix by convention.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum Table {
    Meta,
    Providers,
    ProviderApiKeys,
    ProviderModels,
    ProviderUsageMeters,
    Routes,
    RouteTargets,
    ApiKeys,
    AdminAuth,
    AdminSessionFamilies,
    AdminRefreshTokens,
    SessionIndex,
    StreamRuns,
    StreamEvents,
    StreamRunApiKeyIndex,
    RequestLogs,
    RequestLogIndex,
    UsageMinutes,
    UsageWindows,
    UsageExpiryCursor,
    StatisticsState,
    StatisticsIndex,
    OutputStyles,
    OperationalSettings,
    GatewayResourceLimits,
    Indexes,
    ApiKeyIndex,
    ApiKeyTokenIndex,
    ProviderNameIndex,
    ProviderApiKeyIndex,
    ProviderApiKeyAvailabilityIndex,
    ProviderApiKeyCreatedIndex,
    ProviderModelIndex,
    RouteNameIndex,
    RouteTargetProviderIndex,
}

impl Table {
    /// Every table in environment-creation order. The order is part of the
    /// on-disk format: backup archives reference tables by this index.
    pub const ALL: &'static [Table] = &[
        Table::Meta,
        Table::Providers,
        Table::ProviderApiKeys,
        Table::ProviderModels,
        Table::ProviderUsageMeters,
        Table::Routes,
        Table::RouteTargets,
        Table::ApiKeys,
        Table::AdminAuth,
        Table::AdminSessionFamilies,
        Table::AdminRefreshTokens,
        Table::SessionIndex,
        Table::StreamRuns,
        Table::StreamEvents,
        Table::StreamRunApiKeyIndex,
        Table::RequestLogs,
        Table::RequestLogIndex,
        Table::UsageMinutes,
        Table::UsageWindows,
        Table::UsageExpiryCursor,
        Table::StatisticsState,
        Table::StatisticsIndex,
        Table::OutputStyles,
        Table::OperationalSettings,
        Table::GatewayResourceLimits,
        Table::Indexes,
        Table::ApiKeyIndex,
        Table::ApiKeyTokenIndex,
        Table::ProviderNameIndex,
        Table::ProviderApiKeyIndex,
        Table::ProviderApiKeyAvailabilityIndex,
        Table::ProviderApiKeyCreatedIndex,
        Table::ProviderModelIndex,
        Table::RouteNameIndex,
        Table::RouteTargetProviderIndex,
    ];

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Table::Meta => "meta",
            Table::Providers => "providers",
            Table::ProviderApiKeys => "provider_api_keys",
            Table::ProviderModels => "provider_models",
            Table::ProviderUsageMeters => "provider_usage_meters",
            Table::Routes => "routes",
            Table::RouteTargets => "route_targets",
            Table::ApiKeys => "api_keys",
            Table::AdminAuth => "admin_auth",
            Table::AdminSessionFamilies => "admin_session_families",
            Table::AdminRefreshTokens => "admin_refresh_tokens",
            Table::SessionIndex => "session_index",
            Table::StreamRuns => "stream_runs",
            Table::StreamEvents => "stream_events",
            Table::StreamRunApiKeyIndex => "stream_run_api_key_index",
            Table::RequestLogs => "request_logs",
            Table::RequestLogIndex => "request_log_index",
            Table::UsageMinutes => "usage_minutes",
            Table::UsageWindows => "usage_windows",
            Table::UsageExpiryCursor => "usage_expiry_cursor",
            Table::StatisticsState => "statistics_state",
            Table::StatisticsIndex => "statistics_index",
            Table::OutputStyles => "output_styles",
            Table::OperationalSettings => "operational_settings",
            Table::GatewayResourceLimits => "gateway_resource_limits",
            Table::Indexes => "indexes",
            Table::ApiKeyIndex => "api_key_index",
            Table::ApiKeyTokenIndex => "api_key_token_index",
            Table::ProviderNameIndex => "provider_name_index",
            Table::ProviderApiKeyIndex => "provider_api_key_index",
            Table::ProviderApiKeyAvailabilityIndex => "provider_api_key_availability_index",
            Table::ProviderApiKeyCreatedIndex => "provider_api_key_created_index",
            Table::ProviderModelIndex => "provider_model_index",
            Table::RouteNameIndex => "route_name_index",
            Table::RouteTargetProviderIndex => "route_target_provider_index",
        }
    }
}

/// A dynamically typed field value. `Null` is distinct from absence so a
/// record can explicitly store "tested with no result".
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Field {
    Null,
    Bool(bool),
    I64(i64),
    Text(String),
    Bytes(Vec<u8>),
}

/// A flat string-keyed record. Records are the unit of backup/restore and are
/// validated (field count, name length, value size) before any write.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub(crate) fields: BTreeMap<String, Field>,
}

impl Record {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, name: impl Into<String>, value: Field) -> Self {
        self.fields.insert(name.into(), value);
        self
    }

    pub fn insert(&mut self, name: impl Into<String>, value: Field) {
        self.fields.insert(name.into(), value);
    }

    pub fn field(&self, name: &str) -> Option<&Field> {
        self.fields.get(name)
    }

    pub fn text(&self, name: &str) -> Result<&str, StorageError> {
        match self.fields.get(name) {
            Some(Field::Text(value)) => Ok(value),
            Some(Field::Null) | None => Err(StorageError::Invalid(format!(
                "record field '{name}' is missing"
            ))),
            _ => Err(StorageError::Invalid(format!(
                "record field '{name}' is not text"
            ))),
        }
    }

    pub fn optional_text(&self, name: &str) -> Result<Option<&str>, StorageError> {
        match self.fields.get(name) {
            Some(Field::Text(value)) => Ok(Some(value)),
            Some(Field::Null) | None => Ok(None),
            _ => Err(StorageError::Invalid(format!(
                "record field '{name}' is not text"
            ))),
        }
    }

    pub fn integer(&self, name: &str) -> Result<i64, StorageError> {
        match self.fields.get(name) {
            Some(Field::I64(value)) => Ok(*value),
            Some(Field::Null) | None => Err(StorageError::Invalid(format!(
                "record field '{name}' is missing"
            ))),
            _ => Err(StorageError::Invalid(format!(
                "record field '{name}' is not an integer"
            ))),
        }
    }

    pub fn optional_integer(&self, name: &str) -> Result<Option<i64>, StorageError> {
        match self.fields.get(name) {
            Some(Field::I64(value)) => Ok(Some(*value)),
            Some(Field::Null) | None => Ok(None),
            _ => Err(StorageError::Invalid(format!(
                "record field '{name}' is not an integer"
            ))),
        }
    }

    pub fn boolean(&self, name: &str) -> Result<bool, StorageError> {
        match self.fields.get(name) {
            Some(Field::Bool(value)) => Ok(*value),
            Some(Field::Null) | None => Err(StorageError::Invalid(format!(
                "record field '{name}' is missing"
            ))),
            _ => Err(StorageError::Invalid(format!(
                "record field '{name}' is not a boolean"
            ))),
        }
    }

    pub fn bytes(&self, name: &str) -> Result<&[u8], StorageError> {
        match self.fields.get(name) {
            Some(Field::Bytes(value)) => Ok(value),
            Some(Field::Null) | None => Err(StorageError::Invalid(format!(
                "record field '{name}' is missing"
            ))),
            _ => Err(StorageError::Invalid(format!(
                "record field '{name}' is not bytes"
            ))),
        }
    }

    pub fn optional_bytes(&self, name: &str) -> Result<Option<&[u8]>, StorageError> {
        match self.fields.get(name) {
            Some(Field::Bytes(value)) => Ok(Some(value)),
            Some(Field::Null) | None => Ok(None),
            _ => Err(StorageError::Invalid(format!(
                "record field '{name}' is not bytes"
            ))),
        }
    }
}

/// One key/value pair of a full snapshot, used by backup export/import.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SnapshotEntry {
    pub table: Table,
    pub key: String,
    pub value: Vec<u8>,
}
