# 复核归档与主会话处置

- 角色：`workflow-reviewer`，独立新上下文；run `9252e599-e6e2-42c2-b428-06c5145528cc`。
- 模式/预算：第 1 次 full，`REVIEW_STATUS: CLEAN`，无阻塞项；未启动 focused。
- 范围：唯一 Rust 修改文件、其相邻调用边界和本 change 产物。复核后未修改生产代码或测试。
- 非阻塞测试观察未采纳：逐键保留所有原非 status 字段，加上长度必须减少一，已经能检测新增键；无需额外断言。
- 时间线纠正：报告称临时 harness 已删除，实际复核期间它仍在临时目录，未纳入子代理可审读范围；收尾由主会话删除。“未审阅桩代码”和“非完整构建”的证据边界仍然成立。
- 未完成真实账号与缓存命中验证，不代表用户验收。

## 独立报告原文
# 独立复核：fix-codex-account-input-status

**Active change:** `openspec/changes/fix-codex-account-input-status/`
**Assigned slice:** review / AC-001 至 AC-003
**Phase:** review
**Review mode:** full
**Prior report:** 无（本预算第一轮）
**Blocking:** no
**REVIEW_STATUS: CLEAN**

无阻塞发现。下面的观察不构成 AC、项目规则或正确性/安全不变量违反，不要求修复，也不消耗 focused 复查。

用户验收未完成。`specs/codex-account-input-compat/spec.md` 的 AC 复选框仍为 `[ ]`，`tasks.md` 的复核与 awaiting-acceptance 仍未勾选。本结论只覆盖实现与已记录证据，不代表用户验收通过。

## Standards

无发现。

- 变更留在所选 OpenSpec change；人工产物为中文，规格保留 SHALL/MUST、AC 编号和代码标识，符合 `openspec/config.yaml`。
- 生产改动限于既有 `codex_chatgpt_request_compat_value`：无新依赖、开关、重试、认证或持久化。
- 注释与 `pub(crate)`、`#[cfg(test)]` 同文件风格一致。记录的 `rustfmt --check` 与 rust-analyzer 0 diagnostics 未被本复核重跑，也未发现与之冲突的格式或类型问题。
- 仓库根目录无 `AGENTS.md`。`responses_ws/protocol.rs` 里按 kind 删除 `status` 是另一条既有规范化路径，不在本范围，也不应被当成必须复用的项目规则。
- 验证记录没有把 Cargo 超时、临时 harness 或未跑的端到端写成通过。这与“不得把未执行检查写成 PASS”一致。

## Spec

无发现。实现和调用边界与 AC-001 至 AC-003 一致。

### AC-001 移除账号历史输入项直属 status

- 实现：`src-tauri/src/gateway/proxy/protocol_bridge/cx2cc/mod.rs:96-104` 在白名单克隆出的 `next.input` 上，对数组内每个对象 `remove("status")`，不按索引截断。
- 测试：`mod.rs:283-332` 覆盖 message、reasoning、function_call、function_call_output，断言每一项不再含直属 `status`，且其余字段与源项逐键相等、数组长度不变。
- 已记录执行：RED 失败文案为 `input[0].status must be removed`，与 `mod.rs:324` 的断言一致；修复后该测试进入 6/6。本复核未重跑。

### AC-002 内容与缓存上下文保持稳定

- 同一测试比较 `model`、`instructions`、`prompt_cache_key`、`previous_response_id`、`tools`，并断言 `root == original` 与 `codex_chatgpt_request_compat_value(&next) == next`。
- `mod.rs:336-349` 断言 `output.status` 和嵌套数组里的 `status` 保留。工具参数字符串 `"{\"status\":\"pending\"}"` 与工具 schema 中的 `status` 不在 `input` 项直属键上，循环不会碰到。
- `prompt_cache_key` 与 `previous_response_id` 在白名单内，循环只改 `input` 项。这只证明结构不变量，不证明上游缓存命中。记录与规格都没有把命中率写成通过。

### AC-003 兼容边界不扩大

- `mod.rs:352-360`：字符串、`null`、对象形态的 `input` 原样保留；缺失 `input` 不出现该键。`mod.rs:336-349` 保留 `null`、字符串、数字和非对象数组元素。非数组不会进入 `as_array_mut`，非对象不会进入 `as_object_mut`，无递归、无新 panic 路径。
- 调用点当前代码未把兼容扩散到普通 API：`provider_iterator.rs:168-170` 与 `426-433` 仍只在 `use_codex_chatgpt_backend` 内调用 `maybe_apply_codex_chatgpt_request_compat`；`codex_chatgpt.rs:8-21` 仍要求 `cli_key == "codex"`、`auth_mode == "oauth"` 且 base URL 以 `/backend-api/codex` 结尾；`codex_chatgpt.rs:111-123` 仍只对规范化后的 `/responses` JSON 调用共享函数。全库账号兼容调用图没有新增生产调用点。`remove("status")` 的本变更只出现在 `cx2cc/mod.rs:100`。

