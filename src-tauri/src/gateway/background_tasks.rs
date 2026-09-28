//! Usage: Gateway background writers and refresh-loop ownership.

use crate::{
    app_state::DbInitState, circuit_breaker, db, infra::provider_model_catalog,
    provider_circuit_breakers, providers, request_logs, shared::time::now_unix_seconds,
};
use std::time::Duration;
use tokio::sync::{mpsc, watch};

pub(super) type GatewayBackgroundTaskHandles = (
    tauri::async_runtime::JoinHandle<()>,
    tauri::async_runtime::JoinHandle<()>,
    watch::Sender<bool>,
    tauri::async_runtime::JoinHandle<()>,
    tauri::async_runtime::JoinHandle<()>,
);

pub(super) struct GatewayBackgroundTasks {
    log_tx: mpsc::Sender<request_logs::RequestLogInsert>,
    circuit_persist_tx: mpsc::Sender<circuit_breaker::CircuitPersistedState>,
    log_task: tauri::async_runtime::JoinHandle<()>,
    circuit_task: tauri::async_runtime::JoinHandle<()>,
    oauth_refresh_shutdown: watch::Sender<bool>,
    oauth_refresh_task: tauri::async_runtime::JoinHandle<()>,
    model_catalog_task: tauri::async_runtime::JoinHandle<()>,
}

impl GatewayBackgroundTasks {
    pub(super) fn start<R: tauri::Runtime>(app: tauri::AppHandle<R>, db: db::Db) -> Self {
        let (log_tx, log_task) = request_logs::start_buffered_writer(app.clone(), db.clone());
        let (circuit_persist_tx, circuit_task) =
            provider_circuit_breakers::start_buffered_writer(db.clone());
        let (oauth_refresh_shutdown, oauth_refresh_rx) = watch::channel(false);
        let oauth_refresh_task =
            super::oauth::refresh_loop::spawn(app.clone(), db.clone(), oauth_refresh_rx);
        let model_catalog_task = spawn_model_catalog_refresh(app, db);

        Self {
            log_tx,
            circuit_persist_tx,
            log_task,
            circuit_task,
            oauth_refresh_shutdown,
            oauth_refresh_task,
            model_catalog_task,
        }
    }

    pub(super) fn log_tx(&self) -> mpsc::Sender<request_logs::RequestLogInsert> {
        self.log_tx.clone()
    }

    pub(super) fn circuit_persist_tx(
        &self,
    ) -> mpsc::Sender<circuit_breaker::CircuitPersistedState> {
        self.circuit_persist_tx.clone()
    }

    pub(super) fn into_handles(self) -> GatewayBackgroundTaskHandles {
        let _ = self.oauth_refresh_shutdown.send(true);
        (
            self.log_task,
            self.circuit_task,
            self.oauth_refresh_shutdown,
            self.oauth_refresh_task,
            self.model_catalog_task,
        )
    }

    #[cfg(test)]
    pub(super) fn for_tests(rt: &tokio::runtime::Runtime) -> Self {
        let (log_tx, _log_rx) = mpsc::channel(1);
        let (circuit_persist_tx, _circuit_rx) = mpsc::channel(1);
        let (oauth_refresh_shutdown, _oauth_refresh_rx) = watch::channel(false);

        Self {
            log_tx,
            circuit_persist_tx,
            log_task: tauri::async_runtime::JoinHandle::Tokio(rt.spawn(async {})),
            circuit_task: tauri::async_runtime::JoinHandle::Tokio(rt.spawn(async {})),
            oauth_refresh_shutdown,
            oauth_refresh_task: tauri::async_runtime::JoinHandle::Tokio(rt.spawn(async {})),
            model_catalog_task: tauri::async_runtime::JoinHandle::Tokio(rt.spawn(async {})),
        }
    }
}

const MODEL_CATALOG_REFRESH_INTERVAL: Duration = Duration::from_secs(15 * 60);

fn spawn_model_catalog_refresh<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    db: db::Db,
) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        refresh_model_catalogs(app.clone(), db.clone()).await;
        let mut interval = tokio::time::interval(MODEL_CATALOG_REFRESH_INTERVAL);
        interval.tick().await;
        loop {
            interval.tick().await;
            refresh_model_catalogs(app.clone(), db.clone()).await;
        }
    })
}

async fn refresh_model_catalogs<R: tauri::Runtime>(app: tauri::AppHandle<R>, db: db::Db) {
    let db_state = DbInitState(tokio::sync::Mutex::new(Some(Ok(db.clone()))));
    for cli_key in
        crate::shared::cli_key::cli_keys_with(crate::shared::cli_key::CliCapability::Gateway)
    {
        let selection = match providers::list_enabled_for_gateway_using_active_mode(&db, cli_key) {
            Ok(selection) => selection,
            Err(error) => {
                tracing::warn!(cli_key, error = %error, "model catalog refresh could not load active providers");
                continue;
            }
        };
        for provider in selection.providers {
            let Some(config_version) =
                provider_model_catalog::provider_config_version(&db, provider.id)
                    .ok()
                    .flatten()
            else {
                continue;
            };
            let auth_mode = if provider.auth_mode == providers::ProviderAuthMode::Oauth.as_str() {
                providers::ProviderAuthMode::Oauth
            } else {
                providers::ProviderAuthMode::ApiKey
            };
            let api_key = if auth_mode == providers::ProviderAuthMode::ApiKey {
                Some(provider.api_key_plaintext.clone())
            } else {
                None
            };
            let input = crate::app::provider_model_discovery::ProviderModelDiscoveryInput {
                provider_id: Some(provider.id),
                cli_key: cli_key.to_string(),
                auth_mode,
                base_urls: provider.base_urls.clone(),
                base_url_mode: provider.base_url_mode,
                api_key,
                source_provider_id: provider.source_provider_id,
                bridge_type: provider.bridge_type.clone(),
            };
            let result = crate::app::provider_model_discovery::provider_models_discover(
                app.clone(),
                &db_state,
                input,
            )
            .await;
            let now = now_unix_seconds() as i64;
            match result {
                Ok(crate::app::provider_model_discovery::ProviderModelDiscoveryResult::Ready {
                    models,
                    ..
                }) => {
                    if let Err(error) = provider_model_catalog::save_success(
                        &db,
                        provider.id,
                        config_version,
                        &models,
                        now,
                    ) {
                        tracing::warn!(cli_key, provider_id = provider.id, error = %error, "failed to save refreshed model catalog");
                    }
                }
                Ok(other) => {
                    if let Err(error) = provider_model_catalog::save_failure(
                        &db,
                        provider.id,
                        config_version,
                        &format!("{other:?}"),
                        now,
                    ) {
                        tracing::warn!(cli_key, provider_id = provider.id, error = %error, "failed to save model catalog refresh failure");
                    }
                }
                Err(error) => {
                    if let Err(save_error) = provider_model_catalog::save_failure(
                        &db,
                        provider.id,
                        config_version,
                        &error,
                        now,
                    ) {
                        tracing::warn!(cli_key, provider_id = provider.id, error = %save_error, "failed to save model catalog refresh failure");
                    }
                }
            }
        }
    }
}
