# 独立复核：fix-codex-account-input-status（第 2 轮）

**Active change:** `openspec/changes/fix-codex-account-input-status/`
**Assigned slice:** review / approved expanded scope AC-001 through AC-004
**Phase:** review
**Review mode:** full
**Review scope:** 本变更相关未暂存工作区：第一轮 status 修复、已批准的 reasoning ID 省略、同文件测试，以及点名的未跟踪 OpenSpec 文档。基线 `29f5fccd80c0a5d4aeeab810061d7439b11ad5af`。无暂存、无新提交。本复核只读当前文件，未重放 git。
**Prior report:** `artifacts/review.md` 第 1 轮 full，`REVIEW_STATUS: CLEAN`，只覆盖 AC-001 至 AC-003。不证明本轮 ID 省略。
**Budget:** 扩展范围 full 1/1（本报告消耗），focused 0/1。无嵌套复核。
**Blocking:** no

无阻塞发现。下面的观察不违反 AC、项目规则或正确性/安全不变量，不要求修复，也不应消耗 focused 复查。用户验收未完成：规格 AC 仍为 `[ ]`，`tasks.md` 切片 2 末项仍未勾选（复核与临时探针清理未结束）。本结论只覆盖实现与已记录证据。

## Standards

无发现。

- 人工产物为中文；规格保留 SHALL/MUST、AC 编号和代码标识，符合 `openspec/config.yaml`。
- 生产改动留在既有 `codex_chatgpt_request_compat_value`。无新依赖、开关、重试、认证、凭据或持久化。
- 仓库根目录无 `AGENTS.md`。注释、`pub(crate)` 与同文件 `#[cfg(test)]` 风格一致。
- 顶层白名单仍由 `CODEX_CHATGPT_RESPONSES_ALLOWED_KEYS`（`cx2cc/mod.rs:63-77`）复制，循环只改克隆后的 `input` 项。
- 验证记录把 Cargo 超时、临时 harness 和未跑的端到端写成 UNVERIFIED/NOT RUN，没有写成 PASS。
- 非阻塞：`.openspec.yaml` 的 `goal` 仍只写 status 清理，未提及已批准的 AC-004。行为以 `specs/codex-account-input-compat/spec.md` 为准，不构成实现缺陷。

## Spec

无发现。实现与调用边界符合 AC-001 至 AC-004。

### AC-001 移除账号历史输入项直属 status

- 实现：`src-tauri/src/gateway/proxy/protocol_bridge/cx2cc/mod.rs:97-100` 对克隆后 `input` 数组内每个对象 `remove("status")`，不按索引截断，不递归。
- 测试：`mod.rs:292-341` 覆盖 message、`id: rs_1` 的 reasoning、function_call、function_call_output；断言无直属 `status`，其余键与源项一致，长度减一。
- AC-004 例外只多删不合规 reasoning `id`，不改这条 status 规则。`rs_1` 仍保留，旧断言仍然成立。
- 已记录执行：第 1 轮 RED 为 `input[0].status must be removed`；第 2 轮 GREEN 含该测试。本复核未重跑。

### AC-002 内容与缓存上下文保持稳定

- `mod.rs:103-108` 只在 `type == "reasoning"` 且直属 `id` 已存在、又不是以 `rs` 开头的字符串时 `remove("id")`。不改 `call_id`、正文、工具、嵌套对象或其他类型的 `id`。
- `mod.rs:315-341` 比较 `model`、`instructions`、`prompt_cache_key`、`previous_response_id`、`tools`，并断言 `root == original` 与再调用结果等于 `next`。
- `mod.rs:410-423` 保留 `output.status` 和嵌套数组里的 `status`。工具参数字符串与 schema 中的 `status` 不在项直属键上。
- `mod.rs:360-405` 对整个 `input` 精确比较：嵌套 `metadata.id` / `metadata.status`、`out_message`、`out_call`、`out_result`、`out_untyped`、`call_7`、summary、`encrypted_content`、缓存键和 `previous_response_id` 保持；源对象不变且幂等。
- 这只证明结构不变量，不证明上游缓存命中。记录与规格都没有把命中率写成通过。

### AC-003 兼容边界不扩大

