//! Usage: SQLite migration v39->v40 - Persist provider model catalogs and
//! OAuth quota policies (OAuth quota columns merged from main).

use rusqlite::Connection;

pub(super) fn migrate_v39_to_v40(conn: &mut Connection) -> crate::shared::error::AppResult<()> {
    let tx = conn
        .transaction()
        .map_err(|e| format!("failed to start v39->v40: {e}"))?;
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
    .map_err(|e| format!("failed to create provider model catalog table: {e}"))?;
    ensure_oauth_quota_policy(&tx)?;
    super::set_user_version(&tx, 40)?;
    tx.commit()
        .map_err(|e| format!("failed to commit v39->v40: {e}"))?;
    Ok(())
}

pub(super) fn ensure_oauth_quota_policy(conn: &Connection) -> crate::shared::error::AppResult<()> {
    for (table, columns) in [
        (
            "providers",
            &[
                (
                    "oauth_min_remaining_percent",
                    "REAL CHECK (oauth_min_remaining_percent BETWEEN 0 AND 100)",
                ),
                ("oauth_use_credits", "INTEGER NOT NULL DEFAULT 0"),
            ][..],
        ),
        (
            "provider_oauth_limit_snapshots",
            &[
                ("limit_5h_remaining_percent", "REAL"),
                ("limit_weekly_remaining_percent", "REAL"),
                ("credits_json", "TEXT"),
                ("usage_limit_reached", "INTEGER NOT NULL DEFAULT 0"),
                ("revision", "INTEGER NOT NULL DEFAULT 0"),
            ][..],
        ),
    ] {
        let has_table: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
                [table],
                |row| row.get(0),
            )
            .map_err(|e| format!("failed to inspect OAuth quota table: {e}"))?;
        if !has_table {
            continue;
        }
        for (column, definition) in columns {
            let has_column: bool = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2)",
                    [table, column],
                    |row| row.get(0),
                )
                .map_err(|e| format!("failed to inspect OAuth quota column: {e}"))?;
            if !has_column {
                conn.execute_batch(&format!(
                    "ALTER TABLE {table} ADD COLUMN {column} {definition};"
                ))
                .map_err(|e| format!("failed to add OAuth quota column: {e}"))?;
            }
        }
    }
    Ok(())
}
