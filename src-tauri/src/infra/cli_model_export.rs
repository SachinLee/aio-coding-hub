//! Usage: Export gateway model catalog into Pi (`~/.pi/agent/models.json`) and
//! OpenCode (`~/.config/opencode/opencode.json`) provider model blocks.
//!
//! Each `cli_key` maps to one `aio-<cli>` provider inside those configs. The
//! exporter refreshes only that provider's model list, preserving unrelated
//! providers and the target provider's connection settings when it already
//! exists. A missing provider block is created from a per-CLI template
//! (api/npm + baseURL pointed at the local gateway).

use std::path::PathBuf;

use serde_json::{json, Map, Value};

use crate::app_state::{ensure_db_ready, DbInitState};
use crate::infra::{app_paths, settings};
use crate::shared::error::{AppError, AppResult};
use crate::shared::fs::{read_optional_file_with_max_len, write_file_atomic};
use crate::{blocking, db};

const PI_MODELS_MAX_BYTES: usize = 4 * 1024 * 1024;
const OPENCODE_MODELS_MAX_BYTES: usize = 4 * 1024 * 1024;

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

fn opencode_model_entry(model_id: &str, context_window: i64) -> Value {
    json!({
        "name": model_id,
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

fn export_to_opencode(
    app: &tauri::AppHandle,
    db: &db::Db,
    cli_key: &str,
    provider_name: &str,
) -> AppResult<CliModelExportResult> {
    let tpl = template_for(cli_key)?;
    let path = opencode_config_path(app)?;
    let port = gateway_port(app);
    let base_url = format!("{}{}", gateway_base(port), tpl.opencode_path);
    let models = models_for_provider(db, cli_key, provider_name)?;

    let raw = read_optional_file_with_max_len(&path, OPENCODE_MODELS_MAX_BYTES)?;
    let mut root: Value = match raw {
        Some(bytes) => serde_json::from_slice(&bytes)
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
    for model in &models {
        models_obj.insert(
            model.model_id.clone(),
            opencode_model_entry(&model.model_id, model.context_window),
        );
    }
    let new_models = Value::Object(models_obj);
    let unchanged = entry_obj.get("models") == Some(&new_models);
    entry_obj.insert("models".to_string(), new_models);

    if !unchanged {
        let bytes = serde_json::to_vec_pretty(&root)
            .map_err(|e| format!("failed to serialize {}: {e}", path.display()))?;
        write_file_atomic(&path, &bytes)?;
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

// ─── IPC entry points ───────────────────────────────────────────────────────

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

// ─── tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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
        let entry = opencode_model_entry("gpt-5", 750_000);
        assert_eq!(entry["limit"]["context"], 750_000);
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
