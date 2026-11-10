//! Reference graph for proxunty nesting.
//!
//! A proxy may embed another proxy, which is how one proxy groups several
//! others the same way a combo nests a combo. Embedding is a grouping
//! relationship, not a chained tunnel: at request time the graph is flattened to
//! a single ordered endpoint list and each endpoint is tried on its own.
//!
//! The walk mirrors the combo graph in `crate::admin::combos::references` — same
//! traits so the same traversal runs both as an async pre-check and inside the
//! write transaction — with proxy-specific bounds.

use super::*;
use crate::config::{MAX_PROXY_ENDPOINTS, MAX_PROXY_NESTING_DEPTH};
use crate::infra::storage::{ReadTxn, StorageError, Table, WriteTxn};
use std::collections::HashMap;

use super::storage::{proxy_endpoint_prefix, proxy_endpoint_ref_index_prefix};

/// Upper bound on the number of stored rows the walk may visit. Bounds work on a
/// hostile or corrupt graph rather than a legitimate one.
pub(crate) const MAX_PROXY_NESTING_SCAN: usize = 4096;

pub(crate) trait ProxyStore {
    fn proxy_node(&self, proxy_id: &str) -> Result<Option<ProxyNode>, StorageError>;
}

pub(crate) trait ProxyReferences {
    fn referencing_proxies(&self, proxy_id: &str) -> Result<Vec<String>, StorageError>;
    fn proxy_name(&self, proxy_id: &str) -> Result<Option<String>, StorageError>;
}

/// The routing-relevant shape of one stored proxy.
#[derive(Clone, Default)]
pub(crate) struct ProxyNode {
    pub(crate) enabled: bool,
    /// Enabled server endpoints declared here.
    pub(crate) server_endpoints: usize,
    /// Enabled embedded proxy ids, in stored order.
    pub(crate) references: Vec<String>,
}

/// Reads one node from raw transactions.
fn node_from_endpoints(
    proxy: Option<Record>,
    endpoints: impl IntoIterator<Item = (String, Record)>,
) -> Result<Option<ProxyNode>, StorageError> {
    let Some(proxy) = proxy else {
        return Ok(None);
    };
    let mut node = ProxyNode {
        enabled: proxy.boolean("enabled")?,
        ..ProxyNode::default()
    };
    if !node.enabled {
        // A disabled proxy contributes nothing to routing, so its endpoints do
        // not need decoding.
        return Ok(Some(node));
    }
    for (_, endpoint) in endpoints {
        if !endpoint.boolean("enabled")? {
            continue;
        }
        match endpoint.optional_text("ref_proxy_id")? {
            Some(reference) => node.references.push(reference.to_owned()),
            None => node.server_endpoints += 1,
        }
    }
    Ok(Some(node))
}

trait ProxyNodeReader {
    fn read_proxy(&self, proxy_id: &str) -> Result<Option<Record>, StorageError>;
    fn read_endpoints(&self, proxy_id: &str) -> Result<Vec<(String, Record)>, StorageError>;
}

impl ProxyNodeReader for ReadTxn<'_, '_> {
    fn read_proxy(&self, proxy_id: &str) -> Result<Option<Record>, StorageError> {
        self.get::<Record>(Table::Proxies, proxy_id)
    }

    fn read_endpoints(&self, proxy_id: &str) -> Result<Vec<(String, Record)>, StorageError> {
        self.scan_prefix::<Record>(
            Table::ProxyEndpoints,
            &proxy_endpoint_prefix(proxy_id)?,
            MAX_PROXY_ENDPOINTS,
        )
    }
}

impl ProxyNodeReader for WriteTxn<'_, '_> {
    fn read_proxy(&self, proxy_id: &str) -> Result<Option<Record>, StorageError> {
        self.get::<Record>(Table::Proxies, proxy_id)
    }

    fn read_endpoints(&self, proxy_id: &str) -> Result<Vec<(String, Record)>, StorageError> {
        self.scan_prefix::<Record>(
            Table::ProxyEndpoints,
            &proxy_endpoint_prefix(proxy_id)?,
            MAX_PROXY_ENDPOINTS,
        )
    }
}

fn load_node<S: ProxyNodeReader>(
    store: &S,
    proxy_id: &str,
) -> Result<Option<ProxyNode>, StorageError> {
    let proxy = store.read_proxy(proxy_id)?;
    let endpoints = store.read_endpoints(proxy_id)?;
    node_from_endpoints(proxy, endpoints)
}

impl ProxyStore for ReadTxn<'_, '_> {
    fn proxy_node(&self, proxy_id: &str) -> Result<Option<ProxyNode>, StorageError> {
        load_node(self, proxy_id)
    }
}

