//! Provider URL egress validation.
//!
//! `validate_provider_url` is intentionally a synchronous, syntax and literal-IP
//! check. For hostnames, resolve with `resolve_provider_url` and use the returned
//! addresses to pin the connection. DNS validation is only a point-in-time check:
//! it does not pin a later connection by itself. A caller that needs SSRF
//! protection against DNS rebinding must enforce the same address policy at
//! connect time (or use a trusted egress proxy/firewall). Redirects also need to
//! be disabled or revalidated at every hop.

mod client;
mod headers;
mod url;

pub use client::provider_client;
pub use headers::{provider_auth_header_name, provider_custom_header_name};
pub use url::validate_provider_url;
