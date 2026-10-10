//! Usage: Pure quota-exhaustion classification shared by HTTP and SSE parsing.

const MAX_SCAN_BYTES: usize = 64 * 1024;

pub(crate) fn is_quota_exhausted_code(code: &str) -> bool {
    matches!(
        code,
        "usage_limit_reached" | "insufficient_quota" | "quota_exhausted"
    )
}

/// Returns whether a response body clearly indicates account/provider quota exhaustion.
pub(crate) fn match_quota_exhausted(body: &[u8]) -> bool {
    if body.is_empty() {
        return false;
    }

    let scan = if body.len() > MAX_SCAN_BYTES {
        &body[..MAX_SCAN_BYTES]
    } else {
        body
    };

    if let Ok(payload) = serde_json::from_slice::<serde_json::Value>(scan) {
        let error = payload
            .pointer("/response/error")
            .or_else(|| payload.get("error"))
            .unwrap_or(&payload);
        let codes =
            ["code", "type"].map(|field| error.get(field).and_then(serde_json::Value::as_str));
        // A hard quota type can accompany a generic rate-limit code.
        if codes
            .iter()
            .flatten()
            .any(|code| is_quota_exhausted_code(code))
        {
            return true;
        }
        // The broad rate_limit_error type also represents exhausted quota;
        // only an explicit rate_limit_exceeded code excludes the text fallback.
        if codes.contains(&Some("rate_limit_exceeded")) {
            return false;
        }
    }

    let haystack_lower = String::from_utf8_lossy(scan).to_ascii_lowercase();

    haystack_lower.contains("usage_limit_reached")
        || haystack_lower.contains("insufficient_quota")
        || haystack_lower.contains("quota exhausted")
        || haystack_lower.contains("exceeded your current quota")
        || haystack_lower.contains("you exceeded your current quota")
        || (haystack_lower.contains("quota") && haystack_lower.contains("exceeded"))
        || (haystack_lower.contains("quota") && haystack_lower.contains("exhausted"))
        || (haystack_lower.contains("usage limit") && haystack_lower.contains("reached"))
}
