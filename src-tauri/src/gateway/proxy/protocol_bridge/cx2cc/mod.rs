//! CX2CC (Claude-to-ChatGPT-Compatible) protocol bridge helpers.
//!
//! - **Model mapping**: translates Claude model names to OpenAI-compatible names
//!   using per-provider `ClaudeModels` overrides (or sensible defaults).
//! - **ChatGPT backend compat**: filters and adjusts request bodies so they
//!   conform to the ChatGPT Responses API field whitelist.

use super::traits::{BridgeContext, ModelMapper};
use crate::domain::providers::ClaudeModels;
use crate::gateway::proxy::cx2cc::settings::Cx2ccSettings;
use serde_json::Value;

// ─── Model Mapper ──────────────────────────────────────────────────────────

/// Maps Claude model names to OpenAI-compatible model names.
///
/// The mapping logic mirrors `cx2cc::models::map_claude_to_openai` but is
/// expressed as a [`ModelMapper`] trait implementation so the protocol bridge
/// framework can use it generically.
pub(crate) struct CX2CCModelMapper;

impl ModelMapper for CX2CCModelMapper {
    fn map(&self, source_model: &str, ctx: &BridgeContext) -> String {
        map_claude_to_openai(source_model, &ctx.claude_models, &ctx.cx2cc_settings)
    }
}

fn map_claude_to_openai(source_model: &str, cm: &ClaudeModels, settings: &Cx2ccSettings) -> String {
    if source_model.contains("opus") {
        if let Some(ref m) = cm.opus_model {
            return m.clone();
        }
        return settings.fallback_model_opus.clone();
    }

    if source_model.contains("haiku") {
        if let Some(ref m) = cm.haiku_model {
            return m.clone();
        }
        return settings.fallback_model_haiku.clone();
    }

    if source_model.contains("sonnet") {
        if let Some(ref m) = cm.sonnet_model {
            return m.clone();
        }
        return settings.fallback_model_sonnet.clone();
    }

    if let Some(ref m) = cm.main_model {
        return m.clone();
    }
    settings.fallback_model_main.clone()
}

// ─── ChatGPT Backend Compat ────────────────────────────────────────────────

/// Field whitelist for the ChatGPT Responses API.
///
/// When routing a Codex CLI request through a ChatGPT-compatible backend, only
/// these top-level keys are forwarded; everything else is stripped to avoid
/// 400-level rejections from the upstream provider.
pub(crate) const CODEX_CHATGPT_RESPONSES_ALLOWED_KEYS: &[&str] = &[
    "model",
    "instructions",
    "input",
    "tools",
    "tool_choice",
    "parallel_tool_calls",
    "store",
    "stream",
    "include",
    "reasoning",
    "service_tier",
    "prompt_cache_key",
    "text",
    "previous_response_id",
];

/// Filter a request body to only the ChatGPT Responses API allowed keys, strip input item `status` and invalid reasoning IDs,
/// force `stream: true`, `store: false`, and coerce `instructions` to a string.
///
/// If `root` is not a JSON object it is returned unchanged.
pub(crate) fn codex_chatgpt_request_compat_value(root: &Value) -> Value {
    let Some(obj) = root.as_object() else {
        return root.clone();
    };

    let mut next = serde_json::Map::new();
    for key in CODEX_CHATGPT_RESPONSES_ALLOWED_KEYS {
        if let Some(value) = obj.get(*key).cloned() {
            next.insert((*key).to_string(), value);
        }
    }
    // Response output metadata is not accepted on replayed ChatGPT input items.
    // Only remove the item's own status; nested tool/business data must survive.
    if let Some(items) = next.get_mut("input").and_then(Value::as_array_mut) {
        for item in items {
            if let Some(item) = item.as_object_mut() {
                item.remove("status");
                // Foreign reasoning IDs (e.g. out_7) cannot identify ChatGPT items.
                // Omit invalid IDs rather than inventing server IDs; keep tool call IDs intact.
                if item.get("type").and_then(Value::as_str) == Some("reasoning")
                    && item
                        .get("id")
                        .is_some_and(|id| !id.as_str().is_some_and(|id| id.starts_with("rs")))
                {
                    item.remove("id");
                }
            }
        }
    }
    next.insert("stream".to_string(), Value::Bool(true));
    next.insert("store".to_string(), Value::Bool(false));
    let instructions_needs_coercion = next.get("instructions").and_then(Value::as_str).is_none();
    if instructions_needs_coercion {
        next.insert("instructions".to_string(), Value::String(String::new()));
    }
    Value::Object(next)
}

