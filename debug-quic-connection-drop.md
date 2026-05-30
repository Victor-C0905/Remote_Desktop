# Debug Session: quic-connection-drop

## Status: [CLOSED] ✅ FIXED

## Problem Description
- 连接服务器后几秒钟，Agent 提示连接关闭
- 用户未主动关闭连接
- UI 端连接状态仍显示为已连接

## Root Cause Analysis

| 假设 | 状态 | 证据 |
|------|------|------|
| H1: QUIC 空闲超时 | ✅ 确认 | Agent `build_server_config` 使用默认配置，未设置 `max_idle_timeout` |
| H2: 缺少心跳机制 | ✅ 确认 | 客户端消息循环只等待用户请求，无定期 ping |
| H3: handle_connection 逻辑 | ❌ 正常 | `accept_bi()` 返回 Err 时退出循环是正确行为 |
| H4: 客户端消息循环 | ⚠️ 相关 | 循环阻塞在 `rx.recv()`，无法检测连接关闭 |
| H5: UI 状态同步 | ⚠️ 相关 | `connection-lost` 事件只在循环退出时发送 |

**根因**: Quinn 默认 QUIC 空闲超时 + 客户端无心跳机制 → 连接因无活动自动关闭

## Fix Summary

### 1. Agent 端修复 (quic.rs)
```rust
fn build_server_config(...) -> Result<quinn::ServerConfig> {
    let mut quic_config = quinn::ServerConfig::with_single_cert(certs, key)?;
    
    // 配置传输参数，禁用空闲超时
    let mut transport = quinn::TransportConfig::default();
    transport.max_idle_timeout(None); // 禁用空闲超时
    transport.keep_alive_interval(Some(Duration::from_secs(5))); // 保持活跃
    
    quic_config.transport_config(Arc::new(transport));
    Ok(quic_config)
}
```

### 2. 客户端修复 (connection.rs)
- 添加心跳任务：每 10 秒发送 ping 保持连接活跃
- 添加连接状态监听：监听 `conn.closed()` 事件，及时通知 UI
- 检查连接状态：发送请求前检查 `close_reason()`

## Verification
- 用户确认：连接保持稳定，Agent 不再提示连接关闭
- 心跳正常工作：每 10 秒发送 ping

## Cleanup
- 调试文件已更新
- 无需清理插桩代码（直接修复）