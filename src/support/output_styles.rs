//! Static, bounded response-shaping instructions for gateway requests.
//!
//! Output styles are deliberately applied to the canonical request before any
//! provider adapter runs. They never inspect or rewrite provider responses.
//!
//! Split by concern: `catalog` owns ids/levels/validation, `bypass` owns
//! the pure classifier, `instruction` owns prompt text, and `apply` owns
//! request mutation plus metrics.

pub mod apply;
pub mod bypass;
pub mod catalog;
pub(crate) mod instruction;
#[cfg(test)]
mod tests;

pub use apply::{ApplyResult, Metrics, apply};
#[allow(unused_imports)]
pub use bypass::should_bypass;
#[allow(unused_imports)]
pub use catalog::{
    MAX_INSTRUCTION_BYTES, MAX_STYLE_SELECTIONS, MAX_STYLES_PAYLOAD_BYTES,
    OUTPUT_STYLE_FORMAT_VERSION, OUTPUT_STYLE_MARKER, OutputStyleId, OutputStyleLevel,
    OutputStyleSelection, OutputStylesSnapshot, SHARED_BOUNDARIES, normalize_styles,
    validate_styles,
};
