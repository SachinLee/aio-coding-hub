# 技术方案

## 数据与应用层

SQLite v42→v43 新增 `model_provider_route_overrides`，主键 `(cli_key, model_id, provider_id)`，缺省允许路由。启用/禁用只写该表，不改供应商全局启用、模型策略或模型元信息。使用删除触发器覆盖常规供应商删除与配置导入替换；ensure 路径也安装触发器并清理旧孤儿记录，兼容已使用开发版 v43 的数据库。

模型目录继续返回禁用路由，通过 `routeEnabled` 投影状态。IPC `model_catalog_route_set_enabled` 在应用层校验供应商归属、模型 ID，再写入。前端使用现有开关、乐观更新、失败回滚和查询失效机制。

## 网关

沿用请求级 `retain_routable_providers_for_cli`，按请求模型读取禁用集合后过滤候选；配置不进入供应商全局缓存。返回禁用供应商集合给中间件，用于明确的强制路由错误与 `provider_model_route_filter` 诊断，不新增一套错误协议。

Codex Responses WebSocket 的 `previous_response_id` 只有匹配同连接已记录的 continuation/prewarm 时才获得原供应商豁免。豁免仅作用于路由开关，不绕过供应商启用、模型全局禁用、模型策略及原有传输安全校验。显式供应商指定不获得豁免。所有真正发送仍由现有 socket key、协议所有权和 recovery 校验保护，绝不迁移上游 response ID。

## 本轮验证边界

不跑单元测试，不启动重型编译。可补充精简回归用例，执行定向格式与 `git diff --check`。之前编译/类型检查结果是历史证据，不代表本轮收尾代码通过编译。独立复核尚未获得授权和执行证据，保留 `UNVERIFIED`。
