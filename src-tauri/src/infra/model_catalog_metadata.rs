//! Usage: Persist user-maintained model catalog metadata overrides.

use crate::db;
use crate::providers;
use crate::shared::cli_key::validate_cli_key;
use crate::shared::error::{db_err, AppResult};
use rusqlite::{params, OptionalExtension};
use std::collections::{HashMap, HashSet};

pub(crate) const DEFAULT_CONTEXT_WINDOW: i64 = 1_000_000;
pub(crate) const DEFAULT_REASONING_EFFORT: &str = "high";
pub(crate) const SUPPORTED_REASONING_EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModelCatalogMetadata {
    pub context_window: i64,
    pub reasoning_effort: String,
    pub enabled: bool,
}

pub(crate) fn validate_input(
    cli_key: &str,
    model_id: &str,
    context_window: i64,
    reasoning_effort: &str,
) -> AppResult<(String, String, String)> {
    validate_cli_key(cli_key)?;
    let normalized_model_id = providers::normalize_concrete_model_id(model_id)?;
    if context_window <= 0 {
        return Err("SEC_INVALID_INPUT: context_window must be positive".into());
    }
    let normalized_effort = reasoning_effort.trim().to_ascii_lowercase();
    if !SUPPORTED_REASONING_EFFORTS.contains(&normalized_effort.as_str()) {
        return Err(
            format!("SEC_INVALID_INPUT: unsupported reasoning_effort={normalized_effort}").into(),
        );
    }
    Ok((normalized_model_id, cli_key.to_string(), normalized_effort))
}

fn get(db: &db::Db, cli_key: &str, model_id: &str) -> AppResult<Option<ModelCatalogMetadata>> {
    let conn = db.open_connection()?;
    conn.query_row(
        "SELECT context_window, reasoning_effort, enabled \
         FROM model_catalog_metadata WHERE cli_key = ?1 AND model_id = ?2",
        params![cli_key, model_id],
        |row| {
            Ok(ModelCatalogMetadata {
                context_window: row.get(0)?,
                reasoning_effort: row.get(1)?,
                enabled: row.get::<_, bool>(2)?,
            })
        },
    )
    .optional()
    .map_err(|error| db_err!("failed to load model catalog metadata row: {error}"))
}

pub(crate) fn load_map(
    db: &db::Db,
    cli_key: &str,
) -> AppResult<HashMap<String, ModelCatalogMetadata>> {
    validate_cli_key(cli_key)?;
    let conn = db.open_connection()?;
    let mut statement = conn
        .prepare(
            "SELECT model_id, context_window, reasoning_effort, enabled FROM model_catalog_metadata WHERE cli_key = ?1",
        )
        .map_err(|error| db_err!("failed to prepare model catalog metadata query: {error}"))?;
    let rows = statement
        .query_map(params![cli_key], |row| {
            Ok((
                row.get::<_, String>(0)?,
                ModelCatalogMetadata {
                    context_window: row.get(1)?,
                    reasoning_effort: row.get(2)?,
                    enabled: row.get::<_, bool>(3)?,
                },
            ))
        })
        .map_err(|error| db_err!("failed to query model catalog metadata: {error}"))?;
    let mut result = HashMap::new();
    for row in rows {
        let (model_id, metadata) =
            row.map_err(|error| db_err!("failed to read model catalog metadata: {error}"))?;
        result.insert(model_id, metadata);
    }
    Ok(result)
}

/// Model ids the user explicitly disabled in the catalog. Used to keep
/// disabled models out of the gateway list, routing, and the Codex
/// catalog projection.
pub(crate) fn disabled_model_ids(db: &db::Db, cli_key: &str) -> AppResult<HashSet<String>> {
    validate_cli_key(cli_key)?;
    let conn = db.open_connection()?;
    let mut statement = conn
        .prepare(
            "SELECT model_id FROM model_catalog_metadata \
             WHERE cli_key = ?1 AND enabled = 0",
        )
        .map_err(|error| db_err!("failed to prepare disabled model ids query: {error}"))?;
    let rows = statement
        .query_map(params![cli_key], |row| row.get::<_, String>(0))
        .map_err(|error| db_err!("failed to query disabled model ids: {error}"))?;
    let mut result = HashSet::new();
    for row in rows {
        result.insert(row.map_err(|error| db_err!("failed to read disabled model id: {error}"))?);
    }
    Ok(result)
}

