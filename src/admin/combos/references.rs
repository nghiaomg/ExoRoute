//! Nested combo reference validation.
//!
//! A combo target may point at another combo, whose target list is substituted
//! in place when a request is routed. The reference graph therefore has to stay
//! a bounded forest: no self references, no cycles, bounded depth, and a
//! flattened target count that cannot exceed `MAX_ROUTE_TARGETS`.
//!
//! The traversal reads the graph through [`ComboStore`], so the same code
//! validates a pending save, re-checks it inside the save transaction where it
//! is authoritative, and validates an imported backup before that backup
//! replaces live data.

use super::*;
use crate::config::{MAX_COMBO_NESTING_DEPTH, MAX_ROUTE_TARGETS};
use crate::infra::storage::{ReadTxn, Record, StorageError, Table, WriteTxn};
use std::collections::HashMap;

/// Reachable combos one root may walk before it fails closed.
const MAX_COMBO_NESTING_SCAN: usize = 4_096;

/// Referencing combos named in a blocked deletion before the rest are counted.
pub(super) const MAX_DELETE_BLOCKERS_NAMED: usize = 5;

/// What one combo contributes when it is flattened: whether it is enabled, how
/// many provider targets it declares, and the combos it references, each as
/// stored order.
#[derive(Clone, Default)]
pub(crate) struct ComboNode {
    pub(crate) enabled: bool,
    /// Enabled provider targets declared here.
    pub(crate) provider_targets: usize,
    /// Enabled nested combo references declared here, in stored order.
    pub(crate) references: Vec<String>,
}

/// The reference graph's read side.
///
/// Implemented for the LMDB transactions and for an imported backup's in-memory
/// graph, so one traversal serves both. A validation-only graph maintains only
/// this side; the reverse lookup a blocked deletion needs is [`ComboReferences`].
pub(crate) trait ComboStore {
    /// The routing shape of a stored combo, or `None` when it is not stored.
    fn combo_node(&self, combo_id: &str) -> Result<Option<ComboNode>, StorageError>;
}

/// The reverse lookup a blocked deletion needs, kept separate because a graph
/// built only for validation does not maintain it.
pub(crate) trait ComboReferences {
    /// Route ids recorded in the combo reverse index as referencing the combo.
    fn referencing_routes(&self, combo_id: &str) -> Result<Vec<String>, StorageError>;

    /// The display name of a route, for the message of a blocked deletion.
    fn route_name(&self, combo_id: &str) -> Result<Option<String>, StorageError>;
}

/// Reads a combo's node from the target rows of one LMDB transaction.
///
/// A disabled combo contributes nothing to routing, so its rows are not read.
fn node_from_targets<T: ComboNodeReader>(
    store: &T,
    combo_id: &str,
) -> Result<Option<ComboNode>, StorageError> {
    let Some(route) = store.route_record(combo_id)? else {
        return Ok(None);
    };
    let mut node = ComboNode {
        enabled: route.boolean("enabled")?,
        ..ComboNode::default()
    };
    if !node.enabled {
        return Ok(Some(node));
    }
    let records = store.target_records(combo_id)?;
    for record in &records {
        if !record.boolean("enabled")? {
            continue;
        }
        match record.optional_text("combo_id")? {
            Some(child) => node.references.push(child.to_owned()),
            None => node.provider_targets += 1,
        }
    }
    Ok(Some(node))
}

/// The two raw reads [`node_from_targets`] performs, so the read and write
/// transaction impls do not repeat the traversal itself.
trait ComboNodeReader {
    fn route_record(&self, combo_id: &str) -> Result<Option<Record>, StorageError>;
    fn target_records(&self, combo_id: &str) -> Result<Vec<Record>, StorageError>;
}

impl ComboNodeReader for ReadTxn<'_, '_> {
    fn route_record(&self, combo_id: &str) -> Result<Option<Record>, StorageError> {
        self.get::<Record>(Table::Routes, combo_id)
    }

    fn target_records(&self, combo_id: &str) -> Result<Vec<Record>, StorageError> {
        Ok(self
            .scan_prefix::<Record>(
                Table::RouteTargets,
                &storage::route_target_prefix(combo_id)?,
                MAX_ROUTE_TARGETS_PAGE,
            )?
            .into_iter()
            .map(|(_, record)| record)
            .collect())
    }
}

