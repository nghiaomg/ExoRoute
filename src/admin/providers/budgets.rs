use super::keys::usage_budget;
use super::*;
use crate::infra::storage::{Field, Record, StorageError, Table};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) enum BudgetUpdate {
    #[default]
    Unchanged,
    Clear,
    Set(f64),
}

impl<'de> serde::Deserialize<'de> for BudgetUpdate {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(
            match <Option<f64> as serde::Deserialize<'de>>::deserialize(deserializer)? {
                Some(value) => Self::Set(value),
                None => Self::Clear,
            },
        )
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct ProviderUsageBudgetInput {
    /// A missing field is unchanged; JSON null clears the corresponding budget.
    #[serde(default)]
    pub(super) five_hour_usd: BudgetUpdate,
    #[serde(default)]
    pub(super) seven_day_usd: BudgetUpdate,
    #[serde(default)]
    pub(super) thirty_day_usd: BudgetUpdate,
}

const LOCAL_USAGE_BUDGET_UNSUPPORTED: &str = "provider does not support local usage budgets";

fn budget_to_micros(value: f64) -> Result<i64, (StatusCode, Json<Value>)> {
    if !value.is_finite() || value <= 0.0 || value > 9_000_000_000_000.0 {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "usage budgets must be finite positive USD amounts",
        ));
    }
    let micros = (value * 1_000_000.0).round();
    if !micros.is_finite() || micros < 1.0 || micros > i64::MAX as f64 {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "usage budget is outside the supported USD range",
        ));
    }
    Ok(micros as i64)
}

fn budget_update_to_micros(
    update: BudgetUpdate,
) -> Result<Option<Option<i64>>, (StatusCode, Json<Value>)> {
    match update {
        BudgetUpdate::Unchanged => Ok(None),
        BudgetUpdate::Clear => Ok(Some(None)),
        BudgetUpdate::Set(value) => budget_to_micros(value).map(|value| Some(Some(value))),
    }
}

pub(crate) async fn update_provider_usage_budget(
    State(state): State<AppState>,
    Path((provider_id, key_id)): Path<(String, String)>,
    Json(input): Json<ProviderUsageBudgetInput>,
) -> ApiResult {
    let five_hour = budget_update_to_micros(input.five_hour_usd)?;
    let seven_day = budget_update_to_micros(input.seven_day_usd)?;
    let thirty_day = budget_update_to_micros(input.thirty_day_usd)?;
    let provider_id_for_write = provider_id.clone();
    let key_id_for_write = key_id.clone();
    let budgets = state
        .db
        .write(move |transaction| {
            let provider = transaction
                .get::<Record>(Table::Providers, &provider_id_for_write)?
                .ok_or(StorageError::NotFound)?;
            ensure_provider_not_deleting(&provider)?;
            let mut key = transaction
                .get::<Record>(Table::ProviderApiKeys, &key_id_for_write)?
                .ok_or(StorageError::NotFound)?;
            if key.text("provider_id")? != provider_id_for_write {
                return Err(StorageError::NotFound);
            }
            let adapter_id = provider
                .optional_text("adapter_id")?
                .unwrap_or(provider_adapters::GENERIC_ADAPTER_ID);
            if !provider_adapters::capabilities(adapter_id)
                .is_some_and(|capabilities| capabilities.local_usage_meter)
            {
                return Err(StorageError::Invalid(
                    LOCAL_USAGE_BUDGET_UNSUPPORTED.to_owned(),
                ));
            }
            if let Some(value) = five_hour {
                key.insert(
                    "usage_budget_5h_micros",
                    value.map(Field::I64).unwrap_or(Field::Null),
                );
            }
            if let Some(value) = seven_day {
                key.insert(
                    "usage_budget_7d_micros",
                    value.map(Field::I64).unwrap_or(Field::Null),
                );
            }
            if let Some(value) = thirty_day {
                key.insert(
                    "usage_budget_30d_micros",
                    value.map(Field::I64).unwrap_or(Field::Null),
                );
            }
            transaction.put(Table::ProviderApiKeys, &key_id_for_write, &key)?;
            Ok((
                usage_budget(&key, "usage_budget_5h_micros")?,
                usage_budget(&key, "usage_budget_7d_micros")?,
                usage_budget(&key, "usage_budget_30d_micros")?,
            ))
        })
        .await
        .map_err(|error| match error {
            StorageError::NotFound => fail(StatusCode::NOT_FOUND, "provider API key not found"),
            StorageError::Conflict => fail(StatusCode::CONFLICT, "provider is being deleted"),
            StorageError::Invalid(message) if message == LOCAL_USAGE_BUDGET_UNSUPPORTED => {
                fail(StatusCode::BAD_REQUEST, LOCAL_USAGE_BUDGET_UNSUPPORTED)
            }
            StorageError::Invalid(message) => internal(StorageError::Invalid(message)),
            other => internal(other),
        })?;
    Ok(Json(json!({
        "ok": true,
        "key_id": key_id,
        "budget_usd": {
            "5h": budgets.0.map(|value| value as f64 / 1_000_000.0),
            "7d": budgets.1.map(|value| value as f64 / 1_000_000.0),
            "30d": budgets.2.map(|value| value as f64 / 1_000_000.0),
        }
    })))
}
