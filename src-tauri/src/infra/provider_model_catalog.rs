//! Usage: Persist discovered upstream model catalogs and derive routable unions.

use crate::db;
use crate::providers::{self, ProviderForGateway, ProviderModelPolicyStatus};
use crate::shared::error::db_err;
use rusqlite::{params, OptionalExtension};
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProviderModelCatalogSnapshot {
    pub provider_id: i64,
    pub config_version: i64,
    pub models: Vec<String>,
    pub status: String,
    pub last_success_at: Option<i64>,
    pub last_attempt_at: i64,
    pub last_error: Option<String>,
}

impl ProviderModelCatalogSnapshot {
    pub(crate) fn is_usable(&self, config_version: i64) -> bool {
        self.config_version == config_version
            && matches!(self.status.as_str(), "fresh" | "stale")
            && !self.models.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelCatalogRoute {
    pub provider_id: i64,
    pub provider_name: String,
    pub route_order: usize,
    pub upstream_model_id: String,
    pub snapshot_status: String,
    pub is_mapping: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelCatalogItem {
    pub model_id: String,
    pub routes: Vec<ModelCatalogRoute>,
}

pub(crate) fn list_model_catalog(
    db: &db::Db,
    cli_key: &str,
) -> crate::shared::error::AppResult<Vec<ModelCatalogItem>> {
    let selection = providers::list_enabled_for_gateway_using_active_mode(db, cli_key)?;
    list_model_catalog_for_providers(db, &selection.providers)
}

pub(crate) fn save_success(
    db: &db::Db,
    provider_id: i64,
    config_version: i64,
    models: &[String],
    now: i64,
) -> crate::shared::error::AppResult<()> {
    let mut normalized = models
        .iter()
        .filter_map(|model| providers::normalize_concrete_model_id(model).ok())
        .collect::<Vec<_>>();
    normalized.sort_unstable();
    normalized.dedup();
    let models_json = serde_json::to_string(&normalized)
        .map_err(|error| format!("failed to serialize provider model catalog: {error}"))?;
    let conn = db.open_connection()?;
    conn.execute(
        r#"
INSERT INTO provider_model_catalogs(
  provider_id, config_version, models_json, status,
  last_success_at, last_attempt_at, last_error, updated_at
) VALUES (?1, ?2, ?3, 'fresh', ?4, ?4, NULL, ?4)
ON CONFLICT(provider_id) DO UPDATE SET
  config_version = excluded.config_version,
  models_json = excluded.models_json,
  status = 'fresh',
  last_success_at = excluded.last_success_at,
  last_attempt_at = excluded.last_attempt_at,
  last_error = NULL,
  updated_at = excluded.updated_at
"#,
        params![provider_id, config_version, models_json, now],
    )
    .map_err(|error| db_err!("failed to save provider model catalog: {error}"))?;
    Ok(())
}

pub(crate) fn save_failure(
    db: &db::Db,
    provider_id: i64,
    config_version: i64,
    error: &str,
    now: i64,
) -> crate::shared::error::AppResult<()> {
    let conn = db.open_connection()?;
    conn.execute(
        r#"
INSERT INTO provider_model_catalogs(
  provider_id, config_version, models_json, status,
  last_success_at, last_attempt_at, last_error, updated_at
) VALUES (?1, ?2, '[]', 'failed', NULL, ?3, ?4, ?3)
ON CONFLICT(provider_id) DO UPDATE SET
  status = CASE
    WHEN provider_model_catalogs.config_version = excluded.config_version
      AND provider_model_catalogs.last_success_at IS NOT NULL
      AND provider_model_catalogs.models_json != '[]'
    THEN 'stale'
    ELSE 'failed'
  END,
  config_version = excluded.config_version,
  last_attempt_at = excluded.last_attempt_at,
  last_error = excluded.last_error,
  updated_at = excluded.updated_at
"#,
        params![provider_id, config_version, now, error],
    )
    .map_err(|error| db_err!("failed to save provider model catalog failure: {error}"))?;
    Ok(())
}

pub(crate) fn load_for_provider(
    db: &db::Db,
    provider_id: i64,
) -> crate::shared::error::AppResult<Option<ProviderModelCatalogSnapshot>> {
    let conn = db.open_connection()?;
    conn.query_row(
        r#"
SELECT provider_id, config_version, models_json, status,
       last_success_at, last_attempt_at, last_error
FROM provider_model_catalogs
WHERE provider_id = ?1
"#,
        params![provider_id],
        map_snapshot,
    )
    .optional()
    .map_err(|error| db_err!("failed to load provider model catalog: {error}"))
}

pub(crate) fn load_for_providers(
    db: &db::Db,
    provider_ids: &[i64],
) -> crate::shared::error::AppResult<HashMap<i64, ProviderModelCatalogSnapshot>> {
    let mut result = HashMap::new();
    for provider_id in provider_ids.iter().copied().filter(|id| *id > 0) {
        if let Some(snapshot) = load_for_provider(db, provider_id)? {
            result.insert(provider_id, snapshot);
        }
    }
    Ok(result)
}

pub(crate) fn provider_config_version(
    db: &db::Db,
    provider_id: i64,
) -> crate::shared::error::AppResult<Option<i64>> {
    let conn = db.open_connection()?;
    conn.query_row(
        "SELECT updated_at FROM providers WHERE id = ?1",
        params![provider_id],
        |row| row.get(0),
    )
    .optional()
    .map_err(|error| db_err!("failed to load provider config version: {error}"))
}

pub(crate) fn union_models(
    db: &db::Db,
    providers: &[ProviderForGateway],
) -> crate::shared::error::AppResult<Vec<String>> {
    Ok(list_model_catalog_for_providers(db, providers)?
        .into_iter()
        .map(|item| item.model_id)
        .collect())
}

fn list_model_catalog_for_providers(
    db: &db::Db,
    providers: &[ProviderForGateway],
) -> crate::shared::error::AppResult<Vec<ModelCatalogItem>> {
    let snapshots = load_for_providers(
        db,
        &providers
            .iter()
            .map(|provider| provider.id)
            .collect::<Vec<_>>(),
    )?;
    let mut models: BTreeMap<String, Vec<ModelCatalogRoute>> = BTreeMap::new();
    for (route_order, provider) in providers.iter().enumerate() {
        let Some(snapshot) = snapshots.get(&provider.id) else {
            continue;
        };
        let Some(config_version) = provider_config_version(db, provider.id)? else {
            continue;
        };
        if !snapshot.is_usable(config_version) {
            continue;
        }
        for model_id in public_model_ids(provider, &snapshot.models) {
            if !provider_supports_source_model_from_snapshot(provider, &model_id, snapshot) {
                continue;
            }
            let upstream_model_id = provider
                .model_policy
                .as_ref()
                .map(|policy| policy.resolve_mapping(&model_id))
                .unwrap_or_else(|| model_id.clone());
            models
                .entry(model_id.clone())
                .or_default()
                .push(ModelCatalogRoute {
                    provider_id: provider.id,
                    provider_name: provider.name.clone(),
                    route_order,
                    upstream_model_id: upstream_model_id.clone(),
                    snapshot_status: snapshot.status.clone(),
                    is_mapping: upstream_model_id != model_id,
                });
        }
    }
    Ok(models
        .into_iter()
        .map(|(model_id, routes)| ModelCatalogItem { model_id, routes })
        .collect())
}

pub(crate) fn provider_supports_source_model(
    db: &db::Db,
    provider: &ProviderForGateway,
    source_model: &str,
) -> crate::shared::error::AppResult<bool> {
    let Some(snapshot) = load_for_provider(db, provider.id)? else {
        return Ok(false);
    };
    let Some(config_version) = provider_config_version(db, provider.id)? else {
        return Ok(false);
    };
    if !snapshot.is_usable(config_version) {
        return Ok(false);
    }
    Ok(provider_supports_source_model_from_snapshot(
        provider,
        source_model,
        &snapshot,
    ))
}
pub(crate) fn retain_routable_providers_for_cli(
    db: &db::Db,
    cli_key: &str,
    providers: &mut Vec<ProviderForGateway>,
    source_model: &str,
) -> crate::shared::error::AppResult<()> {
    // A model the user explicitly disabled in the catalog must not route,
    // even when provider snapshots still advertise it.
    if crate::infra::model_catalog_metadata::is_disabled(db, cli_key, source_model)? {
        providers.clear();
        return Ok(());
    }
    if cli_key == "codex" {
        return retain_routable_providers(db, providers, source_model);
    }
    let mut retained = Vec::with_capacity(providers.len());
    for provider in providers.drain(..) {
        if provider_route_unknown_or_supported(db, &provider, source_model)? {
            retained.push(provider);
        }
    }
    *providers = retained;
    Ok(())
}

pub(crate) fn retain_routable_providers(
    db: &db::Db,
    providers: &mut Vec<ProviderForGateway>,
    source_model: &str,
) -> crate::shared::error::AppResult<()> {
    let mut retained = Vec::with_capacity(providers.len());
    for provider in providers.drain(..) {
        if provider_route_unknown_or_supported(db, &provider, source_model)? {
            retained.push(provider);
        }
    }
    *providers = retained;
    Ok(())
}

fn provider_route_unknown_or_supported(
    db: &db::Db,
    provider: &ProviderForGateway,
    source_model: &str,
) -> crate::shared::error::AppResult<bool> {
    let Some(snapshot) = load_for_provider(db, provider.id)? else {
        return Ok(true);
    };
    let Some(config_version) = provider_config_version(db, provider.id)? else {
        return Ok(false);
    };
    if !snapshot.is_usable(config_version) {
        return Ok(true);
    }
    Ok(provider_supports_source_model_from_snapshot(
        provider,
        source_model,
        &snapshot,
    ))
}

fn provider_supports_source_model_from_snapshot(
    provider: &ProviderForGateway,
    source_model: &str,
    snapshot: &ProviderModelCatalogSnapshot,
) -> bool {
    let target = match provider.model_policy_status {
        ProviderModelPolicyStatus::Ready => provider
            .model_policy
            .as_ref()
            .map(|policy| policy.resolve_mapping(source_model))
            .unwrap_or_else(|| source_model.to_string()),
        ProviderModelPolicyStatus::Legacy => source_model.to_string(),
        ProviderModelPolicyStatus::Invalid => return false,
    };
    let policy_allows = match provider.model_policy_status {
        ProviderModelPolicyStatus::Ready => provider.model_policy.as_ref().is_some_and(|policy| {
            policy.eligibility(source_model) != providers::ProviderModelEligibility::Blocked
        }),
        ProviderModelPolicyStatus::Legacy => true,
        ProviderModelPolicyStatus::Invalid => false,
    };
    policy_allows && snapshot.models.iter().any(|model| model == &target)
}

fn public_model_ids(provider: &ProviderForGateway, upstream_models: &[String]) -> BTreeSet<String> {
    match provider.model_policy_status {
        ProviderModelPolicyStatus::Legacy => upstream_models.iter().cloned().collect(),
        ProviderModelPolicyStatus::Invalid => BTreeSet::new(),
        ProviderModelPolicyStatus::Ready => {
            let Some(policy) = provider.model_policy.as_ref() else {
                return BTreeSet::new();
            };
            let mut result = BTreeSet::new();
            for upstream in upstream_models {
                if policy.eligibility(upstream) != providers::ProviderModelEligibility::Blocked {
                    result.insert(upstream.clone());
                }
                for mapping in &policy.mappings {
                    if let Some(capture) = match_pattern(&mapping.target, upstream) {
                        let source = mapping.source.replace('*', capture);
                        if !source.contains('*')
                            && policy.eligibility(&source)
                                != providers::ProviderModelEligibility::Blocked
                        {
                            result.insert(source);
                        }
                    }
                }
            }
            result
        }
    }
}

fn match_pattern<'a>(pattern: &'a str, value: &'a str) -> Option<&'a str> {
    let Some(star) = pattern.find('*') else {
        return (pattern == value).then_some("");
    };
    let prefix = &pattern[..star];
    let suffix = &pattern[star + 1..];
    let remainder = value.strip_prefix(prefix)?;
    remainder.strip_suffix(suffix)
}

fn map_snapshot(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProviderModelCatalogSnapshot> {
    let models_json: String = row.get("models_json")?;
    let mut models = serde_json::from_str::<Vec<String>>(&models_json).unwrap_or_default();
    models.retain(|model| providers::normalize_concrete_model_id(model).is_ok());
    models.sort_unstable();
    models.dedup();
    Ok(ProviderModelCatalogSnapshot {
        provider_id: row.get("provider_id")?,
        config_version: row.get("config_version")?,
        models,
        status: row.get("status")?,
        last_success_at: row.get("last_success_at")?,
        last_attempt_at: row.get("last_attempt_at")?,
        last_error: row.get("last_error")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn union_requires_a_current_snapshot_and_deduplicates_models() {
        let mut models = vec![
            "gpt-x".to_string(),
            "deepseek-y".to_string(),
            "gpt-x".to_string(),
        ];
        models.sort_unstable();
        models.dedup();
        assert_eq!(models, vec!["deepseek-y", "gpt-x"]);
    }

    #[test]
    fn stale_snapshot_remains_usable_for_the_same_configuration() {
        let snapshot = ProviderModelCatalogSnapshot {
            provider_id: 1,
            config_version: 42,
            models: vec!["gpt-x".to_string()],
            status: "stale".to_string(),
            last_success_at: Some(41),
            last_attempt_at: 42,
            last_error: Some("timeout".to_string()),
        };
        assert!(snapshot.is_usable(42));
        assert!(!snapshot.is_usable(43));
    }
}