impl ComboNodeReader for WriteTxn<'_, '_> {
    fn route_record(&self, combo_id: &str) -> Result<Option<Record>, StorageError> {
        self.get::<Record>(Table::Routes, combo_id)
    }

    fn target_records(&self, combo_id: &str) -> Result<Vec<Record>, StorageError> {
        Ok(self
            .scan_prefix::<Record>(
                Table::RouteTargets,
                &storage::route_target_prefix(combo_id)?,
                MAX_ROUTE_TARGETS_PAGE,
            )?
            .into_iter()
            .map(|(_, record)| record)
            .collect())
    }
}

impl ComboStore for ReadTxn<'_, '_> {
    fn combo_node(&self, combo_id: &str) -> Result<Option<ComboNode>, StorageError> {
        node_from_targets(self, combo_id)
    }
}

impl ComboStore for WriteTxn<'_, '_> {
    fn combo_node(&self, combo_id: &str) -> Result<Option<ComboNode>, StorageError> {
        node_from_targets(self, combo_id)
    }
}

impl ComboReferences for ReadTxn<'_, '_> {
    fn referencing_routes(&self, combo_id: &str) -> Result<Vec<String>, StorageError> {
        Ok(self
            .scan_prefix::<String>(
                Table::RouteTargetComboIndex,
                &storage::route_target_combo_index_prefix(combo_id)?,
                MAX_COMBO_NESTING_SCAN + 1,
            )?
            .into_iter()
            .map(|(_, route_id)| route_id)
            .collect())
    }

    fn route_name(&self, combo_id: &str) -> Result<Option<String>, StorageError> {
        let Some(record) = self.get::<Record>(Table::Routes, combo_id)? else {
            return Ok(None);
        };
        Ok(record.optional_text("name")?.map(str::to_owned))
    }
}

impl ComboReferences for WriteTxn<'_, '_> {
    fn referencing_routes(&self, combo_id: &str) -> Result<Vec<String>, StorageError> {
        Ok(self
            .scan_prefix::<String>(
                Table::RouteTargetComboIndex,
                &storage::route_target_combo_index_prefix(combo_id)?,
                MAX_COMBO_NESTING_SCAN + 1,
            )?
            .into_iter()
            .map(|(_, route_id)| route_id)
            .collect())
    }

    fn route_name(&self, combo_id: &str) -> Result<Option<String>, StorageError> {
        let Some(record) = self.get::<Record>(Table::Routes, combo_id)? else {
            return Ok(None);
        };
        Ok(record.optional_text("name")?.map(str::to_owned))
    }
}

/// One rejection reason from the reference graph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ComboReferenceIssue {
    SelfReference,
    Cycle { path: Vec<String> },
    Depth { combo_id: String },
    Missing { combo_id: String },
    TooManyTargets { expanded: usize },
    GraphTooLarge,
}

impl ComboReferenceIssue {
    fn message(&self) -> String {
        match self {
            Self::SelfReference => "a combo target cannot reference its own combo".to_owned(),
            Self::Cycle { path } => {
                format!(
                    "combo references would create a cycle: {}",
                    path.join(" -> ")
                )
            }
            Self::Missing { combo_id } => format!("target combo '{combo_id}' was not found"),
            Self::Depth { combo_id } => format!(
                "combo '{combo_id}' is nested deeper than the supported {MAX_COMBO_NESTING_DEPTH} levels"
            ),
            Self::TooManyTargets { expanded } => format!(
                "combo expands to {expanded} targets; the supported maximum is {MAX_ROUTE_TARGETS}"
            ),
            Self::GraphTooLarge => "combo reference graph is too large to validate".to_owned(),
        }
    }
}

/// A reference check failure: either a rejected graph or a storage fault while
/// reading it. Callers map the first to a 400 and the second to the shared
/// storage error response.
pub(crate) enum ComboReferenceError {
    Issue(ComboReferenceIssue),
    Storage(StorageError),
}

impl ComboReferenceError {
    pub(super) fn into_api(self) -> (StatusCode, Json<Value>) {
        match self {
            Self::Issue(issue) => fail(StatusCode::BAD_REQUEST, issue.message()),
            Self::Storage(error) => internal(error),
        }
    }
}

/// The reference-relevant part of the combo being saved, taken from the request
/// body because it is not committed yet.
#[derive(Clone)]
pub(super) struct PendingCombo {
    references: Vec<String>,
    provider_targets: usize,
}

