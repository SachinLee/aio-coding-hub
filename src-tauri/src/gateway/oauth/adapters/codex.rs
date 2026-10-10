//! Usage: Codex (OpenAI / ChatGPT) OAuth adapter.

use crate::gateway::oauth::provider_trait::*;
use crate::gateway::upstream_identity;
use crate::shared::http_body::read_text_with_limit;
use axum::http::{HeaderMap, HeaderValue};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use std::future::Future;
use std::pin::Pin;

pub(crate) struct CodexOAuthProvider {
    endpoints: OAuthEndpoints,
}

const CODEX_LIMITS_RESPONSE_BODY_LIMIT: usize = 1024 * 1024;

// Discovery fallback is verified against the 0.144.4 manifest protocol; it does not
// change inference/refresh identity. Prefer the installed CLI's valid version.
pub(crate) const CODEX_MODEL_DISCOVERY_FALLBACK_VERSION: &str = "0.144.4";

pub(crate) fn codex_model_discovery_version(raw: Option<&str>) -> &str {
    static VERSION: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let pattern = VERSION.get_or_init(|| {
        regex::Regex::new(
            r"^(?:codex-cli[ \t]+)?v?([0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?)$",
        )
        .expect("valid Codex discovery version pattern")
    });
    raw.and_then(|raw| pattern.captures(raw.trim()))
        .and_then(|captures| captures.get(1))
        .map(|version| version.as_str())
        .unwrap_or(CODEX_MODEL_DISCOVERY_FALLBACK_VERSION)
}

impl CodexOAuthProvider {
    pub(crate) fn new() -> Self {
        Self {
            endpoints: OAuthEndpoints {
                auth_url: "https://auth.openai.com/oauth/authorize",
                token_url: "https://auth.openai.com/oauth/token",
                client_id: "app_EMoamEEZ73f0CkXaXp7hrann".to_string(),
                client_secret: None,
                scopes: vec![
                    "openid",
                    "profile",
                    "email",
                    "offline_access",
                    "api.connectors.read",
                    "api.connectors.invoke",
                ],
                redirect_host: "localhost",
                callback_path: "/auth/callback",
                default_callback_port: 1455,
            },
        }
    }
}

impl OAuthProvider for CodexOAuthProvider {
    fn cli_key(&self) -> &'static str {
        "codex"
    }

    fn provider_type(&self) -> &'static str {
        "codex_oauth"
    }

    fn endpoints(&self) -> &OAuthEndpoints {
        &self.endpoints
    }

    fn default_base_url(&self) -> &'static str {
        "https://chatgpt.com/backend-api/codex"
    }

    fn extra_authorize_params(&self) -> Vec<(&'static str, &'static str)> {
        vec![
            ("id_token_add_organizations", "true"),
            ("codex_cli_simplified_flow", "true"),
            ("originator", upstream_identity::CODEX_CLI_ORIGINATOR),
        ]
    }

    fn resolve_effective_token(
        &self,
        token_set: &OAuthTokenSet,
        stored_id_token: Option<&str>,
    ) -> (String, Option<String>) {
        // Store the raw access_token as the effective token (used for Bearer auth and limits queries).
        // The id_token is stored separately for extracting chatgpt-account-id header.
        let id_token = token_set
            .id_token
            .as_deref()
            .or(stored_id_token)
            .filter(|v| !v.trim().is_empty())
            .map(str::to_string);
        (token_set.access_token.clone(), id_token)
    }

    fn inject_upstream_headers(
        &self,
        headers: &mut HeaderMap,
        access_token: &str,
    ) -> Result<(), String> {
        insert_bearer_auth(headers, access_token, "codex oauth")?;
        headers.insert(
            "originator",
            HeaderValue::from_static(upstream_identity::CODEX_CLI_ORIGINATOR),
        );
        Ok(())
    }

    fn inject_model_discovery_headers(
        &self,
        headers: &mut HeaderMap,
        access_token: &str,
        id_token: Option<&str>,
        client_version: Option<&str>,
    ) -> Result<(), String> {
        self.inject_upstream_headers(headers, access_token)?;
        let version = codex_model_discovery_version(client_version);
        headers.insert(
            axum::http::header::USER_AGENT,
            HeaderValue::from_str(&format!(
                "{}/{version}",
                upstream_identity::CODEX_CLI_ORIGINATOR
            ))
            .map_err(|_| "codex oauth: invalid discovery user agent".to_string())?,
        );
        headers.insert(
            "version",
            HeaderValue::from_str(version)
                .map_err(|_| "codex oauth: invalid discovery version".to_string())?,
        );
        if let Some(account_id) = parse_chatgpt_account_id(id_token) {
            if let Ok(value) = HeaderValue::from_str(&account_id) {
                headers.insert("chatgpt-account-id", value);
            }
        }
        Ok(())
    }

    fn fetch_limits(
        &self,
        client: &reqwest::Client,
        access_token: &str,
    ) -> Pin<Box<dyn Future<Output = Result<OAuthLimitsResult, String>> + Send + '_>> {
        let token = access_token.to_string();
        let client = client.clone();
        Box::pin(async move {
            let json = fetch_codex_usage_payload(
                &client,
                CODEX_USAGE_URL,
                &token,
                parse_chatgpt_account_id(Some(&token)).as_deref(),
            )
            .await?;

            Ok(OAuthLimitsResult {
                raw_json: Some(json),
                ..Default::default()
            })
        })
    }
}

