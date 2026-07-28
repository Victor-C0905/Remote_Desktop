# 安全与监控功能集成设计

## 概述

本文档描述了如何将已实现但未集成的安全与监控功能集成到 gnome-remote-agent 的主业务流程中。

## 背景

### 已实现功能
经过代码审查，发现以下功能已完整实现但未集成：

1. **会话超时机制**
   - 文件：`src/auth/session.rs`
   - 字段：`last_activity`, `max_idle_time`
   - 方法：`is_expired()`, `touch()`, `remaining_time()`

2. **速率限制器**
   - 文件：`src/auth/rate_limiter.rs`
   - 功能：IP速率限制、用户名锁定
   - 状态：✅ 已部分集成（IP限制已实现）

3. **统计管理器**
   - 文件：`src/auth/stats.rs`
   - 功能：认证统计、连接统计、性能指标
   - 状态：✅ 已部分集成（认证统计已实现）

4. **性能监控**
   - 文件：`src/auth/stats.rs`
   - 方法：`record_response_time()`, `record_file_transfer_bytes()`, `record_terminal_bytes()`
   - 状态：❌ 未集成

### 当前状态分析

通过代码探索发现：
- ✅ 速率限制器已集成（`src/server/quic.rs:408`）
- ✅ 认证统计已集成（`src/server/quic.rs:415`）
- ✅ 会话超时框架已实现（`ConnectionContext` 中有相关字段）
- ❌ 会话超时检查未在请求处理中调用
- ❌ 性能监控未在 handler 中埋点
- ❌ 统计查询 API 未实现

## 设计目标

### 主要目标
1. 补齐会话超时检查的调用
2. 添加性能监控埋点
3. 实现统计查询 API

### 非目标
- 不重构现有架构
- 不修改已实现的核心逻辑
- 不添加新功能

## 架构设计

### 整体架构

```
┌─────────────────────────────────────────────────────────────┐
│                        Agent 架构                            │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  QUIC 连接层 (quic.rs)                                       │
│  ├─ ConnectionContext                                        │
│  │  ├─ session_last_activity ───► 会话超时检查              │
│  │  └─ touch_session() ─────────► 更新活动时间              │
│  ├─ AuthRateLimiter ───────────► IP速率限制（已集成）        │
│  └─ StatsManager ──────────────► 认证统计（已集成）          │
│                                                              │
│  请求处理层 (handler.rs)                                     │
│  ├─ handle_envelope()                                        │
│  │  ├─ 会话超时检查 ◄─────────► 新增                        │
│  │  ├─ 响应时间记录 ◄─────────► 新增                        │
│  │  └─ GetStats API ◄──────────► 新增                       │
│  └─ StatsManager ──────────────► 性能监控埋点                │
│                                                              │
│  业务层                                                      │
│  ├─ 文件操作 ─────────────────► 文件传输统计                 │
│  └─ 终端操作 ─────────────────► 终端输出统计                 │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

### 数据流

```
客户端请求
    │
    ▼
QUIC连接处理
    │
    ├─► 检查会话超时 ◄── 新增
    │       │
    │       ├─► 超时：返回错误
    │       └─► 未超时：继续
    │
    ├─► 更新会话活动时间 ◄── 新增
    │
    ▼
请求处理 (handler.rs)
    │
    ├─► 记录请求开始时间 ◄── 新增
    │
    ├─► 处理请求
    │       │
    │       ├─► GetStats：返回统计数据 ◄── 新增
    │       └─► 其他请求：正常处理
    │
    └─► 记录响应时间 ◄── 新增
            │
            ▼
        返回响应
```

## 详细设计

### 1. 会话超时检查

#### 修改位置
- 文件：`src/server/quic.rs`
- 函数：`handle_stream()`

#### 实现逻辑

```rust
// 在处理请求前检查会话超时
if let Some(session) = ctx.session.lock().await.as_ref() {
    if ctx.is_session_timeout() {
        tracing::warn!(
            "会话已超时: username={}, idle_time={}s",
            session.username,
            current_timestamp_secs() - ctx.session_last_activity.load(Ordering::Relaxed)
        );

        // 记录会话超时统计
        stats_manager.record_session_timeout();

        // 返回错误响应
        let error_payload = Payload::Error {
            code: 401,
            message: "会话已过期，请重新登录".to_string(),
        };

        // 发送错误响应并关闭连接
        send_response(&mut send, envelope.request_id, error_payload).await?;
        connection.close(0u32.into(), b"session_timeout");
        return Ok(());
    }

    // 更新会话活动时间
    ctx.touch_session();
}
```

#### 注意事项
- 只在已认证的请求中检查会话超时
- 超时后记录统计并关闭连接
- 每次请求都更新活动时间

### 2. 性能监控埋点

#### 修改位置
- 文件：`src/handler.rs`
- 函数：`handle_envelope()`

#### 实现逻辑

```rust
use std::time::Instant;

pub async fn handle_envelope(
    envelope: Envelope,
    session: Arc<UserSession>,
    stats_manager: Arc<StatsManager>,
    // ... 其他参数
) -> Result<Envelope> {
    // 记录请求开始时间
    let start = Instant::now();

    // 处理请求
    let result = match envelope.payload {
        Payload::ReadFile { path } => {
            handle_read_file(path, session).await
        }
        // ... 其他请求处理
    };

    // 记录响应时间
    let elapsed = start.elapsed();
    stats_manager.record_api_response_time(elapsed);

    // 返回结果
    result
}
```

#### 扩展埋点

在文件传输和终端操作中添加：

```rust
// 文件传输完成时
stats_manager.record_file_transfer_bytes(bytes_transferred);

