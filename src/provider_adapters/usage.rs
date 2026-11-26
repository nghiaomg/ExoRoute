use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct ProviderUsageQuota {
    pub id: String,
    pub label: String,
    /// Backward-compatible consumed percentage. Prefer `remaining_percent`.
    pub used_percent: f64,
    /// Percentage of quota still available: 0 means exhausted, 100 means full.
    pub remaining_percent: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_amount: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_amount: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    pub reset_at: Option<i64>,
    pub window_seconds: Option<i64>,
    #[serde(default)]
    pub uncapped: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reset_period: Option<ProviderUsageResetPeriod>,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderUsageResetPeriod {
    Daily,
    Weekly,
    Monthly,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProviderUsageCreditBalance {
    pub unit: String,
    pub monthly_remaining: Option<f64>,
    pub purchased_remaining: Option<f64>,
    pub free_remaining: Option<f64>,
    pub period_used: Option<f64>,
    pub period_ends_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProviderUsageSnapshot {
    pub plan: Option<String>,
    pub limit_reached: bool,
    pub reset_credits_available: Option<u32>,
    pub quotas: Vec<ProviderUsageQuota>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credit_balance: Option<ProviderUsageCreditBalance>,
}
