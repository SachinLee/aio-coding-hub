//! Usage: Persist per-(model, provider) route disable overrides.
//!
//! A route override lets the user disable one provider for one model while
//! keeping every other model of the same provider routable — unlike the
//! model-level `model_catalog_metadata.enabled` flag, which removes the model
//! everywhere, and unlike the provider-level enable flag, which removes the
//! provider entirely.

use crate::db;
use crate::providers;
use crate::shared::cli_key::validate_cli_key;
use crate::shared::error::{db_err, AppResult};
use rusqlite::{params, OptionalExtension};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModelProviderRouteOverride {
    pub enabled: bool,
}

pub(crate) fn validate_input(cli_key: &str, model_id: &str) -> AppResult<(String, String)> {
    validate_cli_key(cli_key)?;
    let normalized_model_id = providers::normalize_concrete_model_id(model_id)?;
    Ok((normalized_model_id, cli_key.to_string()))
}

/// Whether the (cli_key, model_id, provider_id) route is explicitly disabled.
/// Missing rows are treated as enabled; unrecognizable model ids are treated
/// as enabled so request routing keeps its existing validation semantics.
pub(crate) fn is_disabled(
    db: &db::Db,
    cli_key: &str,
    model_id: &str,
    provider_id: i64,
) -> AppResult<bool> {
    let normalized_model_id = match providers::normalize_concrete_model_id(model_id) {
        Ok(value) => value,
        Err(_) => return Ok(false),
    };
    validate_cli_key(cli_key)?;
    let conn = db.open_connection()?;
    let enabled = conn
        .query_row(
            "SELECT enabled FROM model_provider_route_overrides \
             WHERE cli_key = ?1 AND model_id = ?2 AND provider_id = ?3",
            params![cli_key, normalized_model_id, provider_id],
            |row| row.get::<_, bool>(0),
        )
        .optional()
        .map_err(|error| db_err!("failed to query model provider route override: {error}"))?;
    Ok(matches!(enabled, Some(false)))
}

/// Provider ids explicitly disabled for the (cli_key, model_id) pair. Used to
/// keep disabled routes out of the gateway candidate list and to keep the
/// model catalog projection in sync.
pub(crate) fn disabled_provider_ids(
    db: &db::Db,
    cli_key: &str,
    model_id: &str,
) -> AppResult<Vec<i64>> {
    let normalized_model_id = match providers::normalize_concrete_model_id(model_id) {
        Ok(value) => value,
        Err(_) => return Ok(Vec::new()),
    };
    validate_cli_key(cli_key)?;
    let conn = db.open_connection()?;
    let mut statement = conn
        .prepare(
            "SELECT provider_id FROM model_provider_route_overrides \
             WHERE cli_key = ?1 AND model_id = ?2 AND enabled = 0",
        )
        .map_err(|error| db_err!("failed to prepare disabled provider ids query: {error}"))?;
    let rows = statement
        .query_map(params![cli_key, normalized_model_id], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|error| db_err!("failed to query disabled provider ids: {error}"))?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(|error| db_err!("failed to read disabled provider id: {error}"))?);
    }
    Ok(result)
}

/// Toggle one (model, provider) route without touching sibling models of the
/// same provider or sibling providers of the same model.
pub(crate) fn set_enabled(
    db: &db::Db,
    cli_key: &str,
    model_id: &str,
    provider_id: i64,
    enabled: bool,
    updated_at: i64,
) -> AppResult<ModelProviderRouteOverride> {
    let (normalized_model_id, normalized_cli_key) = validate_input(cli_key, model_id)?;
    let conn = db.open_connection()?;
    // Check ownership in the write itself so a concurrent provider deletion
    // cannot recreate an orphan override after the delete trigger ran.
    let changed = conn
        .execute(
            r#"
INSERT INTO model_provider_route_overrides(
  cli_key, model_id, provider_id, enabled, updated_at
)
SELECT ?1, ?2, ?3, ?4, ?5 FROM providers WHERE id = ?3 AND cli_key = ?1
ON CONFLICT(cli_key, model_id, provider_id) DO UPDATE SET
  enabled = excluded.enabled,
  updated_at = excluded.updated_at
"#,
            params![
                normalized_cli_key,
                normalized_model_id,
                provider_id,
                enabled,
                updated_at
            ],
        )
        .map_err(|error| db_err!("failed to save model provider route override: {error}"))?;
    if changed == 0 {
        return Err("DB_NOT_FOUND: provider not found for cli_key".into());
    }
    Ok(ModelProviderRouteOverride { enabled })
}