/// Whether the requested public model was explicitly disabled in the catalog.
/// Unrecognizable model ids are treated as enabled so request routing keeps
/// its existing validation semantics.
pub(crate) fn is_disabled(db: &db::Db, cli_key: &str, model_id: &str) -> AppResult<bool> {
    validate_cli_key(cli_key)?;
    let normalized_model_id = match providers::normalize_concrete_model_id(model_id) {
        Ok(value) => value,
        Err(_) => return Ok(false),
    };
    let conn = db.open_connection()?;
    let enabled = conn
        .query_row(
            "SELECT enabled FROM model_catalog_metadata \
             WHERE cli_key = ?1 AND model_id = ?2",
            params![cli_key, normalized_model_id],
            |row| row.get::<_, bool>(0),
        )
        .optional()
        .map_err(|error| db_err!("failed to query model catalog enabled flag: {error}"))?;
    Ok(matches!(enabled, Some(false)))
}

pub(crate) fn upsert(
    db: &db::Db,
    cli_key: &str,
    model_id: &str,
    context_window: i64,
    reasoning_effort: &str,
    updated_at: i64,
) -> AppResult<ModelCatalogMetadata> {
    let (normalized_model_id, normalized_cli_key, normalized_effort) =
        validate_input(cli_key, model_id, context_window, reasoning_effort)?;
    {
        let conn = db.open_connection()?;
        conn.execute(
            r#"
INSERT INTO model_catalog_metadata(
  cli_key, model_id, context_window, reasoning_effort, updated_at
) VALUES (?1, ?2, ?3, ?4, ?5)
ON CONFLICT(cli_key, model_id) DO UPDATE SET
  context_window = excluded.context_window,
  reasoning_effort = excluded.reasoning_effort,
  updated_at = excluded.updated_at
"#,
            params![
                normalized_cli_key,
                normalized_model_id,
                context_window,
                normalized_effort,
                updated_at
            ],
        )
        .map_err(|error| db_err!("failed to save model catalog metadata: {error}"))?;
    }
    get(db, &normalized_cli_key, &normalized_model_id)?
        .ok_or_else(|| "failed to load saved model catalog metadata".into())
}

/// Toggle a catalog model without touching its metadata overrides; missing
/// rows are seeded with the default context window and reasoning effort.
pub(crate) fn set_enabled(
    db: &db::Db,
    cli_key: &str,
    model_id: &str,
    enabled: bool,
    updated_at: i64,
) -> AppResult<ModelCatalogMetadata> {
    validate_cli_key(cli_key)?;
    let normalized_model_id = providers::normalize_concrete_model_id(model_id)?;
    {
        let conn = db.open_connection()?;
        conn.execute(
            r#"
INSERT INTO model_catalog_metadata(
  cli_key, model_id, context_window, reasoning_effort, enabled, updated_at
) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
ON CONFLICT(cli_key, model_id) DO UPDATE SET
  enabled = excluded.enabled,
  updated_at = excluded.updated_at
"#,
            params![
                cli_key,
                normalized_model_id,
                DEFAULT_CONTEXT_WINDOW,
                DEFAULT_REASONING_EFFORT,
                enabled,
                updated_at
            ],
        )
        .map_err(|error| db_err!("failed to save model catalog enabled flag: {error}"))?;
    }
    get(db, cli_key, &normalized_model_id)?
        .ok_or_else(|| "failed to load saved model catalog metadata".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> (tempfile::TempDir, db::Db) {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = crate::db::init_for_tests(&dir.path().join("catalog-meta.db")).expect("init db");
        (dir, db)
    }

    /// Regression for the packaged-build GW_INTERNAL_ERROR: the SQL literal in
    /// `disabled_model_ids` / `is_disabled` previously concatenated the table
    /// name and `WHERE` (`model_catalog_metadataWHERE`), so every prepare
    /// failed with `near "=": syntax error` and routed requests to a 500.
    /// These queries must prepare and run against a real schema.
    #[test]
    fn disabled_model_ids_queries_valid_sql() {
        let (_dir, db) = test_db();
        set_enabled(&db, "codex", "gpt-5", false, 1).expect("seed disabled row");
        set_enabled(&db, "codex", "gpt-4o", true, 1).expect("seed enabled row");

        let ids = disabled_model_ids(&db, "codex").expect("query must not be a syntax error");
        assert!(ids.contains("gpt-5"));
        assert!(!ids.contains("gpt-4o"));

        assert!(is_disabled(&db, "codex", "gpt-5").expect("is_disabled"));
        assert!(!is_disabled(&db, "codex", "gpt-4o").expect("is_disabled enabled"));
        // Missing row => not disabled (fail-open).
        assert!(!is_disabled(&db, "codex", "gpt-never").expect("is_disabled missing"));
    }

    #[test]
    fn is_disabled_is_scoped_per_cli_key() {
        let (_dir, db) = test_db();
        set_enabled(&db, "codex", "gpt-5", false, 1).expect("disable in codex");
        // Same model id, different cli_key stays enabled.
        assert!(!is_disabled(&db, "claude", "gpt-5").expect("claude unaffected"));
        assert!(is_disabled(&db, "codex", "gpt-5").expect("codex disabled"));
    }
}
