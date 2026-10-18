# 上下文索引

- `proposal.md`、`specs/codex-account-input-compat/spec.md`、`design.md`、`tasks.md` — 本任务要求、边界、验证与授权。
- `src-tauri/src/gateway/proxy/protocol_bridge/cx2cc/mod.rs` — 唯一生产修改点及已有兼容测试模式。
- `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/codex_chatgpt.rs` — OAuth/backend 检测与 Responses 路径兼容包装，保持不变。
- `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/provider_iterator.rs` — 账号条件调用点，保持不变。
- `openspec/config.yaml` — 项目工作流、中文产物、确认与验收规则。

## 第 2 轮新增范围：reasoning ID

- 用户已明确批准继续修复，调整 AC-002，新增 AC-004；授权新增交付范围一次 full 独立复核。先前第 1 轮 CLEAN 不覆盖新增输入。仍为 standard，生产修改与测试只在原 Rust 模块。
- `src-tauri/src/gateway/proxy/protocol_bridge/cx2cc/mod.rs:96-113`：在克隆后的 input 逐项删除直属 status，再仅在 type == reasoning 且存在非 rs 字符串/非字符串 id 时删除直属 id，不制造服务端 ID。
- 同文件 `compat_omits_only_invalid_reasoning_ids`：第 47 项合成 reasoning，12 种 ID/缺失场景；整体 input 精确对比，其他 item ID、call_id、嵌套业务字段、summary/encrypted_content、缓存键与续接标识保持。
- Pi 本机 `openai-responses-shared.js:183-186` 原样重放 thinkingSignature；`transform-messages.js:94-113` 在 provider/api/model 相同时保留签名。后台路由切换不必然改变客户端身份，out_7 可继续被发送。
- 实际 screenshot 只证实 rs 前缀错误，不证明 item 类型或当前运行构建；离线合成并非账号在线验证。外部密文、服务端配对以及实际缓存命中仍未验证，不得宣称端到端成功。
- 验证：新增在树 RED 退出 101；GREEN 9 个 compat_ 通过（13 过滤）；真实 Pi 转换→真实 AIO 函数合成串联通过；rustfmt +1.90.0 --check、git diff --check PASS，rust-analyzer 0 diagnostics。具体 harness 命令与边界见 artifacts/verification.md 第 2 轮；临时探针尚在，复核时可只读检查，不运行工具、不访问凭据。
