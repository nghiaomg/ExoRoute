//! User-editable provider catalog overlay (`~/.exoroute/providers.yaml`).
//!
//! The built-in preset catalog is generated at compile time; this module only
//! reads a bounded, validated user overlay so an administrator can add models
//! without rebuilding the binary. Invalid user input is rejected before it
//! reaches runtime state — never partially applied.

use super::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

const MAX_CATALOG_BYTES: u64 = 1024 * 1024;
const MAX_PROVIDERS: usize = 256;
const MAX_MODELS_PER_PROVIDER: usize = 2048;
const MAX_PROVIDER_KEY_CHARS: usize = 64;
const MAX_MODEL_ID_BYTES: usize = 256;
const MAX_PROTOCOL_CHARS: usize = 64;
const MAX_PRICE_PER_MILLION: f64 = 1_000_000.0;
const KNOWN_PROTOCOLS: [&str; 4] = [
    "chat_completions",
    "responses",
    "messages",
    "google_generate_content",
];

const TEMPLATE: &str = "\
# ExoRoute provider catalog overlay.
# Edits are applied at startup and via POST /api/v1/admin/provider-presets/reload.
# Unknown keys and invalid model IDs are rejected; the last valid catalog stays active.
providers: {}
# Example — replace the line above with a mapping like this:
# providers:
#   opencode_go:
#     default_models:
#       - hy4-flash-free
#     remove_models: []
#   opencode_zen:
#     default_models:
#       - longcat-2.5-free
#     model_overrides:
#       gpt-5:
#         upstream_protocol: responses
#         pricing: { input_per_million: 2.5, output_per_million: 10.0 }
#         limits: { context_window: 200000, max_output_tokens: 32000 }
";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProvidersYamlConfig {
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderCatalogOverlay>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCatalogOverlay {
    #[serde(default)]
    pub default_models: Vec<String>,
    #[serde(default)]
    pub remove_models: Vec<String>,
    #[serde(default)]
    pub model_overrides: BTreeMap<String, ModelOverrideConfig>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ModelPricingConfig {
    #[serde(default)]
    pub input_per_million: Option<f64>,
    #[serde(default)]
    pub output_per_million: Option<f64>,
    #[serde(default)]
    pub cache_read_per_million: Option<f64>,
    #[serde(default)]
    pub cache_write_per_million: Option<f64>,
    #[serde(default)]
    pub reasoning_per_million: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ModelLimitsConfig {
    #[serde(default)]
    pub context_window: Option<u64>,
    #[serde(default)]
    pub max_output_tokens: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelOverrideConfig {
    #[serde(default)]
    pub upstream_protocol: Option<String>,
    #[serde(default)]
    pub pricing: Option<ModelPricingConfig>,
    #[serde(default)]
    pub limits: Option<ModelLimitsConfig>,
}

/// USD per 1M tokens equals micro-USD per token, so each component is a direct
/// multiplication. Returns `None` only when no price is configured at all.
pub fn estimate_cost_micro_usd(
    pricing: &ModelPricingConfig,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    reasoning_tokens: u64,
) -> Option<u64> {
    let components = [
        (input_tokens, pricing.input_per_million),
        (output_tokens, pricing.output_per_million),
        (cache_read_tokens, pricing.cache_read_per_million),
        (cache_write_tokens, pricing.cache_write_per_million),
        (reasoning_tokens, pricing.reasoning_per_million),
    ];
    if components.iter().all(|(_, price)| price.is_none()) {
        return None;
    }
    let total = components.iter().fold(0.0_f64, |sum, (tokens, price)| {
        price.map_or(sum, |price| sum + (*tokens as f64) * price)
    });
    Some(if total.is_finite() && total > 0.0 {
        total.round() as u64
    } else {
        0
    })
}

#[derive(Debug)]
pub enum ProviderCatalogError {
    Io(io::Error),
    Symlink(PathBuf),
    TooLarge { bytes: u64 },
    InvalidYaml(String),
    Invalid(String),
}

impl fmt::Display for ProviderCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "provider catalog I/O error: {error}"),
            Self::Symlink(path) => write!(
                formatter,
                "provider catalog must be a regular file, not a symlink: {}",
                path.display()
            ),
            Self::TooLarge { bytes } => write!(
                formatter,
                "provider catalog is {bytes} bytes, above the 1 MiB limit"
            ),
            Self::InvalidYaml(message) => {
                write!(formatter, "invalid provider catalog YAML: {message}")
            }
            Self::Invalid(message) => write!(formatter, "invalid provider catalog: {message}"),
        }
    }
}

