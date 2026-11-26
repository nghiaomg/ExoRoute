//! Gateway API key access scope: the providers and models a client key may use.
//!
//! A scope is stored on the `Table::ApiKeys` record as two optional JSON string
//! arrays. A missing or empty list means "unrestricted", which keeps every key
//! created before this feature working unchanged. Model entries are exact model
//! names, or a trailing `*` that turns the entry into a prefix rule.
//!
//! The gateway and the admin API both parse through this module so a stored
//! scope can never mean one thing when written and another when enforced.

use crate::infra::storage::{Field, Record, StorageError};

pub(crate) const ALLOWED_PROVIDERS_FIELD: &str = "allowed_provider_ids";
pub(crate) const ALLOWED_MODELS_FIELD: &str = "allowed_models";
/// Bounds keep one key's scope small; requests must never parse an unbounded
/// list, and the admin UI stays readable.
pub(crate) const MAX_ALLOWED_PROVIDERS: usize = 64;
pub(crate) const MAX_ALLOWED_MODEL_RULES: usize = 64;
pub(crate) const MAX_SCOPE_ENTRY_BYTES: usize = 128;
const MAX_SCOPE_JSON_BYTES: usize = 8 * 1024;

/// One configured model permission: an exact model name or a prefix rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ModelRule {
    Exact(String),
    Prefix(String),
}

impl ModelRule {
    pub(crate) fn parse(raw: &str) -> Result<Self, String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err("an allowed model entry is empty".to_owned());
        }
        if trimmed.len() > MAX_SCOPE_ENTRY_BYTES {
            return Err(format!(
                "an allowed model entry is longer than {MAX_SCOPE_ENTRY_BYTES} bytes"
            ));
        }
        if trimmed.chars().any(|value| value.is_whitespace()) {
            return Err("an allowed model entry must not contain whitespace".to_owned());
        }
        if trimmed.chars().any(char::is_control) {
            return Err("an allowed model entry must not contain control characters".to_owned());
        }
        match trimmed.strip_suffix('*') {
            Some(prefix) => {
                if prefix.is_empty() {
                    return Err("a model prefix must not be only a wildcard".to_owned());
                }
                if prefix.contains('*') {
                    return Err(
                        "'*' is only supported as the last character of a model rule".to_owned(),
                    );
                }
                Ok(Self::Prefix(prefix.to_owned()))
            }
            None => {
                if trimmed.contains('*') {
                    return Err(
                        "'*' is only supported as the last character of a model rule".to_owned(),
                    );
                }
                Ok(Self::Exact(trimmed.to_owned()))
            }
        }
    }

    pub(crate) fn matches(&self, model: &str) -> bool {
        match self {
            Self::Exact(value) => model == value,
            Self::Prefix(prefix) => model.starts_with(prefix.as_str()),
        }
    }

    pub(crate) fn to_stored(&self) -> String {
        match self {
            Self::Exact(value) => value.clone(),
            Self::Prefix(prefix) => format!("{prefix}*"),
        }
    }
}

/// The parsed access scope of one gateway API key.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ApiKeyScope {
    allowed_provider_ids: Vec<String>,
    allowed_models: Vec<ModelRule>,
}