impl PendingCombo {
    /// Reads the enabled references and provider targets out of a combo body.
    /// Disabled rows contribute nothing, exactly as they do when routing.
    pub(super) fn from_input(input: &ComboInput) -> Self {
        let mut references = Vec::new();
        let mut provider_targets = 0;
        for target in &input.targets {
            if !target.enabled {
                continue;
            }
            match target.combo_id.as_deref() {
                Some(combo_id) => references.push(combo_id.to_owned()),
                None => provider_targets += 1,
            }
        }
        Self {
            references,
            provider_targets,
        }
    }
}

#[derive(Clone)]
struct NodeSummary {
    /// Enabled provider targets here and in every enabled combo reached below.
    /// Duplicates are counted once per occurrence, matching how the gateway
    /// flattens without deduplicating.
    leaves: usize,
    /// Longest reference chain below this node; 0 when it references nothing.
    depth: usize,
}

struct ReferenceWalk<'store, S> {
    store: &'store S,
    memo: HashMap<String, NodeSummary>,
    stack: Vec<String>,
    scanned: usize,
}

impl<'store, S: ComboStore> ReferenceWalk<'store, S> {
    fn new(store: &'store S) -> Self {
        Self {
            store,
            memo: HashMap::new(),
            stack: Vec::new(),
            scanned: 0,
        }
    }

    /// Validates a pending combo and the graph reachable below it. The combo
    /// itself is not stored yet, so its node is passed in.
    fn check(&mut self, root_id: &str, root: &PendingCombo) -> Result<(), ComboReferenceError> {
        self.scanned = 0;
        self.stack.clear();
        let own = NodeSummary {
            leaves: root.provider_targets,
            depth: 0,
        };
        self.scan(root_id, &own, &root.references)?;
        Ok(())
    }

    /// Validates a combo that is already stored, memoizing what it shares with
    /// the combos validated before it.
    fn check_stored(&mut self, combo_id: &str) -> Result<(), ComboReferenceError> {
        self.scanned = 0;
        self.stack.clear();
        self.visit(combo_id).map(|_| ())
    }

    /// Walks one node's references and applies the structural and size caps.
    fn scan(
        &mut self,
        combo_id: &str,
        own: &NodeSummary,
        references: &[String],
    ) -> Result<NodeSummary, ComboReferenceError> {
        self.stack.push(combo_id.to_owned());
        // Bounds the recursion itself: the depth check after the loop only sees
        // how deep a chain went once it has been walked all the way down. That
        // check still runs because it is the one that catches a memoized
        // subtree reached through a deeper path.
        if self.stack.len() > MAX_COMBO_NESTING_DEPTH + 1 {
            self.stack.pop();
            return Err(ComboReferenceError::Issue(ComboReferenceIssue::Depth {
                combo_id: combo_id.to_owned(),
            }));
        }
        let mut summary = own.clone();
        let mut failure: Option<ComboReferenceError> = None;
        for reference in references {
            if reference == combo_id {
                failure = Some(ComboReferenceError::Issue(
                    ComboReferenceIssue::SelfReference,
                ));
                break;
            }
            match self.visit(reference) {
                Ok(child) => {
                    summary.leaves += child.leaves;
                    summary.depth = summary.depth.max(child.depth.saturating_add(1));
                }
                Err(error) => {
                    failure = Some(error);
                    break;
                }
            }
        }
        self.stack.pop();
        if let Some(error) = failure {
            return Err(error);
        }
        if summary.depth > MAX_COMBO_NESTING_DEPTH {
            return Err(ComboReferenceError::Issue(ComboReferenceIssue::Depth {
                combo_id: combo_id.to_owned(),
            }));
        }
        if summary.leaves > MAX_ROUTE_TARGETS {
            return Err(ComboReferenceError::Issue(
                ComboReferenceIssue::TooManyTargets {
                    expanded: summary.leaves,
                },
            ));
        }
        Ok(summary)
    }

