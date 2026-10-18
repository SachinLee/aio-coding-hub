# 修复 WS 合并编译错误

## Why
合并提交 `0c47377b` 将 personal 的 `continuation_provider_id()` 校验与 main 的 `require_http` 判断合并后，保留了对已删除局部变量 `continuation` 的引用。`cargo check --locked --offline --manifest-path src-tauri/Cargo.toml` 已复现 `send.rs:115` 的 E0425。

## What Changes
仅在 `send()` 返回 `ContextLost` 的分支中，使用已有 `lock_or_recover()` 读取 `connection.continuation`。保留续传供应商归属、预热响应与 HTTP 回退判断。

## Capabilities
### New Capabilities
- `responses-ws-merge-build`：合并后 Rust 编译与既有续传逻辑兼容。
### Modified Capabilities
- 无。

## Impact
生产修改仅限 `src-tauri/src/gateway/responses_ws/send.rs`；不新增依赖，不修改公共接口。

## 授权、范围与约束
用户已确认创建本任务并批准上述最小方案与实施。档位 `standard`。不提交、不打包、不触碰其他修改。验收标准以 `specs/responses-ws-merge-build/spec.md` 为准。完整打包与真实上游 E2E 不在本次验证范围。未授权独立子代理审查，缺口如实记录，不宣称 CLEAN。
