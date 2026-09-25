//! Typed LMDB record codecs grouped by settings domain.
//!
//! Each submodule owns one settings family end to end: the record it writes
//! and the value it reads back. `fields` holds the helpers shared between
//! them.

mod fields;
mod gateway_limits;
mod operational;
mod output_styles;

pub(crate) use gateway_limits::gateway_limits_from_record;
pub(super) use gateway_limits::gateway_limits_record;
pub(crate) use operational::operational_settings_from_record;
pub(super) use operational::operational_settings_record;
pub(crate) use output_styles::{output_styles_from_record, output_styles_record};
