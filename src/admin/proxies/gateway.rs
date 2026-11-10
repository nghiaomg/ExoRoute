//! Proxy endpoint expansion used by request preparation. The resulting list is
//! bounded by the same limits enforced on save/import and preserves the stored
//! priority/ordinal order, with nested proxies expanded in place.

use super::*;
use crate::{
    config::{MAX_PROXY_ENDPOINTS, MAX_PROXY_NESTING_DEPTH},
    infra::storage::{ReadTxn, StorageError, Table},
};
use std::collections::HashSet;

pub(crate) struct ProxyEndpointConfig {
    pub id: String,
    pub endpoint_id: String,
    pub record: Record,
}

pub(crate) struct ProxyEndpointGroup {
    pub proxy_id: String,
    pub strategy: String,
    pub enabled: bool,
    pub endpoints: Vec<ProxyEndpointConfig>,
}

pub(crate) fn load_proxy_endpoint_configs(
    transaction: &ReadTxn<'_, '_>,
    proxy_id: &str,
) -> Result<ProxyEndpointGroup, StorageError> {
    let Some(proxy) = transaction.get::<Record>(Table::Proxies, proxy_id)? else {
        return Err(StorageError::NotFound);
    };
    let strategy = proxy.text("strategy")?.to_owned();
    let proxy_enabled = proxy.boolean("enabled")?;
    if !proxy_enabled {
        return Ok(ProxyEndpointGroup {
            proxy_id: proxy_id.to_owned(),
            strategy,
            enabled: false,
            endpoints: Vec::new(),
        });
    }
    let mut path = HashSet::new();
    let mut scanned = 0_usize;
    let endpoints = expand_proxy(
        transaction,
        proxy_id,
        0,
        &mut path,
        &mut scanned,
    )?;
    Ok(ProxyEndpointGroup { proxy_id: proxy_id.to_owned(), strategy, enabled: proxy.boolean("enabled")?, endpoints })
}

fn expand_proxy(
    transaction: &ReadTxn<'_, '_>,
    proxy_id: &str,
    depth: usize,
    path: &mut HashSet<String>,
    scanned: &mut usize,
) -> Result<Vec<ProxyEndpointConfig>, StorageError> {
    if depth > MAX_PROXY_NESTING_DEPTH {
        return Err(StorageError::Invalid(
            "proxy reference graph exceeds its nesting limit".to_owned(),
        ));
    }
    if !path.insert(proxy_id.to_owned()) {
        return Err(StorageError::Invalid(
            "proxy reference graph contains a cycle".to_owned(),
        ));
    }
    let result = (|| {
        let Some(proxy) = transaction.get::<Record>(Table::Proxies, proxy_id)? else {
            return Err(StorageError::NotFound);
        };
        if !proxy.boolean("enabled")? {
            return Ok(Vec::new());
        }
        let rows = transaction.scan_prefix::<Record>(
            Table::ProxyEndpoints,
            &proxy_endpoint_prefix(proxy_id)?,
            MAX_PROXY_ENDPOINTS,
        )?;
        *scanned = scanned.saturating_add(rows.len());
        if *scanned > MAX_PROXY_ENDPOINTS * (MAX_PROXY_NESTING_DEPTH + 1) {
            return Err(StorageError::Invalid(
                "proxy reference graph exceeds its scan limit".to_owned(),
            ));
        }
        let mut flattened = Vec::with_capacity(rows.len());
        for (endpoint_key, endpoint) in rows {
            if !endpoint.boolean("enabled")? {
                continue;
            }
            if let Some(reference) = endpoint.optional_text("ref_proxy_id")? {
                let nested = expand_proxy(transaction, reference, depth + 1, path, scanned)?;
                flattened.extend(nested);
            } else {
                flattened.push(ProxyEndpointConfig {
                    id: endpoint_key,
                    endpoint_id: endpoint.text("endpoint_id")?.to_owned(),
                    record: endpoint,
                });
            }
            if flattened.len() > MAX_PROXY_ENDPOINTS {
                return Err(StorageError::Invalid(
                    "expanded proxy has too many endpoints".to_owned(),
                ));
            }
        }
        Ok(flattened)
    })();
    path.remove(proxy_id);
    result
}
