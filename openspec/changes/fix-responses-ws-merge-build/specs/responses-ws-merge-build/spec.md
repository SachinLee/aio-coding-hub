## ADDED Requirements

### Requirement: 合并后 Rust 编译与既有续传行为兼容
系统 SHALL 修复编译错误，同时保留两侧已实现的续传校验和回退条件。

- [ ] AC-001: Rust 编译检查不再报告未定义的 continuation
  - 场景：合并 main 后运行 Rust 编译检查。
  - 动作：执行 `cargo check --locked --offline --manifest-path src-tauri/Cargo.toml`。
  - 期望：退出码为 0，且无 E0425。
- [ ] AC-002: 保留续传归属校验与 HTTP 回退条件
  - 场景：含 previous_response_id 的请求不能直接复用 WS。
  - 动作：读取连接已记录的续传状态，计算 require_http。
  - 期望：继续使用 continuation_provider_id() 校验本地 WS/预热归属；仅当原有 !allow_ws 条件或同供应商同响应的 HTTP 续传状态匹配时要求 HTTP。
  - 禁止：删除校验、锁跨 await、触碰其他未提交修改。
  - 验证方法：最小 diff 检查、Rust 编译与格式检查。

#### Scenario: 合并后的错误分支读取续传状态
- **WHEN** send() 进入 ContextLost 返回分支
- **THEN** 系统 SHALL 从 connection.continuation 获取有效局部锁引用后执行原有 require_http 判断，并在离开分支时释放锁。
