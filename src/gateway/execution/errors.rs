use super::*;

/// Aggregated failure state carried across route targets.
///
/// The original gateway pipeline tracked these values as local variables in
/// a single ~1,300-line function; they now travel with the target loop so
/// each stage records why a target was skipped or failed without threading
/// eight mutable locals through helper parameters.
pub(super) struct TargetLoopFailures {
    pub(super) last_status: StatusCode,
    pub(super) last_error: String,
    pub(super) upstream_retry_after: Option<HeaderValue>,
    pub(super) translation_failed: bool,
    pub(super) request_encodable: bool,
    /// The last credential-level rejection status for the current target.
    pub(super) last_key_rejection: Option<StatusCode>,
    /// The last adapter stream failure that was safe to fail over from.
    pub(super) last_adapter_sse_failure: Option<AdapterSseError>,
    /// The credential whose request produced the final outcome.
    pub(super) selected_credential_id: Option<String>,
}

impl TargetLoopFailures {
    pub(super) fn new() -> Self {
        Self {
            last_status: StatusCode::BAD_GATEWAY,
            last_error: "all route targets failed".to_owned(),
            upstream_retry_after: None,
            translation_failed: false,
            request_encodable: false,
            last_key_rejection: None,
            last_adapter_sse_failure: None,
            selected_credential_id: None,
        }
    }
}
