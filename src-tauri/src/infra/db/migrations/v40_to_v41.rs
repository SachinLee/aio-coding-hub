//! Usage: SQLite migration v40->v41 - Persist passive request client identity.

use rusqlite::Connection;

pub(super) fn migrate_v40_to_v41(conn: &mut Connection) -> Result<(), String> {
    let tx = conn
        .transaction()
        .map_err(|error| format!("failed to start v40->v41: {error}"))?;

    if !super::ensure::column_exists(&tx, "request_logs", "client_identity")? {
        tx.execute_batch(
            "ALTER TABLE request_logs ADD COLUMN client_identity TEXT NOT NULL DEFAULT 'unknown';",
        )
        .map_err(|error| format!("failed to add request_logs.client_identity: {error}"))?;
    }

    super::set_user_version(&tx, 41)?;
    tx.commit()
        .map_err(|error| format!("failed to commit v40->v41: {error}"))
}