impl std::error::Error for ProviderCatalogError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for ProviderCatalogError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl ProvidersYamlConfig {
    /// Ids with an overlay entry, for reload summaries and diagnostics.
    pub fn overlaid_provider_ids(&self) -> Vec<String> {
        self.providers.keys().cloned().collect()
    }

    /// Overlay for a preset, keyed by preset id with an `adapter_id` fallback.
    pub fn overlay_for(&self, id: &str, adapter_id: &str) -> Option<&ProviderCatalogOverlay> {
        self.providers
            .get(id)
            .or_else(|| self.providers.get(adapter_id))
    }
}

fn valid_provider_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= MAX_PROVIDER_KEY_CHARS
        && key
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_-".contains(&byte))
}

fn valid_model_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_MODEL_ID_BYTES
        && !id
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
}

fn valid_protocol(protocol: &str) -> bool {
    KNOWN_PROTOCOLS.contains(&protocol)
        || (!protocol.is_empty()
            && protocol.len() <= MAX_PROTOCOL_CHARS
            && protocol.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_-".contains(&byte)
            }))
}

fn validate(config: &ProvidersYamlConfig) -> Result<(), ProviderCatalogError> {
    if config.providers.len() > MAX_PROVIDERS {
        return Err(ProviderCatalogError::Invalid(format!(
            "at most {MAX_PROVIDERS} providers may be configured"
        )));
    }
    for (key, overlay) in &config.providers {
        if !valid_provider_key(key) {
            return Err(ProviderCatalogError::Invalid(format!(
                "invalid provider key '{key}': expected 1-{MAX_PROVIDER_KEY_CHARS} lowercase letters, digits, '_' or '-'"
            )));
        }
        let mut model_ids = BTreeSet::new();
        for id in overlay
            .default_models
            .iter()
            .chain(overlay.remove_models.iter())
            .chain(overlay.model_overrides.keys())
        {
            if !valid_model_id(id) {
                return Err(ProviderCatalogError::Invalid(format!(
                    "invalid model id in provider '{key}': expected 1-{MAX_MODEL_ID_BYTES} bytes with no whitespace or control characters"
                )));
            }
            model_ids.insert(id.as_str());
        }
        if model_ids.len() > MAX_MODELS_PER_PROVIDER {
            return Err(ProviderCatalogError::Invalid(format!(
                "provider '{key}' has more than {MAX_MODELS_PER_PROVIDER} models"
            )));
        }
        for (model, settings) in &overlay.model_overrides {
            if let Some(protocol) = &settings.upstream_protocol
                && !valid_protocol(protocol)
            {
                return Err(ProviderCatalogError::Invalid(format!(
                    "invalid upstream_protocol '{protocol}' for model '{model}' in provider '{key}'"
                )));
            }
            if let Some(pricing) = &settings.pricing {
                for (name, price) in [
                    ("input_per_million", pricing.input_per_million),
                    ("output_per_million", pricing.output_per_million),
                    ("cache_read_per_million", pricing.cache_read_per_million),
                    ("cache_write_per_million", pricing.cache_write_per_million),
                    ("reasoning_per_million", pricing.reasoning_per_million),
                ] {
                    if let Some(price) = price
                        && !(0.0..=MAX_PRICE_PER_MILLION).contains(&price)
                    {
                        return Err(ProviderCatalogError::Invalid(format!(
                            "invalid {name} {price} for model '{model}' in provider '{key}': expected a finite value in 0..={MAX_PRICE_PER_MILLION}"
                        )));
                    }
                }
            }
            if let Some(limits) = &settings.limits {
                if limits.context_window == Some(0) {
                    return Err(ProviderCatalogError::Invalid(format!(
                        "context_window must be positive for model '{model}' in provider '{key}'"
                    )));
                }
                if limits.max_output_tokens == Some(0) {
                    return Err(ProviderCatalogError::Invalid(format!(
                        "max_output_tokens must be positive for model '{model}' in provider '{key}'"
                    )));
                }
                if let (Some(context), Some(output)) =
                    (limits.context_window, limits.max_output_tokens)
                    && output > context
                {
                    return Err(ProviderCatalogError::Invalid(format!(
                        "max_output_tokens exceeds context_window for model '{model}' in provider '{key}'"
                    )));
                }
            }
        }
    }
    Ok(())
}