impl ProxyStore for WriteTxn<'_, '_> {
    fn proxy_node(&self, proxy_id: &str) -> Result<Option<ProxyNode>, StorageError> {
        load_node(self, proxy_id)
    }
}

impl ProxyReferences for ReadTxn<'_, '_> {
    fn referencing_proxies(&self, proxy_id: &str) -> Result<Vec<String>, StorageError> {
        referencing_proxies(self, proxy_id)
    }

    fn proxy_name(&self, proxy_id: &str) -> Result<Option<String>, StorageError> {
        self.get::<Record>(Table::Proxies, proxy_id)?
            .map(|record| record.text("name").map(str::to_owned))
            .transpose()
    }
}

impl ProxyReferences for WriteTxn<'_, '_> {
    fn referencing_proxies(&self, proxy_id: &str) -> Result<Vec<String>, StorageError> {
        referencing_proxies(self, proxy_id)
    }

    fn proxy_name(&self, proxy_id: &str) -> Result<Option<String>, StorageError> {
        self.get::<Record>(Table::Proxies, proxy_id)?
            .map(|record| record.text("name").map(str::to_owned))
            .transpose()
    }
}

/// Reads the reverse index written by `save_proxy` for every embedding proxy.
fn referencing_proxies<S>(store: &S, proxy_id: &str) -> Result<Vec<String>, StorageError>
where
    S: ProxyEndpointRefScan,
{
    let entries = store.scan_reference_index(&proxy_endpoint_ref_index_prefix(proxy_id)?)?;
    let mut ids = Vec::with_capacity(entries.len());
    for (_, referencing) in entries {
        if !ids.contains(&referencing) {
            ids.push(referencing);
        }
    }
    Ok(ids)
}

/// The reverse-index read needed by [`referencing_proxies`].
trait ProxyEndpointRefScan {
    fn scan_reference_index(
        &self,
        prefix: &str,
    ) -> Result<Vec<(String, String)>, StorageError>;
}

impl ProxyEndpointRefScan for ReadTxn<'_, '_> {
    fn scan_reference_index(
        &self,
        prefix: &str,
    ) -> Result<Vec<(String, String)>, StorageError> {
        self.scan_prefix::<String>(Table::ProxyEndpointRefIndex, prefix, MAX_PROXY_NESTING_SCAN)
    }
}

impl ProxyEndpointRefScan for WriteTxn<'_, '_> {
    fn scan_reference_index(
        &self,
        prefix: &str,
    ) -> Result<Vec<(String, String)>, StorageError> {
        self.scan_prefix::<String>(Table::ProxyEndpointRefIndex, prefix, MAX_PROXY_NESTING_SCAN)
    }
}

/// The not-yet-committed proxy being validated, built only from enabled
/// endpoints so validation matches request-time routing.
#[derive(Clone, Default)]
pub(super) struct PendingProxy {
    pub enabled: bool,
    pub server_endpoints: usize,
    pub references: Vec<String>,
}

impl PendingProxy {
    pub(super) fn from_input(input: &ProxyInput) -> Self {
        let mut pending = PendingProxy {
            enabled: input.enabled,
            ..PendingProxy::default()
        };
        if !pending.enabled {
            return pending;
        }
        for endpoint in &input.endpoints {
            if !endpoint.enabled {
                continue;
            }
            match endpoint.normalized_reference() {
                Some(reference) => pending.references.push(reference.to_owned()),
                None => pending.server_endpoints += 1,
            }
        }
        pending
    }
}

/// Why a proxy graph was rejected.
#[derive(Debug)]
pub(crate) enum ProxyReferenceIssue {
    SelfReference,
    Cycle { path: Vec<String> },
    Depth { proxy_id: String },
    Missing { proxy_id: String },
    TooManyEndpoints { expanded: usize },
    GraphTooLarge,
}

#[derive(Debug)]
pub(crate) enum ProxyReferenceError {
    Issue(ProxyReferenceIssue),
    Storage(StorageError),
}

impl From<StorageError> for ProxyReferenceError {
    fn from(error: StorageError) -> Self {
        ProxyReferenceError::Storage(error)
    }
}

impl ProxyReferenceError {
    pub(super) fn into_api(self) -> (axum::http::StatusCode, axum::Json<serde_json::Value>) {
        match self {
            ProxyReferenceError::Storage(error) => internal(error),
            ProxyReferenceError::Issue(issue) => fail(
                axum::http::StatusCode::BAD_REQUEST,
                proxy_issue_message(&issue),
            ),
        }
    }
}

