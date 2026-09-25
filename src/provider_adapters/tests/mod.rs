//! Tests for provider adapter registry, capabilities, and dispatch.
use super::*;

use crate::{
    infra::storage::{Field, Record, StorageError},
    security,
    support::test_support::{ProviderSeed, TestDatabase, seed_provider},
};
use std::collections::{HashMap, HashSet};
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
};

mod codex_accounts;
mod codex_responses;
mod command_code_usage;
mod dispatch_routing;
mod model_test;
mod opencode_usage;
mod openrouter_usage;
mod registry_catalog;
mod support;
