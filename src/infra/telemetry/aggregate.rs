use super::*;

#[derive(Hash, PartialEq, Eq)]
pub(crate) struct ProviderUsageBucketKey {
    pub(crate) credential_id: String,
    pub(crate) minute: i64,
}

#[derive(Default)]
pub(crate) struct ProviderUsageBucketCounts {
    pub(crate) requests: i64,
    pub(crate) cost_micro_usd: i64,
    pub(crate) cost_events: i64,
    pub(crate) missing_cost_events: i64,
    pub(crate) input_tokens: i64,
    pub(crate) output_tokens: i64,
}

impl ProviderUsageBucketCounts {
    pub(crate) fn add(&mut self, event: &ProviderUsageEvent) {
        self.requests = self.requests.saturating_add(1);
        if let Some(cost) = event.cost_micro_usd {
            self.cost_micro_usd = self.cost_micro_usd.saturating_add(cost);
            self.cost_events = self.cost_events.saturating_add(1);
        } else {
            self.missing_cost_events = self.missing_cost_events.saturating_add(1);
        }
        self.input_tokens = self
            .input_tokens
            .saturating_add(event.input_tokens.unwrap_or(0));
        self.output_tokens = self
            .output_tokens
            .saturating_add(event.output_tokens.unwrap_or(0));
    }

    pub(crate) fn add_counts(&mut self, other: &Self) {
        self.requests = self.requests.saturating_add(other.requests);
        self.cost_micro_usd = self.cost_micro_usd.saturating_add(other.cost_micro_usd);
        self.cost_events = self.cost_events.saturating_add(other.cost_events);
        self.missing_cost_events = self
            .missing_cost_events
            .saturating_add(other.missing_cost_events);
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
    }
}

#[derive(Clone)]
pub(crate) struct StatisticsEvent {
    pub(crate) api_key_id: String,
    pub(crate) requested_model: String,
    pub(crate) minute: i64,
    pub(crate) succeeded: bool,
    pub(crate) duration_ms: i64,
    pub(crate) input_tokens: i64,
    pub(crate) output_tokens: i64,
    pub(crate) cached_tokens: i64,
    pub(crate) cache_input_tokens: i64,
}

#[derive(Default, Clone, Copy)]
pub(crate) struct Counts {
    pub(crate) requests: i64,
    pub(crate) succeeded: i64,
    pub(crate) failed: i64,
    pub(crate) duration_ms: i64,
    pub(crate) input_tokens: i64,
    pub(crate) output_tokens: i64,
    pub(crate) cached_tokens: i64,
    pub(crate) cache_input_tokens: i64,
}

impl Counts {
    pub(crate) fn add_event(&mut self, event: &StatisticsEvent) {
        self.requests = self.requests.saturating_add(1);
        if event.succeeded {
            self.succeeded = self.succeeded.saturating_add(1);
        } else {
            self.failed = self.failed.saturating_add(1);
        }
        self.duration_ms = self.duration_ms.saturating_add(event.duration_ms);
        self.input_tokens = self.input_tokens.saturating_add(event.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(event.output_tokens);
        self.cached_tokens = self.cached_tokens.saturating_add(event.cached_tokens);
        self.cache_input_tokens = self
            .cache_input_tokens
            .saturating_add(event.cache_input_tokens);
    }

    pub(crate) fn add(self, other: Self) -> Self {
        Self {
            requests: self.requests.saturating_add(other.requests),
            succeeded: self.succeeded.saturating_add(other.succeeded),
            failed: self.failed.saturating_add(other.failed),
            duration_ms: self.duration_ms.saturating_add(other.duration_ms),
            input_tokens: self.input_tokens.saturating_add(other.input_tokens),
            output_tokens: self.output_tokens.saturating_add(other.output_tokens),
            cached_tokens: self.cached_tokens.saturating_add(other.cached_tokens),
            cache_input_tokens: self
                .cache_input_tokens
                .saturating_add(other.cache_input_tokens),
        }
    }

    pub(crate) fn subtract(self, other: Self) -> Self {
        Self {
            requests: self.requests.saturating_sub(other.requests).max(0),
            succeeded: self.succeeded.saturating_sub(other.succeeded).max(0),
            failed: self.failed.saturating_sub(other.failed).max(0),
            duration_ms: self.duration_ms.saturating_sub(other.duration_ms).max(0),
            input_tokens: self.input_tokens.saturating_sub(other.input_tokens).max(0),
            output_tokens: self
                .output_tokens
                .saturating_sub(other.output_tokens)
                .max(0),
            cached_tokens: self
                .cached_tokens
                .saturating_sub(other.cached_tokens)
                .max(0),
            cache_input_tokens: self
                .cache_input_tokens
                .saturating_sub(other.cache_input_tokens)
                .max(0),
        }
    }
}

#[derive(Hash, PartialEq, Eq)]
pub(crate) struct AggregateKey {
    pub(crate) minute: i64,
    pub(crate) dimension_kind: i64,
    pub(crate) dimension_id: String,
}