pub(crate) const CODEX_USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";

pub(crate) fn apply_codex_quota_headers(
    request: reqwest::RequestBuilder,
    access_token: &str,
    chatgpt_account_id: Option<&str>,
) -> reqwest::RequestBuilder {
    let request = request
        .header("authorization", format!("Bearer {access_token}"))
        .header("accept", "application/json")
        .header("content-type", "application/json")
        .header("oai-language", "zh-CN")
        .header("originator", upstream_identity::CODEX_CLI_ORIGINATOR)
        .header("user-agent", upstream_identity::CODEX_CLI_USER_AGENT);
    match chatgpt_account_id
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        Some(id) => request.header("chatgpt-account-id", id),
        None => request,
    }
}

pub(crate) async fn fetch_codex_usage_payload(
    client: &reqwest::Client,
    usage_url: &str,
    access_token: &str,
    chatgpt_account_id: Option<&str>,
) -> Result<serde_json::Value, String> {
    let response =
        apply_codex_quota_headers(client.get(usage_url), access_token, chatgpt_account_id)
            .send()
            .await
            .map_err(|e| format!("codex usage fetch failed: {e}"))?;
    if !response.status().is_success() {
        let status = response.status();
        let text = read_text_with_limit(response, 64 * 1024, "codex usage")
            .await
            .unwrap_or_default();
        return Err(format!("codex usage fetch status: {status} - {text}"));
    }
    let body = read_text_with_limit(response, CODEX_LIMITS_RESPONSE_BODY_LIMIT, "codex usage")
        .await
        .map_err(|e| format!("codex usage body read failed: {e}"))?;
    serde_json::from_str(&body).map_err(|e| format!("codex usage parse failed: {e}"))
}

pub(crate) fn parse_chatgpt_account_id(id_token: Option<&str>) -> Option<String> {
    let token = id_token.map(str::trim).filter(|value| !value.is_empty())?;
    let payload_part = token.split('.').nth(1)?;
    // RFC 7515 JWT segments are unpadded base64url; NO_PAD rejects padded input,
    // so there is no fallback worth attempting.
    let payload = URL_SAFE_NO_PAD.decode(payload_part).ok()?;
    let json: serde_json::Value = serde_json::from_slice(&payload).ok()?;
    json.get("https://api.openai.com/auth")
        .and_then(|value| value.get("chatgpt_account_id"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::header;

    #[test]
    fn authorize_params_use_centralized_originator() {
        let provider = CodexOAuthProvider::new();

        assert!(provider
            .extra_authorize_params()
            .contains(&("originator", upstream_identity::CODEX_CLI_ORIGINATOR)));
    }

    #[test]
    fn inject_upstream_headers_uses_centralized_originator() {
        let provider = CodexOAuthProvider::new();
        let mut headers = HeaderMap::new();

        provider
            .inject_upstream_headers(&mut headers, "access-token")
            .expect("inject headers");

        assert_eq!(
            headers
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok()),
            Some("Bearer access-token")
        );
        assert_eq!(
            headers.get("originator").and_then(|v| v.to_str().ok()),
            Some(upstream_identity::CODEX_CLI_ORIGINATOR)
        );
    }

    #[test]
    fn inject_model_discovery_headers_uses_id_token_account_id() {
        let provider = CodexOAuthProvider::new();
        let payload = URL_SAFE_NO_PAD
            .encode(br#"{"https://api.openai.com/auth":{"chatgpt_account_id":"account-new"}}"#);
        let id_token = format!("header.{payload}.signature");
        let mut headers = HeaderMap::new();

        provider
            .inject_model_discovery_headers(&mut headers, "new-access", Some(&id_token), None)
            .expect("inject discovery headers");

        assert_eq!(
            headers
                .get(header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer new-access")
        );
        assert_eq!(
            headers
                .get("chatgpt-account-id")
                .and_then(|value| value.to_str().ok()),
            Some("account-new")
        );
        assert_eq!(
            headers
                .get(header::USER_AGENT)
                .and_then(|value| value.to_str().ok()),
            Some("codex_cli_rs/0.144.4")
        );
        assert_eq!(
            headers.get("version").and_then(|value| value.to_str().ok()),
            Some("0.144.4")
        );
    }
}
