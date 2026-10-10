//! Usage: Export gateway model catalog into Pi (`~/.pi/agent/models.json`),
//! OpenCode (`~/.config/opencode/opencode.json`), and OMP
//! (`~/.omp/agent/models.yml`) provider model blocks.
//!
//! Each `cli_key` maps to one `aio-<cli>` provider inside those configs. The
//! exporter refreshes only that provider's model list, preserving unrelated
//! providers and the target provider's connection settings when it already
//! exists. A missing provider block is created from a per-CLI template
//! (api/npm + baseURL pointed at the local gateway).
//!
//! OpenCode input accepts JSONC comments and trailing commas. Changed configs
//! are serialized as strict JSON (comments are not retained); unchanged configs
//! are left byte-for-byte intact.

use std::path::PathBuf;

use serde_json::{json, Map, Value};

use crate::app_state::{ensure_db_ready, DbInitState};
use crate::infra::{app_paths, settings};
use crate::shared::error::{AppError, AppResult};
use crate::shared::fs::{read_optional_file_with_max_len, write_file_atomic};
use crate::{blocking, db};

const PI_MODELS_MAX_BYTES: usize = 4 * 1024 * 1024;
const OPENCODE_MODELS_MAX_BYTES: usize = 4 * 1024 * 1024;
const OMP_MODELS_MAX_BYTES: usize = 4 * 1024 * 1024;

fn invalid(message: impl Into<String>) -> AppError {
    AppError::from(format!("SEC_INVALID_INPUT: {}", message.into()))
}

/// Static per-CLI provider template used when creating a new provider block and
/// to derive the default provider name in the UI.
struct ProviderTemplate {
    cli_key: &'static str,
    default_provider: &'static str,
    display_name: &'static str,
    /// Pi `api` field.
    pi_api: &'static str,
    /// OpenCode `npm` field.
    opencode_npm: &'static str,
    /// Gateway path appended to `http://127.0.0.1:<port>` for this CLI.
    /// Pi anthropic providers do NOT take `/v1` (Pi appends it), while OpenCode
    /// `@ai-sdk/anthropic` requires it — hence separate path fields.
    pi_path: &'static str,
    opencode_path: &'static str,
}

const TEMPLATES: &[ProviderTemplate] = &[
    ProviderTemplate {
        cli_key: "codex",
        default_provider: "aio-codex",
        display_name: "AIO Codex (local proxy)",
        pi_api: "openai-responses",
        opencode_npm: "@ai-sdk/openai",
        pi_path: "/v1",
        opencode_path: "/v1",
    },
    ProviderTemplate {
        cli_key: "claude",
        default_provider: "aio-claude",
        display_name: "AIO Claude (local proxy)",
        pi_api: "anthropic-messages",
        opencode_npm: "@ai-sdk/anthropic",
        // Pi anthropic baseURL has no /v1; OpenCode anthropic needs /v1.
        pi_path: "/claude",
        opencode_path: "/claude/v1",
    },
    ProviderTemplate {
        cli_key: "grok",
        default_provider: "aio-grok",
        display_name: "AIO Grok (local proxy)",
        pi_api: "openai-responses",
        opencode_npm: "@ai-sdk/openai",
        pi_path: "/grok",
        opencode_path: "/grok",
    },
    ProviderTemplate {
        cli_key: "gemini",
        default_provider: "aio-gemini",
        display_name: "AIO Gemini (local proxy)",
        pi_api: "google-generative-ai",
        opencode_npm: "@ai-sdk/google",
        pi_path: "/gemini/v1beta",
        opencode_path: "/gemini/v1beta",
    },
];

fn template_for(cli_key: &str) -> AppResult<&'static ProviderTemplate> {
    TEMPLATES
        .iter()
        .find(|t| t.cli_key == cli_key)
        .ok_or_else(|| invalid(format!("unsupported cli_key for model export: {cli_key}")))
}

/// Default provider name suggested to the user for a CLI (e.g. `aio-codex`).
pub(crate) fn default_provider_name(cli_key: &str) -> AppResult<String> {
    Ok(template_for(cli_key)?.default_provider.to_string())
}

/// Result of one export run.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CliModelExportResult {
    pub cli_key: String,
    pub provider_name: String,
    pub target: String,
    pub path: String,
    /// Models written into the provider block.
    pub models: Vec<String>,
    /// True when the provider block was created rather than updated.
    pub created: bool,
    /// True when nothing changed on disk.
    pub unchanged: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExportModel {
    model_id: String,
    context_window: i64,
    reasoning_effort: String,
}