/// Returns `true` when the original Anthropic request body (captured before
/// CX2CC translation) had `"stream": true`.
///
/// `introspection_json` is the pre-translation snapshot of the request body
/// that the failover loop keeps for diagnostics / compat decisions.
pub(crate) fn original_anthropic_stream_requested(introspection_json: Option<&Value>) -> bool {
    introspection_json
        .and_then(|body| body.get("stream"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::providers::ClaudeModels;
    use serde_json::json;

    fn default_ctx() -> BridgeContext {
        BridgeContext {
            claude_models: ClaudeModels::default(),
            cx2cc_settings: Cx2ccSettings::default(),
            requested_model: None,
            mapped_model: None,
            stream_requested: false,
            is_chatgpt_backend: false,
        }
    }

    fn ctx_with_models(cm: ClaudeModels) -> BridgeContext {
        BridgeContext {
            claude_models: cm,
            ..default_ctx()
        }
    }

    // ── Model mapping: default values ──────────────────────────────────────

    #[test]
    fn maps_opus_to_default_o3() {
        let mapper = CX2CCModelMapper;
        assert_eq!(
            mapper.map("claude-3-opus-20240229", &default_ctx()),
            "gpt-5.4"
        );
    }

    #[test]
    fn maps_haiku_to_default_gpt41_mini() {
        let mapper = CX2CCModelMapper;
        assert_eq!(
            mapper.map("claude-3-haiku-20240307", &default_ctx()),
            "gpt-5.4"
        );
    }

    #[test]
    fn maps_sonnet_to_default_gpt41() {
        let mapper = CX2CCModelMapper;
        assert_eq!(
            mapper.map("claude-3-5-sonnet-20241022", &default_ctx()),
            "gpt-5.4"
        );
    }

    #[test]
    fn maps_unknown_model_to_default_gpt41() {
        let mapper = CX2CCModelMapper;
        assert_eq!(mapper.map("some-unknown-model", &default_ctx()), "gpt-5.4");
    }

    // ── Model mapping: custom overrides ────────────────────────────────────

    #[test]
    fn opus_override() {
        let cm = ClaudeModels {
            opus_model: Some("my-opus".into()),
            ..ClaudeModels::default()
        };
        let mapper = CX2CCModelMapper;
        assert_eq!(
            mapper.map("claude-3-opus-20240229", &ctx_with_models(cm)),
            "my-opus"
        );
    }

    #[test]
    fn haiku_override() {
        let cm = ClaudeModels {
            haiku_model: Some("my-haiku".into()),
            ..ClaudeModels::default()
        };
        let mapper = CX2CCModelMapper;
        assert_eq!(
            mapper.map("claude-3-haiku-20240307", &ctx_with_models(cm)),
            "my-haiku"
        );
    }

    #[test]
    fn sonnet_override() {
        let cm = ClaudeModels {
            sonnet_model: Some("my-sonnet".into()),
            ..ClaudeModels::default()
        };
        let mapper = CX2CCModelMapper;
        assert_eq!(
            mapper.map("claude-3-5-sonnet-20241022", &ctx_with_models(cm)),
            "my-sonnet"
        );
    }

    #[test]
    fn main_model_override_for_unknown() {
        let cm = ClaudeModels {
            main_model: Some("my-main".into()),
            ..ClaudeModels::default()
        };
        let mapper = CX2CCModelMapper;
        assert_eq!(
            mapper.map("some-unknown-model", &ctx_with_models(cm)),
            "my-main"
        );
    }

    #[test]
    fn runtime_settings_override_fallbacks() {
        let mut ctx = default_ctx();
        ctx.cx2cc_settings = Cx2ccSettings {
            fallback_model_opus: "custom-opus".into(),
            fallback_model_sonnet: "custom-sonnet".into(),
            fallback_model_haiku: "custom-haiku".into(),
            fallback_model_main: "custom-main".into(),
            ..Cx2ccSettings::default()
        };

        let mapper = CX2CCModelMapper;
        assert_eq!(mapper.map("claude-3-opus-20240229", &ctx), "custom-opus");
        assert_eq!(mapper.map("claude-3-haiku-20240307", &ctx), "custom-haiku");
        assert_eq!(
            mapper.map("claude-3-5-sonnet-20241022", &ctx),
            "custom-sonnet"
        );
        assert_eq!(mapper.map("some-unknown-model", &ctx), "custom-main");
    }

    // ── ChatGPT compat filter ──────────────────────────────────────────────

    #[test]
    fn compat_keeps_allowed_keys_only() {
        let root = json!({
            "model": "gpt-5",
            "instructions": "system prompt",
            "input": "hello",
            "temperature": 0.7,
            "extra_field": true,
        });
        let next = codex_chatgpt_request_compat_value(&root);

        assert_eq!(next["model"], "gpt-5");
        assert_eq!(next["instructions"], "system prompt");
        assert_eq!(next["input"], "hello");
        // Stripped fields
        assert!(next.get("temperature").is_none());
        assert!(next.get("extra_field").is_none());
    }

    #[test]
    fn compat_strips_input_item_status_without_changing_history_or_cache_context() {
        let root = json!({
            "model": "gpt-5",
            "instructions": "system prompt",
            "prompt_cache_key": "same-session",
            "previous_response_id": "resp_previous",
            "tools": [{"type": "function", "name": "check", "parameters": {
                "type": "object", "properties": {"status": {"type": "string"}}
            }}],
            "input": [
                {"type": "message", "role": "assistant", "id": "msg_pi_19",
                 "status": "completed", "content": [{"type": "output_text",
                    "text": "之前的回答", "annotations": []}]},
                {"type": "reasoning", "id": "rs_1", "status": "completed",
                 "summary": [], "encrypted_content": "opaque-reasoning"},
                {"type": "function_call", "id": "fc_1", "call_id": "call_1",
                 "status": "completed", "name": "check", "arguments": "{\"status\":\"pending\"}"},
                {"type": "function_call_output", "call_id": "call_1",
                 "status": "completed", "output": "{\"status\":\"ok\"}"}
            ]
        });
        let original = root.clone();
        let next = codex_chatgpt_request_compat_value(&root);

        for key in [
            "model",
            "instructions",
            "prompt_cache_key",
            "previous_response_id",
            "tools",
        ] {
            assert_eq!(next[key], original[key], "changed {key}");
        }
        let actual = next["input"].as_array().unwrap();
        let expected = original["input"].as_array().unwrap();
        assert_eq!(actual.len(), expected.len());
        for (index, (item, source)) in actual.iter().zip(expected).enumerate() {
            let item = item.as_object().unwrap();
            let source = source.as_object().unwrap();
            assert!(
                !item.contains_key("status"),
                "input[{index}].status must be removed"
            );
            assert_eq!(item.len(), source.len() - 1);
            for (key, value) in source.iter().filter(|(key, _)| key.as_str() != "status") {
                assert_eq!(item.get(key), Some(value), "changed input[{index}].{key}");
            }
        }
        assert_eq!(root, original);
        assert_eq!(codex_chatgpt_request_compat_value(&next), next);
    }

    #[test]
    fn compat_omits_only_invalid_reasoning_ids() {
        for (id, keep_id) in [
            (Some(json!("out_7")), false),
            (Some(json!("")), false),
            (Some(json!("RS_7")), false),
            (Some(json!(null)), false),
            (Some(json!(7)), false),
            (Some(json!(true)), false),
            (Some(json!({"id": "rs_nested"})), false),
            (Some(json!(["rs_nested"])), false),
            (Some(json!("rs_7")), true),
            (Some(json!("rs7")), true),
            (Some(json!("rs")), true),
            (None, false),
        ] {
            let mut reasoning = json!({
                "type": "reasoning", "status": "completed",
                "summary": [{"type": "summary_text", "text": "历史推理摘要"}],
                "encrypted_content": "opaque-reasoning",
                "metadata": {"id": "out_nested", "status": "business-data"}
            });
            if let Some(id) = id {
                reasoning["id"] = id;
            }
            let mut input = vec![json!({"role": "user", "content": "历史消息"}); 46];
            input.extend([
                reasoning,
                json!({"type": "message", "id": "out_message", "role": "assistant",
                    "content": [{"type": "output_text", "text": "回答"}]}),
                json!({"type": "function_call", "id": "out_call", "call_id": "call_7",
                    "name": "check", "arguments": "{\"id\":\"out_arg\"}"}),
                json!({"type": "function_call_output", "id": "out_result",
                    "call_id": "call_7", "output": {"id": "out_business", "status": "ok"}}),
                json!({"id": "out_untyped"}),
            ]);
            let root = json!({
                "model": "gpt-5", "instructions": "system prompt", "input": input,
                "prompt_cache_key": "same-session", "previous_response_id": "resp_previous"
            });
            let original = root.clone();
            let next = codex_chatgpt_request_compat_value(&root);
            let mut expected_input = original["input"].clone();
            let expected_reasoning = expected_input[46].as_object_mut().unwrap();
            expected_reasoning.remove("status");
            if !keep_id {
                expected_reasoning.remove("id");
            }
            assert_eq!(
                next["input"], expected_input,
                "only invalid reasoning ID and item status may be omitted"
            );
            for key in [
                "model",
                "instructions",
                "prompt_cache_key",
                "previous_response_id",
            ] {
                assert_eq!(next[key], original[key], "changed {key}");
            }
            assert_eq!(root, original);
            assert_eq!(codex_chatgpt_request_compat_value(&next), next);
        }
    }

    #[test]
    fn compat_preserves_nested_status_and_non_object_input_items() {
        let root = json!({"input": [
            {"status": "completed", "output": {"status": "business-data"}},
            null, "unchanged", 42, [{"status": "nested-array-data"}]
        ]});
        let next = codex_chatgpt_request_compat_value(&root);
        assert_eq!(
            next["input"],
            json!([
                {"output": {"status": "business-data"}},
                null, "unchanged", 42, [{"status": "nested-array-data"}]
            ])
        );
    }

    #[test]
    fn compat_preserves_non_array_and_missing_input() {
        for input in [json!("hello"), json!(null), json!({"status": "keep"})] {
            let root = json!({"input": input});
            assert_eq!(codex_chatgpt_request_compat_value(&root)["input"], input);
        }
        assert!(codex_chatgpt_request_compat_value(&json!({}))
            .get("input")
            .is_none());
    }

    #[test]
    fn compat_forces_stream_true_and_store_false() {
        let root = json!({
            "model": "gpt-5",
            "stream": false,
            "store": true,
        });
        let next = codex_chatgpt_request_compat_value(&root);

        assert_eq!(next["stream"], true);
        assert_eq!(next["store"], false);
    }

    #[test]
    fn compat_injects_empty_instructions_when_missing() {
        let root = json!({
            "model": "gpt-5",
            "input": "hello"
        });

        let next = codex_chatgpt_request_compat_value(&root);

        assert_eq!(next["stream"], true);
        assert_eq!(next["store"], false);
        assert_eq!(next["instructions"], "");
    }

    #[test]
    fn compat_coerces_null_instructions_to_empty_string() {
        let root = json!({
            "model": "gpt-5",
            "input": "hello",
            "instructions": null
        });

        let next = codex_chatgpt_request_compat_value(&root);

        assert_eq!(next["stream"], true);
        assert_eq!(next["store"], false);
        assert_eq!(next["instructions"], "");
    }

    #[test]
    fn compat_returns_non_object_unchanged() {
        let root = json!("just a string");
        let next = codex_chatgpt_request_compat_value(&root);
        assert_eq!(next, root);
    }

    // ── original_anthropic_stream_requested ────────────────────────────────

    #[test]
    fn detects_stream_true() {
        let body = json!({"stream": true, "model": "claude-3"});
        assert!(original_anthropic_stream_requested(Some(&body)));
    }

    #[test]
    fn detects_stream_false() {
        let body = json!({"stream": false, "model": "claude-3"});
        assert!(!original_anthropic_stream_requested(Some(&body)));
    }

    #[test]
    fn returns_false_when_no_stream_field() {
        let body = json!({"model": "claude-3"});
        assert!(!original_anthropic_stream_requested(Some(&body)));
    }

    #[test]
    fn returns_false_when_none() {
        assert!(!original_anthropic_stream_requested(None));
    }
}
