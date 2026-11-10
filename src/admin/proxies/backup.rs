use super::*;
use super::references::{MAX_PROXY_NESTING_SCAN, ProxyReferenceError, ProxyReferenceIssue, validate_pending_proxy};
use crate::infra::storage::{ReadTxn, StorageError, Table};
use std::collections::{HashMap, HashSet};

pub(crate) struct BackupProxyGraph {
    pub nodes: HashMap<String, ProxyNode>,
}

impl ProxyStore for BackupProxyGraph {
    fn proxy_node(&self, proxy_id: &str) -> Result<Option<ProxyNode>, StorageError> {
        Ok(self.nodes.get(proxy_id).cloned())
    }
}

pub(crate) fn build_backup_proxy_graph(
    proxies: &HashMap<String, Record>,
    endpoint_records: &[(String, Record)],
) -> Result<BackupProxyGraph, &'static str> {
    let mut nodes = HashMap::with_capacity(proxies.len());
    for (id, record) in proxies {
        nodes.insert(id.clone(), ProxyNode {
            enabled: record.boolean("enabled").map_err(|_| "A proxy record in the backup is invalid.")?,
            server_endpoints: 0,
            references: Vec::new(),
        });
    }
    for (_, endpoint) in endpoint_records {
        let owner = endpoint.text("proxy_id").map_err(|_| "A proxy endpoint record in the backup is invalid.")?;
        let node = nodes.get_mut(owner).ok_or("A proxy endpoint refers to a proxy that is missing from the backup.")?;
        if !node.enabled || !endpoint.boolean("enabled").map_err(|_| "A proxy endpoint record in the backup is invalid.")? {
            continue;
        }
        if let Some(reference) = endpoint.optional_text("ref_proxy_id").map_err(|_| "A proxy endpoint record in the backup is invalid.")? {
            node.references.push(reference.to_owned());
        } else {
            node.server_endpoints += 1;
        }
    }
    Ok(BackupProxyGraph { nodes })
}

pub(crate) fn validate_backup_proxy_entries(
    entries: &[crate::infra::storage::SnapshotEntry],
) -> Result<(), &'static str> {
    let mut proxies = HashMap::<String, Record>::new();
    let mut endpoints = Vec::<(String, Record)>::new();
    let mut endpoint_ids = HashMap::<String, HashSet<String>>::new();
    let mut ordinals = HashMap::<String, HashSet<u32>>::new();
    let mut seen_priorities = HashMap::<String, HashSet<u32>>::new();
    for entry in entries {
        match entry.table {
            Table::Proxies => {
                let record: Record = bincode::deserialize(&entry.value)
                    .map_err(|_| "A proxy record in the backup is invalid.")?;
                let id = record.text("id").map_err(|_| "A proxy record in the backup is invalid.")?;
                let name = record.text("name").map_err(|_| "A proxy record in the backup is invalid.")?;
                let strategy = record.text("strategy").map_err(|_| "A proxy record in the backup is invalid.")?;
                let _ = record.boolean("enabled").map_err(|_| "A proxy record in the backup is invalid.")?;
                let _ = record.text("created_at").map_err(|_| "A proxy record in the backup is invalid.")?;
                let _ = record.text("updated_at").map_err(|_| "A proxy record in the backup is invalid.")?;
                if id != entry.key || id.trim().is_empty() || id.len() > 128
                    || id.bytes().any(|byte| byte.is_ascii_control())
                    || name.trim().is_empty() || name.len() > 256
                    || name.bytes().any(|byte| byte.is_ascii_control())
                    || !matches!(strategy, "priority" | "round_robin")
                    || proxies.insert(id.to_owned(), record).is_some()
                {
                    return Err("The backup contains an invalid or duplicate proxy record.");
                }
            }
            Table::ProxyEndpoints => {
                let record: Record = bincode::deserialize(&entry.value)
                    .map_err(|_| "A proxy endpoint record in the backup is invalid.")?;
                let owner = record.text("proxy_id").map_err(|_| "A proxy endpoint record in the backup is invalid.")?.to_owned();
                let endpoint_id = record.text("endpoint_id").map_err(|_| "A proxy endpoint record in the backup is invalid.")?;
                let (key_priority, ordinal) = proxy_endpoint_key_components(&entry.key, &owner)?;
                let priority = u32::try_from(record.integer("priority").map_err(|_| "A proxy endpoint record in the backup is invalid.")?)
                    .map_err(|_| "A proxy endpoint has an invalid priority.")?;
                if priority != key_priority || priority > MAX_PROXY_ENDPOINTS as u32
                    || endpoint_id.trim().is_empty() || endpoint_id.len() > 128
                    || endpoint_id.bytes().any(|byte| byte.is_ascii_control())
                    || !endpoint_ids.entry(owner.clone()).or_default().insert(endpoint_id.to_owned())
                    || !ordinals.entry(owner.clone()).or_default().insert(ordinal)
                    || !seen_priorities.entry(owner.clone()).or_default().insert(priority)
                {
                    return Err("The backup contains an invalid or duplicate proxy endpoint identity, priority, or order.");
                }
                let enabled = record.boolean("enabled").map_err(|_| "A proxy endpoint record in the backup is invalid.")?;
                let reference = record.optional_text("ref_proxy_id").map_err(|_| "A proxy endpoint record in the backup is invalid.")?;
                if let Some(reference) = reference {
                    if reference.trim().is_empty() || reference.len() > 128
                        || reference.bytes().any(|byte| byte.is_ascii_control())
                        || !null_text(&record, "url")? || !null_text(&record, "username")?
                        || !null_bytes(&record, "secret")? || !null_text(&record, "label")?
                    {
                        return Err("The backup contains an invalid embedded proxy endpoint.");
                    }
                } else {
                    let url = record.text("url").map_err(|_| "A proxy endpoint record in the backup is invalid.")?;
                    crate::admin::proxies::normalize_proxy_url(url)
                        .map_err(|_| "The backup contains an unsupported proxy URL.")?;
                    let username = record.optional_text("username").map_err(|_| "A proxy endpoint record in the backup is invalid.")?;
                    if username.is_some_and(|value| value.len() > 256 || value.bytes().any(|byte| byte.is_ascii_control())) {
                        return Err("The backup contains an invalid proxy username.");
                    }
                    if let Some(secret) = record.optional_bytes("secret").map_err(|_| "A proxy endpoint record in the backup is invalid.")?
                        && !(28..=1024).contains(&secret.len())
                    {
                        return Err("The backup contains an invalid encrypted proxy credential.");
                    }
                    let label = record.optional_text("label").map_err(|_| "A proxy endpoint record in the backup is invalid.")?;
                    if label.is_some_and(|value| value.len() > 64 || value.bytes().any(|byte| byte.is_ascii_control())) {
                        return Err("The backup contains an invalid proxy label.");
                    }
                }
                let _ = enabled;
                endpoints.push((entry.key.clone(), record));
            }
            _ => {}
        }
    }
    for (owner, positions) in &ordinals {
        if !proxies.contains_key(owner) {
            return Err("A proxy endpoint refers to a proxy that is missing from the backup.");
        }
        if positions.len() > MAX_PROXY_ENDPOINTS {
            return Err("A proxy in the backup has too many endpoints.");
        }
        if let Some(max_ordinal) = positions.iter().max().copied() {
            if usize::try_from(max_ordinal).ok().and_then(|n| n.checked_add(1)) != Some(positions.len())
                || !(0..=max_ordinal).all(|ordinal| positions.contains(&ordinal))
            {
                return Err("The backup contains non-contiguous proxy endpoint order.");
            }
        }
        if let Some(proxy) = proxies.get(owner) {
            let proxy_enabled = proxy.boolean("enabled").map_err(|_| "A proxy record in the backup is invalid.")?;
            if proxy_enabled && seen_priorities.get(owner).is_none_or(|priorities| priorities.is_empty()) {
                return Err("An enabled proxy in the backup must have an enabled endpoint.");
            }
        }
    }
    let graph = build_backup_proxy_graph(&proxies, &endpoints)?;
    let proxy_ids = proxies.keys().cloned().collect::<Vec<_>>();
    crate::admin::proxies::validate_stored_proxy_graph(&graph, &proxy_ids)
        .map_err(|_| "The backup contains a circular, too deeply nested, missing, or oversized proxy reference graph.")
}

