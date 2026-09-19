use super::{DropKind, TelemetryMessage, unix_minute};
use std::sync::atomic::Ordering;

impl super::RequestAnalytics {
    pub fn api_key_id(&self) -> &str {
        &self.inner.api_key_id
    }

    pub fn set_requested_model(&self, model: Option<&str>) {
        let value = model
            .map(str::trim)
            .filter(|model| !model.is_empty())
            .map(|model| model.chars().take(256).collect::<String>());
        if let Ok(mut requested_model) = self.inner.requested_model.lock() {
            *requested_model = value;
        }
    }

    pub fn set_token_usage(&self, input_tokens: Option<u64>, output_tokens: Option<u64>) {
        if let Some(tokens) = input_tokens {
            self.inner
                .input_tokens
                .store(tokens.min(i64::MAX as u64) as i64, Ordering::Relaxed);
        }
        if let Some(tokens) = output_tokens {
            self.inner
                .output_tokens
                .store(tokens.min(i64::MAX as u64) as i64, Ordering::Relaxed);
        }
    }

    pub fn set_cached_token_usage(
        &self,
        cached_tokens: Option<u64>,
        cache_input_tokens: Option<u64>,
    ) {
        if let Some(tokens) = cached_tokens {
            self.inner
                .cached_tokens
                .store(tokens.min(i64::MAX as u64) as i64, Ordering::Relaxed);
        }
        if let Some(tokens) = cache_input_tokens {
            self.inner
                .cache_input_tokens
                .store(tokens.min(i64::MAX as u64) as i64, Ordering::Relaxed);
        }
    }

    pub fn defer_completion(&self) {
        self.inner.deferred.store(true, Ordering::Release);
    }

    pub fn completion_is_deferred(&self) -> bool {
        self.inner.deferred.load(Ordering::Acquire)
    }

    pub fn completion_guard(&self) -> super::AnalyticsCompletionGuard {
        super::AnalyticsCompletionGuard {
            analytics: self.clone(),
            finished: false,
        }
    }

    pub fn finish(&self, succeeded: bool) {
        if self
            .inner
            .finished
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let requested_model = self
            .inner
            .requested_model
            .lock()
            .ok()
            .and_then(|model| model.clone())
            .unwrap_or_else(|| "Unknown model".to_owned());
        let input_tokens = self.inner.input_tokens.load(Ordering::Relaxed);
        let cache_input_tokens = self
            .inner
            .cache_input_tokens
            .load(Ordering::Relaxed)
            .max(input_tokens);
        let event = super::aggregate::StatisticsEvent {
            api_key_id: self.inner.api_key_id.clone(),
            requested_model,
            minute: unix_minute(),
            succeeded,
            duration_ms: self
                .inner
                .started
                .elapsed()
                .as_millis()
                .min(i64::MAX as u128) as i64,
            input_tokens,
            output_tokens: self.inner.output_tokens.load(Ordering::Relaxed),
            cached_tokens: self
                .inner
                .cached_tokens
                .load(Ordering::Relaxed)
                .min(cache_input_tokens),
            cache_input_tokens,
        };
        self.inner
            .telemetry
            .enqueue(TelemetryMessage::Statistics(event), DropKind::Statistics);
    }
}

impl super::AnalyticsCompletionGuard {
    pub fn finish(&mut self, succeeded: bool) {
        if !self.finished {
            self.analytics.finish(succeeded);
            self.finished = true;
        }
    }
}

impl Drop for super::AnalyticsCompletionGuard {
    fn drop(&mut self) {
        self.finish(false);
    }
}
