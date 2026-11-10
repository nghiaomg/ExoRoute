use super::*;

/// Overlays not-yet-written proxies on top of an existing read or write
/// transaction. In bulk creation `prior` contains earlier rows in request order,
/// so later rows may safely embed them before the batch commits.
pub(crate) struct BulkProxyValidationStore<'a, T: ProxyStore> {
    pub transaction: &'a T,
    pub pending_id: &'a str,
    pub pending: &'a PendingProxy,
    pub prior: &'a [(String, PendingProxy)],
}

impl<'a, T: ProxyStore> BulkProxyValidationStore<'a, T> {
    pub fn new(
        transaction: &'a T,
        pending_id: &'a str,
        pending: &'a PendingProxy,
        prior: &'a [(String, PendingProxy)],
    ) -> Self {
        Self { transaction, pending_id, pending, prior }
    }
}
impl<T: ProxyStore> ProxyStore for BulkProxyValidationStore<'_, T> {
    fn proxy_node(&self, proxy_id: &str) -> Result<Option<ProxyNode>, StorageError> {
        if proxy_id == self.pending_id {
            return Ok(Some(node_from_pending(self.pending)));
        }
        if let Some((_, pending)) = self.prior.iter().find(|(id, _)| id == proxy_id) {
            return Ok(Some(node_from_pending(pending)));
        }
        self.transaction.proxy_node(proxy_id)
    }
}

fn node_from_pending(pending: &PendingProxy) -> ProxyNode {
    ProxyNode {
        enabled: pending.enabled,
        server_endpoints: pending.server_endpoints,
        references: pending.references.clone(),
    }
}