- `mod.rs:426-433`：字符串、`null`、对象形态的 `input` 原样保留；缺失 `input` 不出现该键。`mod.rs:410-423` 保留 `null`、字符串、数字和非对象数组元素。非数组不进 `as_array_mut`，非对象不进 `as_object_mut`，无递归、无新 panic 路径。
- 普通 API 无新调用。`provider_iterator.rs:168-170` 与 `427-433` 仍只在 `use_codex_chatgpt_backend` 内调用 `maybe_apply_codex_chatgpt_request_compat`。`codex_chatgpt.rs:8-20` 仍要求 `cli_key == "codex"`、`auth_mode == "oauth"` 且 base URL 以 `/backend-api/codex` 结尾。`codex_chatgpt.rs:117` 仍只在规范化后的 `/responses` 上进入兼容函数。全库生产调用点仍只有这一处。`failover_loop/mod.rs:88` 只是 re-export。

### AC-004 仅省略账号历史推理项中格式不合规的 ID

- 条件是 `mod.rs:103-106`：`type` 必须为字符串 `"reasoning"`；`id` 必须已存在；`as_str()` 失败，或字符串不以 `"rs"` 开头，才删除。缺失 `id` 时 `is_some_and` 为假，不插入 ID。
- 不改写 ID 字符串，不删除 reasoning 对象，不按固定下标。`rs_7`、`rs7`、`rs` 保留；`out_7`、空串、`RS_7`、null、数字、布尔、对象、数组被省略。大小写按字面 `rs`，没有额外收成 `rs_`。
- 测试：`mod.rs:345-407`，12 例在 `mod.rs:347-358`，reasoning 放在 `mod.rs:369` 的 46 个前缀项之后（下标 46，第 47 项）。断言文案 `only invalid reasoning ID and item status may be omitted` 与记录的 RED 一致。
- 已记录：修复前 RED 退出 101，因为 `out_7` 仍在而 status 已被上一轮删除；修复后 `compat_` 9 passed / 0 failed / 13 filtered。本复核未重跑。离线结果不证明加密推理可重放或账号端到端成功。

## 正确性

循环在白名单克隆出的 `next` 上就地删除键，不改数组长度或顺序。入参是 `&Value`，源对象不是可变引用。`remove` 对缺失键是空操作。第二次调用时 status 已无、非法 `id` 已无或合法 `id` 仍以 `rs` 开头，再叠加原有的确定性 `stream`/`store`/`instructions` 规范化，幂等断言与实现一致。

`codex_chatgpt.rs:218-314` 的相邻包装测试使用无 status、无 reasoning ID 的 input，新条件不会改变它们的断言。

未改 Pi、未清空历史、未伪造 `rs_*`。若真实 `input[46]` 不是 `type: "reasoning"`，本条件按规格不会动它的 `id`；这是已声明的推断边界，不是实现偏离。

## 安全

无新认证、授权、凭据、日志或持久化路径。删除的是直属 status 和格式不合规的 reasoning `id`，不是正文、`call_id` 或密文。源请求值不被原地改写。`maybe_apply` 仅在序列化成功时替换转发字节。无注入面或破坏性迁移。省略 ID 是规格要求的保守选择，不是凭据绕过。

## 测试

同文件 9 个 `compat_*` 与记录的 9/9 相符。另有模型映射与 4 个流标志测试，合计过滤 13 个，与文件中 22 个 `#[test]` 一致。新测试在非法 ID 仍存在、合法 ID 被删、其他项 ID/`call_id`/嵌套字段被改、源对象被改或结果不幂等时都会失败。失败文案能抓住第 2 轮 RED 所描述的 `out_7` 残留。

非阻塞：`compat_omits_only_invalid_reasoning_ids` 只把不合规 reasoning 放在下标 46。若将来有人只对下标 46 删 ID，而 status 仍走全量循环，这个测试仍会通过。当前生产代码是 `mod.rs:98-112` 的逐项循环，没有硬编码下标。不要求补测试，也不启动 focused。

## Ponytail 复杂度

无应删复杂度。ID 判断内联在既有循环，无新类型、配置、依赖或兼容开关。布尔条件只表达“已存在且不是 `rs` 前缀字符串”。注释与不变量一致，没有多出来的抽象。

## 证据诚实度