fn gateway_base(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

fn gateway_port(app: &tauri::AppHandle) -> u16 {
    settings::read(app)
        .map(|s| s.preferred_port.max(settings::DEFAULT_GATEWAY_PORT))
        .unwrap_or(settings::DEFAULT_GATEWAY_PORT)
}

/// Collect the enabled catalog models to write for `provider_name`.
///
/// The default provider name (`aio-<cli>`) denotes the local gateway proxy
/// block, which routes every enabled provider of the CLI, so it exports the
/// whole enabled catalog. Any other name must match a real provider: only
/// models whose routes carry that provider name are exported. Exporting an
/// empty list is rejected so a mismatched provider name can never wipe an
/// existing provider block in the target config.
fn models_for_provider(
    db: &db::Db,
    cli_key: &str,
    provider_name: &str,
) -> AppResult<Vec<ExportModel>> {
    let result = crate::app::model_catalog_service::list_blocking(db, cli_key)?;
    let default_provider = template_for(cli_key)?.default_provider;
    let export_all = provider_name == default_provider;
    let mut models = Vec::new();
    for item in result.items {
        if !item.enabled {
            continue;
        }
        if !export_all
            && !item
                .routes
                .iter()
                .any(|route| route.provider_name == provider_name)
        {
            continue;
        }
        models.push(ExportModel {
            model_id: item.model_id,
            context_window: item.context_window,
            reasoning_effort: item.reasoning_effort,
        });
    }
    if models.is_empty() {
        return Err(invalid(format!(
            "no enabled models available for provider '{provider_name}' in the {cli_key} catalog; refresh the model catalog first or use the default provider name"
        )));
    }
    models.sort_by(|left, right| left.model_id.cmp(&right.model_id));
    models.dedup_by(|left, right| left.model_id == right.model_id);
    Ok(models)
}

// ─── Pi (`~/.pi/agent/models.json`) ─────────────────────────────────────────

fn pi_config_path(app: &tauri::AppHandle) -> AppResult<PathBuf> {
    Ok(app_paths::home_dir(app)?
        .join(".pi")
        .join("agent")
        .join("models.json"))
}

fn pi_model_entry(model_id: &str, context_window: i64, tpl: &ProviderTemplate) -> Value {
    json!({
        "id": model_id,
        "name": model_id,
        "reasoning": true,
        "input": ["text", "image"],
        "contextWindow": context_window,
        "maxTokens": 128000,
        "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0 },
        "thinkingLevelMap": {
            "off": null, "minimal": null, "low": "low", "medium": "medium",
            "high": "high", "xhigh": "xhigh", "max": "max"
        },
        // Match the per-CLI api shape.
        "api": tpl.pi_api,
    })
}

fn export_to_pi(
    app: &tauri::AppHandle,
    db: &db::Db,
    cli_key: &str,
    provider_name: &str,
) -> AppResult<CliModelExportResult> {
    let tpl = template_for(cli_key)?;
    let path = pi_config_path(app)?;
    let port = gateway_port(app);
    let base_url = format!("{}{}", gateway_base(port), tpl.pi_path);
    let models = models_for_provider(db, cli_key, provider_name)?;

    let raw = read_optional_file_with_max_len(&path, PI_MODELS_MAX_BYTES)?;
    let mut root: Value = match raw {
        Some(bytes) => serde_json::from_slice(&bytes)
            .map_err(|e| format!("failed to parse {}: {e}", path.display()))?,
        None => json!({ "providers": {} }),
    };
    if !root.is_object() {
        return Err(invalid(format!("{} root is not an object", path.display())));
    }
    let providers = root
        .as_object_mut()
        .unwrap()
        .entry("providers".to_string())
        .or_insert_with(|| json!({}));
    if !providers.is_object() {
        return Err(invalid(format!(
            "{}.providers is not an object",
            path.display()
        )));
    }
    let providers_obj = providers.as_object_mut().unwrap();

    let created = !providers_obj.contains_key(provider_name);
    let entry = providers_obj
        .entry(provider_name.to_string())
        .or_insert_with(|| {
            json!({
                "baseUrl": base_url,
                "api": tpl.pi_api,
                "authHeader": true,
                "apiKey": crate::infra::cli_proxy::PLACEHOLDER_KEY,
                "models": [],
            })
        });
    if !entry.is_object() {
        return Err(invalid(format!(
            "{}.providers.{provider_name} is not an object",
            path.display()
        )));
    }
    let entry_obj = entry.as_object_mut().unwrap();
    let new_models: Vec<Value> = models
        .iter()
        .map(|model| pi_model_entry(&model.model_id, model.context_window, tpl))
        .collect();
    let unchanged = entry_obj.get("models") == Some(&Value::Array(new_models.clone()));
    entry_obj.insert("models".to_string(), Value::Array(new_models));

    if !unchanged {
        let bytes = serde_json::to_vec_pretty(&root)
            .map_err(|e| format!("failed to serialize {}: {e}", path.display()))?;
        write_file_atomic(&path, &bytes)?;
    }

    Ok(CliModelExportResult {
        cli_key: cli_key.to_string(),
        provider_name: provider_name.to_string(),
        target: "pi".to_string(),
        path: path.display().to_string(),
        models: models.iter().map(|model| model.model_id.clone()).collect(),
        created,
        unchanged,
    })
}

// ─── OpenCode (`~/.config/opencode/opencode.json`) ───────────────────────────

fn opencode_config_path(app: &tauri::AppHandle) -> AppResult<PathBuf> {
    Ok(app_paths::home_dir(app)?
        .join(".config")
        .join("opencode")
        .join("opencode.json"))
}

/// Model entry for one catalog model.
///
/// Reasoning variants are deliberately left to OpenCode: it derives them from
/// the SDK family itself, and the exporter must not invent `variants` payloads.
fn opencode_model_entry(model_id: &str, context_window: i64, provider_name: &str) -> Value {
    json!({
        "name": format!("{provider_name}/{model_id}"),
        "limit": {
            "context": context_window,
            "output": 128000
        },
        "attachment": true,
        "reasoning": true,
        "temperature": false,
        "tool_call": true,
    })
}

fn parse_opencode_config(bytes: &[u8]) -> Result<Value, serde_json_lenient::Error> {
    // JSONC permits comments and trailing commas, not JSON5 escapes or lossy UTF-8.
    let mut parser = serde_json_lenient::Deserializer::from_slice_with_options(
        bytes, false, false, false, false, false,
    );
    parser.set_allow_comments(true);
    parser.set_ignore_trailing_commas(true);
    let root = serde::Deserialize::deserialize(&mut parser)?;
    parser.end()?;
    Ok(root)
}

fn export_to_opencode(
    app: &tauri::AppHandle,
    db: &db::Db,
    cli_key: &str,
    provider_name: &str,
) -> AppResult<CliModelExportResult> {
    let path = opencode_config_path(app)?;
    let models = models_for_provider(db, cli_key, provider_name)?;
    export_opencode_to_path(&path, cli_key, provider_name, gateway_port(app), &models)
}

fn export_opencode_to_path(
    path: &std::path::Path,
    cli_key: &str,
    provider_name: &str,
    port: u16,
    models: &[ExportModel],
) -> AppResult<CliModelExportResult> {
    let tpl = template_for(cli_key)?;
    let base_url = format!("{}{}", gateway_base(port), tpl.opencode_path);

    let raw = read_optional_file_with_max_len(path, OPENCODE_MODELS_MAX_BYTES)?;
    let mut root: Value = match raw {
        Some(bytes) => parse_opencode_config(&bytes)
            .map_err(|e| format!("failed to parse {}: {e}", path.display()))?,
        None => json!({ "provider": {} }),
    };
    if !root.is_object() {
        return Err(invalid(format!("{} root is not an object", path.display())));
    }
    let providers = root
        .as_object_mut()
        .unwrap()
        .entry("provider".to_string())
        .or_insert_with(|| json!({}));
    if !providers.is_object() {
        return Err(invalid(format!(
            "{}.provider is not an object",
            path.display()
        )));
    }
    let providers_obj = providers.as_object_mut().unwrap();

    let created = !providers_obj.contains_key(provider_name);
    let entry = providers_obj
        .entry(provider_name.to_string())
        .or_insert_with(|| {
            json!({
                "name": tpl.display_name,
                "npm": tpl.opencode_npm,
                "options": {
                    "apiKey": crate::infra::cli_proxy::PLACEHOLDER_KEY,
                    "baseURL": base_url,
                },
                "models": {},
            })
        });
    if !entry.is_object() {
        return Err(invalid(format!(
            "{}.provider.{provider_name} is not an object",
            path.display()
        )));
    }
    let entry_obj = entry.as_object_mut().unwrap();
    let mut models_obj = Map::new();
    for model in models {
        models_obj.insert(
            model.model_id.clone(),
            opencode_model_entry(&model.model_id, model.context_window, provider_name),
        );
    }
    let new_models = Value::Object(models_obj);
    let unchanged = entry_obj.get("models") == Some(&new_models);
    entry_obj.insert("models".to_string(), new_models);

    if !unchanged {
        let bytes = serde_json::to_vec_pretty(&root)
            .map_err(|e| format!("failed to serialize {}: {e}", path.display()))?;
        write_file_atomic(path, &bytes)?;
    }

    Ok(CliModelExportResult {
        cli_key: cli_key.to_string(),
        provider_name: provider_name.to_string(),
        target: "opencode".to_string(),
        path: path.display().to_string(),
        models: models.iter().map(|model| model.model_id.clone()).collect(),
        created,
        unchanged,
    })
}

// ─── OMP (`~/.omp/agent/models.yml`) ─────────────────────────────────────────

fn omp_config_path(app: &tauri::AppHandle) -> AppResult<PathBuf> {
    Ok(app_paths::home_dir(app)?
        .join(".omp")
        .join("agent")
        .join("models.yml"))
}

fn omp_model_override(model: &ExportModel) -> serde_yaml::Value {
    let mut override_map = serde_yaml::Mapping::new();
    override_map.insert(
        serde_yaml::Value::String("reasoning".to_string()),
        serde_yaml::Value::Bool(true),
    );
    let mut thinking = serde_yaml::Mapping::new();
    thinking.insert(
        serde_yaml::Value::String("mode".to_string()),
        serde_yaml::Value::String("effort".to_string()),
    );
    thinking.insert(
        serde_yaml::Value::String("efforts".to_string()),
        serde_yaml::Value::Sequence(
            ["low", "medium", "high", "xhigh", "max"]
                .into_iter()
                .map(|value| serde_yaml::Value::String(value.to_string()))
                .collect(),
        ),
    );
    thinking.insert(
        serde_yaml::Value::String("defaultLevel".to_string()),
        serde_yaml::Value::String(model.reasoning_effort.clone()),
    );
    override_map.insert(
        serde_yaml::Value::String("thinking".to_string()),
        serde_yaml::Value::Mapping(thinking),
    );
    override_map.insert(
        serde_yaml::Value::String("input".to_string()),
        serde_yaml::Value::Sequence(
            ["text", "image"]
                .into_iter()
                .map(|value| serde_yaml::Value::String(value.to_string()))
                .collect(),
        ),
    );
    override_map.insert(
        serde_yaml::Value::String("contextWindow".to_string()),
        serde_yaml::Value::Number(serde_yaml::Number::from(model.context_window)),
    );
    serde_yaml::Value::Mapping(override_map)
}

fn omp_template_provider(cli_key: &str, port: u16) -> serde_yaml::Value {
    let tpl = template_for(cli_key).expect("validated CLI template");
    let mut provider = serde_yaml::Mapping::new();
    provider.insert(
        serde_yaml::Value::String("baseUrl".to_string()),
        serde_yaml::Value::String(format!("{}{}", gateway_base(port), tpl.pi_path)),
    );
    provider.insert(
        serde_yaml::Value::String("apiKey".to_string()),
        serde_yaml::Value::String(crate::infra::cli_proxy::PLACEHOLDER_KEY.to_string()),
    );
    provider.insert(
        serde_yaml::Value::String("api".to_string()),
        serde_yaml::Value::String(tpl.pi_api.to_string()),
    );
    provider.insert(
        serde_yaml::Value::String("discovery".to_string()),
        serde_yaml::Value::Mapping(serde_yaml::Mapping::from_iter([(
            serde_yaml::Value::String("type".to_string()),
            serde_yaml::Value::String("openai-models-list".to_string()),
        )])),
    );
    serde_yaml::Value::Mapping(provider)
}

fn yaml_mapping<'a>(
    value: &'a mut serde_yaml::Value,
    label: &str,
) -> AppResult<&'a mut serde_yaml::Mapping> {
    value
        .as_mapping_mut()
        .ok_or_else(|| invalid(format!("OMP {label} must be a mapping")))
}

