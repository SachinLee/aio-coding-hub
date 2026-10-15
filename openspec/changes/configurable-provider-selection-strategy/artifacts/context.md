# 上下文包

## 当前状态

- Active change: `openspec/changes/configurable-provider-selection-strategy/`
- 阶段：方案已记录，等待用户批准；不可开始实现。
- 质量档位：`critical`（持久化设置、IPC/generated contract、跨层路由与 Codex 上下文数据完整性）。
- 用户已明确确认“初始化并创建”，未明确批准 `design.md` 或授权修改生产代码。

## 已确认的事实

- `src-tauri/src/gateway/proxy/handler/provider_selection.rs`：`resolve_session_bound_provider_id` 读取 session binding；`apply_session_reuse_provider_binding` 当前在 `allow_session_reuse` 且有 bound provider 时调用 `provider_order::apply_session_provider_preference`。该调用会旋转候选列表。
- `src-tauri/src/gateway/proxy/handler/provider_order.rs`：已有 `apply_session_provider_preference` 及其顺序测试；ordered 模式应复用基础顺序而不复制 gate。
- `src-tauri/src/gateway/proxy/handler/middleware/runtime_settings_reader.rs`：每个请求读取 `settings::read`，然后调用 `handler_runtime_settings`，适合作为策略快照边界。
- `src-tauri/src/gateway/proxy/handler/runtime_settings.rs`：`HandlerRuntimeSettings` 集中承载运行时开关和 failover 设置；新增策略字段应从 `AppSettings` 映射到这里。
- `src-tauri/src/gateway/responses_ws/send.rs`：解析 `previous_response_id`，比较当前 `provider_id` 与 generation/warm context 的归属；不匹配返回 `SendOutcome::ContextLost`。ordered 不得绕过该检查。
- `src-tauri/src/gateway/responses_ws/integration_tests.rs`：已有续接和恢复测试，包括 `ws_incremental_continuation_logs_session_reuse`、`ws_without_recovery_metadata_cannot_replay_after_context_loss`、`full_input_recovery_after_attempt_deadline_uses_remaining_provider_budget`、`recovered_budget_does_not_cache_global_unavailability`。
- `src-tauri/src/infra/settings/types.rs`、`defaults.rs`、`migration.rs` 与现有设置 service 是持久化字段、schema migration 和 IPC 的真实入口；前端 `src/services/settings/settings.ts`、`src/pages/settings/settingsPersistenceModel.ts` 与 `src/components/cli-manager/tabs/GeneralTab.tsx` 是对应 UI/持久化入口。
- `src/generated/bindings.ts` 必须由 `pnpm tauri:gen-types` 生成，并由 `pnpm check:generated-bindings` 校验，不能手工编辑。

## 规范与设计关键点

- 规范文件：`specs/provider-selection-strategy/spec.md`，AC-001 至 AC-009 必须全部覆盖。
- `ordered` 仅改变普通会话偏好层：模型过滤后保持基础顺序；不改变额度、冷却、熔断、显式 provider 校验、failover budget 或 Codex 上下文归属。
- `session_preferred` 是默认和旧配置回退，保持已有行为。
- 设置成功后下一次新请求读取新策略；在途请求和单次请求重试使用同一策略快照。
- Codex 依赖已校验上游上下文时保持原 provider；只有安全完整重发时才对剩余候选按策略选路，不重置 generation 的失败/访问集合、次数、deadline。
- 不新增依赖、数据库表、余额后台轮询或新的调度服务；不改变 Codex/Claude 客户端分组。

## 实施时允许读取的路径

- `openspec/changes/configurable-provider-selection-strategy/proposal.md`
- `openspec/changes/configurable-provider-selection-strategy/specs/provider-selection-strategy/spec.md`
- `openspec/changes/configurable-provider-selection-strategy/design.md`
- `openspec/changes/configurable-provider-selection-strategy/tasks.md`
- `src-tauri/src/infra/settings/types.rs`
- `src-tauri/src/infra/settings/defaults.rs`
- `src-tauri/src/infra/settings/migration.rs`
- 设置 service 中实际定义 `SettingsUpdate` / `SettingsView` 的文件
- `src-tauri/src/gateway/proxy/handler/runtime_settings.rs`
- `src-tauri/src/gateway/proxy/handler/middleware/runtime_settings_reader.rs`
- `src-tauri/src/gateway/proxy/handler/middleware/provider_resolution.rs`
- `src-tauri/src/gateway/proxy/handler/provider_selection.rs`
- `src-tauri/src/gateway/proxy/handler/provider_order.rs`
- `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/provider_iterator.rs`
- `src-tauri/src/gateway/proxy/handler/provider_selection/tests.rs`
- `src-tauri/src/gateway/responses_ws/send.rs`
- `src-tauri/src/gateway/responses_ws/integration_tests.rs`
- `src/services/settings/settings.ts`
- `src/pages/settings/settingsPersistenceModel.ts`
- `src/pages/settings/__tests__/settingsPersistenceModel.test.ts`
- `src/pages/settings/__tests__/useSettingsPersistence.test.tsx`
- `src/components/cli-manager/tabs/GeneralTab.tsx`
- `src/components/cli-manager/tabs/__tests__/GeneralTab.test.tsx`
- `package.json`

## 精确验证命令

实施后按实际受影响范围执行并保存输出：

```text
pnpm tauri:gen-types
pnpm check:generated-bindings
pnpm typecheck
pnpm lint
pnpm test:unit
pnpm build
cd src-tauri && cargo fmt -- --check
cd src-tauri && cargo test --locked
```

关键测试必须覆盖 settings migration/persistence、provider order/selection、Codex responses WebSocket continuation/recovery。若完整命令在本机成本过高，仍需执行所有能证明 AC 的最小命令，并在 `artifacts/verification.md` 标明 `NOT RUN` 与原因；critical 交付不得以未验证替代 `REVIEW_STATUS: CLEAN`。

## 派发约束

- 方案批准前不得派发 implementer，也不得修改生产代码。
- critical 实现如需派发，handoff 必须以 `Active change: openspec/changes/configurable-provider-selection-strategy/` 和 `Assigned slice: Slice N / AC-XXX` 开头，并包含 `Phase`、`Read`、`Must preserve`、`May modify`、`Verification`。
- 每次只允许一个 writer；主会话拥有集成、修复、验收和交付声明。
- 实现完成后使用唯一独立 `workflow-reviewer` 做 Standards、Spec、correctness、security、tests、complexity 审查；材料变化后重新检查和审查。
- 最终证据写入本 change 的 `artifacts/verification.md`，状态到 `awaiting-acceptance`；用户明确验收并要求归档前不得移动到 archive。