## 正确性

循环在克隆后的 `next` 上就地删除直属键，不改数组长度或元素顺序，源 `root` 不是可变引用。`remove` 对缺失键是空操作，因此“可带 status”的混合数组不会误删其他项。重复调用不再有 `status` 可删，既有的 `stream`/`store`/`instructions` 规范化是确定性的，幂等断言与实现一致。

未把 `responses_ws/protocol.rs:300-381` 的按 kind 规范化接进来是对的：那边会丢弃不合规项并改写 content，会破坏必须保留的正文、顺序和工具关联。

## 安全

无新认证、授权、凭据或日志路径。删除的是上游已拒绝的输入元数据，不是业务正文。源对象不被原地改写，后续 failover 仍可使用原请求体。无注入面或破坏性持久化。

## 测试

模块内 8 个 `compat_*` 测试与记录的 8/8 相符；同文件另有 9 个模型映射测试和 4 个流标志测试，与“13 个无关测试被过滤”相符。主回归的失败文案能抓住“未删除 status”。嵌套测试能抓住误递归。边界测试能抓住非数组被改写。

非阻塞观察：`mod.rs:327-329` 用 `source.len() - 1` 且只遍历源键。若将来在删除 `status` 的同时插入新键，长度可能抵消，多余键不会被看见。当前实现不插入新键，现有断言仍能抓住本次缺陷。数字/布尔 `input`、部分项缺少 `status` 没有单独样例，但走的是同一 `as_array_mut`/`remove` 路径。二者都不构成要修复的缺陷。

## Ponytail 复杂度

无应删复杂度。9 行循环内联在既有函数，无新类型、配置、依赖或兼容开关。注释只说明为何不递归，与不变量匹配。

## 证据诚实度

本复核只读，未执行 git、cargo、rustc 或 rustfmt。

| 检查 | 本复核认定 |
| --- | --- |
| `cargo test ... gateway::proxy::protocol_bridge::cx2cc::tests::compat_ -j 1 --offline` | 记录为依赖重编译 180 秒超时，未进入测试。不是失败，也不是通过。 |
| 临时 `rustc +1.90.0 --edition=2021 --test` harness | 记录为 RED 后 6/6，补边界后 8/8。失败文案与在树断言一致。harness 已删除，本复核不能审阅桩代码，不能把该命令升格为自己执行的 PASS。 |
| `rustfmt --check`、`git diff --check`、rust-analyzer | 仅采用记录，未重跑。 |
| 完整 Tauri/Cargo 构建、真实账号、缓存命中 | NOT RUN / UNVERIFIED。不能用来证明端到端或缓存复用。 |

交接声明的基线 `29f5fccd80c0a5d4aeeab810061d7439b11ad5af`、无新提交、无暂存差异，本复核未重放 git。审查的是当前调用点源码和声明范围内的未跟踪产物。

## 已审输入

- `openspec/changes/fix-codex-account-input-status/.openspec.yaml`
- `proposal.md`、`design.md`、`tasks.md`、`README.md`（仅标题，无额外要求）
- `specs/codex-account-input-compat/spec.md`
- `artifacts/context.md`、`artifacts/verification.md`
- `src-tauri/src/gateway/proxy/protocol_bridge/cx2cc/mod.rs`（函数 `84-108`，测试 `264-410`）
- `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/codex_chatgpt.rs:8-21`、`101-133`
- `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/provider_iterator.rs:168-170`、`426-433`
- `cx2cc_preparation.rs` 只确认它更新既有 `use_codex_chatgpt_backend` 标志，不新增兼容调用

## 覆盖缺口

- 未重放 `git status` / `git diff`，不能独立证明范围外工作区绝对干净。
- 未审阅已删除的临时 harness 桩。
- 未完成在树 Cargo 测试和 Tauri 构建。
- 未做真实账号同会话切换，也未测量 cached token。

这些缺口已在设计和验证记录中声明，不构成阻塞。

## 剩余风险与未运行项

- 真实失败请求的 item 类型未抓取。直属 `input[n].status` 会被去掉；`input[n].content[i].status` 这类更深层字段按规格保留，若上游同样拒绝，仍会 400。
- API 与账号之间不保证共享 prompt cache。键和续接标识不变不能推定命中率。兼容改写本来就会重新序列化 JSON。
- 临时 harness 不是完整 Tauri 集成编译。资源允许时应用记录中的 Cargo 命令复跑在树测试。
- 用户环境复验仍待做：加载新构建后，用 API 生成历史，再在同会话切到账号发送，确认不再出现 `input[n].status` 400，且工具调用可继续。
