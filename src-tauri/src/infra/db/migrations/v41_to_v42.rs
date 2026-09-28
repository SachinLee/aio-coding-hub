//! Usage: SQLite migration v41->v42 - Persist model catalog metadata overrides.

use rusqlite::Connection;

pub(super) fn migrate_v41_to_v42(conn: &mut Connection) -> Result<(), String> {
    let tx = conn
        .transaction()
        .map_err(|error| format!("failed to start v41->v42: {error}"))?;

    tx.execute_batch(
        r#"
CREATE TABLE IF NOT EXISTS model_catalog_metadata (
  cli_key TEXT NOT NULL,
  model_id TEXT NOT NULL,
  context_window INTEGER NOT NULL,
  reasoning_effort TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY(cli_key, model_id)
);
CREATE INDEX IF NOT EXISTS idx_model_catalog_metadata_cli_key
  ON model_catalog_metadata(cli_key);
"#,
    )
    .map_err(|error| format!("failed to create model catalog metadata table: {error}"))?;

    super::set_user_version(&tx, 42)?;
    tx.commit()
        .map_err(|error| format!("failed to commit v41->v42: {error}"))
}