本复核只读，未执行 git、cargo、rustc、rustfmt 或 rust-analyzer。

| 检查 | 本复核认定 |
| --- | --- |
| `cargo test ... cx2cc::tests::compat_ -j 1 --offline` | 第 1 轮记录为依赖重编译 180 秒超时，未进入测试。第 2 轮未重跑。不是失败，也不是通过。 |
| `rustc +1.90.0 --edition=2021 --test C:/Users/shuan/AppData/Local/Temp/aio-reasoning-id-probe.rs ...` 后 `compat_ --nocapture` | 记录为 RED 退出 101，然后 9 passed / 0 failed / 13 filtered。断言文案与 `mod.rs:394` 一致，测试数量与源文件一致。本复核未执行，不能升格为自己的 PASS。 |
| 临时 harness | 已读。`#[path]` 指向真实 `cx2cc/mod.rs`，没有复制兼容算法。桩只补 `ClaudeModels`、`Cx2ccSettings`、`BridgeContext`、`ModelMapper`；其 `Default` 不是真实模型设置。`compat_` 不依赖这些 Default。13 个被过滤测试不算通过。 |
| Pi `convertResponsesMessages` 到真实 AIO 函数 | 记录为同一 provider/api/model 下保留 `out_7`+status，AIO 输出无 ID/status 的 reasoning，并保留 summary 与 `prompt_cache_key`，退出 0。探针 `main` 从 stdin 调用真实函数，命令未单独列入代码块，stdin 原文不在探针内。不是在线账号请求，本复核未重放。 |
| `rustfmt +1.90.0 --edition 2021 --check`、`git diff --check`、rust-analyzer | 仅采用记录：最终 PASS / PASS / 0 diagnostics。未重跑。 |
| 完整 Tauri/Cargo 构建、真实账号切换、cached token | UNVERIFIED / NOT RUN。不能证明端到端或缓存复用。 |

## 已审输入

- `openspec/config.yaml`
- `openspec/changes/fix-codex-account-input-status/.openspec.yaml`、`README.md`、`proposal.md`、`design.md`、`tasks.md`
- `specs/codex-account-input-compat/spec.md`
- `artifacts/context.md`、`artifacts/verification.md`、`artifacts/review.md`（仅作第 1 轮历史边界）
- `src-tauri/src/gateway/proxy/protocol_bridge/cx2cc/mod.rs`（函数 `84-121`，白名单 `63-77`，测试 `292-433`）
- `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/codex_chatgpt.rs:8-20`、`101-133`、`218-314`
- `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/provider_iterator.rs:168-170`、`427-433`
- `C:/Users/shuan/AppData/Local/Temp/aio-reasoning-id-probe.rs`（桩与 `#[path]` 导入）
- 生产调用检索：`codex_chatgpt_request_compat_value`、`maybe_apply_codex_chatgpt_request_compat`

## 覆盖缺口

- 未重放 `git status` / `git diff`，不能独立证明范围外工作区绝对干净，也不能对白名单做基线逐字节比对。当前函数体已整段阅读。
- 未重跑在树 Cargo、临时 `rustc --test`、rustfmt、`git diff --check`。
- 未看到 Pi 串联的原始 stdin，也未打开用户截图；错误原文采用验证记录。
- 未做真实账号同会话切换，也未测量 cached token。
- 第 1 轮 CLEAN 不覆盖本轮输入。

这些缺口已在设计和验证记录中声明，不构成阻塞。

## 剩余风险与未运行项

- 截图只证明 `input[46].id = out_7` 被拒绝且期望 `rs` 前缀。原始项类型和当时运行的构建未知。
- 保留 `encrypted_content` 不等于账号后端能解密或重放该历史。
- 直属更深的 `content[i].status` 或非 `reasoning` 项上的非 `rs` ID 按规格保留；上游若拒绝，仍会 400。
- `rs` 前缀按规格宽于 `rs_`。上游若再收紧格式，现有合法保留项仍可能失败。
- 删除 status 和非法 ID 会改变转发 JSON。`prompt_cache_key` 不变不能推定缓存命中。
- 临时 harness 不是完整 Tauri 集成编译。完整构建与账号复验仍待用户环境。

REVIEW_STATUS: CLEAN