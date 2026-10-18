# 交付验证

## 交付

- 状态：partial（源码修复、定点验证及独立复核完成；完整构建和账号端到端 UNVERIFIED）；handoff：awaiting-acceptance（2026-10-16T21:06:29+08:00）。新增范围报告 CLEAN，无阻塞项；未宣称同会话切换已通过用户验收。
- 第 1 轮范围（历史）：只在账号兼容函数中清理 `input[]` 对象直属 `status`，同文件增加 3 个回归测试。
- 基线：`29f5fccd80c0a5d4aeeab810061d7439b11ad5af`；生产差异为未暂存修改，新任务产物为未跟踪文件，无其他工作区改动。

## 验收标准

| 标准 | 结果 | 证据 |
| --- | --- | --- |
| AC-001 | PASS（函数级） | `compat_strips_input_item_status_without_changing_history_or_cache_context`；先观察 RED，再观察 GREEN |
| AC-002 | PASS（结构不变量） | 混合历史测试保留正文、工具、合法 ID/call_id、cache key、previous_response_id；新增 `compat_omits_only_invalid_reasoning_ids` 精确验证除 AC-004 外内容不变、源对象不变与幂等 |
| AC-003 | PASS（边界测试及调用点检查） | `compat_preserves_non_array_and_missing_input`；非对象数组元素；provider_iterator 仍仅在既有账号条件下调用兼容包装，调用点无修改 |
| AC-004 | PASS（函数级） | `compat_omits_only_invalid_reasoning_ids`，12 种 ID/缺失场景，先 RED 后 GREEN；合成 Pi→AIO 串联检查通过，不等于真实账号端到端通过 |

## TDD 证据

首选命令：

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib gateway::proxy::protocol_bridge::cx2cc::tests::compat_ -j 1 --offline
```

结果：180 秒超时，仍在重编译依赖，尚未进入目标测试；不能算测试失败或通过。依照已批准备用方案，改用轻量临时 harness。

```sh
rustc +1.90.0 --edition=2021 --test 'C:/Users/shuan/AppData/Local/Temp/aio-compat-regression.rs' --extern serde_json=src-tauri/target/debug/deps/libserde_json-534f2c55af3731d7.rlib -L dependency=src-tauri/target/debug/deps -o 'C:/Users/shuan/AppData/Local/Temp/aio-compat-regression.exe'
'C:/Users/shuan/AppData/Local/Temp/aio-compat-regression.exe' compat_ --nocapture
```

Harness 用 `#[path]` 导入真实 `cx2cc/mod.rs` 及其测试，没有复制兼容函数；仅桩替代编译该模块需要的、不参与兼容逻辑的模型映射设置/上下文类型。不代表完整 Tauri 集成编译。临时源文件、可执行文件及调试产物已在收尾删除；长期测试保留在原模块，资源允许时可用上述 Cargo 命令复跑。

- RED：新混合历史测试失败，`input[0].status must be removed`，退出 101；5 个既有兼容测试通过。
- GREEN：实现后该测试通过（6/6）；补充两个边界测试并完成格式调整后最终 8/8 兼容测试通过，13 个无关映射/流标志测试被过滤。

## 其他验证

- `rustfmt --edition 2021 --check src-tauri/src/gateway/proxy/protocol_bridge/cx2cc/mod.rs`：PASS。
- `git diff --check`：PASS。
- `lsp_diagnostics`，路径为修改的 Rust 文件：rust-analyzer 0 diagnostics。
- 完整 Cargo 测试/应用构建：UNVERIFIED，依赖重编译超时；没有宣称完整构建通过。
- 前端构建：NOT RUN，没有前端修改。
- 真实账号请求、同会话切换和实际缓存命中：NOT RUN；未访问凭据、未发送用户历史。

## 独立复核

已完成第 1 次 full：`workflow-reviewer`，独立新上下文，run `9252e599-e6e2-42c2-b428-06c5145528cc`，`REVIEW_STATUS: CLEAN`，无阻塞项。报告及主会话对非阻塞观察的处置保存在 `artifacts/review.md`，覆盖 Standards、Spec、正确性、安全、测试和复杂度。未启动 focused；复核后只更新交付记录，未修改生产代码或测试。复核没有自行运行验证，也未审阅临时 harness 桩，不能把它视为独立执行测试。报告对临时文件删除时间线的误述已在报告前注明；不影响未复核桩代码这一边界。