pub(super) fn proxy_issue_message(issue: &ProxyReferenceIssue) -> String {
    match issue {
        ProxyReferenceIssue::SelfReference => {
            "a proxy endpoint cannot embed its own proxy".to_owned()
        }
        ProxyReferenceIssue::Cycle { path } => {
            format!("embedded proxies form a cycle: {}", path.join(" -> "))
        }
        ProxyReferenceIssue::Depth { proxy_id } => format!(
            "embedded proxies are nested deeper than the supported limit at '{}'",
            proxy_id
        ),
        ProxyReferenceIssue::Missing { proxy_id } => {
            format!("embedded proxy '{proxy_id}' does not exist")
        }
        ProxyReferenceIssue::TooManyEndpoints { expanded } => format!(
            "expanded proxy has {expanded} endpoints, above the supported limit of {}",
            MAX_PROXY_ENDPOINTS
        ),
        ProxyReferenceIssue::GraphTooLarge => {
            "embedded proxy graph is larger than the supported limit".to_owned()
        }
    }
}

/// Flattens one proxy graph, memoizing each subtree summary.
struct ReferenceWalk<'a, S: ProxyStore> {
    store: &'a S,
    memo: std::collections::HashMap<String, NodeSummary>,
    stack: Vec<String>,
    scanned: usize,
}

#[derive(Clone, Copy, Default)]
struct NodeSummary {
    endpoints: usize,
    depth: usize,
}

impl<'a, S: ProxyStore> ReferenceWalk<'a, S> {
    fn new(store: &'a S) -> Self {
        ReferenceWalk {
            store,
            memo: std::collections::HashMap::new(),
            stack: Vec::new(),
            scanned: 0,
        }
    }

    /// Expands the pending proxy and reports its endpoint total and depth.
    fn expand_pending(
        &mut self,
        proxy_id: &str,
        pending: &PendingProxy,
    ) -> Result<NodeSummary, ProxyReferenceError> {
        if !pending.enabled {
            return Ok(NodeSummary::default());
        }
        self.stack.push(proxy_id.to_owned());
        let summary = self.scan_children(&pending.references, pending.server_endpoints)?;
        self.stack.pop();
        Ok(summary)
    }

    fn scan_children(
        &mut self,
        references: &[String],
        server_endpoints: usize,
    ) -> Result<NodeSummary, ProxyReferenceError> {
        let mut endpoints = server_endpoints;
        let mut depth = 0;
        for reference in references {
            let child = self.visit(reference)?;
            endpoints = endpoints.saturating_add(child.endpoints);
            depth = depth.max(child.depth.saturating_add(1));
        }
        Ok(NodeSummary { endpoints, depth })
    }

    fn visit(&mut self, proxy_id: &str) -> Result<NodeSummary, ProxyReferenceError> {
        if let Some(summary) = self.memo.get(proxy_id) {
            return Ok(*summary);
        }
        if let Some(start) = self.stack.iter().position(|entry| entry == proxy_id) {
            let mut path = self.stack[start..].to_vec();
            path.push(proxy_id.to_owned());
            return Err(ProxyReferenceError::Issue(ProxyReferenceIssue::Cycle {
                path,
            }));
        }
        self.scanned += 1;
        if self.scanned > MAX_PROXY_NESTING_SCAN {
            return Err(ProxyReferenceError::Issue(
                ProxyReferenceIssue::GraphTooLarge,
            ));
        }
        let Some(node) = self.store.proxy_node(proxy_id)? else {
            return Err(ProxyReferenceError::Issue(
                ProxyReferenceIssue::Missing {
                    proxy_id: proxy_id.to_owned(),
                },
            ));
        };
        if !node.enabled {
            // A disabled child contributes nothing; memoize the empty summary so
            // repeated references stay cheap.
            self.memo.insert(proxy_id.to_owned(), NodeSummary::default());
            return Ok(NodeSummary::default());
        }
        if self.stack.len() > MAX_PROXY_NESTING_DEPTH {
            return Err(ProxyReferenceError::Issue(ProxyReferenceIssue::Depth {
                proxy_id: proxy_id.to_owned(),
            }));
        }
        self.stack.push(proxy_id.to_owned());
        let summary = self.scan_children(&node.references, node.server_endpoints)?;
        self.stack.pop();
        if summary.depth >= MAX_PROXY_NESTING_DEPTH {
            return Err(ProxyReferenceError::Issue(ProxyReferenceIssue::Depth {
                proxy_id: proxy_id.to_owned(),
            }));
        }
        self.memo.insert(proxy_id.to_owned(), summary);
        Ok(summary)
    }
}

