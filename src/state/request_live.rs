use super::RequestLogRecord;
use crate::config::OperationalSettings;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex as StdMutex},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::broadcast;

pub(crate) const MAX_REQUEST_LIVE_ENTRIES: usize = OperationalSettings::MAX_GATEWAY_MAX_IN_FLIGHT;
const REQUEST_LIVE_EVENT_CAPACITY: usize = 256;

#[derive(Clone, Debug)]
pub(crate) struct RequestLiveRow {
    pub(crate) id: Option<String>,
    pub(crate) live_id: String,
    pub(crate) request_id: String,
    pub(crate) route_alias: String,
    pub(crate) provider_id: Option<String>,
    pub(crate) provider_credential_id: Option<String>,
    pub(crate) api_key_id: Option<String>,
    pub(crate) model: String,
    pub(crate) client_protocol: String,
    pub(crate) upstream_protocol: Option<String>,
    pub(crate) input_tokens: Option<i64>,
    pub(crate) output_tokens: Option<i64>,
    pub(crate) started_at_ms: i64,
}

#[derive(Clone, Debug)]
pub(crate) struct RequestLiveSnapshot {
    pub(crate) requests: Vec<RequestLiveRow>,
    pub(crate) truncated: bool,
    pub(crate) limit: usize,
    pub(crate) active_count: usize,
}

#[derive(Clone, Debug)]
pub(crate) enum RequestLiveEvent {
    Started {
        row: RequestLiveRow,
        active_count: usize,
    },
    Updated {
        live_id: String,
        provider_id: Option<String>,
        latest_log_id: Option<String>,
        input_tokens: Option<i64>,
        output_tokens: Option<i64>,
    },
    Finished {
        live_id: String,
        request: Option<RequestLogRecord>,
        finished_at_ms: i64,
        active_count: usize,
    },
    Capacity {
        truncated: bool,
        limit: usize,
        active_count: usize,
    },
}

struct RequestLiveEntry {
    live_id: String,
    request_id: String,
    route_alias: String,
    model: String,
    client_protocol: String,
    api_key_id: Option<String>,
    provider_id: Option<String>,
    started_at_ms: i64,
    latest: Option<RequestLogRecord>,
}

struct RequestLiveRegistryState {
    entries: HashMap<String, RequestLiveEntry>,
    untracked_active: usize,
    truncated: bool,
}

pub(crate) struct RequestLiveRegistry {
    state: StdMutex<RequestLiveRegistryState>,
    events: broadcast::Sender<RequestLiveEvent>,
}

impl RequestLiveRegistry {
    pub(super) fn new() -> Self {
        let (events, _) = broadcast::channel(REQUEST_LIVE_EVENT_CAPACITY);
        Self {
            state: StdMutex::new(RequestLiveRegistryState {
                entries: HashMap::new(),
                untracked_active: 0,
                truncated: false,
            }),
            events,
        }
    }

    pub(super) fn subscribe(&self) -> broadcast::Receiver<RequestLiveEvent> {
        self.events.subscribe()
    }

    pub(super) fn snapshot(&self) -> RequestLiveSnapshot {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        RequestLiveSnapshot {
            requests: state.entries.values().map(RequestLiveEntry::row).collect(),
            truncated: state.truncated,
            limit: MAX_REQUEST_LIVE_ENTRIES,
            active_count: active_request_count(&state),
        }
    }

    pub(super) fn start(
        self: &Arc<Self>,
        request_id: &str,
        route_alias: &str,
        model: &str,
        client_protocol: &str,
        api_key_id: Option<&str>,
    ) -> RequestLiveGuard {
        let live_id = uuid::Uuid::new_v4().simple().to_string();
        let started_at_ms = unix_time_ms();
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let tracked = state.entries.len() < MAX_REQUEST_LIVE_ENTRIES;
        if tracked {
            state.entries.insert(
                live_id.clone(),
                RequestLiveEntry {
                    live_id: live_id.clone(),
                    request_id: bounded_string(request_id, 128),
                    route_alias: bounded_string(route_alias, 256),
                    model: bounded_string(model, 256),
                    client_protocol: bounded_string(client_protocol, 64),
                    api_key_id: api_key_id.map(|value| bounded_string(value, 128)),
                    provider_id: None,
                    started_at_ms,
                    latest: None,
                },
            );
        } else {
            state.untracked_active = state.untracked_active.saturating_add(1);
            self.update_capacity_locked(&mut state, true, true);
        }
        if tracked && let Some(entry) = state.entries.get(&live_id) {
            let _ = self.events.send(RequestLiveEvent::Started {
                row: entry.row(),
                active_count: active_request_count(&state),
            });
        }
        RequestLiveGuard {
            inner: Arc::new(RequestLiveGuardInner {
                registry: Arc::clone(self),
                live_id: tracked.then_some(live_id),
            }),
        }
    }