// 终端输出时
stats_manager.record_terminal_bytes(output_bytes);
```

### 3. 统计查询 API

#### 修改位置
- 文件：`src/protocol.rs` - 添加 Payload 类型
- 文件：`src/handler.rs` - 添加处理逻辑

#### Payload 定义

```rust
pub enum Payload {
    // ... 现有 Payload

    /// 统计查询请求
    GetStats {
        stats_type: String, // "auth" | "connection" | "performance" | "all"
    },

    /// 统计查询响应
    StatsResponse {
        auth: Option<AuthStatsSnapshot>,
        connection: ConnectionStatsSnapshot,
        performance: Option<PerformanceStatsSnapshot>,
    },
}
```

#### 处理逻辑

```rust
Payload::GetStats { stats_type } => {
    // 权限检查：只有 root 用户可以查看全局统计
    if session.uid != 0 && stats_type != "connection" {
        return Ok(Envelope {
            request_id: envelope.request_id,
            payload: Payload::Error {
                code: 403,
                message: "权限不足".to_string(),
            },
        });
    }

    // 获取统计数据
    let stats = match stats_type.as_str() {
        "auth" => {
            let snapshot = stats_manager.get_auth_stats();
            Payload::StatsResponse {
                auth: Some(snapshot),
                connection: ConnectionStatsSnapshot::default(),
                performance: None,
            }
        },
        "connection" => {
            let snapshot = if session.uid == 0 {
                stats_manager.get_connection_stats()
            } else {
                stats_manager.get_connection_stats_for_user(session.uid)
            };
            Payload::StatsResponse {
                auth: None,
                connection: snapshot,
                performance: None,
            }
        },
        "performance" => {
            let snapshot = stats_manager.get_performance_stats();
            Payload::StatsResponse {
                auth: None,
                connection: ConnectionStatsSnapshot::default(),
                performance: Some(snapshot),
            }
        },
        "all" => {
            Payload::StatsResponse {
                auth: Some(stats_manager.get_auth_stats()),
                connection: stats_manager.get_connection_stats(),
                performance: Some(stats_manager.get_performance_stats()),
            }
        },
        _ => {
            return Ok(Envelope {
                request_id: envelope.request_id,
                payload: Payload::Error {
                    code: 400,
                    message: "未知的统计类型".to_string(),
                },
            });
        }
    };

    Ok(Envelope {
        request_id: envelope.request_id,
        payload: stats,
    })
}
```

#### 权限控制
- root 用户：可以查看所有统计
- 普通用户：只能查看个人连接统计

## 实施计划

### 并行实施策略

使用 3 个独立的子代理并行处理：

#### 子代理 1：会话超时检查
- 任务：在 `quic.rs` 中添加会话超时检查
- 文件：`src/server/quic.rs`
- 代码量：约 20 行
- 预计时间：15 分钟

#### 子代理 2：性能监控埋点
- 任务：在 `handler.rs` 中添加性能监控
- 文件：`src/handler.rs`
- 代码量：约 30 行
- 预计时间：20 分钟

#### 子代理 3：统计查询 API
- 任务：实现 GetStats Payload 处理
- 文件：`src/protocol.rs`, `src/handler.rs`
- 代码量：约 50 行
- 预计时间：30 分钟

### 实施顺序

由于三个任务相互独立，可以完全并行执行。所有子代理完成后，进行集成测试。

### 验证步骤

1. **会话超时验证**
   - 登录后等待24小时（或临时调整超时时间为1分钟）
   - 发送请求，验证返回"会话已过期"错误

2. **性能监控验证**
   - 执行多个文件操作
   - 查询统计数据，验证响应时间记录

3. **统计查询验证**
   - 使用 root 用户查询所有统计
   - 使用普通用户查询连接统计
   - 验证权限控制生效

## 风险评估

### 潜在风险

1. **会话超时检查**
   - 风险：可能误判活跃用户为超时
   - 缓解：确保每次请求都调用 `touch_session()`

2. **性能监控**
   - 风险：增加请求处理延迟
   - 缓解：使用异步统计记录，不阻塞主流程

3. **统计查询**
   - 风险：泄露敏感信息
   - 缓解：严格的权限检查

### 回滚计划

每个功能都是独立的，如果出现问题：
- 会话超时：临时禁用检查逻辑
- 性能监控：移除埋点代码
- 统计查询：移除 API 处理逻辑

## 成功标准

1. ✅ 编译无警告（除了预期的未使用功能警告）
2. ✅ 会话超时功能正常工作
3. ✅ 性能监控数据正确记录
4. ✅ 统计查询 API 返回正确数据
5. ✅ 权限控制正确实施

## 后续工作

完成本次集成后，后续可以：
1. 添加配置文件支持（速率限制参数、会话超时时间）
2. 前端集成统计展示面板
3. 添加告警系统（基于统计数据触发告警）

## 附录

### 相关文件

- `src/auth/session.rs` - 会话管理
- `src/auth/rate_limiter.rs` - 速率限制
- `src/auth/stats.rs` - 统计管理
- `src/server/quic.rs` - QUIC 连接处理
- `src/handler.rs` - 请求处理
- `src/protocol.rs` - Payload 定义

### 参考资料

- [编译警告分析报告](../../agent-warnings-analysis.md)
- [项目约束文档](../../project-memory.md)