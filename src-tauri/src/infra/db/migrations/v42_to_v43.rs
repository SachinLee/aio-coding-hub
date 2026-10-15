//! Usage: SQLite migration v42->v43 - Persist per-(model, provider) route overrides.

use rusqlite::Connection;

pub(super) fn migrate_v42_to_v43(conn: &mut Connection) -> Result<(), String> {
    let tx = conn
        .transaction()
        .map_err(|error| format!("failed to start v42->v43: {error}"))?;

    tx.execute_batch(
        r#"
CREATE TABLE IF NOT EXISTS model_provider_route_overrides (
  cli_key TEXT NOT NULL,
  model_id TEXT NOT NULL,
  provider_id INTEGER NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY(cli_key, model_id, provider_id)
);
CREATE INDEX IF NOT EXISTS idx_model_provider_route_overrides_cli_model
  ON model_provider_route_overrides(cli_key, model_id);
CREATE TRIGGER IF NOT EXISTS delete_provider_model_route_overrides
AFTER DELETE ON providers
BEGIN
  DELETE FROM model_provider_route_overrides WHERE provider_id = OLD.id;
END;
"#,
    )
    .map_err(|error| format!("failed to create model_provider_route_overrides table: {error}"))?;

    super::set_user_version(&tx, 43)?;
    tx.commit()
        .map_err(|error| format!("failed to commit v42->v43: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_preserves_route_overrides_and_cleans_only_deleted_provider() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE providers(id INTEGER PRIMARY KEY); INSERT INTO providers VALUES (1), (2);").unwrap();
        migrate_v42_to_v43(&mut conn).unwrap();
        conn.execute_batch(
            "INSERT INTO model_provider_route_overrides VALUES
             ('codex', 'model-a', 1, 0, 1), ('codex', 'model-b', 1, 0, 1),
             ('codex', 'model-a', 2, 0, 1), ('claude', 'model-a', 2, 0, 1);",
        )
        .unwrap();
        migrate_v42_to_v43(&mut conn).unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM model_provider_route_overrides",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            4
        );
        conn.execute("DELETE FROM providers WHERE id = 1", [])
            .unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM model_provider_route_overrides WHERE provider_id = 1",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM model_provider_route_overrides WHERE provider_id = 2",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            2
        );
    }
}