fn yaml_mapping_ref<'a>(
    value: &'a serde_yaml::Value,
    label: &str,
) -> AppResult<&'a serde_yaml::Mapping> {
    value
        .as_mapping()
        .ok_or_else(|| invalid(format!("OMP {label} must be a mapping")))
}

fn export_omp_to_path(
    path: &std::path::Path,
    cli_key: &str,
    provider_name: &str,
    port: u16,
    models: &[ExportModel],
) -> AppResult<CliModelExportResult> {
    if models.is_empty() {
        return Err(invalid(
            "no enabled models available for OMP synchronization",
        ));
    }
    if models.iter().any(|model| model.context_window <= 0) {
        return Err(invalid("model contextWindow must be a positive integer"));
    }
    let raw = read_optional_file_with_max_len(path, OMP_MODELS_MAX_BYTES)?;
    let mut root = match raw {
        Some(bytes) => serde_yaml::from_slice::<serde_yaml::Value>(&bytes)
            .map_err(|error| format!("failed to parse {}: {error}", path.display()))?,
        None => serde_yaml::Value::Mapping(serde_yaml::Mapping::new()),
    };
    let root_map = yaml_mapping(&mut root, "root")?;
    let providers_key = serde_yaml::Value::String("providers".to_string());
    let providers = root_map
        .entry(providers_key)
        .or_insert_with(|| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));
    let providers_map = yaml_mapping(providers, "providers")?;
    let provider_key = serde_yaml::Value::String(provider_name.to_string());
    let created = !providers_map.contains_key(&provider_key);
    let provider = providers_map
        .entry(provider_key)
        .or_insert_with(|| omp_template_provider(cli_key, port));
    let provider_map = yaml_mapping(provider, "provider")?;
    let overrides_key = serde_yaml::Value::String("modelOverrides".to_string());
    let overrides = provider_map
        .entry(overrides_key)
        .or_insert_with(|| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));
    let overrides_map = yaml_mapping(overrides, "modelOverrides")?;

    let mut expected = overrides_map.clone();
    for model in models {
        let model_key = serde_yaml::Value::String(model.model_id.clone());
        let existing = expected
            .entry(model_key)
            .or_insert_with(|| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));
        let existing_map = yaml_mapping(existing, "model override")?;
        let managed = omp_model_override(model);
        let managed_map = yaml_mapping_ref(&managed, "generated model override")?;
        for (key, value) in managed_map {
            existing_map.insert(key.clone(), value.clone());
        }
    }
    let new_overrides = serde_yaml::Value::Mapping(expected);
    let unchanged = overrides == &new_overrides;
    if !unchanged {
        *overrides = new_overrides;
        let bytes = serde_yaml::to_string(&root)
            .map_err(|error| format!("failed to serialize {}: {error}", path.display()))?;
        write_file_atomic(path, bytes.as_bytes())?;
    }

    Ok(CliModelExportResult {
        cli_key: cli_key.to_string(),
        provider_name: provider_name.to_string(),
        target: "omp".to_string(),
        path: path.display().to_string(),
        models: models.iter().map(|model| model.model_id.clone()).collect(),
        created,
        unchanged,
    })
}