    /// Visits a stored combo once, so a shared subgraph is walked once.
    fn visit(&mut self, combo_id: &str) -> Result<NodeSummary, ComboReferenceError> {
        if let Some(cached) = self.memo.get(combo_id) {
            return Ok(cached.clone());
        }
        if let Some(start) = self.stack.iter().position(|id| id == combo_id) {
            let mut path = self.stack[start..].to_vec();
            path.push(combo_id.to_owned());
            return Err(ComboReferenceError::Issue(ComboReferenceIssue::Cycle {
                path,
            }));
        }
        self.scanned += 1;
        if self.scanned > MAX_COMBO_NESTING_SCAN {
            return Err(ComboReferenceError::Issue(
                ComboReferenceIssue::GraphTooLarge,
            ));
        }
        let Some(node) = self
            .store
            .combo_node(combo_id)
            .map_err(ComboReferenceError::Storage)?
        else {
            return Err(ComboReferenceError::Issue(ComboReferenceIssue::Missing {
                combo_id: combo_id.to_owned(),
            }));
        };
        if !node.enabled {
            // A disabled combo contributes nothing to routing, so neither it
            // nor anything below it can affect a request. Re-enabling it
            // validates the graph again with that combo as the root.
            let disabled = NodeSummary {
                leaves: 0,
                depth: 0,
            };
            self.memo.insert(combo_id.to_owned(), disabled.clone());
            return Ok(disabled);
        }
        let summary = self.scan(combo_id, &summary_of(&node), &node.references)?;
        self.memo.insert(combo_id.to_owned(), summary.clone());
        Ok(summary)
    }
}

fn summary_of(node: &ComboNode) -> NodeSummary {
    NodeSummary {
        leaves: node.provider_targets,
        depth: 0,
    }
}

/// Validates the reference graph of a combo that is about to be saved.
pub(super) fn validate_pending_combo<S: ComboStore>(
    store: &S,
    combo_id: &str,
    pending: &PendingCombo,
) -> Result<(), ComboReferenceError> {
    ReferenceWalk::new(store).check(combo_id, pending)
}

/// Validates every stored combo in a graph, sharing memoized results between
/// roots.
pub(crate) fn validate_stored_graph<'ids, S: ComboStore>(
    store: &S,
    combo_ids: impl IntoIterator<Item = &'ids String>,
) -> Result<(), ComboReferenceError> {
    let mut walk = ReferenceWalk::new(store);
    for combo_id in combo_ids {
        walk.check_stored(combo_id)?;
    }
    Ok(())
}

/// Validates the reference graph of a combo that is about to be saved, against
/// the committed state.
///
/// This pre-check produces a precise 400. The same walk runs again inside the
/// save transaction, where it is the authoritative one.
pub(super) async fn validate_combo_references(
    state: &AppState,
    combo_id: String,
    pending: PendingCombo,
) -> Result<(), (StatusCode, Json<Value>)> {
    state
        .db
        .read(move |transaction| Ok(validate_pending_combo(transaction, &combo_id, &pending)))
        .await
        .map_err(internal)?
        .map_err(ComboReferenceError::into_api)
}

/// Re-runs the reference check inside the save transaction, where it is
/// authoritative: a combo saved after the pre-check may have changed the graph.
///
/// A rejected graph becomes a conflict because the caller cannot report a
/// message from inside the write transaction; a storage fault is propagated
/// unchanged so it is not reported as a reference problem.
pub(super) fn ensure_reference_graph<S: ComboStore>(
    store: &S,
    combo_id: &str,
    pending: &PendingCombo,
) -> Result<(), StorageError> {
    match validate_pending_combo(store, combo_id, pending) {
        Ok(()) => Ok(()),
        Err(ComboReferenceError::Storage(error)) => Err(error),
        Err(ComboReferenceError::Issue(_)) => Err(StorageError::Conflict),
    }
}

/// Names the combos that reference `combo_id`, for a blocked deletion.
pub(super) fn referencing_combo_names<S: ComboReferences>(
    store: &S,
    combo_id: &str,
) -> Result<Vec<String>, StorageError> {
    let routes = store.referencing_routes(combo_id)?;
    if routes.len() > MAX_COMBO_NESTING_SCAN {
        return Err(StorageError::Invalid(
            "combo reference graph is too large to validate".to_owned(),
        ));
    }
    let mut names = Vec::with_capacity(routes.len().min(MAX_DELETE_BLOCKERS_NAMED));
    for route_id in routes {
        if names.len() >= MAX_DELETE_BLOCKERS_NAMED {
            break;
        }
        let name = store
            .route_name(&route_id)?
            .unwrap_or_else(|| route_id.clone());
        names.push(name);
    }
    Ok(names)
}