## 提交与验收

- NOT COMMITTED；不发布、不归档。
- 待用户验收：加载新构建，Pi 使用 API 生成几轮历史，同会话切到账号继续发送；确认不再出现 `input[n].status` 400，工具调用可继续；缓存命中需看服务端 cached token，不能由 key 保持推定。
- 剩余风险：真实失败项具体类型尚未抓取；可能仍有其他不兼容字段；API 与账号之间不保证共享缓存。

## 第 2 轮：用户验收反馈与 ID 格式诊断（历史诊断记录，实施结果见下文）

- 用户截图：`C:/Users/shuan/AppData/Local/Temp/orca-paste-1792154526078-598e002f-6ffc-4f34-a150-f29b7a4963ba.png`。
- 原文：`Error: aio-codex API error (400): {"message":"Invalid 'input[46].id': 'out_7'. Expected an ID that begins with 'rs'.","type":"invalid_request_error","param":"input[46].id","code":"invalid_value"}`。
- 这是 ID 校验，不是 status 再次被拒绝。截图不证明当前应用使用的构建版本，也没有提供原始请求；具体 input[46] 类型仍需区分实证与推断。
- 离线红色信号：用合成历史构造第 47 项 reasoning，签名为 `{type:reasoning,id:out_7,status:completed,summary:[...]}`；调用本机 Pi `convertResponsesMessages`，再通过 `#[path]` 导入真实 AIO 兼容函数，断言输出 reasoning 的 id 缺失或以 rs 开头。结果退出 101：Pi 保留 out_7，AIO 删除 status 后仍保留 out_7。该检查证明兼容缺口可达，不是对真实账号请求的在线重放。
- 探针：`C:/Users/shuan/AppData/Local/Temp/aio-reasoning-id-probe.rs`，使用 `rustc +1.90.0 --edition=2021` 与已有 serde_json rlib 编译；仅桩模型映射依赖，生产兼容函数不复制。尚未完成修复，探针暂留，完成后清理。
- Pi 证据：`C:/Users/shuan/.pi/agent/install/releases/1.3.0/node_modules/@earendil-works/pi-ai/dist/api/openai-responses-shared.js:183-186` 把 thinkingSignature JSON 原样推入历史 input；同目录 `transform-messages.js:94-113` 按 provider/api/model 判断是否保留推理签名。AIO 后台路由切换并不必然改变客户端的三元身份。
- 本轮建议：仅在既有账号兼容函数内，对 reasoning 项不符合 rs 前缀的直属 id 省略转发，不伪造服务端 ID；保留已有合法 rs ID、summary/encrypted_content、其他消息及工具项、call_id、cache key 和续接标识。可能仍有跨后端加密推理不可复用的问题，不能据本地结构检查承诺端到端成功。
- 这会收窄第 1 轮 AC-002 的“所有 id 不变”承诺。须用户明确批准扩展范围及其独立复核；既有 CLEAN 只覆盖第 1 轮，不自动覆盖新修改。档位仍为 standard，无凭据、认证或持久化修改。
- 真实账号请求未发送，未读取凭据；尚未修改新的生产代码或在树测试。

### 第 2 轮实施与检查（新增批准范围）

- 用户选择“批准继续修复”，批准调整 AC-002 并增加 AC-004，授权新增范围一次 full 独立复核。不提交、不发布、不归档。下述结果取代上节“尚未实施”的历史状态。
- 生产变更：沿用 `codex_chatgpt_request_compat_value` 的同一循环，仅 reasoning 对象的直属 id 已存在且不是以 `rs` 开头的字符串时省略 id；不重命名或伪造，不修改其他项 ID、call_id、正文、summary、encrypted_content 或缓存/续接键。
- 在树 RED：新增 `compat_omits_only_invalid_reasoning_ids`，用 `rustc --test` 导入真实模块后运行该测试，退出 101，`assertion left == right failed: only invalid reasoning ID and item status may be omitted`。差异为第 47 项 reasoning 的 out_7 仍存在，status 已被上一轮正确删除。
- GREEN：修复后执行全部 `compat_`，9 passed / 0 failed / 13 filtered out。新测试表格 12 例：out_7、空字符串、RS_7、null、数值、布尔、对象、数组、rs_7、rs7、rs、缺失 ID。精确比较整个 input，验证其他消息/工具 ID、嵌套业务 ID/status、工具 call_id、summary、encrypted_content、cache key、previous_response_id 保留，源对象不变及幂等。

