#[derive(Clone, Debug)]
pub struct RequestLogRecord {
    pub id: String,
    pub request_id: String,
    pub route_alias: String,
    pub provider_id: Option<String>,
    pub provider_credential_id: Option<String>,
    pub api_key_id: Option<String>,
    pub model: String,
    pub client_protocol: String,
    pub upstream_protocol: Option<String>,
    /// The id of the proxy the request egressed through, when the serving
    /// provider is bound to one. Records written before the field existed
    /// decode (and are stored) as `None`.
    pub proxy_id: Option<String>,
    pub status: i64,
    pub duration_ms: i64,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cached_tokens: Option<i64>,
    pub cache_input_tokens: Option<i64>,
    pub reasoning_effort: Option<String>,
    pub cost_micro_usd: Option<i64>,
    pub error: Option<String>,
}
