//! Usage: Runtime cache and gateway gating for OAuth provider quota snapshots.

use crate::db;
use crate::shared::error::{db_err, AppError, AppResult};
use crate::shared::time::now_unix_seconds;
use rusqlite::{params, Connection, OptionalExtension};

const TEXT_MAX_CHARS: usize = 96;
const SHORT_LABEL_MAX_CHARS: usize = 32;
const FALLBACK_COOLDOWN_SECS: i64 = 5 * 60;
pub(crate) const SNAPSHOT_FRESHNESS_SECS: i64 = 180;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub(crate) struct ProviderOAuthCreditBalance {
    pub has_credits: bool,
    pub unlimited: bool,
    pub balance: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct OAuthQuotaPolicy {
    pub oauth_min_remaining_percent: Option<f64>,
    pub oauth_use_credits: bool,
}

pub(crate) fn read_policy(conn: &Connection, provider_id: i64) -> AppResult<OAuthQuotaPolicy> {
    validate_provider_id(provider_id)?;
    conn.query_row(
        "SELECT oauth_min_remaining_percent, oauth_use_credits FROM providers WHERE id = ?1",
        params![provider_id],
        |row| {
            Ok(OAuthQuotaPolicy {
                oauth_min_remaining_percent: row.get(0)?,
                oauth_use_credits: row.get(1)?,
            })
        },
    )
    .optional()
    .map(|policy| policy.unwrap_or_default())
    .map_err(|e| db_err!("failed to read OAuth quota policy: {e}"))
}

#[derive(Debug, Clone)]
pub(crate) struct OAuthLimitSnapshotInput<'a> {
    pub provider_id: i64,
    pub limit_short_label: Option<&'a str>,
    pub limit_5h_text: Option<&'a str>,
    pub limit_weekly_text: Option<&'a str>,
    pub limit_5h_reset_at: Option<i64>,
    pub limit_weekly_reset_at: Option<i64>,
    pub reset_credit_available_count: Option<i64>,
    pub limit_5h_remaining_percent: Option<f64>,
    pub limit_weekly_remaining_percent: Option<f64>,
    pub credits: Option<&'a ProviderOAuthCreditBalance>,
    pub usage_limit_reached: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OAuthLimitGate {
    Allow,
    Limited { reset_at: Option<i64> },
}

#[derive(Debug, Clone)]
pub(crate) struct OAuthLimitSnapshot {
    pub limit_5h_text: Option<String>,
    pub limit_weekly_text: Option<String>,
    pub limit_5h_reset_at: Option<i64>,
    pub limit_weekly_reset_at: Option<i64>,
    pub reset_credit_available_count: Option<i64>,
    pub limit_5h_remaining_percent: Option<f64>,
    pub limit_weekly_remaining_percent: Option<f64>,
    pub credits: Option<ProviderOAuthCreditBalance>,
    pub usage_limit_reached: bool,
    pub checked_at: i64,
    pub revision: i64,
}

impl OAuthLimitSnapshot {
    pub(crate) fn is_fresh(&self, now_unix: i64) -> bool {
        (self.checked_at..self.checked_at.saturating_add(SNAPSHOT_FRESHNESS_SECS))
            .contains(&now_unix)
            && ![self.limit_5h_reset_at, self.limit_weekly_reset_at]
                .into_iter()
                .flatten()
                .any(|reset_at| self.checked_at < reset_at && reset_at <= now_unix)
    }
}

fn validate_provider_id(provider_id: i64) -> AppResult<i64> {
    if provider_id <= 0 {
        return Err(AppError::from(format!(
            "SEC_INVALID_INPUT: invalid provider_id={provider_id}"
        )));
    }
    Ok(provider_id)
}

fn take_first_chars(value: &str, max_chars: usize) -> String {
    if value.chars().nth(max_chars).is_none() {
        return value.to_string();
    }
    value.chars().take(max_chars).collect()
}

fn normalize_text(input: Option<&str>, max_chars: usize) -> Option<String> {
    let value = input.map(str::trim).filter(|value| !value.is_empty())?;
    Some(take_first_chars(value, max_chars))
}

fn normalize_reset_at(input: Option<i64>) -> Option<i64> {
    input.filter(|value| *value > 0)
}

fn normalize_reset_credit_available_count(input: Option<i64>) -> Option<i64> {
    input.filter(|value| *value >= 0)
}

fn update_latest(latest: &mut Option<i64>, candidate: i64) {
    if candidate <= 0 {
        return;
    }
    match latest {
        Some(existing) if *existing >= candidate => {}
        _ => *latest = Some(candidate),
    }
}

fn parse_leading_number(text: &str) -> Option<(f64, &str)> {
    let mut end = 0usize;
    let mut seen_digit = false;
    let mut seen_dot = false;

    for (idx, ch) in text.char_indices() {
        if ch.is_ascii_digit() {
            seen_digit = true;
            end = idx + ch.len_utf8();
            continue;
        }
        if ch == '.' && !seen_dot {
            seen_dot = true;
            end = idx + ch.len_utf8();
            continue;
        }
        break;
    }

    if !seen_digit || end == 0 {
        return None;
    }

    let number = text[..end].parse::<f64>().ok()?;
    Some((number, &text[end..]))
}

fn is_exhausted_quota_text(input: Option<&str>) -> bool {
    let Some(text) = input.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    let normalized = text.replace(',', "");
    let Some((value, rest)) = parse_leading_number(&normalized) else {
        return false;
    };

    if value.abs() > f64::EPSILON {
        return false;
    }

    let rest = rest.trim_start();
    let starts_with_unit = rest
        .chars()
        .next()
        .is_some_and(|ch| ch == '%' || ch == '/' || ch.is_alphabetic());
    rest.is_empty() || starts_with_unit
}

fn active_limited_window_reset_at(
    limited: bool,
    reset_at: Option<i64>,
    checked_at: i64,
    now_unix: i64,
) -> Option<i64> {
    if !limited {
        return None;
    }
    if let Some(reset_at) = reset_at {
        return (reset_at > now_unix).then_some(reset_at);
    }
    let fallback_until = checked_at.saturating_add(FALLBACK_COOLDOWN_SECS);
    (fallback_until > now_unix).then_some(fallback_until)
}

#[cfg(test)]
pub(crate) fn save_snapshot(db: &db::Db, input: OAuthLimitSnapshotInput<'_>) -> AppResult<()> {
    save_snapshot_conditionally(db, input, None, None).map(|_| ())
}

pub(crate) fn save_snapshot_if_access_token_matches(
    db: &db::Db,
    input: OAuthLimitSnapshotInput<'_>,
    expected_access_token: &str,
    expected_snapshot_revision: i64,
) -> AppResult<bool> {
    save_snapshot_conditionally(
        db,
        input,
        Some(expected_access_token),
        Some(expected_snapshot_revision),
    )
}

fn save_snapshot_conditionally(
    db: &db::Db,
    input: OAuthLimitSnapshotInput<'_>,
    expected_access_token: Option<&str>,
    expected_snapshot_revision: Option<i64>,
) -> AppResult<bool> {
    let provider_id = validate_provider_id(input.provider_id)?;
    let conn = db.open_connection()?;
    let now = now_unix_seconds();
    let credits_json = input
        .credits
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| AppError::from(format!("failed to serialize OAuth credits: {e}")))?;
    let valid_percent = |value: Option<f64>| {
        value.filter(|value| value.is_finite() && (0.0..=100.0).contains(value))
    };

    let changed = conn
        .execute(
            r#"
INSERT INTO provider_oauth_limit_snapshots(
  provider_id, limit_short_label, limit_5h_text, limit_weekly_text,
  limit_5h_reset_at, limit_weekly_reset_at, reset_credit_available_count,
  limit_5h_remaining_percent, limit_weekly_remaining_percent, credits_json,
  usage_limit_reached, checked_at, updated_at, revision
)
SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12, 1
WHERE (?13 IS NULL OR EXISTS (
  SELECT 1 FROM providers WHERE id = ?1 AND auth_mode = 'oauth' AND oauth_access_token = ?13
)) AND (?14 IS NULL OR COALESCE((
  SELECT revision FROM provider_oauth_limit_snapshots WHERE provider_id = ?1
), 0) = ?14)
ON CONFLICT(provider_id) DO UPDATE SET
  limit_short_label = excluded.limit_short_label,
  limit_5h_text = excluded.limit_5h_text,
  limit_weekly_text = excluded.limit_weekly_text,
  limit_5h_reset_at = excluded.limit_5h_reset_at,
  limit_weekly_reset_at = excluded.limit_weekly_reset_at,
  reset_credit_available_count = excluded.reset_credit_available_count,
  limit_5h_remaining_percent = excluded.limit_5h_remaining_percent,
  limit_weekly_remaining_percent = excluded.limit_weekly_remaining_percent,
  credits_json = excluded.credits_json,
  usage_limit_reached = excluded.usage_limit_reached,
  revision = provider_oauth_limit_snapshots.revision + 1,
  checked_at = excluded.checked_at,
  updated_at = excluded.updated_at
"#,
            params![
                provider_id,
                normalize_text(input.limit_short_label, SHORT_LABEL_MAX_CHARS),
                normalize_text(input.limit_5h_text, TEXT_MAX_CHARS),
                normalize_text(input.limit_weekly_text, TEXT_MAX_CHARS),
                normalize_reset_at(input.limit_5h_reset_at),
                normalize_reset_at(input.limit_weekly_reset_at),
                normalize_reset_credit_available_count(input.reset_credit_available_count),
                valid_percent(input.limit_5h_remaining_percent),
                valid_percent(input.limit_weekly_remaining_percent),
                credits_json,
                input.usage_limit_reached,
                now,
                expected_access_token,
                expected_snapshot_revision,
            ],
        )
        .map_err(|e| db_err!("failed to save OAuth limit snapshot: {e}"))?;
    Ok(changed > 0)
}

