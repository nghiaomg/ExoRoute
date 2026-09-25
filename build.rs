use serde::Deserialize;
use std::{
    env, fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderPresetCatalog {
    providers: Vec<YamlProviderPreset>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct YamlProviderPreset {
    id: String,
    adapter_id: String,
    name: String,
    description: String,
    category: String,
    labels: Vec<String>,
    default_base_url: Option<String>,
    default_logo_url: Option<String>,
    default_model_prefix: Option<String>,
    #[serde(default)]
    default_models: Vec<String>,
    default_auth_type: String,
    supported_auth_types: Vec<String>,
    default_preferred_protocol: String,
    default_supported_protocols: Vec<String>,
    protocol_selectable: bool,
    capabilities: YamlAdapterCapabilities,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct YamlAdapterCapabilities {
    api_keys: bool,
    oauth_accounts: bool,
    local_usage_meter: bool,
    #[serde(default)]
    local_quota_tracking: bool,
    model_discovery: bool,
    usage_limits: bool,
    api_key_usage: bool,
    api_key_usage_status: String,
    model_protocol_routing: bool,
    supported_upstream_protocols: Vec<String>,
    api_key_auth_assist: bool,
    model_catalog_authoritative: bool,
    event_stream_response: bool,
    auth_panel: Option<String>,
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_-".contains(&byte))
}

fn valid_model_prefix(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes.iter().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || (index > 0 && b"._-".contains(byte))
        })
}

fn valid_default_model(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty()
        && trimmed.len() <= 256
        && trimmed == value
        && value
            .bytes()
            .all(|byte| !byte.is_ascii_control() && byte != b' ')
}

fn validate_provider_presets(presets: &[YamlProviderPreset]) -> Result<(), String> {
    if presets.is_empty() {
        return Err("the catalog must contain at least one provider preset".to_owned());
    }

    let mut ids = std::collections::HashSet::with_capacity(presets.len());
    let mut model_prefixes = std::collections::HashSet::with_capacity(presets.len());
    const CATEGORIES: &[&str] = &["cloud_api", "gateway", "oauth"];
    const LABELS: &[&str] = &["free", "free_tier"];
    const AUTH_TYPES: &[&str] = &[
        "none",
        "bearer",
        "header",
        "codex_oauth",
        "antigravity_oauth",
        "kilocode_oauth",
    ];
    const PROTOCOLS: &[&str] = &[
        "chat_completions",
        "responses",
        "messages",
        "google_generate_content",
    ];

    for (index, preset) in presets.iter().enumerate() {
        if !valid_id(&preset.id) || !valid_id(&preset.adapter_id) {
            return Err(format!(
                "provider preset at index {index} has an invalid id or adapter_id"
            ));
        }
        if !ids.insert(preset.id.as_str()) {
            return Err(format!("duplicate provider preset id: {}", preset.id));
        }
        if preset.name.trim().is_empty() || preset.description.trim().is_empty() {
            return Err(format!(
                "provider preset {} must have a name and description",
                preset.id
            ));
        }
        if preset
            .default_model_prefix
            .as_deref()
            .is_some_and(|prefix| !valid_model_prefix(prefix) || !model_prefixes.insert(prefix))
        {
            return Err(format!(
                "provider preset {} has an invalid or duplicate default model prefix",
                preset.id
            ));
        }
        let mut default_models =
            std::collections::HashSet::with_capacity(preset.default_models.len());
        if preset
            .default_models
            .iter()
            .any(|model| !valid_default_model(model) || !default_models.insert(model.as_str()))
        {
            return Err(format!(
                "provider preset {} has an invalid or duplicate default model",
                preset.id
            ));
        }
        if !CATEGORIES.contains(&preset.category.as_str()) {
            return Err(format!(
                "provider preset {} has an unsupported category",
                preset.id
            ));
        }
        let mut labels = std::collections::HashSet::with_capacity(preset.labels.len());
        if preset
            .labels
            .iter()
            .any(|label| !LABELS.contains(&label.as_str()) || !labels.insert(label.as_str()))
        {
            return Err(format!(
                "provider preset {} has an unsupported or duplicate label",
                preset.id
            ));
        }
        if preset
            .default_base_url
            .as_deref()
            .is_some_and(|url| !url.starts_with("https://"))
            || preset
                .default_logo_url
                .as_deref()
                .is_some_and(|url| !url.starts_with("https://"))
        {
            return Err(format!("provider preset {} URLs must use HTTPS", preset.id));
        }
        if preset.supported_auth_types.is_empty()
            || !preset
                .supported_auth_types
                .contains(&preset.default_auth_type)
            || preset
                .supported_auth_types
                .iter()
                .any(|auth_type| !AUTH_TYPES.contains(&auth_type.as_str()))
        {
            return Err(format!(
                "provider preset {} has invalid authentication metadata",
                preset.id
            ));
        }
        if preset.default_supported_protocols.is_empty()
            || !preset
                .default_supported_protocols
                .contains(&preset.default_preferred_protocol)
            || preset
                .default_supported_protocols
                .iter()
                .any(|protocol| !PROTOCOLS.contains(&protocol.as_str()))
        {
            return Err(format!(
                "provider preset {} has invalid protocol metadata",
                preset.id
            ));
        }
        let capabilities = &preset.capabilities;
        if !matches!(
            capabilities.api_key_usage_status.as_str(),
            "supported" | "unverified" | "unsupported"
        ) || (capabilities.api_key_usage && capabilities.api_key_usage_status == "unsupported")
            || (!capabilities.api_key_usage && capabilities.api_key_usage_status == "supported")
        {
            return Err(format!(
                "provider preset {} has inconsistent API-key usage capability metadata",
                preset.id
            ));
        }
        if capabilities.supported_upstream_protocols.is_empty()
            || capabilities
                .supported_upstream_protocols
                .iter()
                .any(|protocol| !PROTOCOLS.contains(&protocol.as_str()))
            || capabilities.model_protocol_routing && !capabilities.model_discovery
        {
            return Err(format!(
                "provider preset {} has invalid upstream protocol capability metadata",
                preset.id
            ));
        }
        if !capabilities.api_keys && !capabilities.oauth_accounts {
            return Err(format!(
                "provider preset {} must support API keys or OAuth accounts",
                preset.id
            ));
        }
        if (capabilities.oauth_accounts || capabilities.api_key_auth_assist)
            != capabilities.auth_panel.is_some()
        {
            return Err(format!(
                "provider preset {} has an unsupported authentication panel",
                preset.id
            ));
        }
        if let Some(panel) = capabilities.auth_panel.as_deref()
            && !matches!(
                panel,
                "openai_codex" | "command_code" | "cline" | "kilocode"
            )
        {
            return Err(format!(
                "provider preset {} has an unsupported authentication panel",
                preset.id
            ));
        }

        for sibling in &presets[..index] {
            if sibling.adapter_id == preset.adapter_id
                && (sibling.supported_auth_types != preset.supported_auth_types
                    || sibling.capabilities != preset.capabilities)
            {
                return Err(format!(
                    "presets sharing adapter {} must have identical capabilities and authentication types",
                    preset.adapter_id
                ));
            }
        }
    }

    Ok(())
}

fn rust_string(value: &str) -> String {
    format!("{value:?}")
}

fn rust_string_slice(values: &[String]) -> String {
    format!(
        "&[{}]",
        values
            .iter()
            .map(|value| rust_string(value))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn rust_optional_string(value: Option<&str>) -> String {
    value.map_or_else(
        || "None".to_owned(),
        |value| format!("Some({})", rust_string(value)),
    )
}

fn rust_category_variant(value: &str) -> Option<&'static str> {
    match value {
        "cloud_api" => Some("CloudApi"),
        "gateway" => Some("Gateway"),
        "oauth" => Some("Oauth"),
        _ => None,
    }
}

fn rust_label_variant(value: &str) -> Option<&'static str> {
    match value {
        "free" => Some("Free"),
        "free_tier" => Some("FreeTier"),
        _ => None,
    }
}

fn generate_provider_presets(manifest_dir: &Path, out_dir: &Path) -> Result<(), String> {
    const CATALOG_PATH: &str = "data/provider-presets.yaml";
    println!("cargo:rerun-if-changed={CATALOG_PATH}");

    let yaml_path = manifest_dir.join(CATALOG_PATH);
    let yaml = fs::read_to_string(&yaml_path)
        .map_err(|error| format!("could not read {}: {error}", yaml_path.display()))?;
    let catalog: ProviderPresetCatalog = serde_yaml_ng::from_str(&yaml)
        .map_err(|error| format!("{} is invalid YAML: {error}", yaml_path.display()))?;
    validate_provider_presets(&catalog.providers)
        .map_err(|error| format!("{} is invalid: {error}", yaml_path.display()))?;

    let mut generated = format!(
        "static YAML_PROVIDER_PRESETS: [ProviderPreset; {}] = [\n",
        catalog.providers.len()
    );
    for preset in &catalog.providers {
        let capabilities = &preset.capabilities;
        let category = rust_category_variant(&preset.category).ok_or_else(|| {
            format!(
                "{} has an unsupported category for provider preset {}",
                CATALOG_PATH, preset.id
            )
        })?;
        let labels = preset
            .labels
            .iter()
            .map(|label| {
                rust_label_variant(label)
                    .map(|variant| format!("ProviderLabel::{variant}"))
                    .ok_or_else(|| {
                        format!(
                            "{} has an unsupported label for provider preset {}",
                            CATALOG_PATH, preset.id
                        )
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        generated.push_str(&format!(
            "    ProviderPreset {{\n\
                 id: {},\n\
                 adapter_id: {},\n\
                 name: {},\n\
                 description: {},\n\
                 category: ProviderCategory::{},\n\
                 labels: &[{}],\n\
                 default_base_url: {},\n\
                 default_logo_url: {},\n\
                 default_model_prefix: {},\n\
                 default_models: {},\n\
                 default_auth_type: {},\n\
                 supported_auth_types: {},\n\
                 default_preferred_protocol: {},\n\
                 default_supported_protocols: {},\n\
                 protocol_selectable: {},\n\
                 capabilities: AdapterCapabilities {{\n\
                     api_keys: {},\n\
                     oauth_accounts: {},\n\
                     local_usage_meter: {},\n\
                     local_quota_tracking: {},\n\
                     model_discovery: {},\n\
                     usage_limits: {},\n\
                     api_key_usage: {},\n\
                     api_key_usage_status: {},\n\
                     model_protocol_routing: {},\n\
                     supported_upstream_protocols: {},\n\
                     api_key_auth_assist: {},\n\
                     model_catalog_authoritative: {},\n\
                     event_stream_response: {},\n\
                     auth_panel: {},\n\
                 }},\n\
             }},\n",
            rust_string(&preset.id),
            rust_string(&preset.adapter_id),
            rust_string(&preset.name),
            rust_string(&preset.description),
            category,
            labels.join(", "),
            rust_optional_string(preset.default_base_url.as_deref()),
            rust_optional_string(preset.default_logo_url.as_deref()),
            rust_optional_string(preset.default_model_prefix.as_deref()),
            rust_string_slice(&preset.default_models),
            rust_string(&preset.default_auth_type),
            rust_string_slice(&preset.supported_auth_types),
            rust_string(&preset.default_preferred_protocol),
            rust_string_slice(&preset.default_supported_protocols),
            preset.protocol_selectable,
            capabilities.api_keys,
            capabilities.oauth_accounts,
            capabilities.local_usage_meter,
            capabilities.local_quota_tracking,
            capabilities.model_discovery,
            capabilities.usage_limits,
            capabilities.api_key_usage,
            rust_string(&capabilities.api_key_usage_status),
            capabilities.model_protocol_routing,
            rust_string_slice(&capabilities.supported_upstream_protocols),
            capabilities.api_key_auth_assist,
            capabilities.model_catalog_authoritative,
            capabilities.event_stream_response,
            rust_optional_string(capabilities.auth_panel.as_deref()),
        ));
    }
    generated.push_str("];\n");

    let output_path = out_dir.join("provider_presets.rs");
    fs::write(&output_path, generated)
        .map_err(|error| format!("could not write {}: {error}", output_path.display()))
}

fn collect_paths(path: &Path, tracked: &mut Vec<PathBuf>, files: &mut Vec<PathBuf>) {
    tracked.push(path.to_path_buf());
    if !path.is_dir() {
        return;
    }

    let entries = fs::read_dir(path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    for entry in entries {
        let entry = entry.unwrap_or_else(|error| panic!("failed to read dashboard entry: {error}"));
        let child = entry.path();
        if child.is_dir() {
            collect_paths(&child, tracked, files);
        } else {
            tracked.push(child.clone());
            files.push(child);
        }
    }
}

fn main() {
    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo sets CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR"));
    if let Err(error) = generate_provider_presets(&manifest_dir, &out_dir) {
        panic!("failed to generate the embedded provider preset catalog: {error}");
    }

    let dashboard_dir = manifest_dir.join("web").join("dist");
    let mut tracked_paths = Vec::new();
    let mut files = Vec::new();
    collect_paths(&dashboard_dir, &mut tracked_paths, &mut files);

    println!("cargo:rerun-if-changed=build.rs");
    tracked_paths.sort();
    tracked_paths.dedup();
    for path in tracked_paths {
        let relative_path = path
            .strip_prefix(&manifest_dir)
            .expect("dashboard paths are inside the project");
        println!("cargo:rerun-if-changed={}", relative_path.display());
    }

    files.sort();
    let mut digest = std::collections::hash_map::DefaultHasher::new();
    for path in files {
        let relative_path = path
            .strip_prefix(&manifest_dir)
            .expect("dashboard paths are inside the project");
        relative_path.to_string_lossy().hash(&mut digest);
        fs::read(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
            .hash(&mut digest);
    }

    println!(
        "cargo:rustc-env=EXOROUTE_DASHBOARD_ASSET_DIGEST={:016x}",
        digest.finish()
    );
}
