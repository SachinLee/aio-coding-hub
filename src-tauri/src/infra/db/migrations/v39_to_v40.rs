//! Usage: SQLite migration v39->v40 - Persist provider model catalogs.

use rusqlite::Connection;

pub(super) fn migrate_v39_to_v40(conn: &mut Connection) -> Result<(), String> {
    let tx = conn
        .transaction()
        .map_err(|error| format!("failed to start v39->v40: {error}"))?;
    tx.execute_batch(
        r#"
CREATE TABLE IF NOT EXISTS provider_model_catalogs (
  provider_id INTEGER PRIMARY KEY,
  config_version INTEGER NOT NULL,
  models_json TEXT NOT NULL DEFAULT '[]',
  status TEXT NOT NULL DEFAULT 'failed',
  last_success_at INTEGER,
  last_attempt_at INTEGER NOT NULL,
  last_error TEXT,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY(provider_id) REFERENCES providers(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_provider_model_catalogs_status
  ON provider_model_catalogs(status);
"#,
    )
    .map_err(|error| format!("failed to create provider model catalog table: {error}"))?;
    super::set_user_version(&tx, 40)?;
    tx.commit()
        .map_err(|error| format!("failed to commit v39->v40: {error}"))
}
