use crate::app_state::{ensure_db_ready, DbInitState};

pub(crate) use crate::gateway::oauth::limits::ProviderOAuthLimitsResult;

#[tauri::command]
#[specta::specta]
pub(crate) async fn provider_oauth_fetch_limits(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    provider_id: i64,
) -> Result<ProviderOAuthLimitsResult, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    crate::gateway::oauth::limits::fetch_and_save(&app, &db, provider_id).await
}