/// Validates one pending proxy against committed storage.
pub(super) fn validate_pending_proxy<S: ProxyStore>(
    store: &S,
    proxy_id: &str,
    pending: &PendingProxy,
) -> Result<(), ProxyReferenceError> {
    if !pending.enabled {
        return Ok(());
    }
    if pending.references.iter().any(|reference| reference == proxy_id) {
        return Err(ProxyReferenceError::Issue(
            ProxyReferenceIssue::SelfReference,
        ));
    }
    let mut walk = ReferenceWalk::new(store);
    walk.stack.push(proxy_id.to_owned());
    let summary = walk.scan_children(&pending.references, pending.server_endpoints)?;
    walk.stack.pop();
    if summary.endpoints > MAX_PROXY_ENDPOINTS {
        return Err(ProxyReferenceError::Issue(
            ProxyReferenceIssue::TooManyEndpoints {
                expanded: summary.endpoints,
            },
        ));
    }
    Ok(())
}

pub(crate) fn validate_stored_proxy_graph<S: ProxyStore>(
    store: &S,
    proxy_ids: &[String],
) -> Result<(), ProxyReferenceError> {
    let mut all_nodes = HashMap::with_capacity(proxy_ids.len());
    for proxy_id in proxy_ids {
        let Some(node) = store.proxy_node(proxy_id)? else {
            return Err(ProxyReferenceError::Issue(ProxyReferenceIssue::Missing {
                proxy_id: proxy_id.clone(),
            }));
        };
        all_nodes.insert(proxy_id.clone(), node);
    }
    let graph_store = StoredProxyGraph { nodes: &all_nodes };
    for proxy_id in proxy_ids {
        let node = all_nodes.get(proxy_id).ok_or_else(|| {
            ProxyReferenceError::Issue(ProxyReferenceIssue::Missing {
                proxy_id: proxy_id.clone(),
            })
        })?;
        let pending = PendingProxy {
            enabled: node.enabled,
            server_endpoints: node.server_endpoints,
            references: node.references.clone(),
        };
        validate_pending_proxy(&graph_store, proxy_id, &pending)?;
    }
    Ok(())
}

struct StoredProxyGraph<'a> {
    nodes: &'a HashMap<String, ProxyNode>,
}

impl ProxyStore for StoredProxyGraph<'_> {
    fn proxy_node(&self, proxy_id: &str) -> Result<Option<ProxyNode>, StorageError> {
        Ok(self.nodes.get(proxy_id).cloned())
    }
}

/// Async pre-check that yields a precise 400 before the write is attempted.
pub(super) async fn validate_proxy_references(
    state: &AppState,
    proxy_id: String,
    pending: PendingProxy,
) -> Result<(), (axum::http::StatusCode, axum::Json<serde_json::Value>)> {
    state
        .db
        .read(move |transaction| {
            validate_pending_proxy(transaction, &proxy_id, &pending)
                .map_err(|error| match error {
                    ProxyReferenceError::Storage(error) => error,
                    ProxyReferenceError::Issue(issue) => {
                        StorageError::Invalid(proxy_issue_message(&issue))
                    }
                })
        })
        .await
        .map_err(|error| match error {
            StorageError::Invalid(message) => fail(axum::http::StatusCode::BAD_REQUEST, message),
            other => internal(other),
        })
}

/// Authoritative re-check inside the write transaction.
pub(super) fn ensure_reference_graph<S: ProxyStore>(
    store: &S,
    proxy_id: &str,
    pending: &PendingProxy,
) -> Result<(), StorageError> {
    validate_pending_proxy(store, proxy_id, pending).map_err(|error| match error {
        ProxyReferenceError::Storage(error) => error,
        ProxyReferenceError::Issue(_) => StorageError::Conflict,
    })
}

pub(super) fn ensure_proxy_update_graph<S: ProxyStore + ProxyReferences>(
    store: &S,
    proxy_id: &str,
    pending: &PendingProxy,
) -> Result<(), StorageError> {
    validate_proxy_candidate_and_ancestors(store, store, proxy_id, pending).map_err(|error| match error {
        ProxyReferenceError::Storage(error) => error,
        ProxyReferenceError::Issue(_) => StorageError::Conflict,
    })
}

/// Names of the proxies that embed one proxy, capped for a 409 message.
pub(super) fn referencing_proxy_names<S: ProxyReferences>(
    store: &S,
    proxy_id: &str,
) -> Result<Vec<String>, StorageError> {
    let references = store.referencing_proxies(proxy_id)?;
    if references.len() > MAX_PROXY_NESTING_SCAN {
        return Err(StorageError::Invalid(
            "proxy reference index is larger than the supported limit".to_owned(),
        ));
    }
    let mut names = Vec::new();
    for reference in references.into_iter().take(super::MAX_DELETE_BLOCKERS_NAMED) {
        names.push(match store.proxy_name(&reference)? {
            Some(name) => name,
            None => reference,
        });
    }
    Ok(names)
}
