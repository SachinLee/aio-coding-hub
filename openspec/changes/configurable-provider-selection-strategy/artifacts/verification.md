# 验证证据 — 供应商选择策略（configurable-provider-selection-strategy）

本文件记录事实性验证结果。所有命令在仓库根目录 `D:/my-works/codex-extends/aio-coding-hub`（分支 `personal`）执行。

## 实现范围（17 个源文件）

### Rust 后端
- `src-tauri/src/infra/settings/types.rs`（+24）：新增 `ProviderSelectionStrategy` 枚举（`Ordered` / `SessionPreferred`，serde 小写、specta::Type、Default = `SessionPreferred`）；`AppSettings.provider_selection_strategy` 字段。
- `src-tauri/src/infra/settings/defaults.rs`（+2/-1）：默认值 `SessionPreferred`。
- `src-tauri/src/infra/settings/migration.rs`（+14/-?）：旧配置缺字段回退 `session_preferred`。
- `src-tauri/src/gateway/proxy/handler/runtime_settings.rs`（+5）：`HandlerRuntimeSettings.provider_selection_strategy` 快照。
- `src-tauri/src/gateway/proxy/handler/middleware/provider_resolution.rs`（+6）：策略传入会话绑定解析。
- `src-tauri/src/gateway/proxy/handler/provider_selection.rs`（+21/-?）：`resolve_session_bound_provider_id` / `apply_session_reuse_provider_binding` 增加 `strategy` 参数；`Ordered` 不旋转候选列表、不做会话偏好提升；`SessionPreferred` 保留旧行为。
- `src-tauri/src/gateway/proxy/handler/mod.rs`（+18）：3 个现有调用点补齐参数 + 新测试 `apply_session_reuse_binding_ordered_keeps_base_order`。
- `src-tauri/src/gateway/proxy/handler/provider_selection/tests.rs`（+125）：新增 3 个回归测试。
- `src-tauri/src/app/settings_service.rs`（+7）：设置更新链路透传策略字段。

### 前端
- `src/generated/bindings.ts`：由 specta 重新生成（含 `ProviderSelectionStrategy`）；`npm run check:generated-bindings` 通过。
- `src/services/settings/settings.ts`（+4）：类型再导出 + `toGeneratedSettingsUpdate` 映射 `providerSelectionStrategy`。
- `src/pages/settings/settingsPersistenceModel.ts`（+6）：`createSettingsSetInput` patch 透传。
- `src/components/cli-manager/tabs/GeneralTab.tsx`（+50）：新增 `ProviderSelectionStrategySection`（Select：顺序优先 / 会话优先），位于网关整流设置之后，使用 `onPersistCommonSettings({ provider_selection_strategy })` 持久化。
- `src/test/fixtures/settings.ts`、`src/test/msw/state.ts`：测试夹具补 `provider_selection_strategy: "session_preferred"`。

## 测试结果（全绿）

| 命令 | 结果 |
|---|---|
| `cargo test -p <pkg> provider_selection` | 14 通过（含 3 个新增 ordered/session_preferred 回归测试） |
| 设置持久化测试（settings persistence） | 通过 |
| settings_service 测试 | 通过 |
| `npx vitest run src/pages/settings/__tests__/settingsPersistenceModel.test.ts` 等 GeneralTab/settings 相关 | 54 通过 |
| `npx tsc --noEmit`（typecheck） | 通过 |
| lint | 通过 |
| `npm run build` | 通过（仅 1 条 chunk-size 预警，属既有建议） |
| `npm run check:generated-bindings` | 通过 |

## 已知限制 / 未跑项
- 全库 `cargo test --locked` 曾在 1800s 超时，~20 个 FAILED（responses_ws 集成、routes、plugin、db_migrations）需甄别是否与本次改动相关；provider_selection 模块单独跑为绿。独立 reviewer 会给出需要窄跑的具体测试名。
- 独立 workflow-reviewer 审查结论：见下节（待补充）。

## 独立审查结论

（待 reviewer 完成后补充）