fn null_text(record: &Record, field: &str) -> Result<bool, &'static str> {
    Ok(record.optional_text(field)
        .map_err(|_| "A proxy endpoint record in the backup is invalid.")?
        .is_none())
}

fn null_bytes(record: &Record, field: &str) -> Result<bool, &'static str> {
    Ok(record.optional_bytes(field)
        .map_err(|_| "A proxy endpoint record in the backup is invalid.")?
        .is_none())
}

/// Revalidates every parent affected by replacing one node in the stored graph.
pub(crate) fn validate_proxy_candidate_and_ancestors(
    store: &impl ProxyStore,
    references: &impl ProxyReferences,
    proxy_id: &str,
    pending: &PendingProxy,
) -> Result<(), ProxyReferenceError> {
    validate_pending_proxy(store, proxy_id, pending)?;
    let mut queue = references.referencing_proxies(proxy_id)?;
    let mut seen = HashSet::new();
    while let Some(parent_id) = queue.pop() {
        if !seen.insert(parent_id.clone()) { continue; }
        let Some(parent) = store.proxy_node(&parent_id)? else {
            return Err(ProxyReferenceError::Issue(ProxyReferenceIssue::Missing { proxy_id: parent_id }));
        };
        let pending_parent = PendingProxy {
            enabled: parent.enabled,
            server_endpoints: parent.server_endpoints,
            references: parent.references.clone(),
        };
        validate_pending_proxy(store, &parent_id, &pending_parent)?;
        queue.extend(references.referencing_proxies(&parent_id)?);
        if seen.len() > MAX_PROXY_NESTING_SCAN {
            return Err(ProxyReferenceError::Issue(ProxyReferenceIssue::GraphTooLarge));
        }
    }
    Ok(())
}

pub(crate) fn proxy_record_by_id(
    transaction: &ReadTxn<'_, '_>,
    proxy_id: &str,
) -> Result<Option<Record>, StorageError> {
    transaction.get::<Record>(Table::Proxies, proxy_id)
}
