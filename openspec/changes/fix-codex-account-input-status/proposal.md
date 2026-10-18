# Codex 账号历史输入 status 兼容修复

## Why

用户报告 Pi 同会话从 API 切换到账号后出现 `Unknown parameter: 'input[19].status'`。本机 Pi 的历史序列化会添加输入项 `status`，AIO 当前账号兼容函数仅过滤顶层字段，内部字段仍被转发。真实失败请求尚未抓取；不能保证消除所有其他协议差异。

## What Changes

- 在现有 ChatGPT 账号 Responses 请求兼容函数中清理 `input` 数组各对象自身的 `status`。
- 按用户新增批准，仅省略 `type: "reasoning"` 对象中非字符串或不以 `rs` 开头的直属 `id`；缺失 ID 保持缺失，不伪造服务端 ID，不删除推理内容。
- 不递归清理业务数据，除新增 AC-004 允许省略的 reasoning ID 外，不改变其他 ID、正文、顺序、工具关联、缓存键和续接标识；每次使用固定规则。
- 在既有测试模块补充回归，保持普通 API 请求路径不变。

## Capabilities

### New Capabilities

- `codex-account-input-compat`：账号后端的历史输入元数据兼容。

### Modified Capabilities

- 无。

## Impact

生产代码仅涉及 `src-tauri/src/gateway/proxy/protocol_bridge/cx2cc/mod.rs`。复用现有账号检测与转发处理，不增加依赖、开关、重试或持久化变更。

## 授权与边界

用户已确认创建 `fix-codex-account-input-status`。档位为 `standard`；不改变认证、授权或凭据处理。用户随后已批准方案与实施，并授权一次只读独立复核；验收与归档仍由用户决定。
用户在新增 `input[46].id = out_7` 错误反馈后选择“批准继续修复”，批准调整原 AC-002 的 ID 保留约束，授权此扩展范围一次 full 独立复核；不提交、不发布。上一轮 CLEAN 仅覆盖 status，当前新增范围单独验证。

验收标准以 `specs/codex-account-input-compat/spec.md` 的 AC-001 至 AC-004 为唯一来源。真实账号端到端行为与缓存命中率需要用户环境复验，不承诺缓存跨上游复用。
