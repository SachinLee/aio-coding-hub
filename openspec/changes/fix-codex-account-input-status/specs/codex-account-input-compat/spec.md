## ADDED Requirements

### Requirement: 账号 Responses 输入项元数据兼容

AIO SHALL 在既有 ChatGPT 账号 Responses 兼容路径中移除 `input` 数组内对象的直属 `status`，且 MUST 保持业务内容、工具关联和缓存相关标识。

- [ ] AC-001: 移除账号历史输入项直属 status
  - 场景：`input` 数组含历史消息、推理及工具调用等对象，各对象可带 `status`。
  - 动作：执行现有账号请求兼容处理。
  - 期望：所有输入项对象不再含直属 `status`；顺序、除 AC-004 所列不合规 reasoning ID 外的其他字段保持原值。
  - 验证方法：已有模块内增加失败回归测试，再验证修复后通过。
- [ ] AC-002: 内容与缓存上下文保持稳定
  - 场景：正文、工具参数或工具结果含业务 `status`，请求带 `prompt_cache_key` 和 `previous_response_id`。
  - 期望：嵌套业务字段、除 AC-004 所列不合规 reasoning ID 外的 `id`、`call_id`、正文、工具定义、缓存键和续接标识不变；源对象不被原地修改；重复兼容处理结果相同。
  - 验证方法：结构精确比较、源对象不变与幂等性断言。不以此证明真实服务端缓存命中率。
- [ ] AC-003: 兼容边界不扩大
  - 场景：普通 API 请求、非数组或缺失 `input`、数组内非对象元素。
  - 期望：普通 API 不新增兼容调用；非数组、缺失项及非对象元素保留原行为，不新增异常或递归清理。
  - 验证方法：调用点差异审查及边界值回归测试。
- [ ] AC-004: 仅省略账号历史推理项中格式不合规的 ID
  - 场景：`input` 含 `type: "reasoning"` 对象，直属 `id` 可为 `out_7`、空串、非字符串、缺失或以 `rs` 开头的字符串；同一数组还有消息和工具调用。
  - 期望：AIO SHALL 仅省略 reasoning 对象中已存在且非字符串或不以 `rs` 开头的直属 `id`，不按固定索引处理；以 `rs` 开头的字符串 ID 保持原值，缺失 ID 保持缺失。MUST NOT 伪造服务端 ID、修改其他类型的 ID 或工具关联 `call_id`；summary、encrypted_content 与嵌套业务字段不变。
  - 验证方法：在树回归先观察 RED 后 GREEN；结构精确比较、合法与非法 ID 边界、无 ID、工具关联、源对象不变及幂等性断言。离线验证不证明跨后端加密推理可重放或端到端成功。

#### Scenario: API 历史切换账号

- **WHEN** Pi 将带直属 `status` 的历史输入交给 AIO 的 ChatGPT 账号 Responses 分支
- **THEN** 发往上游的对应输入项不带该字段，除不合规 reasoning ID 省略之外的正文与关联标识保持不变