    pub(super) fn update_provider(&self, live_id: &str, provider_id: Option<&str>) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(entry) = state.entries.get_mut(live_id) else {
            return;
        };
        let provider_id = provider_id.map(|value| bounded_string(value, 256));
        if entry.provider_id == provider_id {
            return;
        }
        entry.provider_id = provider_id.clone();
        let _ = self.events.send(RequestLiveEvent::Updated {
            live_id: live_id.to_owned(),
            provider_id,
            latest_log_id: entry.latest.as_ref().map(|record| record.id.clone()),
            input_tokens: entry.latest.as_ref().and_then(|record| record.input_tokens),
            output_tokens: entry
                .latest
                .as_ref()
                .and_then(|record| record.output_tokens),
        });
    }

    pub(super) fn update_log(&self, live_id: &str, record: &RequestLogRecord) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(entry) = state.entries.get_mut(live_id) {
            entry.latest = Some(record.clone());
            let provider_id = entry.provider_id.clone();
            let latest_log_id = Some(record.id.clone());
            let _ = self.events.send(RequestLiveEvent::Updated {
                live_id: live_id.to_owned(),
                provider_id,
                latest_log_id,
                input_tokens: record.input_tokens,
                output_tokens: record.output_tokens,
            });
        }
    }

    pub(super) fn finish(&self, live_id: Option<&str>) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut untracked_changed = false;
        if let Some(live_id) = live_id {
            let Some(entry) = state.entries.remove(live_id) else {
                return;
            };
            let _ = self.events.send(RequestLiveEvent::Finished {
                live_id: live_id.to_owned(),
                request: entry.latest,
                finished_at_ms: unix_time_ms(),
                active_count: active_request_count(&state),
            });
        } else if state.untracked_active > 0 {
            state.untracked_active -= 1;
            untracked_changed = true;
        }
        let truncated = state.untracked_active > 0;
        self.update_capacity_locked(&mut state, truncated, untracked_changed);
    }

    fn update_capacity_locked(
        &self,
        state: &mut RequestLiveRegistryState,
        truncated: bool,
        force_event: bool,
    ) {
        let changed = state.truncated != truncated;
        if !changed && !force_event {
            return;
        }
        state.truncated = truncated;
        let _ = self.events.send(RequestLiveEvent::Capacity {
            truncated,
            limit: MAX_REQUEST_LIVE_ENTRIES,
            active_count: active_request_count(state),
        });
    }
}

fn active_request_count(state: &RequestLiveRegistryState) -> usize {
    state.entries.len().saturating_add(state.untracked_active)
}

impl RequestLiveEntry {
    fn row(&self) -> RequestLiveRow {
        let latest = self.latest.as_ref();
        RequestLiveRow {
            id: latest.map(|record| record.id.clone()),
            live_id: self.live_id.clone(),
            request_id: self.request_id.clone(),
            route_alias: self.route_alias.clone(),
            provider_id: self
                .provider_id
                .clone()
                .or_else(|| latest.and_then(|record| record.provider_id.clone())),
            provider_credential_id: latest.and_then(|record| record.provider_credential_id.clone()),
            api_key_id: self
                .api_key_id
                .clone()
                .or_else(|| latest.and_then(|record| record.api_key_id.clone())),
            model: latest
                .map(|record| record.model.clone())
                .unwrap_or_else(|| self.model.clone()),
            client_protocol: latest
                .map(|record| record.client_protocol.clone())
                .unwrap_or_else(|| self.client_protocol.clone()),
            upstream_protocol: latest.and_then(|record| record.upstream_protocol.clone()),
            input_tokens: latest.and_then(|record| record.input_tokens),
            output_tokens: latest.and_then(|record| record.output_tokens),
            started_at_ms: self.started_at_ms,
        }
    }
}

#[derive(Clone)]
pub(crate) struct RequestLiveGuard {
    inner: Arc<RequestLiveGuardInner>,
}

struct RequestLiveGuardInner {
    registry: Arc<RequestLiveRegistry>,
    live_id: Option<String>,
}

impl RequestLiveGuard {
    pub(crate) fn update_provider(&self, provider_id: Option<&str>) {
        if let Some(live_id) = self.inner.live_id.as_deref() {
            self.inner.registry.update_provider(live_id, provider_id);
        }
    }

    pub(crate) fn update_log(&self, record: &RequestLogRecord) {
        if let Some(live_id) = self.inner.live_id.as_deref() {
            self.inner.registry.update_log(live_id, record);
        }
    }
}

impl Drop for RequestLiveGuard {
    fn drop(&mut self) {
        // Extensions require Clone. If a response is cloned, the lifecycle
        // ends only when the final shared guard is dropped.
        if Arc::strong_count(&self.inner) == 1 {
            self.inner.registry.finish(self.inner.live_id.as_deref());
        }
    }
}

fn bounded_string(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn unix_time_ms() -> i64 {
    let Ok(duration) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        return 0;
    };
    let millis = duration.as_millis();
    if millis > i64::MAX as u128 {
        i64::MAX
    } else {
        millis as i64
    }
}
