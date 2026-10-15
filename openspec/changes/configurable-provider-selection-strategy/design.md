# 技术设计

## 背景与现状

当前 `src-tauri/src/gateway/proxy/handler/provider_selection.rs` 的 `resolve_session_bound_provider_id` 读取会话绑定供应商，并通过 `apply_session_reuse_provider_binding` 调用 `provider_order::apply_session_provider_preference` 旋转候选列表。这样会话黏性覆盖分组基础顺序：A 不可用时切到 B，A 恢复后仍可能从 B 开始；模型从 GPT 切换到 DeepSeek 时，也可能从旧供应商后面的 C 开始。

`src-tauri/src/gateway/proxy/handler/middleware/runtime_settings_reader.rs` 先读取一次 `AppSettings`，`runtime_settings.rs` 将其转换为 `HandlerRuntimeSettings`，因此策略可以作为同一请求的不可变运行时快照传入解析链路。`src-tauri/src/gateway/responses_ws/send.rs` 负责校验 Codex `previous_response_id` 的供应商归属，不能让排序策略绕过该约束。

## 方案与改动边界

新增 Rust 枚举 `ProviderSelectionStrategy`，序列化值固定为 `ordered` 和 `session_preferred`。建议归属 `src-tauri/src/infra/settings/types.rs` 或与现有设置枚举相同的设置模块，并通过 `specta` 暴露到 TypeScript。

`AppSettings` 增加带 serde 默认函数的字段；schema 版本沿现有 `defaults.rs` 与 `migration.rs` 规则递增。设置读写、`SettingsUpdate`、`SettingsView`、前端 `PersistedSettings`、默认快照、更新字段映射和验证均增加同一字段。旧配置缺失字段解析为 `session_preferred`；非法值在反序列化或更新校验时失败。

`HandlerRuntimeSettings` 增加策略字段。`provider_resolution.rs` 将策略传给 `resolve_session_bound_provider_id` 或等价的会话路由函数：

- `ordered`：完成现有模型过滤后，保持候选基础顺序，不调用列表旋转；清理过期绑定不能改变本次起点。
- `session_preferred`：保留现有 `apply_session_provider_preference` 行为。

只在“普通会话偏好”层使用策略。额度、冷却、熔断、显式供应商校验、模型策略、失败候选集合、重试预算和截止时间继续由现有门禁与 failover 逻辑管理。

Codex 增量续接继续由 `responses_ws/send.rs` 的归属校验决定；`ordered` 不得强制迁移依赖上游上下文的请求。只有已有安全恢复条件允许完整重发时，才按策略对剩余候选排序，并保留 generation 的失败集合、访问集合、剩余预算和 deadline。

前端在 `src/components/cli-manager/tabs/GeneralTab.tsx` 的通用设置区域新增选择控件，复用已有设置 mutation、错误反馈和文案风格。`ordered` 显示“顺序优先”，`session_preferred` 显示“会话优先”，并说明必要的 Codex 续接仍可能保持原供应商归属。

## 组件与职责

- `src-tauri/src/infra/settings/types.rs`、`defaults.rs`、`migration.rs`：定义、默认值、旧配置迁移。
- 设置 service / command 对应的 `SettingsUpdate`、`SettingsView`：IPC 读写和字段传递。
- `src-tauri/src/gateway/proxy/handler/runtime_settings.rs`：将持久设置形成请求级策略快照。
- `src-tauri/src/gateway/proxy/handler/middleware/provider_resolution.rs`：将快照传入候选解析。
- `src-tauri/src/gateway/proxy/handler/provider_selection.rs`、`provider_order.rs`：实现两种排序分支和测试。
- `src-tauri/src/gateway/responses_ws/send.rs`、`responses_ws/integration_tests.rs`：保持续接归属并补充恢复回归。
- `src/services/settings/settings.ts`、`src/pages/settings/settingsPersistenceModel.ts`、`GeneralTab.tsx`：前端模型、保存字段和 UI。
- `src/generated/bindings.ts`：只通过 `pnpm tauri:gen-types` 生成，不手工编辑。

## 公开接口与不变量

- 持久字段：`provider_selection_strategy`；JSON/TypeScript 使用 camelCase 映射为 `providerSelectionStrategy`，具体以项目现有 `specta`/settings 映射约定为准。
- 允许值只有 `ordered`、`session_preferred`。
- 默认值为 `session_preferred`，确保升级不改变旧用户行为。
- `ordered` 只改变普通会话候选起点，不改变候选资格、失败切换和协议上下文归属。
- 同一请求及其重试使用同一策略；新设置不影响在途请求。
- `ordered` 普通请求不得产生“session preference applied”语义；Codex 上下文保持属于协议约束，不等同于普通会话偏好。