```sh
rustc +1.90.0 --edition=2021 --test 'C:/Users/shuan/AppData/Local/Temp/aio-reasoning-id-probe.rs' --extern serde_json=src-tauri/target/debug/deps/libserde_json-534f2c55af3731d7.rlib -L dependency=src-tauri/target/debug/deps -o 'C:/Users/shuan/AppData/Local/Temp/aio-reasoning-id-tests.exe'
'C:/Users/shuan/AppData/Local/Temp/aio-reasoning-id-tests.exe' compat_ --nocapture
```

- Harness 的非业务桩只补模型映射上下文与设置类型，`Default` 不代表真实模型设置；仅运行 compat_，13 个无关映射/流标志测试被过滤，不宣称其通过。真实模块及其中新增测试通过 `#[path]` 导入，不复制生产算法。
- 合成 Pi→AIO 串联检查：本机 Pi 真实 `convertResponsesMessages` 对同一 provider/api/model 的合成历史输出 `input[46]` 带 `out_7` 与 status；修改后的真实 AIO 函数输出 `{"summary":[{"text":"synthetic summary","type":"summary_text"}],"type":"reasoning"}`，ID/status 均省略，摘要与 prompt_cache_key 保留，退出 0。不是在线账号验证。
- `rustfmt +1.90.0 --edition 2021 --check src-tauri/src/gateway/proxy/protocol_bridge/cx2cc/mod.rs`：最终 PASS。初次检查仅报告两处排版差异，已通过锚点编辑修正并重新通过。
- `git diff --check`：PASS。`lsp_diagnostics`：rust-analyzer 对修改文件 0 diagnostics。
- 完整 Cargo/Tauri 构建与账号端到端：UNVERIFIED。本轮没有重跑上一轮在依赖编译阶段 180 秒超时的全包测试；未打包或替换应用。真实缓存命中率：NOT RUN；保留缓存键不等于命中证明。
- 新增范围独立复核：已完成 full 1/1，focused 0/1；`workflow-reviewer`，run `e431cf77-3299-4221-b922-34a128193b1f`，fresh，`REVIEW_STATUS: CLEAN`，无阻塞项。完整报告原样保存至 artifacts/review-round-2.md。requested_model 未覆盖（使用角色配置），effective_model/fallback 未暴露，不猜测。复核只读并审阅了临时 harness 桩，未重跑测试；不升格为独立执行验证。原第 1 轮 CLEAN 与报告保留。
- 临时探针源及两个可执行文件已在复核读完并保存报告后清理，逐个检查均 REMOVED。长期回归留在真实模块内；无需保留临时桩，资源允许时使用上文 Cargo 定点命令复跑。

### 第 2 轮复核处置与验收

- 无阻塞发现，不启动 focused。非阻塞观察记录不改代码：`.openspec.yaml` 的 goal 仍为原 status 摘要，完整扩展范围以 proposal/spec/design 为准；单一下标 46 的回归没有防住未来人为硬编码下标的突变，但当前实现是全量循环，没有该缺陷。不为偏好建议追加复核或扩大范围。
- 复核后生产代码与测试未再修改，仅保存原始报告、记录交付与清理临时产物；最终 git status 仍仅显示既有 Rust 修改与选定变更目录，未触碰其他任务。
- AC-001 至 AC-004 的规格勾选仍未标记用户接受：函数级验证已有 PASS；完整应用构建、原会话真实账号验收和 cached token 测量尚缺。升级触发：使用新构建后的实际错误、脱敏原始出错 input 项类型/字段或失败工具配对证据。
- 手工验收：先编译并运行包含本次修改的新 AIO（当前运行构建未被替换）。在 Pi 原会话由 API 切到账号继续发消息，确认既无 `input[n].status` 错误也无 `out_7` / rs 前缀错误（AC-001/004）；确认正文连续、工具调用与结果能关联，缓存键/续接上下文不被改写，实际命中另看服务端 cached token（AC-002）；切回普通 API 确认仍正常（AC-003）。如仍报错，提供新构建标识、完整错误原文及脱敏出错项，不提供凭据。
- 状态：待用户验收；NOT COMMITTED、未打包发布、未归档。验收通过后仅在用户明确要求时归档，不因 CLEAN 自动归档。