impl ApiKeyScope {
    /// Trims, validates, and de-duplicates submitted entries. Order is kept so
    /// the admin UI shows the scope the operator configured.
    pub(crate) fn from_entries(
        provider_ids: &[String],
        model_rules: &[String],
    ) -> Result<Self, String> {
        if provider_ids.len() > MAX_ALLOWED_PROVIDERS {
            return Err(format!(
                "an API key can allow at most {MAX_ALLOWED_PROVIDERS} providers"
            ));
        }
        if model_rules.len() > MAX_ALLOWED_MODEL_RULES {
            return Err(format!(
                "an API key can allow at most {MAX_ALLOWED_MODEL_RULES} model rules"
            ));
        }
        let mut allowed_provider_ids = Vec::with_capacity(provider_ids.len());
        for provider_id in provider_ids {
            let trimmed = provider_id.trim();
            if trimmed.is_empty() {
                return Err("an allowed provider entry is empty".to_owned());
            }
            if trimmed.len() > MAX_SCOPE_ENTRY_BYTES {
                return Err(format!(
                    "an allowed provider entry is longer than {MAX_SCOPE_ENTRY_BYTES} bytes"
                ));
            }
            if trimmed.chars().any(char::is_control) {
                return Err(
                    "an allowed provider entry must not contain control characters".to_owned(),
                );
            }
            if !allowed_provider_ids
                .iter()
                .any(|existing| existing == trimmed)
            {
                allowed_provider_ids.push(trimmed.to_owned());
            }
        }
        let mut allowed_models: Vec<ModelRule> = Vec::with_capacity(model_rules.len());
        for rule in model_rules {
            let parsed = ModelRule::parse(rule)?;
            let stored = parsed.to_stored();
            if !allowed_models
                .iter()
                .any(|existing| existing.to_stored() == stored)
            {
                allowed_models.push(parsed);
            }
        }
        Ok(Self {
            allowed_provider_ids,
            allowed_models,
        })
    }

    pub(crate) fn is_unrestricted(&self) -> bool {
        self.allowed_provider_ids.is_empty() && self.allowed_models.is_empty()
    }

    pub(crate) fn allowed_provider_ids(&self) -> &[String] {
        &self.allowed_provider_ids
    }

    pub(crate) fn stored_model_rules(&self) -> Vec<String> {
        self.allowed_models
            .iter()
            .map(ModelRule::to_stored)
            .collect()
    }

    pub(crate) fn allows_provider(&self, provider_id: &str) -> bool {
        self.allowed_provider_ids.is_empty()
            || self
                .allowed_provider_ids
                .iter()
                .any(|allowed| allowed == provider_id)
    }

    pub(crate) fn allows_model(&self, model: &str) -> bool {
        self.allowed_models.is_empty() || self.allowed_models.iter().any(|rule| rule.matches(model))
    }

    /// Writes both scope fields onto a key record. Empty lists are written
    /// explicitly so an unrestricted key never falls back to a stale value.
    pub(crate) fn write_to_record(&self, record: &mut Record) -> Result<(), StorageError> {
        let providers = serde_json::to_string(&self.allowed_provider_ids)
            .map_err(|error| StorageError::Codec(error.to_string()))?;
        let models = serde_json::to_string(&self.stored_model_rules())
            .map_err(|error| StorageError::Codec(error.to_string()))?;
        record.insert(ALLOWED_PROVIDERS_FIELD, Field::Text(providers));
        record.insert(ALLOWED_MODELS_FIELD, Field::Text(models));
        Ok(())
    }

    /// Reads a stored scope. Malformed data is an error, never a silent
    /// unrestricted fallback, so a corrupt record fails closed.
    pub(crate) fn from_record(record: &Record) -> Result<Self, StorageError> {
        let provider_ids = optional_entry_list(record, ALLOWED_PROVIDERS_FIELD)?;
        let model_rules = optional_entry_list(record, ALLOWED_MODELS_FIELD)?;
        Self::from_entries(&provider_ids, &model_rules).map_err(|message| {
            StorageError::Invalid(format!("API key scope is invalid: {message}"))
        })
    }
}