## 数据流与状态迁移

```text
AppSettings 持久化
  -> SettingsView / SettingsUpdate
  -> RuntimeSettingsMiddleware 读取一次
  -> HandlerRuntimeSettings.provider_selection_strategy
  -> provider_resolution
  -> model policy 过滤后的候选顺序
       ordered: A -> B -> C
       session_preferred: 绑定 B 时 B -> C -> A
  -> provider gates / failover
```

新增字段使用现有 schema 迁移机制；不新增数据库表，不重写会话绑定记录，不清空额度或熔断状态。

## 错误处理与失败模式

- 设置非法值：复用现有验证错误返回，不保存非法值。
- 设置读取失败：复用当前 `RuntimeSettingsMiddleware` 的默认运行时设置路径，策略默认 `session_preferred`，并记录既有 warn；不得把读取失败当成 `ordered`。
- A 不可用：由现有 provider gate 跳过并继续 B/C；排序策略不复活被 gate 排除的供应商。
- Codex 归属不匹配：沿用 `ContextLost` 及现有恢复处理，不跨供应商发送不属于当前供应商的 `previous_response_id`。

## 兼容性与迁移

默认保持当前会话优先行为。旧配置缺字段时不改变任何已有设置含义。设置 schema 版本按现有常量和迁移表新增版本；迁移测试验证从上一版本载入后字段为 `session_preferred`。生成绑定必须由 `pnpm tauri:gen-types` 更新并通过 `pnpm check:generated-bindings`。

## 可观测性

复用现有 provider selection 和 failover 诊断字段；在选择相关诊断中记录策略值和“普通顺序 / 会话偏好 / Codex 上下文约束”原因，不记录密钥、完整请求体或提示词。日志文案必须能区分 `ordered` 未旋转列表与 Codex 因上下文归属保持供应商。

## 备选方案与否决理由

- 仅把会话绑定键扩展为“会话 + 模型”：不能解决同一模型下 A 恢复后仍黏住 B，因此不足以满足 AC-002。
- 永久删除会话绑定和会话偏好：会破坏现有用户行为，也无法处理 Codex 协议续接，因此改为可配置策略。
- 新增轮询或余额后台服务：当前需求只要求候选起点和配置，现有 gate 已负责可用性；新增服务扩大状态和失败面，没有必要性证据。
- 无条件每次切换 Codex 供应商：会把 `previous_response_id` 交给错误上游，违反现有上下文不变量。

## 发布与回滚

先以默认 `session_preferred` 发布，用户可显式选择 `ordered`。发现问题时将配置切回 `session_preferred` 即可关闭新排序行为；设置迁移保持向前兼容。代码回滚必须同时回滚设置字段、迁移版本和生成绑定，避免运行时读取不匹配。

## 未决技术风险

需要在实现前确认设置模块中 `SettingsUpdate`、`SettingsView` 和字段映射的准确文件位置，以及 `provider_resolution.rs` 中运行时设置已存在的调用参数；若实际接口与本设计不同，更新 `artifacts/context.md` 后再实施。需要确认当前生成 bindings 的枚举命名，以避免手工假设 TypeScript 类型名。

## 验收追溯（AC → 设计点）

- AC-001：设置枚举、默认值、schema migration、SettingsView/Update、前端持久化模型。
- AC-002/AC-003：`provider_selection.rs`、`provider_order.rs` 的 ordered 分支和回归测试。
- AC-004：保留现有 session-preferred 分支及原有 provider order 测试。
- AC-005：复用 provider model policy、provider gate、failover budget，不改资格判断。
- AC-006：`RuntimeSettingsMiddleware` 请求快照和设置持久化测试。
- AC-007/AC-008：`send.rs` 归属校验及 `responses_ws/integration_tests.rs` 续接/恢复测试。
- AC-009：`GeneralTab.tsx`、设置 persistence 测试及诊断断言。

## 质量档位

本变更为 `critical`：涉及持久化设置迁移、IPC/public generated contract、跨层路由行为和 Codex 上游上下文数据完整性。实施前需要用户批准本设计；实施后需要完整相关测试、回滚检查和一次独立 `workflow-reviewer` 审查。