#[cfg(test)]
pub(crate) fn save_exhausted_snapshot(
    db: &db::Db,
    provider_id: i64,
    reset_at: Option<i64>,
) -> AppResult<()> {
    save_exhausted_snapshot_conditionally(db, provider_id, None, reset_at).map(|_| ())
}

pub(crate) fn save_exhausted_snapshot_if_access_token_matches(
    db: &db::Db,
    provider_id: i64,
    expected_access_token: &str,
    reset_at: Option<i64>,
) -> AppResult<bool> {
    save_exhausted_snapshot_conditionally(db, provider_id, Some(expected_access_token), reset_at)
}

fn save_exhausted_snapshot_conditionally(
    db: &db::Db,
    provider_id: i64,
    expected_access_token: Option<&str>,
    reset_at: Option<i64>,
) -> AppResult<bool> {
    let now = now_unix_seconds();
    let existing_snapshot = {
        let conn = db.open_connection()?;
        read_snapshot(&conn, provider_id)?
    };
    let effective_reset_at = match reset_at {
        Some(reset_at) => Some(reset_at),
        None => existing_snapshot.as_ref().and_then(|snapshot| {
            [snapshot.limit_5h_reset_at, snapshot.limit_weekly_reset_at]
                .into_iter()
                .flatten()
                .filter(|candidate| *candidate > now)
                .max()
        }),
    };
    let reset_credit_available_count = existing_snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.reset_credit_available_count);

    save_snapshot_conditionally(
        db,
        OAuthLimitSnapshotInput {
            provider_id,
            limit_short_label: None,
            limit_5h_text: Some("0"),
            limit_weekly_text: None,
            limit_5h_reset_at: effective_reset_at,
            limit_weekly_reset_at: None,
            reset_credit_available_count,
            limit_5h_remaining_percent: Some(0.0),
            limit_weekly_remaining_percent: None,
            credits: None,
            usage_limit_reached: true,
        },
        expected_access_token,
        None,
    )
}

