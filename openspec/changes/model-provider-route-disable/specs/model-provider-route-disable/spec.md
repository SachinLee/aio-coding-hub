## ADDED Requirements

### Requirement: 按模型禁用指定供应商路由
系统 MUST 按 `(cli_key, model_id, provider_id)` 保存路由开关，缺省启用，禁用不影响同供应商其他模型或其他 CLI。

#### Scenario: 单路由禁用与恢复
- **WHEN** 用户在模型目录关闭模型 A 的供应商 1 路由
- **THEN** 新请求只从模型 A 的其他合格供应商中选路，供应商 1 仍可提供模型 B
- **AND** 该路由仍显示且可重新开启；保存失败时回滚并提示

#### Scenario: 显式指定与全部禁用
- **WHEN** 请求显式指定已禁用路由，或该模型所有路由都已禁用
- **THEN** 网关返回明确错误，不向已禁用上游发送，不静默替换强制指定供应商

### Requirement: 路由配置生命周期
系统 MUST 在模型目录刷新、模型暂时移除和应用重启后保留开关；供应商删除时 MUST 清理其路由配置。保存成功后的新请求 MUST 读取新配置，在途请求不切换供应商。

#### Scenario: 刷新与删除
- **WHEN** 模型目录刷新或模型短暂缺失
- **THEN** 禁用配置保留；供应商删除时其配置被清理

### Requirement: Codex 上下文所有权
系统 MUST 保留现有 Codex Responses 上下文安全机制。仅同连接记录匹配的 continuation/prewarm 可保留原供应商路由，不得凭客户端 response ID 绕过禁用。

#### Scenario: 续接与新请求
- **WHEN** 原供应商路由被禁用而已验证的 WebSocket 续接仍需原上下文
- **THEN** 原供应商可继续服务该续接，且 response ID 不能发送给另一供应商
- **AND** 下一次独立选路和显式指定禁用路由的请求遵守开关
