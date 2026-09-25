//! Google GenerateContent codec, split by conversion direction: request
//! encoding and response decoding are independent concerns that change for
//! different reasons, so each lives in its own module. Helpers shared by
//! both directions (inline data validation, finish reasons, usage) live in
//! [`shared`].

use super::*;

mod request;
mod response;
mod shared;

#[path = "tests.rs"]
#[cfg(test)]
mod tests;

pub(in crate::protocol) use request::encode_request;
pub(in crate::protocol) use response::decode_response;
