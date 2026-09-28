//! Usage: Classify the local client that sent a request using standard
//! protocol headers already present on official harness requests.
//!
//! This is a passive source label, not a security boundary. Clients can spoof
//! headers; unknown or ambiguous requests intentionally fall back to "unknown".

use axum::http::{header, HeaderMap};

pub(crate) const CLIENT_IDENTITY_UNKNOWN: &str = "unknown";

const CLIENT_IDENTITY_MAX_HEADER_CHARS: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RequestClient {
    Pi,
    Omp,
    Codex,
    OpenCode,
    ClaudeCode,
    Unknown,
}

impl RequestClient {
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Pi => "pi",
            Self::Omp => "omp",
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
            Self::ClaudeCode => "claude-code",
            Self::Unknown => CLIENT_IDENTITY_UNKNOWN,
        }
    }
}

fn header_value_bounded(headers: &HeaderMap, name: &'static str) -> Option<String> {
    let value = headers.get(name)?.to_str().ok()?.trim();
    if value.is_empty() {
        return None;
    }
    let mut bounded = String::new();
    for ch in value.chars().take(CLIENT_IDENTITY_MAX_HEADER_CHARS) {
        bounded.push(ch);
    }
    Some(bounded)
}

fn user_agent_starts_with(user_agent: Option<&str>, prefix: &str) -> bool {
    user_agent
        .map(|value| value.to_ascii_lowercase().starts_with(prefix))
        .unwrap_or(false)
}

fn user_agent_contains(user_agent: Option<&str>, needle: &str) -> bool {
    user_agent
        .map(|value| value.to_ascii_lowercase().contains(needle))
        .unwrap_or(false)
}

fn header_eq(value: Option<&str>, expected: &str) -> bool {
    value
        .map(|value| value.eq_ignore_ascii_case(expected))
        .unwrap_or(false)
}

pub(crate) fn classify_request_client(headers: &HeaderMap) -> RequestClient {
    let originator = header_value_bounded(headers, "originator");
    let user_agent = header_value_bounded(headers, header::USER_AGENT.as_str());
    let x_app = header_value_bounded(headers, "x-app");
    let has_opencode_session = headers.contains_key("x-opencode-session");
    let originator = originator.as_deref();
    let user_agent = user_agent.as_deref();
    let x_app = x_app.as_deref();

    if header_eq(originator, "omp") || user_agent_starts_with(user_agent, "omp/") {
        return RequestClient::Omp;
    }

    if header_eq(originator, "pi") || user_agent_starts_with(user_agent, "pi (") {
        return RequestClient::Pi;
    }

    if header_eq(originator, "codex_cli_rs") || user_agent_starts_with(user_agent, "codex_cli_rs/")
    {
        return RequestClient::Codex;
    }

    if user_agent_starts_with(user_agent, "claude-cli/") && header_eq(x_app, "cli") {
        return RequestClient::ClaudeCode;
    }

    if has_opencode_session || user_agent_contains(user_agent, "opencode") {
        return RequestClient::OpenCode;
    }

    RequestClient::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers(entries: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in entries {
            headers.insert(*name, HeaderValue::from_static(value));
        }
        headers
    }

    #[test]
    fn recognizes_omp_by_originator_or_user_agent() {
        assert_eq!(
            classify_request_client(&headers(&[("originator", "omp")])),
            RequestClient::Omp
        );
        assert_eq!(
            classify_request_client(&headers(&[("user-agent", "omp/18.2.11")])),
            RequestClient::Omp
        );
    }

    #[test]
    fn recognizes_pi_by_originator_or_user_agent() {
        assert_eq!(
            classify_request_client(&headers(&[("originator", "pi")])),
            RequestClient::Pi
        );
        assert_eq!(
            classify_request_client(&headers(&[("user-agent", "pi (win32 10.0.0; x64)")])),
            RequestClient::Pi
        );
    }

    #[test]
    fn recognizes_codex_by_originator_or_user_agent() {
        assert_eq!(
            classify_request_client(&headers(&[("originator", "codex_cli_rs")])),
            RequestClient::Codex
        );
        assert_eq!(
            classify_request_client(&headers(&[("user-agent", "codex_cli_rs/0.155.1")])),
            RequestClient::Codex
        );
    }

    #[test]
    fn recognizes_claude_code_by_user_agent_and_x_app() {
        assert_eq!(
            classify_request_client(&headers(&[
                ("user-agent", "claude-cli/2.1.278 (external, cli)"),
                ("x-app", "cli"),
            ])),
            RequestClient::ClaudeCode
        );
        assert_eq!(
            classify_request_client(&headers(&[("user-agent", "claude-cli/2.1.278")])),
            RequestClient::Unknown
        );
    }

    #[test]
    fn recognizes_opencode_by_session_header_or_user_agent() {
        assert_eq!(
            classify_request_client(&headers(&[("x-opencode-session", "session-1")])),
            RequestClient::OpenCode
        );
        assert_eq!(
            classify_request_client(&headers(&[("user-agent", "opencode-ai/1.18.31")])),
            RequestClient::OpenCode
        );
    }

    #[test]
    fn unknown_when_headers_are_missing_or_generic() {
        assert_eq!(
            classify_request_client(&HeaderMap::new()),
            RequestClient::Unknown
        );
        assert_eq!(
            classify_request_client(&headers(&[("user-agent", "node-fetch/1.0")])),
            RequestClient::Unknown
        );
    }
}
