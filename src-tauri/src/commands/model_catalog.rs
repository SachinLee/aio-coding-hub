//! Usage: Tauri IPC commands for the cross-CLI model catalog.

use crate::app::model_catalog_service::{
    self, ModelCatalogApplyInput, ModelCatalogRefreshPreview, ModelCatalogResult,
    ModelCatalogSetEnabledInput, ModelCatalogUpdateInput,
};
use crate::app_state::DbInitState;

#[tauri::command]
#[specta::specta]
pub(crate) async fn model_catalog_list(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    cli_key: String,
) -> Result<ModelCatalogResult, String> {
    model_catalog_service::model_catalog_list(app, db_state.inner(), cli_key).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn model_catalog_update(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    input: ModelCatalogUpdateInput,
) -> Result<model_catalog_service::ModelCatalogItem, String> {
    model_catalog_service::model_catalog_update(app, db_state.inner(), input).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn model_catalog_set_enabled(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    input: ModelCatalogSetEnabledInput,
) -> Result<model_catalog_service::ModelCatalogItem, String> {
    model_catalog_service::model_catalog_set_enabled(app, db_state.inner(), input).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn model_catalog_refresh_preview(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    cli_key: String,
) -> Result<ModelCatalogRefreshPreview, String> {
    model_catalog_service::model_catalog_refresh_preview(app, db_state.inner(), cli_key).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn model_catalog_refresh_apply(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    input: ModelCatalogApplyInput,
) -> Result<(), String> {
    model_catalog_service::model_catalog_refresh_apply(app, db_state.inner(), input).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn model_catalog_export_default_provider(
    cli_key: String,
) -> Result<String, String> {
    crate::infra::cli_model_export::default_provider_name(&cli_key)
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn model_catalog_export_pi(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    cli_key: String,
    provider_name: String,
) -> Result<crate::infra::cli_model_export::CliModelExportResult, String> {
    crate::infra::cli_model_export::export_to_pi_cmd(app, db_state.inner(), cli_key, provider_name)
        .await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn model_catalog_export_opencode(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    cli_key: String,
    provider_name: String,
) -> Result<crate::infra::cli_model_export::CliModelExportResult, String> {
    crate::infra::cli_model_export::export_to_opencode_cmd(
        app,
        db_state.inner(),
        cli_key,
        provider_name,
    )
    .await
}
