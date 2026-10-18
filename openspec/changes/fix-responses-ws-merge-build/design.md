# 最小方案

## 背景与边界
main 的错误分支引用局部 continuation；personal 将原先的锁读取封装进 continuation_provider_id()。Git 自动合并造成语义冲突。

## 决策
在 `if !valid || !allow_ws || reusable.is_none()` 内增加一行 `let continuation = connection.continuation.lock_or_recover();`。必须在 continuation_provider_id() 调用结束后才取锁，避免同一互斥锁重入；失败分支立即返回，不跨 await。

不删除 require_http 判断，不改回旧的供应商归属校验，不引入新 helper 或测试框架。编译器即本次未定义变量缺陷的回归检查；不为编译错误新增冗余单元测试。

## 追溯与回滚
AC-001：Rust 编译检查。AC-002：保留原表达式，仅增加分支局部锁，检查 diff 与格式。回滚时仅删除本次新增行；不恢复整个文件，以免覆盖其他修改。