/// Creates a commented template only when the file is absent. Existing files,
/// including symlinks, are left untouched; returns whether a file was created.
pub fn ensure_providers_yaml_template(path: &Path) -> io::Result<bool> {
    if fs::symlink_metadata(path).is_ok() {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        ensure_private_dir(parent)?;
    }
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return Ok(false),
        Err(error) => return Err(error),
    };
    file.write_all(TEMPLATE.as_bytes())?;
    file.flush()?;
    drop(file);
    ensure_private_file(path)?;
    Ok(true)
}

/// Loads and validates a user overlay. A missing file is an empty overlay;
/// symlinks, oversize files, malformed YAML and invalid values are errors.
pub fn load_providers_yaml(path: &Path) -> Result<ProvidersYamlConfig, ProviderCatalogError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(ProvidersYamlConfig::default());
        }
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() {
        return Err(ProviderCatalogError::Symlink(path.to_path_buf()));
    }
    if metadata.len() > MAX_CATALOG_BYTES {
        return Err(ProviderCatalogError::TooLarge {
            bytes: metadata.len(),
        });
    }
    let bytes = fs::read(path)?;
    if bytes.len() as u64 > MAX_CATALOG_BYTES {
        return Err(ProviderCatalogError::TooLarge {
            bytes: bytes.len() as u64,
        });
    }
    let config: ProvidersYamlConfig = serde_yaml_ng::from_slice(&bytes)
        .map_err(|error| ProviderCatalogError::InvalidYaml(error.to_string()))?;
    validate(&config)?;
    Ok(config)
}

/// Precedence for runtime lookups: exact provider, exact adapter, provider
/// wildcard, then adapter wildcard. The catalog is already validated.
pub fn find_model_override(
    config: &ProvidersYamlConfig,
    provider_id: &str,
    adapter_id: &str,
    model_id: &str,
) -> Option<ModelOverrideConfig> {
    let for_key = |key: &str| {
        config.providers.get(key).and_then(|overlay| {
            overlay
                .model_overrides
                .get(model_id)
                .or_else(|| overlay.model_overrides.get("*"))
        })
    };
    for_key(provider_id)
        .or_else(|| for_key(adapter_id))
        .cloned()
}