pub(crate) fn clear_snapshot(db: &db::Db, provider_id: i64) -> AppResult<()> {
    let provider_id = validate_provider_id(provider_id)?;
    let conn = db.open_connection()?;
    conn.execute(
        "DELETE FROM provider_oauth_limit_snapshots WHERE provider_id = ?1",
        params![provider_id],
    )
    .map_err(|e| db_err!("failed to clear OAuth limit snapshot: {e}"))?;
    Ok(())
}

pub(crate) fn read_snapshot(
    conn: &Connection,
    provider_id: i64,
) -> AppResult<Option<OAuthLimitSnapshot>> {
    validate_provider_id(provider_id)?;
    conn.query_row(
        r#"
SELECT
  limit_5h_text,
  limit_weekly_text,
  limit_5h_reset_at,
  limit_weekly_reset_at,
  reset_credit_available_count,
  checked_at,
  limit_5h_remaining_percent,
  limit_weekly_remaining_percent,
  credits_json,
  usage_limit_reached,
  revision
FROM provider_oauth_limit_snapshots
WHERE provider_id = ?1
"#,
        params![provider_id],
        |row| {
            Ok(OAuthLimitSnapshot {
                limit_5h_text: row.get(0)?,
                limit_weekly_text: row.get(1)?,
                limit_5h_reset_at: row.get(2)?,
                limit_weekly_reset_at: row.get(3)?,
                reset_credit_available_count: row.get(4)?,
                checked_at: row.get(5)?,
                limit_5h_remaining_percent: row.get(6)?,
                limit_weekly_remaining_percent: row.get(7)?,
                credits: row
                    .get::<_, Option<String>>(8)?
                    .map(|value| {
                        serde_json::from_str(&value).map_err(|error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                8,
                                rusqlite::types::Type::Text,
                                Box::new(error),
                            )
                        })
                    })
                    .transpose()?,
                usage_limit_reached: row.get(9)?,
                revision: row.get(10)?,
            })
        },
    )
    .optional()
    .map_err(|e| db_err!("failed to read OAuth limit snapshot: {e}"))
}

