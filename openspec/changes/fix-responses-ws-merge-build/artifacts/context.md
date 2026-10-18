# 上下文

- src-tauri/src/gateway/responses_ws/send.rs — send() 的归属检查与 ContextLost.require_http 分支；唯一生产改动范围。
- src-tauri/src/gateway/responses_ws/state.rs — continuation_provider_id() 在内部获取 continuation 锁，调用后才可再次取锁。
- src-tauri/src/gateway/proxy/handler/failover_loop/attempt/attempt_executor.rs — send() 唯一生产调用点，传递 require_http。
- src-tauri/src/gateway/proxy/handler/failover_loop/attempt/retry_engine.rs — 将 ContextLost 转交 recover()。
- src-tauri/src/gateway/proxy/handler/failover_loop/attempt/ws_attempt.rs — recover() 消费 require_http，原有行为保持不变。
- openspec/changes/fix-responses-ws-merge-build/specs/responses-ws-merge-build/spec.md — 本任务 AC-001 / AC-002。
- openspec/changes/fix-responses-ws-merge-build/design.md — 一行分支局部锁方案与禁止锁重入约束。