fn optional_entry_list(record: &Record, field: &str) -> Result<Vec<String>, StorageError> {
    let Some(raw) = record.optional_text(field)? else {
        return Ok(Vec::new());
    };
    if raw.len() > MAX_SCOPE_JSON_BYTES {
        return Err(StorageError::Invalid(format!(
            "API key scope field '{field}' exceeds {MAX_SCOPE_JSON_BYTES} bytes"
        )));
    }
    serde_json::from_str(raw).map_err(|error| {
        StorageError::Invalid(format!(
            "API key scope field '{field}' is not a valid JSON string array: {error}"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_rules_match_exact_names_and_trailing_wildcards() {
        let exact = ModelRule::parse("gpt-4o").expect("exact rule");
        assert!(exact.matches("gpt-4o"));
        assert!(!exact.matches("gpt-4o-mini"));
        assert!(!exact.matches("gpt-4"));

        let prefix = ModelRule::parse("gpt-4*").expect("prefix rule");
        assert!(prefix.matches("gpt-4"));
        assert!(prefix.matches("gpt-4o-mini"));
        assert!(!prefix.matches("gpt-3.5"));
        assert_eq!(prefix.to_stored(), "gpt-4*");
    }

    #[test]
    fn model_rules_reject_malformed_entries() {
        for raw in ["", "   ", "*", "gpt*4o", "gpt 4o", "gpt-4*o*"] {
            assert!(ModelRule::parse(raw).is_err(), "{raw:?} should be rejected");
        }
        let long = "m".repeat(MAX_SCOPE_ENTRY_BYTES + 1);
        assert!(ModelRule::parse(&long).is_err());
        assert!(ModelRule::parse("provider/model:free").is_ok());
    }

    #[test]
    fn scope_canonicalizes_entries_and_applies_both_dimensions() {
        let scope = ApiKeyScope::from_entries(
            &[
                " provider-a ".to_owned(),
                "provider-a".to_owned(),
                "provider-b".to_owned(),
            ],
            &[
                "claude-*".to_owned(),
                "claude-3-5".to_owned(),
                " claude-* ".to_owned(),
            ],
        )
        .expect("valid scope");

        assert_eq!(scope.allowed_provider_ids(), ["provider-a", "provider-b"]);
        assert_eq!(scope.stored_model_rules(), ["claude-*", "claude-3-5"]);
        assert!(scope.allows_provider("provider-a"));
        assert!(!scope.allows_provider("provider-c"));
        assert!(scope.allows_model("claude-3-5-sonnet"));
        assert!(!scope.allows_model("gpt-4o"));
        assert!(!scope.is_unrestricted());
        assert!(ApiKeyScope::default().is_unrestricted());
        assert!(ApiKeyScope::default().allows_model("anything"));
        assert!(ApiKeyScope::default().allows_provider("anything"));
    }

    #[test]
    fn scope_bounds_and_empty_entries_are_rejected() {
        let too_many_providers: Vec<String> = (0..=MAX_ALLOWED_PROVIDERS)
            .map(|index| format!("provider-{index}"))
            .collect();
        assert!(ApiKeyScope::from_entries(&too_many_providers, &[]).is_err());
        let too_many_rules: Vec<String> = (0..=MAX_ALLOWED_MODEL_RULES)
            .map(|index| format!("model-{index}"))
            .collect();
        assert!(ApiKeyScope::from_entries(&[], &too_many_rules).is_err());
        assert!(ApiKeyScope::from_entries(&["".to_owned()], &[]).is_err());
        assert!(ApiKeyScope::from_entries(&[], &["".to_owned()]).is_err());
    }

    #[test]
    fn stored_scope_round_trips_and_corruption_fails_closed() {
        let scope = ApiKeyScope::from_entries(
            &["provider-a".to_owned()],
            &["gpt-4*".to_owned(), "claude-3-5-sonnet".to_owned()],
        )
        .expect("valid scope");
        let mut record = Record::new();
        scope.write_to_record(&mut record).expect("write scope");
        assert_eq!(
            ApiKeyScope::from_record(&record).expect("read scope"),
            scope
        );

        // A key created before scoping has neither field and stays unrestricted.
        assert!(
            ApiKeyScope::from_record(&Record::new())
                .expect("legacy key")
                .is_unrestricted()
        );

        let mut corrupt = Record::new();
        corrupt.insert(ALLOWED_PROVIDERS_FIELD, Field::Text("{".to_owned()));
        assert!(ApiKeyScope::from_record(&corrupt).is_err());

        let mut corrupt_rule = Record::new();
        corrupt_rule.insert(ALLOWED_MODELS_FIELD, Field::Text("[\"**\"]".to_owned()));
        assert!(ApiKeyScope::from_record(&corrupt_rule).is_err());

        let mut not_text = Record::new();
        not_text.insert(ALLOWED_MODELS_FIELD, Field::I64(1));
        assert!(ApiKeyScope::from_record(&not_text).is_err());
    }
}