fn export_to_omp(
    app: &tauri::AppHandle,
    db: &db::Db,
    cli_key: &str,
    provider_name: &str,
) -> AppResult<CliModelExportResult> {
    let path = omp_config_path(app)?;
    let models = models_for_provider(db, cli_key, provider_name)?;
    export_omp_to_path(&path, cli_key, provider_name, gateway_port(app), &models)
}

async fn run_export(
    app: tauri::AppHandle,
    db_state: &DbInitState,
    cli_key: String,
    provider_name: String,
    export: impl Fn(&tauri::AppHandle, &db::Db, &str, &str) -> AppResult<CliModelExportResult>
        + Send
        + 'static,
) -> Result<CliModelExportResult, String> {
    crate::shared::cli_key::validate_cli_key(&cli_key).map_err(|e| e.to_string())?;
    let provider_name = provider_name.trim().to_string();
    if provider_name.is_empty() {
        return Err(invalid("provider name must not be empty").to_string());
    }
    let db = ensure_db_ready(app.clone(), db_state).await?;
    let cli_key2 = cli_key.clone();
    blocking::run("cli_model_export", move || {
        export(&app, &db, &cli_key2, &provider_name)
    })
    .await
    .map_err(Into::into)
}

pub(crate) async fn export_to_pi_cmd(
    app: tauri::AppHandle,
    db_state: &DbInitState,
    cli_key: String,
    provider_name: String,
) -> Result<CliModelExportResult, String> {
    run_export(app, db_state, cli_key, provider_name, export_to_pi).await
}