/// Adds overlay models to the built-in list, dedupes in order, then removes
/// every id listed in `remove_models`.
pub fn merge_provider_models(
    base: &[String],
    overlay: Option<&ProviderCatalogOverlay>,
) -> Vec<String> {
    let mut result = Vec::with_capacity(
        base.len()
            .saturating_add(overlay.map_or(0, |overlay| overlay.default_models.len())),
    );
    let mut seen = BTreeSet::new();
    for id in base.iter().chain(
        overlay
            .into_iter()
            .flat_map(|overlay| overlay.default_models.iter()),
    ) {
        if seen.insert(id.as_str()) {
            result.push(id.clone());
        }
    }
    if let Some(overlay) = overlay
        && !overlay.remove_models.is_empty()
    {
        let removed: BTreeSet<&str> = overlay.remove_models.iter().map(|id| id.as_str()).collect();
        result.retain(|id| !removed.contains(id.as_str()));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = env::temp_dir().join(format!("exoroute-catalog-{}", uuid::Uuid::new_v4()));
        ensure_private_dir(&dir).expect("temporary catalog directory");
        dir
    }

    fn write(path: &Path, contents: &str) {
        fs::write(path, contents).expect("write temporary catalog");
    }

    #[test]
    fn missing_file_is_an_empty_overlay() {
        let dir = temp_dir();
        let config = load_providers_yaml(&dir.join("providers.yaml")).expect("missing file");
        assert!(config.providers.is_empty());
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn valid_yaml_adds_and_removes_models() {
        let dir = temp_dir();
        let path = dir.join("providers.yaml");
        write(
            &path,
            "providers:\n  opencode_go:\n    default_models: [hy4-flash-free, extra]\n    remove_models: [old-model]\n  opencode_zen:\n    default_models: [longcat-2.5-free]\n    model_overrides:\n      gpt-5:\n        upstream_protocol: responses\n",
        );
        let config = load_providers_yaml(&path).expect("valid catalog");
        assert_eq!(
            config.overlaid_provider_ids(),
            ["opencode_go", "opencode_zen"]
        );
        let overlay = config.providers.get("opencode_go").expect("go overlay");
        let merged =
            merge_provider_models(&["base".to_owned(), "old-model".to_owned()], Some(overlay));
        assert_eq!(merged, ["base", "hy4-flash-free", "extra"]);
        let zen = config.providers.get("opencode_zen").expect("zen overlay");
        assert!(config.overlay_for("opencode_zen", "zen").is_some());
        assert_eq!(
            merge_provider_models(&["old".to_owned()], Some(zen)),
            ["old", "longcat-2.5-free"]
        );
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn merge_dedupes_and_ignores_missing_overlay() {
        let base = ["a".to_owned(), "b".to_owned(), "a".to_owned()];
        assert_eq!(merge_provider_models(&base, None), ["a", "b"]);
        let overlay = ProviderCatalogOverlay {
            default_models: vec!["b".to_owned(), "c".to_owned(), "c".to_owned()],
            remove_models: vec!["a".to_owned()],
            ..ProviderCatalogOverlay::default()
        };
        assert_eq!(merge_provider_models(&base, Some(&overlay)), ["b", "c"]);
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let dir = temp_dir();
        let path = dir.join("providers.yaml");
        write(&path, "providers:\n  opencode_go:\n    surprise: true\n");
        assert!(matches!(
            load_providers_yaml(&path),
            Err(ProviderCatalogError::InvalidYaml(_))
        ));
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn invalid_model_ids_are_rejected() {
        let dir = temp_dir();
        let path = dir.join("providers.yaml");
        for id in ["has space", "control\u{1}", &"x".repeat(257)] {
            write(
                &path,
                &format!("providers:\n  p:\n    default_models: [{id}]\n"),
            );
            assert!(
                load_providers_yaml(&path).is_err(),
                "invalid model id must be rejected: {id}"
            );
        }
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn invalid_provider_key_and_protocol_are_rejected() {
        let dir = temp_dir();
        let path = dir.join("providers.yaml");
        write(&path, "providers:\n  Bad_Key:\n    default_models: [m]\n");
        assert!(matches!(
            load_providers_yaml(&path),
            Err(ProviderCatalogError::Invalid(_))
        ));
        write(
            &path,
            "providers:\n  p:\n    model_overrides:\n      m:\n        upstream_protocol: ../../etc\n",
        );
        assert!(matches!(
            load_providers_yaml(&path),
            Err(ProviderCatalogError::Invalid(_))
        ));
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn oversize_file_is_rejected_before_parsing() {
        let dir = temp_dir();
        let path = dir.join("providers.yaml");
        fs::write(&path, vec![b'x'; MAX_CATALOG_BYTES as usize + 1]).expect("oversize file");
        assert!(matches!(
            load_providers_yaml(&path),
            Err(ProviderCatalogError::TooLarge { .. })
        ));
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[cfg(unix)]
    #[test]
    fn symlink_is_rejected() {
        use std::os::unix::fs::symlink;
        let dir = temp_dir();
        let target = dir.join("target.yaml");
        write(&target, "providers: {}\n");
        let link = dir.join("providers.yaml");
        symlink(&target, &link).expect("create symlink");
        assert!(matches!(
            load_providers_yaml(&link),
            Err(ProviderCatalogError::Symlink(_))
        ));
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn template_creation_is_idempotent_and_never_overwrites() {
        let dir = temp_dir();
        let path = dir.join("providers.yaml");
        assert!(ensure_providers_yaml_template(&path).expect("create template"));
        let first = fs::read(&path).expect("read template");
        assert!(!ensure_providers_yaml_template(&path).expect("second call"));
        assert_eq!(fs::read(&path).expect("reread template"), first);
        let loaded = load_providers_yaml(&path).expect("template is valid YAML");
        assert!(loaded.providers.is_empty());
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn pricing_and_limits_parse_and_validate() {
        let dir = temp_dir();
        let path = dir.join("providers.yaml");
        write(
            &path,
            "providers:\n  p:\n    model_overrides:\n      m:\n        pricing: { input_per_million: 2.5, output_per_million: 10.0, cache_read_per_million: 0.1, cache_write_per_million: 1.25, reasoning_per_million: 15.0 }\n        limits: { context_window: 200000, max_output_tokens: 32000 }\n",
        );
        let config = load_providers_yaml(&path).expect("valid pricing and limits");
        let settings = config
            .providers
            .get("p")
            .and_then(|overlay| overlay.model_overrides.get("m"))
            .expect("model override");
        assert_eq!(
            settings.pricing.as_ref().unwrap().input_per_million,
            Some(2.5)
        );
        assert_eq!(
            settings.limits.as_ref().unwrap().context_window,
            Some(200000)
        );
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn invalid_prices_and_limits_are_rejected() {
        let dir = temp_dir();
        let path = dir.join("providers.yaml");
        for yaml in [
            "providers:\n  p:\n    model_overrides:\n      m:\n        pricing: { input_per_million: -1.0 }\n",
            "providers:\n  p:\n    model_overrides:\n      m:\n        pricing: { output_per_million: .nan }\n",
            "providers:\n  p:\n    model_overrides:\n      m:\n        pricing: { input_per_million: .inf }\n",
            "providers:\n  p:\n    model_overrides:\n      m:\n        pricing: { reasoning_per_million: 1000001.0 }\n",
            "providers:\n  p:\n    model_overrides:\n      m:\n        limits: { context_window: 0 }\n",
            "providers:\n  p:\n    model_overrides:\n      m:\n        limits: { max_output_tokens: 0 }\n",
            "providers:\n  p:\n    model_overrides:\n      m:\n        limits: { context_window: 100, max_output_tokens: 101 }\n",
        ] {
            write(&path, yaml);
            assert!(
                matches!(
                    load_providers_yaml(&path),
                    Err(ProviderCatalogError::Invalid(_))
                ),
                "{yaml}"
            );
        }
        write(
            &path,
            "providers:\n  p:\n    model_overrides:\n      m:\n        pricing: { mystery: 1.0 }\n",
        );
        assert!(matches!(
            load_providers_yaml(&path),
            Err(ProviderCatalogError::InvalidYaml(_))
        ));
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn cost_estimation_covers_components_and_free_tier() {
        let none = ModelPricingConfig::default();
        assert_eq!(estimate_cost_micro_usd(&none, 100, 100, 0, 0, 0), None);
        let pricing = ModelPricingConfig {
            input_per_million: Some(2.5),
            output_per_million: Some(10.0),
            cache_read_per_million: Some(0.1),
            cache_write_per_million: Some(1.25),
            reasoning_per_million: Some(15.0),
        };
        assert_eq!(
            estimate_cost_micro_usd(&pricing, 1_000_000, 500_000, 0, 0, 0),
            Some(7_500_000)
        );
        assert_eq!(
            estimate_cost_micro_usd(&pricing, 0, 0, 1_000_000, 1_000_000, 1_000_000),
            Some(16_350_000)
        );
        let free = ModelPricingConfig {
            input_per_million: Some(0.0),
            ..ModelPricingConfig::default()
        };
        assert_eq!(estimate_cost_micro_usd(&free, 1000, 1000, 0, 0, 0), Some(0));
    }

    #[test]
    fn model_override_lookup_follows_precedence() {
        let config: ProvidersYamlConfig = serde_yaml_ng::from_str(
            "providers:\n  provider-a:\n    model_overrides:\n      exact: { pricing: { input_per_million: 1.0 } }\n      '*': { pricing: { input_per_million: 2.0 } }\n  adapter-a:\n    model_overrides:\n      exact: { pricing: { input_per_million: 3.0 } }\n      '*': { pricing: { input_per_million: 4.0 } }\n",
        )
        .expect("parse precedence config");
        validate(&config).expect("valid precedence config");
        let price = |provider: &str, adapter: &str, model: &str| {
            find_model_override(&config, provider, adapter, model)
                .and_then(|entry| entry.pricing)
                .and_then(|pricing| pricing.input_per_million)
        };
        assert_eq!(price("provider-a", "adapter-a", "exact"), Some(1.0));
        assert_eq!(price("provider-a", "adapter-a", "other"), Some(2.0));
        assert_eq!(price("unknown", "adapter-a", "exact"), Some(3.0));
        assert_eq!(price("unknown", "adapter-a", "other"), Some(4.0));
        assert_eq!(price("unknown", "unknown", "any"), None);
    }
}
