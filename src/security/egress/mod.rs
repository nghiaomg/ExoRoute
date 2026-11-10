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

pub(crate) use client::proxy_egress_from_record;
pub use client::{ProxyEgress, provider_client, provider_client_with_proxy};
pub use headers::{provider_auth_header_name, provider_custom_header_name};
pub use url::{validate_provider_url, validate_provider_url_via_proxy, validate_proxy_url};
