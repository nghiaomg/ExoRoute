//! Protocol frame handlers for the streaming translation driver.
//!
//! The driver loop in [`super::translate`] stays protocol-agnostic: it reads
//! SSE frames and dispatches each decoded frame to the handler for the active
//! upstream protocol. Handlers mutate the shared [`TranslationState`] and
//! append client-visible bytes to a per-frame output buffer, returning a
//! single [`StreamError`] failure path instead of duplicating the driver's
//! error boilerplate per handler.
//!
//! The frame handlers and shared passes mirror the sibling `messages::` /
//! `google::` split: this module declares the tree and shares the streaming
//! namespace with its children through `use super::*`, re-exporting the
//! handler entry points for the driver. This tree is one implementation unit:
//! the translation state machine, the block encoders, the upstream decoders,
//! and the client encoders compose directly, so they share a single
//! namespace.

mod chat;
mod finalize;
mod google;
mod messages;
mod responses;
mod shared;
mod state;

#[path = "translate_driver.rs"]
mod driver;

use super::*;

pub(in crate::gateway::streaming) use chat::handle_chat_frame;
pub(in crate::gateway::streaming) use driver::translation_stream;
pub(in crate::gateway::streaming) use finalize::{
    complete_events, fail_events, log_completed, log_failed,
};
pub(in crate::gateway::streaming) use google::handle_google_frame;
pub(in crate::gateway::streaming) use messages::handle_messages_frame;
pub(in crate::gateway::streaming) use responses::handle_responses_frame;
pub(in crate::gateway::streaming) use state::{
    StreamError, StreamFrame, StreamTerminal, TranslationState,
};
