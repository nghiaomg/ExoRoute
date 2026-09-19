//! Prometheus metrics rendering.
//!
//! Reads a point-in-time snapshot from runtime gates and telemetry, then
//! formats the exposition text. No handler logic lives here besides
//! collecting the already-aggregated counters.

use crate::state::AppState;
use axum::{extract::State, http::header, response::IntoResponse};
use std::sync::atomic::Ordering;

pub(crate) async fn prometheus_metrics(State(state): State<AppState>) -> axum::response::Response {
    let now = std::time::Instant::now();
    let open_circuits = state
        .gates
        .circuit_breakers
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .values()
        .filter(|circuit| {
            circuit
                .open_until
                .is_some_and(|open_until| now < open_until)
        })
        .count();
    let gateway_in_flight = state.gates.gateway_in_flight.active_count();
    let gateway_in_flight_limit = state.operational_settings().settings.gateway_max_in_flight;
    let body_processing = state.gates.gateway_body_processing.active_count();
    let output_styles = state.output_styles();
    let style_metrics = &state.runtime.output_styles_metrics;
    let injected =
        [0usize, 1, 2].map(|index| style_metrics.injected[index].load(Ordering::Relaxed));
    let bypassed =
        [0usize, 1, 2, 3].map(|index| style_metrics.bypassed[index].load(Ordering::Relaxed));
    let content = format!(
        "# HELP exoroute_uptime_seconds Seconds since this process started.\n# TYPE exoroute_uptime_seconds gauge\nexoroute_uptime_seconds {}\n# HELP exoroute_gateway_requests_total Authenticated gateway requests admitted by the process.\n# TYPE exoroute_gateway_requests_total counter\nexoroute_gateway_requests_total {}\n# HELP exoroute_gateway_in_flight Current gateway responses being consumed.\n# TYPE exoroute_gateway_in_flight gauge\nexoroute_gateway_in_flight {gateway_in_flight}\n# HELP exoroute_gateway_body_processing Current request bodies being processed.\n# TYPE exoroute_gateway_body_processing gauge\nexoroute_gateway_body_processing {body_processing}\n# HELP exoroute_open_circuits Current provider circuits in the open state.\n# TYPE exoroute_open_circuits gauge\nexoroute_open_circuits {open_circuits}\n# HELP exoroute_dropped_request_logs_total Request log records dropped or not persisted.\n# TYPE exoroute_dropped_request_logs_total counter\nexoroute_dropped_request_logs_total {}\n# HELP exoroute_dropped_api_key_usage_total API key usage events dropped because the bounded queue was full or closed.\n# TYPE exoroute_dropped_api_key_usage_total counter\nexoroute_dropped_api_key_usage_total {}\n",
        state.started_at.elapsed().as_secs(),
        state.request_count.load(Ordering::Relaxed),
        state.telemetry.drop_counters().request_logs(),
        state.telemetry.drop_counters().api_key_usage(),
    );
    let content = format!(
        "{content}# HELP exoroute_gateway_in_flight_limit Configured gateway admission limit; 0 means unlimited.\n# TYPE exoroute_gateway_in_flight_limit gauge\nexoroute_gateway_in_flight_limit {gateway_in_flight_limit}\n# HELP exoroute_output_styles_revision Active output style configuration revision.\n# TYPE exoroute_output_styles_revision gauge\nexoroute_output_styles_revision {}\n# HELP exoroute_output_styles_enabled Whether at least one output style is active.\n# TYPE exoroute_output_styles_enabled gauge\nexoroute_output_styles_enabled {}\n# HELP exoroute_output_styles_injected_total Requests that received a static output style instruction.\n# TYPE exoroute_output_styles_injected_total counter\nexoroute_output_styles_injected_total{{style=\"terse-prose\"}} {}\nexoroute_output_styles_injected_total{{style=\"less-code\"}} {}\nexoroute_output_styles_injected_total{{style=\"ponytail\"}} {}\n# HELP exoroute_output_styles_bypassed_total Requests where output styles were conservatively bypassed.\n# TYPE exoroute_output_styles_bypassed_total counter\nexoroute_output_styles_bypassed_total{{reason=\"security_warning\"}} {}\nexoroute_output_styles_bypassed_total{{reason=\"irreversible_action\"}} {}\nexoroute_output_styles_bypassed_total{{reason=\"clarification_requested\"}} {}\nexoroute_output_styles_bypassed_total{{reason=\"order_sensitive_sequence\"}} {}\n",
        output_styles.revision,
        if output_styles.styles.is_empty() {
            0
        } else {
            1
        },
        injected[0],
        injected[1],
        injected[2],
        bypassed[0],
        bypassed[1],
        bypassed[2],
        bypassed[3],
    );
    (
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        content,
    )
        .into_response()
}
