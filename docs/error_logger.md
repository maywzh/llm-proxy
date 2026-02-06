# 错误日志字符串截断

## 规则

- 字符串按字节长度上限截断，但会回退到合法的 UTF-8 边界，避免 panic。
- 仅截断字符串值，JSON 的结构保持不变。

## 相关实现

- 截断实现：[rust-server/src/core/error_logger.rs](../rust-server/src/core/error_logger.rs#L233)
- UTF-8 边界处理：[rust-server/src/core/error_logger.rs](../rust-server/src/core/error_logger.rs#L246)

**Last Updated**: 2026-02-08
