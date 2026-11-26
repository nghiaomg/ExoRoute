use super::*;
use crate::config::provider_catalog::estimate_cost_micro_usd;

#[tokio::test]
async fn model_pricing_lookup_and_reload_reflect_validated_yaml() {
    let database = TestDatabase::open().await;
    let path = database.config().app_dir.join("providers.yaml");
    std::fs::write(
        &path,
        "providers:\n  p:\n    model_overrides:\n      m:\n        pricing: { input_per_million: 2.5, output_per_million: 10.0 }\n",
    )
    .expect("write catalog");
    let state = AppState::new(database.config(), database.db.clone());

    let pricing = state
        .resolve_model_pricing("p", "generic", "m")
        .expect("pricing from yaml");
    assert_eq!(
        estimate_cost_micro_usd(&pricing, 1_000_000, 500_000, 0, 0, 0),
        Some(7_500_000)
    );
    assert!(state.resolve_model_limits("p", "generic", "m").is_none());

    std::fs::write(
        &path,
        "providers:\n  p:\n    model_overrides:\n      m:\n        pricing: { input_per_million: 1.0 }\n        limits: { context_window: 1000, max_output_tokens: 100 }\n",
    )
    .expect("rewrite catalog");
    state.reload_provider_catalog().expect("reload catalog");
    let reloaded = state
        .resolve_model_pricing("p", "generic", "m")
        .expect("reloaded pricing");
    assert_eq!(reloaded.input_per_million, Some(1.0));
    assert_eq!(
        state
            .resolve_model_limits("p", "generic", "m")
            .and_then(|limits| limits.max_output_tokens),
        Some(100)
    );

    // An invalid rewrite leaves the last valid snapshot active.
    std::fs::write(&path, "providers:\n  p:\n    model_overrides:\n      m:\n        pricing: { input_per_million: -2.0 }\n")
        .expect("write invalid catalog");
    assert!(state.reload_provider_catalog().is_err());
    assert_eq!(
        state
            .resolve_model_pricing("p", "generic", "m")
            .and_then(|pricing| pricing.input_per_million),
        Some(1.0)
    );
}
