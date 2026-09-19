//! Stream continuity: records, background workers, and replay endpoints.

mod records;
mod replay;
#[cfg(test)]
mod tests;
mod worker;

pub(in crate::gateway) use records::{
    BackgroundRequestSettings, create_run_with_settings, finish_run, load_run_with_settings,
};
#[cfg(test)]
pub(in crate::gateway) use records::{append_event, create_run, load_run};
#[cfg(test)]
pub(in crate::gateway) use replay::live_response;
pub(in crate::gateway) use replay::live_response_with_settings;
pub(in crate::gateway) use replay::{continuity_capacity_error, resume_response};
pub(in crate::gateway) use worker::start_background_request;
#[cfg(test)]
pub(in crate::gateway) use worker::start_background_stream;
