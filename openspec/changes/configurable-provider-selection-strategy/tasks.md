# 实施任务

状态：`awaiting-plan-approval`

本变更已创建并完成方案记录，但尚未获得实施批准。不得在本任务完成前修改生产代码。

### 切片 1：AC-001 / AC-006 - 设置契约与运行时快照

- [ ] 1.1 完成设置契约、旧配置兼容、运行时策略快照与 AC-001 / AC-006 验证。

- 行为：新增 `provider_selection_strategy`，支持 `ordered` 与 `session_preferred`；旧配置默认 `session_preferred`；保存、重启读取和请求级快照正确。
- 代码边界：`src-tauri/src/infra/settings/types.rs`、`defaults.rs`、`migration.rs`、设置 service 的 `SettingsUpdate`/`SettingsView`、`src-tauri/src/gateway/proxy/handler/runtime_settings.rs`、`src/services/settings/settings.ts`、`src/pages/settings/settingsPersistenceModel.ts`、生成 bindings。
- 测试接缝：现有 settings CRUD、migration、settings persistence model 和 runtime settings 测试。
- RED：先为旧配置默认、非法值、字段往返和请求快照写测试，确认新字段尚不存在时失败。
- 实现：沿现有 enum、serde default、schema migration、settings mapping 和 `HandlerRuntimeSettings` 模式接入，不手工编辑 generated bindings。
- GREEN：`pnpm test:unit -- ...`（使用仓库实际测试筛选方式）及 `cd src-tauri && cargo test settings` 相关命令通过。
- 验证：`pnpm tauri:gen-types`、`pnpm check:generated-bindings`、相关 Vitest、`pnpm typecheck`、`cd src-tauri && cargo test --locked` 或受影响测试。
- 依赖：无；但开始实现前必须获得方案批准。
- 回滚：将设置字段、迁移版本、运行时字段和生成绑定一起移除或将策略固定回 `session_preferred`；不得留下版本不匹配。

### 切片 2：AC-002 / AC-003 / AC-004 / AC-005 - 候选顺序策略

- [ ] 2.1 实现 ordered / session_preferred 候选策略并完成 AC-002 / AC-003 / AC-004 / AC-005 回归验证。

- 行为：`ordered` 不旋转会话候选，模型过滤后的第一项优先；`session_preferred` 保持现有旋转与过滤行为；额度、冷却、熔断和 failover 不变。
- 代码边界：`src-tauri/src/gateway/proxy/handler/provider_selection.rs`、`provider_order.rs`、`middleware/provider_resolution.rs`、`failover_loop/prepare/provider_iterator.rs`（仅在需要传递策略时）。
- 测试接缝：`provider_selection` 与 `provider_order` 现有单元测试，以及 failover candidate 相关测试。
- RED：增加 A/B/C 顺序、A 恢复、GPT→DeepSeek、会话优先兼容、受限供应商跳过和单次失败预算测试。
- 实现：在普通 session preference 层按枚举分支；不得把 ordered 逻辑复制到 gate 或另造调度器。
- GREEN：目标 Rust 单元测试通过，顺序和 session preference 测试均覆盖。
- 验证：`cd src-tauri && cargo fmt -- --check`、受影响 `cargo test`、`pnpm lint`（如前端未改可记录范围）。
- 依赖：切片 1。
- 回滚：策略分支去除后默认调用既有 session preference；保留候选过滤和 gate 代码。

### 切片 3：AC-007 / AC-008 - Codex 续接与恢复


- [ ] 3.1 保持 Codex 续接归属与恢复预算约束并完成 AC-007 / AC-008 验证。
- 行为：顺序优先不跨越经校验的 Codex 上游上下文归属；安全恢复只使用剩余候选和原有预算。
- 代码边界：`src-tauri/src/gateway/responses_ws/send.rs`、`src-tauri/src/gateway/responses_ws/integration_tests.rs`、必要时 `failover_loop/attempt` 与 `failover_loop/mod.rs` 的策略传递。
- 测试接缝：现有 `ws_incremental_continuation_logs_session_reuse`、`ws_without_recovery_metadata_cannot_replay_after_context_loss`、`full_input_recovery_after_attempt_deadline_uses_remaining_provider_budget`、`recovered_budget_does_not_cache_global_unavailability` 及相邻测试。
- RED：补充 ordered 下 B 上下文不发给 A、无效上下文返回 `ContextLost`、恢复不复活失败 provider 和不重置 budget 的测试。
- 实现：复用现有 provider_id/response_id 校验与 generation budget；只改变允许完整重发时的候选排序。
- GREEN：相关 `cargo test` 与 responses WebSocket 集成测试通过。
- 验证：`cd src-tauri && cargo test --locked` 相关目标，必要时按仓库实际集成测试脚本运行；保存完整失败与恢复证据。
- 依赖：切片 2。
- 回滚：恢复原上下文归属分支；策略仅影响普通可独立选路请求。

### 切片 4：AC-009 - 设置界面与诊断


- [ ] 4.1 增加设置界面、保存反馈和诊断说明并完成 AC-009 验证。
- 行为：通用设置提供“顺序优先 / 会话优先”，保存失败反馈正确，诊断能区分策略与上下文约束。
- 代码边界：`src/components/cli-manager/tabs/GeneralTab.tsx`、其测试、`src/pages/settings` persistence 测试、必要的诊断类型或文案文件。
- 测试接缝：`src/components/cli-manager/tabs/__tests__/GeneralTab.test.tsx`、`src/pages/settings/__tests__/settingsPersistenceModel.test.ts`、`useSettingsPersistence.test.tsx`。
- RED：补充选择、默认值、保存错误和读回断言。
- 实现：复用现有 Select/Radio 控件、mutation 和错误提示；不记录敏感请求内容。
- GREEN：目标 Vitest 通过。
- 验证：`pnpm typecheck`、`pnpm lint`、`pnpm test:unit` 受影响范围、`pnpm build`。
- 依赖：切片 1；诊断文案可与切片 2/3 合并但不扩大路由职责。
- 回滚：隐藏设置项并将默认值固定为 session preferred，后端兼容字段仍可安全读取。

### 切片 5：完整验证、独立审查与交付记录

- [ ] 5.1 执行 critical profile 的完整验证、唯一独立审查并写入交付证据。

- 行为：所有 AC 有执行证据；迁移、路由、Codex 恢复和 UI 检查通过。
- 代码边界：只修复前述切片暴露的问题；审查记录写入本 change 的 `artifacts/`。
- 测试接缝：完整相关 Rust/TypeScript 测试、生成绑定检查、格式/lint/typecheck/build。
- RED：不适用；先运行现有基线检查并记录失败。
- 实现：主会话修复独立 reviewer 发现的问题；材料变化后重新运行受影响检查和唯一 final review。
- GREEN：仓库相关检查通过；独立 `workflow-reviewer` 返回 `REVIEW_STATUS: CLEAN`。
- 验证：按 critical profile 执行集成覆盖、完整相关 suite、回滚配置验证，并用 `finish-with-evidence` 写 `artifacts/verification.md`。
- 依赖：切片 1–4；用户验收前保持 `awaiting-acceptance`，不得自动归档。
- 回滚：验证失败保持未完成状态，记录失败证据并回到对应切片。

## 方案批准门

用户批准 `design.md` 与本 `tasks.md` 后，才可将任务状态改为 `in_progress` 并开始切片 1。当前状态为 `awaiting-plan-approval`。