pub(crate) async fn export_to_opencode_cmd(
    app: tauri::AppHandle,
    db_state: &DbInitState,
    cli_key: String,
    provider_name: String,
) -> Result<CliModelExportResult, String> {
    run_export(app, db_state, cli_key, provider_name, export_to_opencode).await
}

pub(crate) async fn export_to_omp_cmd(
    app: tauri::AppHandle,
    db_state: &DbInitState,
    cli_key: String,
    provider_name: String,
) -> Result<CliModelExportResult, String> {
    run_export(app, db_state, cli_key, provider_name, export_to_omp).await
}

// ─── tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opencode_jsonc_export_preserves_unmanaged_settings() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("opencode.json");
        std::fs::write(
            &path,
            r#"{
  // Global preferences must survive synchronization.
  "plugin": ["keep-plugin",],
  "description": "literal // and /* comment */ and ,} and ,] and \"quoted\"",
  "provider": {
    "other": { "options": { "apiKey": "other-key", }, },
    "aio-codex": {
      "npm": "@ai-sdk/openai",
      "options": { "baseURL": "https://existing.example/v1", "apiKey": "keep-key", },
      /* Connection options are user-owned. */
      "custom": true,
      "models": { "legacy": { "tool_call": true, }, },
    },
  },
}"#,
        )
        .unwrap();
        let models = vec![ExportModel {
            model_id: "gpt-5".into(),
            context_window: 750_000,
            reasoning_effort: "high".into(),
        }];

        let first = export_opencode_to_path(&path, "codex", "aio-codex", 15721, &models)
            .expect("OpenCode should accept comments and trailing commas");
        assert!(!first.created);
        assert!(!first.unchanged);
        let written = std::fs::read(&path).unwrap();
        let parsed: Value = serde_json::from_slice(&written).unwrap();
        assert_eq!(parsed["plugin"], json!(["keep-plugin"]));
        assert_eq!(
            parsed["description"],
            "literal // and /* comment */ and ,} and ,] and \"quoted\""
        );
        assert_eq!(
            parsed["provider"]["other"]["options"]["apiKey"],
            "other-key"
        );
        let provider = &parsed["provider"]["aio-codex"];
        assert_eq!(provider["npm"], "@ai-sdk/openai");
        assert_eq!(
            provider["options"]["baseURL"],
            "https://existing.example/v1"
        );
        assert_eq!(provider["options"]["apiKey"], "keep-key");
        assert_eq!(provider["custom"], true);
        assert!(provider["models"].get("legacy").is_none());
        assert_eq!(provider["models"]["gpt-5"]["limit"]["context"], 750_000);
        assert_eq!(provider["models"]["gpt-5"]["name"], "aio-codex/gpt-5");

        let second = export_opencode_to_path(&path, "codex", "aio-codex", 15721, &models).unwrap();
        assert!(second.unchanged);
        assert_eq!(written, std::fs::read(&path).unwrap());

        let jsonc = format!(
            "/* Keep comments when models are unchanged. */\n{},\n}}",
            std::str::from_utf8(&written)
                .unwrap()
                .strip_suffix('}')
                .unwrap()
        );
        std::fs::write(&path, &jsonc).unwrap();
        let third = export_opencode_to_path(&path, "codex", "aio-codex", 15721, &models).unwrap();
        assert!(third.unchanged);
        assert_eq!(jsonc.as_bytes(), std::fs::read(&path).unwrap());
    }

    #[test]
    fn opencode_invalid_config_is_never_overwritten() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("opencode.json");
        let invalid_configs: &[&[u8]] = &[
            br#"{"provider": {"aio-codex": {"models": {}}}"#,
            br#"{"first": 1 "second": 2}"#,
            br#"{} trailing garbage"#,
            br#"{} {}"#,
            br#"{unquoted: true}"#,
            br#"{"single": 'quoted'}"#,
            br#"{"escape": "\x41"}"#,
            br#"{"escape": "\v"}"#,
            br#"{/* unterminated comment }"#,
            b"{\"value\": \"\xff\"}",
        ];
        for source in invalid_configs {
            std::fs::write(&path, source).unwrap();
            let error = export_opencode_to_path(&path, "codex", "aio-codex", 15721, &[])
                .expect_err("invalid configuration must not be replaced");
            assert!(error.to_string().contains("failed to parse"), "{error}");
            assert!(
                error.to_string().contains(&path.display().to_string()),
                "{error}"
            );
            assert_eq!(*source, std::fs::read(&path).unwrap());
        }
    }

    #[test]
    fn omp_missing_file_creates_provider_in_isolated_home() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join(".omp/agent/models.yml");
        let models = vec![ExportModel {
            model_id: "gpt-5".into(),
            context_window: 750_000,
            reasoning_effort: "high".into(),
        }];
        let result = export_omp_to_path(&path, "codex", "aio-codex", 15721, &models)
            .expect("OMP export should create config");
        assert_eq!(result.target, "omp");
        assert!(result.created);
        assert!(!result.unchanged);
        assert_eq!(result.models, vec!["gpt-5"]);
        assert!(path.is_file());
    }

    #[test]
    fn omp_existing_provider_preserves_connection_and_unknown_fields() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join(".omp/agent/models.yml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"providers:
  aio-codex:
    baseUrl: https://existing.example/v1
    apiKey: secret-value
    headers:
      X-Keep: keep
    models:
      legacy:
        name: Legacy
    modelOverrides:
      legacy:
        customField: keep-me
"#,
        )
        .unwrap();
        let models = vec![ExportModel {
            model_id: "gpt-5".into(),
            context_window: 750_000,
            reasoning_effort: "high".into(),
        }];

        let first = export_omp_to_path(&path, "codex", "aio-codex", 15721, &models).unwrap();
        assert!(!first.created);
        assert!(!first.unchanged);
        let written = std::fs::read(&path).unwrap();
        let parsed: serde_yaml::Value = serde_yaml::from_slice(&written).unwrap();
        let provider = &parsed["providers"]["aio-codex"];
        assert_eq!(provider["baseUrl"], "https://existing.example/v1");
        assert_eq!(provider["apiKey"], "secret-value");
        assert_eq!(provider["headers"]["X-Keep"], "keep");
        assert_eq!(provider["models"]["legacy"]["name"], "Legacy");
        assert_eq!(
            provider["modelOverrides"]["legacy"]["customField"],
            "keep-me"
        );
        assert_eq!(
            provider["modelOverrides"]["gpt-5"]["contextWindow"],
            750_000
        );

        let second = export_omp_to_path(&path, "codex", "aio-codex", 15721, &models).unwrap();
        assert!(second.unchanged);
        assert_eq!(written, std::fs::read(&path).unwrap());
    }

    #[test]
    fn template_defaults_cover_all_gateway_clis() {
        for cli in ["codex", "claude", "grok", "gemini"] {
            let name = default_provider_name(cli).expect("template");
            assert_eq!(name, format!("aio-{cli}"));
        }
        assert!(default_provider_name("bogus").is_err());
    }

    #[test]
    fn opencode_model_entry_uses_catalog_context_window() {
        let entry = opencode_model_entry("gpt-5", 750_000, "aio-codex");
        assert_eq!(entry["limit"]["context"], 750_000);
    }

    #[test]
    fn opencode_model_entry_prefixes_provider_name() {
        let entry = opencode_model_entry("cline-free/gpt-5.6", 1_000_000, "aio-codex");
        assert_eq!(entry["name"], "aio-codex/cline-free/gpt-5.6");
    }

    #[test]
    fn opencode_model_entry_never_writes_variants() {
        let entry = opencode_model_entry("gpt-5.6", 1_000_000, "aio-codex");
        let obj = entry.as_object().expect("entry object");
        let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "attachment",
                "limit",
                "name",
                "reasoning",
                "temperature",
                "tool_call"
            ]
        );
    }

    #[test]
    fn pi_model_entry_uses_cli_api_template() {
        let tpl = template_for("claude").unwrap();
        let entry = pi_model_entry("claude-sonnet-5", 250_000, tpl);
        assert_eq!(entry["api"], "anthropic-messages");
        assert_eq!(entry["contextWindow"], 250_000);
        assert_eq!(entry["id"], "claude-sonnet-5");
    }

    #[test]
    fn opencode_anthropic_path_has_v1_suffix() {
        let tpl = template_for("claude").unwrap();
        assert_eq!(tpl.opencode_path, "/claude/v1");
        // Pi anthropic must NOT carry /v1.
        assert_eq!(tpl.pi_path, "/claude");
    }
}
