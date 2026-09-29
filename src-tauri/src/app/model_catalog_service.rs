//! Usage: Model catalog query and metadata update application service.

use crate::app_state::{ensure_db_ready, DbInitState};
use crate::infra::{model_catalog_metadata, provider_model_catalog};
use crate::{blocking, db};
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelCatalogItem {
    pub model_id: String,
    pub context_window: i64,
    pub reasoning_effort: String,
    pub metadata_source: String,
    pub enabled: bool,
    pub routes: Vec<provider_model_catalog::ModelCatalogRoute>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelCatalogResult {
    pub cli_key: String,
    pub default_context_window: i64,
    pub default_reasoning_effort: String,
    pub supported_reasoning_efforts: Vec<String>,
    pub items: Vec<ModelCatalogItem>,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelCatalogUpdateInput {
    pub cli_key: String,
    pub model_id: String,
    pub context_window: i64,
    pub reasoning_effort: String,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelCatalogSetEnabledInput {
    pub cli_key: String,
    pub model_id: String,
    pub enabled: bool,
}

/// Per-provider remote-vs-local model diff produced by `model_catalog_refresh_preview`.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelCatalogProviderDiff {
    pub provider_id: i64,
    pub provider_name: String,
    /// Models present in the fresh remote list but not in the stored snapshot.
    pub added: Vec<String>,
    /// Models in the stored snapshot but missing from the fresh remote list.
    pub removed: Vec<String>,
    /// Discovery was unreachable/failed; the provider is excluded from apply.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The remote list staged at preview time, echoed back on apply.
    pub remote_models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelCatalogRefreshPreview {
    pub cli_key: String,
    pub providers: Vec<ModelCatalogProviderDiff>,
}

/// One provider's user-approved apply decision.
#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelCatalogApplyDecision {
    pub provider_id: i64,
    /// Remote model list staged at preview time.
    pub remote_models: Vec<String>,
    pub apply_added: bool,
    pub apply_removed: bool,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelCatalogApplyInput {
    pub cli_key: String,
    pub decisions: Vec<ModelCatalogApplyDecision>,
}

pub(crate) async fn model_catalog_list(
    app: tauri::AppHandle,
    db_state: &DbInitState,
    cli_key: String,
) -> Result<ModelCatalogResult, String> {
    let db = ensure_db_ready(app, db_state).await?;
    blocking::run("model_catalog_list", move || list_blocking(&db, &cli_key))
        .await
        .map_err(Into::into)
}

pub(crate) async fn model_catalog_update(
    app: tauri::AppHandle,
    db_state: &DbInitState,
    input: ModelCatalogUpdateInput,
) -> Result<ModelCatalogItem, String> {
    let db = ensure_db_ready(app, db_state).await?;
    blocking::run("model_catalog_update", move || update_blocking(&db, input))
        .await
        .map_err(Into::into)
}

pub(crate) async fn model_catalog_set_enabled(
    app: tauri::AppHandle,
    db_state: &DbInitState,
    input: ModelCatalogSetEnabledInput,
) -> Result<ModelCatalogItem, String> {
    let db = ensure_db_ready(app, db_state).await?;
    blocking::run("model_catalog_set_enabled", move || {
        set_enabled_blocking(&db, input)
    })
    .await
    .map_err(Into::into)
}

pub(crate) fn list_blocking(
    db: &db::Db,
    cli_key: &str,
) -> crate::shared::error::AppResult<ModelCatalogResult> {
    let items = provider_model_catalog::list_model_catalog(db, cli_key)?;
    let metadata = model_catalog_metadata::load_map(db, cli_key)?;
    let items = items
        .into_iter()
        .map(|item| {
            let (context_window, reasoning_effort, metadata_source, enabled) =
                match metadata.get(&item.model_id) {
                    Some(value) => (
                        value.context_window,
                        value.reasoning_effort.clone(),
                        "user".to_string(),
                        value.enabled,
                    ),
                    None => (
                        model_catalog_metadata::DEFAULT_CONTEXT_WINDOW,
                        model_catalog_metadata::DEFAULT_REASONING_EFFORT.to_string(),
                        "default".to_string(),
                        true,
                    ),
                };
            ModelCatalogItem {
                model_id: item.model_id,
                context_window,
                reasoning_effort,
                metadata_source,
                enabled,
                routes: item.routes,
            }
        })
        .collect();
    Ok(ModelCatalogResult {
        cli_key: cli_key.to_string(),
        default_context_window: model_catalog_metadata::DEFAULT_CONTEXT_WINDOW,
        default_reasoning_effort: model_catalog_metadata::DEFAULT_REASONING_EFFORT.to_string(),
        supported_reasoning_efforts: model_catalog_metadata::SUPPORTED_REASONING_EFFORTS
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        items,
    })
}

fn catalog_item_with_metadata(
    item: provider_model_catalog::ModelCatalogItem,
    metadata: &model_catalog_metadata::ModelCatalogMetadata,
) -> ModelCatalogItem {
    ModelCatalogItem {
        model_id: item.model_id,
        context_window: metadata.context_window,
        reasoning_effort: metadata.reasoning_effort.clone(),
        metadata_source: "user".to_string(),
        enabled: metadata.enabled,
        routes: item.routes,
    }
}

fn find_catalog_item(
    db: &db::Db,
    cli_key: &str,
    model_id: &str,
) -> crate::shared::error::AppResult<provider_model_catalog::ModelCatalogItem> {
    let catalog = provider_model_catalog::list_model_catalog(db, cli_key)?;
    catalog
        .into_iter()
        .find(|item| item.model_id == model_id.trim())
        .ok_or_else(|| "DB_NOT_FOUND: model is not in the current catalog".to_string())
        .map_err(Into::into)
}

fn update_blocking(
    db: &db::Db,
    input: ModelCatalogUpdateInput,
) -> crate::shared::error::AppResult<ModelCatalogItem> {
    let item = find_catalog_item(db, &input.cli_key, &input.model_id)?;
    let metadata = model_catalog_metadata::upsert(
        db,
        &input.cli_key,
        &item.model_id,
        input.context_window,
        &input.reasoning_effort,
        crate::shared::time::now_unix_seconds(),
    )?;
    Ok(catalog_item_with_metadata(item, &metadata))
}

fn set_enabled_blocking(
    db: &db::Db,
    input: ModelCatalogSetEnabledInput,
) -> crate::shared::error::AppResult<ModelCatalogItem> {
    let item = find_catalog_item(db, &input.cli_key, &input.model_id)?;
    let metadata = model_catalog_metadata::set_enabled(
        db,
        &input.cli_key,
        &item.model_id,
        input.enabled,
        crate::shared::time::now_unix_seconds(),
    )?;
    Ok(catalog_item_with_metadata(item, &metadata))
}

fn other_status(
    result: &crate::app::provider_model_discovery::ProviderModelDiscoveryResult,
) -> &'static str {
    use crate::app::provider_model_discovery::ProviderModelDiscoveryResult as R;
    match result {
        R::Ready { .. } => "ready",
        R::Empty { .. } => "empty",
        R::Unsupported { .. } => "unsupported",
        R::Error { .. } => "error",
    }
}

fn diff_models(
    current: &[String],
    remote: &[String],
    provider_id: i64,
    provider_name: String,
) -> ModelCatalogProviderDiff {
    use std::collections::BTreeSet;
    let current_set: BTreeSet<&String> = current.iter().collect();
    let remote_set: BTreeSet<&String> = remote.iter().collect();
    ModelCatalogProviderDiff {
        provider_id,
        provider_name,
        added: remote_set
            .difference(&current_set)
            .map(|m| (*m).clone())
            .collect(),
        removed: current_set
            .difference(&remote_set)
            .map(|m| (*m).clone())
            .collect(),
        error: None,
        remote_models: remote.to_vec(),
    }
}

pub(crate) async fn model_catalog_refresh_preview(
    app: tauri::AppHandle,
    db_state: &DbInitState,
    cli_key: String,
) -> Result<ModelCatalogRefreshPreview, String> {
    let db = ensure_db_ready(app.clone(), db_state).await?;
    let providers = {
        let db = db.clone();
        let cli_key = cli_key.clone();
        blocking::run("model_catalog_refresh_providers", move || {
            crate::providers::list_enabled_for_gateway_using_active_mode(&db, &cli_key)
                .map(|selection| selection.providers)
        })
        .await
        .map_err(|error: crate::shared::error::AppError| error.to_string())?
    };

    let mut diffs = Vec::with_capacity(providers.len());
    for provider in providers {
        let auth_mode = if provider.auth_mode == crate::providers::ProviderAuthMode::Oauth.as_str()
        {
            crate::providers::ProviderAuthMode::Oauth
        } else {
            crate::providers::ProviderAuthMode::ApiKey
        };
        let api_key = if auth_mode == crate::providers::ProviderAuthMode::ApiKey {
            Some(provider.api_key_plaintext.clone())
        } else {
            None
        };
        let input = crate::app::provider_model_discovery::ProviderModelDiscoveryInput {
            custom_headers: None,
            provider_id: Some(provider.id),
            cli_key: cli_key.clone(),
            auth_mode,
            base_urls: provider.base_urls.clone(),
            base_url_mode: provider.base_url_mode,
            api_key,
            source_provider_id: provider.source_provider_id,
            bridge_type: provider.bridge_type.clone(),
        };
        let result = crate::app::provider_model_discovery::provider_models_discover(
            app.clone(),
            db_state,
            input,
        )
        .await;
        let current: Vec<String> = {
            let db = db.clone();
            let provider_id = provider.id;
            blocking::run("model_catalog_current_models", move || {
                provider_model_catalog::load_for_provider(&db, provider_id)
                    .map(|snapshot| snapshot.map(|s| s.models).unwrap_or_default())
            })
            .await
            .map_err(|error: crate::shared::error::AppError| error.to_string())?
        };
        let diff = match result {
            Ok(crate::app::provider_model_discovery::ProviderModelDiscoveryResult::Ready {
                models,
                ..
            }) => diff_models(&current, &models, provider.id, provider.name.clone()),
            Ok(other) => ModelCatalogProviderDiff {
                provider_id: provider.id,
                provider_name: provider.name.clone(),
                added: vec![],
                removed: vec![],
                error: Some(format!("discovery {}", other_status(&other))),
                remote_models: vec![],
            },
            Err(error) => ModelCatalogProviderDiff {
                provider_id: provider.id,
                provider_name: provider.name.clone(),
                added: vec![],
                removed: vec![],
                error: Some(error.to_string()),
                remote_models: vec![],
            },
        };
        diffs.push(diff);
    }
    Ok(ModelCatalogRefreshPreview {
        cli_key,
        providers: diffs,
    })
}

pub(crate) async fn model_catalog_refresh_apply(
    app: tauri::AppHandle,
    db_state: &DbInitState,
    input: ModelCatalogApplyInput,
) -> Result<(), String> {
    let db = ensure_db_ready(app, db_state).await?;
    blocking::run("model_catalog_refresh_apply", move || {
        apply_blocking(&db, &input)
    })
    .await
    .map_err(Into::into)
}

fn apply_blocking(
    db: &db::Db,
    input: &ModelCatalogApplyInput,
) -> crate::shared::error::AppResult<()> {
    use std::collections::BTreeSet;
    let now = crate::shared::time::now_unix_seconds();
    for decision in &input.decisions {
        let Some(config_version) =
            provider_model_catalog::provider_config_version(db, decision.provider_id)?
        else {
            continue;
        };
        let current: Vec<String> =
            provider_model_catalog::load_for_provider(db, decision.provider_id)?
                .map(|s| s.models)
                .unwrap_or_default();
        let remote_set: BTreeSet<&String> = decision.remote_models.iter().collect();
        let merged: BTreeSet<String> = current
            .iter()
            // Keep current models unless the user approved removals and the model
            // is absent from the staged remote list.
            .filter(|m| !decision.apply_removed || remote_set.contains(*m))
            .cloned()
            // Add remote models only when the user approved additions.
            .chain(
                decision
                    .remote_models
                    .iter()
                    .filter(|_| decision.apply_added)
                    .cloned(),
            )
            .collect();
        let models: Vec<String> = merged.into_iter().collect();
        provider_model_catalog::save_success(
            db,
            decision.provider_id,
            config_version,
            &models,
            now,
        )?;
    }
    Ok(())
}