pub(crate) fn gate_snapshot(
    conn: &Connection,
    provider_id: i64,
    now_unix: i64,
) -> AppResult<OAuthLimitGate> {
    let policy = read_policy(conn, provider_id)?;
    let Some(snapshot) = read_snapshot(conn, provider_id)? else {
        return Ok(
            if policy.oauth_min_remaining_percent.is_some() || policy.oauth_use_credits {
                OAuthLimitGate::Limited { reset_at: None }
            } else {
                OAuthLimitGate::Allow
            },
        );
    };
    let mut reset_at = None;
    if snapshot.usage_limit_reached {
        let known_reset = [snapshot.limit_5h_reset_at, snapshot.limit_weekly_reset_at]
            .into_iter()
            .flatten()
            // A reset that predates the observed hard limit cannot release it.
            .filter(|reset_at| *reset_at > snapshot.checked_at)
            .max();
        if let Some(until) =
            active_limited_window_reset_at(true, known_reset, snapshot.checked_at, now_unix)
        {
            return Ok(OAuthLimitGate::Limited {
                reset_at: Some(until),
            });
        }
    }
    if policy.oauth_use_credits
        && snapshot.is_fresh(now_unix)
        && snapshot
            .credits
            .as_ref()
            .is_some_and(|credits| credits.has_credits || credits.unlimited)
    {
        return Ok(OAuthLimitGate::Allow);
    }

    if policy.oauth_min_remaining_percent.is_some()
        && snapshot.limit_5h_remaining_percent.is_none()
        && snapshot.limit_weekly_remaining_percent.is_none()
    {
        return Ok(OAuthLimitGate::Limited { reset_at: None });
    }
    let threshold = policy.oauth_min_remaining_percent.unwrap_or(0.0);
    for (remaining, text, window_reset) in [
        (
            snapshot.limit_5h_remaining_percent,
            snapshot.limit_5h_text.as_deref(),
            snapshot.limit_5h_reset_at,
        ),
        (
            snapshot.limit_weekly_remaining_percent,
            snapshot.limit_weekly_text.as_deref(),
            snapshot.limit_weekly_reset_at,
        ),
    ] {
        let limited = remaining
            .map(|remaining| remaining <= threshold)
            .unwrap_or_else(|| is_exhausted_quota_text(text));
        if let Some(candidate) =
            active_limited_window_reset_at(limited, window_reset, snapshot.checked_at, now_unix)
        {
            update_latest(&mut reset_at, candidate);
        }
    }

    match reset_at {
        Some(reset_at) => Ok(OAuthLimitGate::Limited {
            reset_at: Some(reset_at),
        }),
        None => Ok(OAuthLimitGate::Allow),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn create_snapshot_table(conn: &Connection) {
        conn.execute_batch(
            r#"
CREATE TABLE providers (
  id INTEGER PRIMARY KEY,
  oauth_min_remaining_percent REAL,
  oauth_use_credits INTEGER NOT NULL DEFAULT 0
);
INSERT INTO providers(id) VALUES (7);
CREATE TABLE provider_oauth_limit_snapshots (
  provider_id INTEGER PRIMARY KEY,
  limit_short_label TEXT,
  limit_5h_text TEXT,
  limit_weekly_text TEXT,
  limit_5h_reset_at INTEGER,
  limit_weekly_reset_at INTEGER,
  reset_credit_available_count INTEGER,
  limit_5h_remaining_percent REAL,
  limit_weekly_remaining_percent REAL,
  credits_json TEXT,
  usage_limit_reached INTEGER NOT NULL DEFAULT 0,
  revision INTEGER NOT NULL DEFAULT 0,
  checked_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
"#,
        )
        .expect("create snapshot table");
    }

    fn insert_snapshot(
        conn: &Connection,
        provider_id: i64,
        limit_5h_text: Option<&str>,
        limit_weekly_text: Option<&str>,
        limit_5h_reset_at: Option<i64>,
        limit_weekly_reset_at: Option<i64>,
        checked_at: i64,
    ) {
        conn.execute(
            r#"
INSERT INTO provider_oauth_limit_snapshots(
  provider_id,
  limit_5h_text,
  limit_weekly_text,
  limit_5h_reset_at,
  limit_weekly_reset_at,
  reset_credit_available_count,
  checked_at,
  updated_at
) VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, ?6)
"#,
            params![
                provider_id,
                limit_5h_text,
                limit_weekly_text,
                limit_5h_reset_at,
                limit_weekly_reset_at,
                checked_at
            ],
        )
        .expect("insert snapshot");
    }

    fn insert_test_provider(db: &db::Db) -> i64 {
        insert_test_provider_named(db, "OAuth limit snapshot test")
    }

    fn insert_test_provider_named(db: &db::Db, name: &str) -> i64 {
        crate::providers::upsert(
            db,
            crate::providers::ProviderUpsertParams {
                custom_headers: None,
                provider_id: None,
                cli_key: "codex".to_string(),
                name: name.to_string(),
                base_urls: vec!["https://example.test".to_string()],
                base_url_mode: crate::providers::ProviderBaseUrlMode::Order,
                auth_mode: Some(crate::providers::ProviderAuthMode::ApiKey),
                api_key: Some("sk-test".to_string()),
                enabled: true,
                cost_multiplier: 1.0,
                priority: Some(0),
                claude_models: None,
                model_policy: None,
                limit_5h_usd: None,
                limit_daily_usd: None,
                daily_reset_mode: None,
                daily_reset_time: None,
                limit_weekly_usd: None,
                limit_monthly_usd: None,
                limit_total_usd: None,
                oauth_min_remaining_percent: None,
                oauth_use_credits: false,
                tags: None,
                note: None,
                source_provider_id: None,
                bridge_type: None,
                stream_idle_timeout_seconds: None,
                supports_websockets: None,
                extension_values: None,
            },
        )
        .expect("insert provider")
        .id
    }

    #[test]
    fn exhausted_snapshot_limits_until_latest_reset() {
        let conn = Connection::open_in_memory().expect("open");
        create_snapshot_table(&conn);
        insert_snapshot(
            &conn,
            7,
            Some("0%"),
            Some("0"),
            Some(1_800),
            Some(3_600),
            1_000,
        );

        let gate = gate_snapshot(&conn, 7, 1_200).expect("gate");

        assert_eq!(
            gate,
            OAuthLimitGate::Limited {
                reset_at: Some(3_600)
            }
        );
    }

    #[test]
    fn expired_exhausted_snapshot_allows_provider() {
        let conn = Connection::open_in_memory().expect("open");
        create_snapshot_table(&conn);
        insert_snapshot(&conn, 7, Some("0%"), None, Some(1_800), None, 1_000);

        let gate = gate_snapshot(&conn, 7, 1_800).expect("gate");

        assert_eq!(gate, OAuthLimitGate::Allow);
    }

    #[test]
    fn exhausted_snapshot_without_reset_uses_short_fallback_window() {
        let conn = Connection::open_in_memory().expect("open");
        create_snapshot_table(&conn);
        insert_snapshot(&conn, 7, Some("0 requests"), None, None, None, 1_000);

        let gate = gate_snapshot(&conn, 7, 1_100).expect("gate");
        assert_eq!(
            gate,
            OAuthLimitGate::Limited {
                reset_at: Some(1_000 + FALLBACK_COOLDOWN_SECS)
            }
        );

        let expired = gate_snapshot(&conn, 7, 1_000 + FALLBACK_COOLDOWN_SECS).expect("gate");
        assert_eq!(expired, OAuthLimitGate::Allow);
    }

    #[test]
    fn non_zero_snapshot_allows_provider() {
        let conn = Connection::open_in_memory().expect("open");
        create_snapshot_table(&conn);
        insert_snapshot(
            &conn,
            7,
            Some("1%"),
            Some("2"),
            Some(1_800),
            Some(3_600),
            1_000,
        );

        let gate = gate_snapshot(&conn, 7, 1_200).expect("gate");

        assert_eq!(gate, OAuthLimitGate::Allow);
    }

    #[test]
    fn refreshed_available_snapshot_overwrites_exhausted_snapshot() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = db::init_for_tests(&dir.path().join("oauth-limits.db")).expect("init db");
        let now = now_unix_seconds();
        let provider_id = insert_test_provider(&db);

        save_exhausted_snapshot(&db, provider_id, Some(now + 3_600)).expect("save exhausted");
        {
            let conn = db.open_connection().expect("open");
            let gate = gate_snapshot(&conn, provider_id, now).expect("gate");
            assert_eq!(
                gate,
                OAuthLimitGate::Limited {
                    reset_at: Some(now + 3_600)
                }
            );
        }

        save_snapshot(
            &db,
            OAuthLimitSnapshotInput {
                provider_id,
                limit_short_label: Some("5h"),
                limit_5h_text: Some("25%"),
                limit_weekly_text: Some("80%"),
                limit_5h_reset_at: None,
                limit_weekly_reset_at: None,
                reset_credit_available_count: Some(2),
                limit_5h_remaining_percent: None,
                limit_weekly_remaining_percent: None,
                credits: None,
                usage_limit_reached: false,
            },
        )
        .expect("save refreshed snapshot");

        let conn = db.open_connection().expect("open");
        let gate = gate_snapshot(&conn, provider_id, now).expect("gate");
        assert_eq!(gate, OAuthLimitGate::Allow);
    }

    #[test]
    fn exhausted_snapshot_preserves_existing_future_reset_when_missing_new_reset() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = db::init_for_tests(&dir.path().join("oauth-limits-preserve-reset.db"))
            .expect("init db");
        let now = now_unix_seconds();
        let provider_id = insert_test_provider(&db);

        save_snapshot(
            &db,
            OAuthLimitSnapshotInput {
                provider_id,
                limit_short_label: Some("5h"),
                limit_5h_text: Some("1%"),
                limit_weekly_text: Some("10%"),
                limit_5h_reset_at: Some(now + 1_800),
                limit_weekly_reset_at: Some(now + 86_400),
                reset_credit_available_count: Some(5),
                limit_5h_remaining_percent: None,
                limit_weekly_remaining_percent: None,
                credits: None,
                usage_limit_reached: false,
            },
        )
        .expect("save current snapshot");

        save_exhausted_snapshot(&db, provider_id, None).expect("save exhausted snapshot");

        let conn = db.open_connection().expect("open");
        let gate = gate_snapshot(&conn, provider_id, now).expect("gate");
        assert_eq!(
            gate,
            OAuthLimitGate::Limited {
                reset_at: Some(now + 86_400)
            }
        );
        let reset_count: Option<i64> = conn
            .query_row(
                "SELECT reset_credit_available_count FROM provider_oauth_limit_snapshots WHERE provider_id = ?1",
                params![provider_id],
                |row| row.get(0),
            )
            .expect("read reset count");
        assert_eq!(reset_count, Some(5));
    }

    #[test]
    fn save_snapshot_persists_reset_credit_available_count_per_provider() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db =
            db::init_for_tests(&dir.path().join("oauth-limits-reset-count.db")).expect("init db");
        let first_provider_id = insert_test_provider_named(&db, "OAuth limit snapshot test 1");
        let second_provider_id = insert_test_provider_named(&db, "OAuth limit snapshot test 2");

        save_snapshot(
            &db,
            OAuthLimitSnapshotInput {
                provider_id: first_provider_id,
                limit_short_label: Some("5h"),
                limit_5h_text: Some("25%"),
                limit_weekly_text: Some("80%"),
                limit_5h_reset_at: None,
                limit_weekly_reset_at: None,
                reset_credit_available_count: Some(4),
                limit_5h_remaining_percent: None,
                limit_weekly_remaining_percent: None,
                credits: None,
                usage_limit_reached: false,
            },
        )
        .expect("save first snapshot");
        save_snapshot(
            &db,
            OAuthLimitSnapshotInput {
                provider_id: second_provider_id,
                limit_short_label: Some("5h"),
                limit_5h_text: Some("90%"),
                limit_weekly_text: Some("95%"),
                limit_5h_reset_at: None,
                limit_weekly_reset_at: None,
                reset_credit_available_count: Some(1),
                limit_5h_remaining_percent: None,
                limit_weekly_remaining_percent: None,
                credits: None,
                usage_limit_reached: false,
            },
        )
        .expect("save second snapshot");

        let conn = db.open_connection().expect("open");
        let first_count: Option<i64> = conn
            .query_row(
                "SELECT reset_credit_available_count FROM provider_oauth_limit_snapshots WHERE provider_id = ?1",
                params![first_provider_id],
                |row| row.get(0),
            )
            .expect("read first count");
        let second_count: Option<i64> = conn
            .query_row(
                "SELECT reset_credit_available_count FROM provider_oauth_limit_snapshots WHERE provider_id = ?1",
                params![second_provider_id],
                |row| row.get(0),
            )
            .expect("read second count");

        assert_eq!(first_count, Some(4));
        assert_eq!(second_count, Some(1));
    }

    #[test]
    fn acceptance_oauth_exhausted_snapshot_is_scoped_to_provider() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = db::init_for_tests(&dir.path().join("oauth-limits-scope.db")).expect("init db");
        let now = now_unix_seconds();
        let exhausted_provider_id = insert_test_provider_named(&db, "OAuth exhausted");
        let healthy_provider_id = insert_test_provider_named(&db, "OAuth healthy");

        save_exhausted_snapshot(&db, exhausted_provider_id, Some(now + 3_600))
            .expect("save exhausted snapshot");
        save_snapshot(
            &db,
            OAuthLimitSnapshotInput {
                provider_id: healthy_provider_id,
                limit_short_label: Some("5h"),
                limit_5h_text: Some("25%"),
                limit_weekly_text: Some("80%"),
                limit_5h_reset_at: None,
                limit_weekly_reset_at: None,
                reset_credit_available_count: Some(3),
                limit_5h_remaining_percent: None,
                limit_weekly_remaining_percent: None,
                credits: None,
                usage_limit_reached: false,
            },
        )
        .expect("save healthy snapshot");

        let conn = db.open_connection().expect("open");
        assert_eq!(
            gate_snapshot(&conn, exhausted_provider_id, now).expect("gate exhausted"),
            OAuthLimitGate::Limited {
                reset_at: Some(now + 3_600)
            }
        );
        assert_eq!(
            gate_snapshot(&conn, healthy_provider_id, now).expect("gate healthy"),
            OAuthLimitGate::Allow
        );
    }

    #[test]
    fn quota_policy_uses_exact_percent_and_either_window() {
        let conn = Connection::open_in_memory().expect("open");
        create_snapshot_table(&conn);
        insert_snapshot(
            &conn,
            7,
            Some("5%"),
            Some("80%"),
            Some(1800),
            Some(3600),
            1000,
        );
        conn.execute(
            "UPDATE providers SET oauth_min_remaining_percent = 5 WHERE id = 7",
            [],
        )
        .unwrap();
        for (short, weekly, expected) in [
            (5.01, 80.0, OAuthLimitGate::Allow),
            (
                5.0,
                80.0,
                OAuthLimitGate::Limited {
                    reset_at: Some(1800),
                },
            ),
            (
                4.99,
                80.0,
                OAuthLimitGate::Limited {
                    reset_at: Some(1800),
                },
            ),
            (
                80.0,
                5.0,
                OAuthLimitGate::Limited {
                    reset_at: Some(3600),
                },
            ),
            (
                5.0,
                5.0,
                OAuthLimitGate::Limited {
                    reset_at: Some(3600),
                },
            ),
        ] {
            conn.execute("UPDATE provider_oauth_limit_snapshots SET limit_5h_remaining_percent = ?1, limit_weekly_remaining_percent = ?2", params![short, weekly]).unwrap();
            assert_eq!(
                gate_snapshot(&conn, 7, 1100).unwrap(),
                expected,
                "{short}/{weekly}"
            );
        }
        assert_eq!(
            gate_snapshot(&conn, 7, 3600).unwrap(),
            OAuthLimitGate::Allow
        );
        conn.execute(
            "UPDATE providers SET oauth_min_remaining_percent = NULL",
            [],
        )
        .unwrap();
        conn.execute("UPDATE provider_oauth_limit_snapshots SET limit_5h_text = '0%', limit_5h_remaining_percent = 0.01, limit_weekly_remaining_percent = 80", []).unwrap();
        assert_eq!(
            gate_snapshot(&conn, 7, 1100).unwrap(),
            OAuthLimitGate::Allow,
            "numeric value takes priority over rounded text"
        );
    }

    #[test]
    fn credits_require_opt_in_known_availability_and_fresh_snapshot() {
        let conn = Connection::open_in_memory().expect("open");
        create_snapshot_table(&conn);
        insert_snapshot(&conn, 7, Some("4%"), None, Some(1800), None, 1000);
        conn.execute("UPDATE providers SET oauth_min_remaining_percent = 5", [])
            .unwrap();
        conn.execute(
            "UPDATE provider_oauth_limit_snapshots SET limit_5h_remaining_percent = 4",
            [],
        )
        .unwrap();
        let limited = OAuthLimitGate::Limited {
            reset_at: Some(1800),
        };
        for (enabled, credits, expected) in [
            (
                false,
                Some(r#"{"has_credits":true,"unlimited":false,"balance":"1.000000000000000001"}"#),
                limited,
            ),
            (true, None, limited),
            (
                true,
                Some(r#"{"has_credits":false,"unlimited":false,"balance":"0"}"#),
                limited,
            ),
            (
                true,
                Some(r#"{"has_credits":true,"unlimited":false,"balance":"1.000000000000000001"}"#),
                OAuthLimitGate::Allow,
            ),
            (
                true,
                Some(r#"{"has_credits":false,"unlimited":true,"balance":null}"#),
                OAuthLimitGate::Allow,
            ),
        ] {
            conn.execute("UPDATE providers SET oauth_use_credits = ?1", [enabled])
                .unwrap();
            conn.execute(
                "UPDATE provider_oauth_limit_snapshots SET credits_json = ?1",
                [credits],
            )
            .unwrap();
            assert_eq!(gate_snapshot(&conn, 7, 1100).unwrap(), expected);
        }
        assert_eq!(
            gate_snapshot(&conn, 7, 1180).unwrap(),
            limited,
            "stale credits cannot bypass the gate"
        );
        conn.execute(
            "UPDATE provider_oauth_limit_snapshots SET usage_limit_reached = 1",
            [],
        )
        .unwrap();
        assert_eq!(
            gate_snapshot(&conn, 7, 1100).unwrap(),
            limited,
            "hard upstream limit wins over credits"
        );
        conn.execute(r#"UPDATE provider_oauth_limit_snapshots SET usage_limit_reached = 0, limit_5h_remaining_percent = 80, credits_json = '{"has_credits":false,"unlimited":false,"balance":"0"}'"#, []).unwrap();
        assert_eq!(
            gate_snapshot(&conn, 7, 1100).unwrap(),
            OAuthLimitGate::Allow,
            "zero credits cannot block available subscription quota"
        );
    }

    #[test]
    fn fresh_hard_limit_is_not_released_by_a_reset_before_it_was_observed() {
        let conn = Connection::open_in_memory().unwrap();
        create_snapshot_table(&conn);
        insert_snapshot(&conn, 7, Some("80%"), Some("80%"), None, None, 1000);
        conn.execute(
            "UPDATE providers SET oauth_min_remaining_percent = 5, oauth_use_credits = 1",
            [],
        )
        .unwrap();
        for reset in [Some(900), Some(1000), Some(1500), None] {
            for credits in [
                None,
                Some(r#"{"has_credits":true,"unlimited":false,"balance":"50"}"#),
            ] {
                conn.execute("UPDATE provider_oauth_limit_snapshots SET usage_limit_reached = 1, limit_5h_remaining_percent = 80, limit_weekly_remaining_percent = 80, limit_5h_reset_at = ?1, limit_weekly_reset_at = ?1, credits_json = ?2", params![reset, credits]).unwrap();
                assert_eq!(
                    gate_snapshot(&conn, 7, 1001).unwrap(),
                    OAuthLimitGate::Limited {
                        reset_at: Some(
                            reset
                                .filter(|reset| *reset > 1000)
                                .unwrap_or(1000 + FALLBACK_COOLDOWN_SECS)
                        ),
                    },
                    "reset={reset:?}, credits={credits:?}"
                );
                // A subsequent usage response explicitly clears the hard limit.
                conn.execute(
                    "UPDATE provider_oauth_limit_snapshots SET usage_limit_reached = 0",
                    [],
                )
                .unwrap();
                assert_eq!(
                    gate_snapshot(&conn, 7, 1001).unwrap(),
                    OAuthLimitGate::Allow
                );
            }
        }
    }

    #[test]
    fn guarded_snapshot_does_not_overwrite_a_new_account_and_hard_limit_clears_credits() {
        let dir = tempfile::tempdir().unwrap();
        let db = db::init_for_tests(&dir.path().join("guarded-snapshot.db")).unwrap();
        let provider_id = insert_test_provider(&db);
        db.open_connection().unwrap().execute("UPDATE providers SET auth_mode = 'oauth', oauth_access_token = 'current', oauth_use_credits = 1 WHERE id = ?1", [provider_id]).unwrap();
        let credits = ProviderOAuthCreditBalance {
            has_credits: true,
            unlimited: false,
            balance: Some("1.000000000000000001".into()),
        };
        let input = OAuthLimitSnapshotInput {
            provider_id,
            limit_short_label: Some("5h"),
            limit_5h_text: Some("0%"),
            limit_weekly_text: Some("80%"),
            limit_5h_reset_at: Some(now_unix_seconds() + 3600),
            limit_weekly_reset_at: None,
            reset_credit_available_count: Some(4),
            limit_5h_remaining_percent: Some(0.0),
            limit_weekly_remaining_percent: Some(80.0),
            credits: Some(&credits),
            usage_limit_reached: false,
        };
        assert!(!save_snapshot_if_access_token_matches(&db, input.clone(), "old", 0).unwrap());
        assert!(read_snapshot(&db.open_connection().unwrap(), provider_id)
            .unwrap()
            .is_none());
        assert!(save_snapshot_if_access_token_matches(&db, input.clone(), "current", 0).unwrap());
        let saved = read_snapshot(&db.open_connection().unwrap(), provider_id)
            .unwrap()
            .unwrap();
        assert_eq!(saved.credits, Some(credits.clone()));
        assert!(saved.is_fresh(saved.checked_at));
        assert!(!saved.is_fresh(saved.checked_at + 180));
        assert!(!saved.is_fresh(saved.checked_at - 1));
        db.open_connection()
            .unwrap()
            .execute(
                "UPDATE providers SET oauth_access_token = 'new' WHERE id = ?1",
                [provider_id],
            )
            .unwrap();
        let mut outdated = input;
        outdated.limit_weekly_remaining_percent = Some(0.0);
        assert!(!save_snapshot_if_access_token_matches(
            &db,
            outdated.clone(),
            "current",
            saved.revision
        )
        .unwrap());
        assert_eq!(
            read_snapshot(&db.open_connection().unwrap(), provider_id)
                .unwrap()
                .unwrap()
                .limit_weekly_remaining_percent,
            Some(80.0)
        );
        outdated.limit_5h_remaining_percent = Some(f64::NAN);
        outdated.limit_weekly_remaining_percent = Some(-1.0);
        assert!(
            save_snapshot_if_access_token_matches(&db, outdated, "new", saved.revision).unwrap()
        );
        let saved = read_snapshot(&db.open_connection().unwrap(), provider_id)
            .unwrap()
            .unwrap();
        assert_eq!(saved.limit_5h_remaining_percent, None);
        assert_eq!(saved.limit_weekly_remaining_percent, None);
        assert!(!save_exhausted_snapshot_if_access_token_matches(
            &db,
            provider_id,
            "current",
            None
        )
        .unwrap());
        let retained = read_snapshot(&db.open_connection().unwrap(), provider_id)
            .unwrap()
            .unwrap();
        assert_eq!(retained.revision, saved.revision);
        assert_eq!(retained.credits, saved.credits);
        assert!(!retained.usage_limit_reached);
        assert!(
            save_exhausted_snapshot_if_access_token_matches(&db, provider_id, "new", None).unwrap()
        );
        let exhausted = read_snapshot(&db.open_connection().unwrap(), provider_id)
            .unwrap()
            .unwrap();
        assert!(exhausted.usage_limit_reached);
        assert!(exhausted.credits.is_none());
        assert_eq!(exhausted.reset_credit_available_count, Some(4));
        assert!(matches!(
            gate_snapshot(
                &db.open_connection().unwrap(),
                provider_id,
                now_unix_seconds()
            )
            .unwrap(),
            OAuthLimitGate::Limited { .. }
        ));
        db.open_connection()
            .unwrap()
            .execute(
                "UPDATE providers SET auth_mode = 'api_key' WHERE id = ?1",
                [provider_id],
            )
            .unwrap();
        clear_snapshot(&db, provider_id).unwrap();
        assert!(
            !save_exhausted_snapshot_if_access_token_matches(&db, provider_id, "new", None)
                .unwrap()
        );
        assert!(read_snapshot(&db.open_connection().unwrap(), provider_id)
            .unwrap()
            .is_none());
    }

    #[test]
    fn configured_quota_policy_blocks_unknown_usage_and_reset_invalidates_freshness() {
        let conn = Connection::open_in_memory().unwrap();
        create_snapshot_table(&conn);
        assert_eq!(
            gate_snapshot(&conn, 7, 1100).unwrap(),
            OAuthLimitGate::Allow
        );
        conn.execute("UPDATE providers SET oauth_min_remaining_percent = 5", [])
            .unwrap();
        assert_eq!(
            gate_snapshot(&conn, 7, 1100).unwrap(),
            OAuthLimitGate::Limited { reset_at: None }
        );
        insert_snapshot(&conn, 7, Some("unknown"), None, Some(1150), None, 1000);
        assert_eq!(
            gate_snapshot(&conn, 7, 1100).unwrap(),
            OAuthLimitGate::Limited { reset_at: None }
        );
        conn.execute(r#"UPDATE provider_oauth_limit_snapshots SET credits_json = '{"has_credits":true,"unlimited":false,"balance":"1"}'"#, []).unwrap();
        assert_eq!(
            gate_snapshot(&conn, 7, 1100).unwrap(),
            OAuthLimitGate::Limited { reset_at: None }
        );
        conn.execute("UPDATE providers SET oauth_use_credits = 1", [])
            .unwrap();
        assert_eq!(
            gate_snapshot(&conn, 7, 1100).unwrap(),
            OAuthLimitGate::Allow
        );
        let snapshot = read_snapshot(&conn, 7).unwrap().unwrap();
        assert!(snapshot.is_fresh(1149));
        assert!(!snapshot.is_fresh(1150));
        assert_eq!(
            gate_snapshot(&conn, 7, 1150).unwrap(),
            OAuthLimitGate::Limited { reset_at: None }
        );
        conn.execute(
            "UPDATE providers SET oauth_min_remaining_percent = NULL, oauth_use_credits = 0",
            [],
        )
        .unwrap();
        assert_eq!(
            gate_snapshot(&conn, 7, 1100).unwrap(),
            OAuthLimitGate::Allow
        );
    }

    #[test]
    fn old_usage_response_cannot_overwrite_new_hard_limit_with_credits() {
        let dir = tempfile::tempdir().unwrap();
        let db = db::init_for_tests(&dir.path().join("snapshot-revision.db")).unwrap();
        let provider_id = insert_test_provider(&db);
        db.open_connection().unwrap().execute("UPDATE providers SET auth_mode = 'oauth', oauth_access_token = 'same-token', oauth_use_credits = 1 WHERE id = ?1", [provider_id]).unwrap();
        let credits = ProviderOAuthCreditBalance {
            has_credits: true,
            unlimited: false,
            balance: Some("1".into()),
        };
        let input = OAuthLimitSnapshotInput {
            provider_id,
            limit_short_label: Some("5h"),
            limit_5h_text: Some("0%"),
            limit_weekly_text: None,
            limit_5h_reset_at: Some(now_unix_seconds() + 3600),
            limit_weekly_reset_at: None,
            reset_credit_available_count: None,
            limit_5h_remaining_percent: Some(0.0),
            limit_weekly_remaining_percent: None,
            credits: Some(&credits),
            usage_limit_reached: false,
        };
        save_snapshot(&db, input.clone()).unwrap();
        let before_request = read_snapshot(&db.open_connection().unwrap(), provider_id)
            .unwrap()
            .unwrap();
        assert_eq!(before_request.revision, 1);
        save_exhausted_snapshot(&db, provider_id, None).unwrap();
        let after_limit = read_snapshot(&db.open_connection().unwrap(), provider_id)
            .unwrap()
            .unwrap();
        assert_eq!(after_limit.revision, 2);
        assert!(!save_snapshot_if_access_token_matches(
            &db,
            input.clone(),
            "same-token",
            before_request.revision
        )
        .unwrap());
        let retained = read_snapshot(&db.open_connection().unwrap(), provider_id)
            .unwrap()
            .unwrap();
        assert!(retained.usage_limit_reached);
        assert!(retained.credits.is_none());
        assert_eq!(retained.revision, 2);
        assert!(save_snapshot_if_access_token_matches(
            &db,
            input,
            "same-token",
            after_limit.revision
        )
        .unwrap());
        let refreshed = read_snapshot(&db.open_connection().unwrap(), provider_id)
            .unwrap()
            .unwrap();
        assert!(!refreshed.usage_limit_reached);
        assert_eq!(refreshed.credits, Some(credits));
        assert_eq!(refreshed.revision, 3);
        assert_eq!(
            gate_snapshot(
                &db.open_connection().unwrap(),
                provider_id,
                now_unix_seconds()
            )
            .unwrap(),
            OAuthLimitGate::Allow
        );
    }
}
