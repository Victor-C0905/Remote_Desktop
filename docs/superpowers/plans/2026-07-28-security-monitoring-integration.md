# 安全与监控功能集成实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将已实现但未集成的安全与监控功能集成到 gnome-remote-agent 主业务流程中

**Architecture:** 在现有代码基础上，在关键位置补充调用已实现的功能模块，无需重构架构

**Tech Stack:** Rust, Tokio, Quinn (QUIC), sled (统计存储)

---

## 文件结构

本计划将修改以下文件：

```
agent/src/
├── server/quic.rs       # 任务1：会话超时检查
├── handler.rs           # 任务2：性能监控埋点 + 任务3：统计查询API
└── protocol.rs          # 任务3：Payload类型定义
```

**职责划分：**
- `quic.rs`: QUIC连接处理，包含会话超时检查
- `handler.rs`: 请求处理中心，包含性能监控和统计查询
- `protocol.rs`: Payload类型定义

---

## 任务分解

由于三个任务相互独立，可并行执行。本计划按任务编号组织，支持多个子代理同时实施。

---

### 任务 1: 会话超时检查集成

**目标：** 在每次请求处理前检查会话是否超时，超时则返回错误并关闭连接

**文件：**
- 修改：`agent/src/server/quic.rs:562-580` (handle_stream函数)

**背景：**
- `ConnectionContext` 已有 `session_last_activity` 字段和 `is_session_timeout()` 方法
- 只需要在请求处理前调用这些方法

#### 步骤

- [ ] **步骤 1.1：添加会话超时检查逻辑**

在 `handle_stream` 函数中，在处理请求前添加会话超时检查：

```rust
// 文件：agent/src/server/quic.rs
// 位置：handle_stream 函数，在 let result = handler::handle_envelope 之前

// 检查会话超时（只在已认证的连接中）
if let Some(session) = ctx.session.lock().await.as_ref() {
    if ctx.is_session_timeout() {
        let idle_secs = current_timestamp_secs() - ctx.session_last_activity.load(Ordering::Relaxed);

        tracing::warn!(
            "会话已超时: username={}, idle_time={}s",
            session.username,
            idle_secs
        );

        // 记录会话超时统计
        stats_manager.record_session_timeout();

        // 返回错误响应
        let error_payload = Payload::Error {
            code: 401,
            message: "会话已过期，请重新登录".to_string(),
        };

        // 发送错误响应
        let error_env = Envelope {
            request_id: 0,
            payload: error_payload,
        };
        let error_bytes = serde_json::to_vec(&error_env)?;
        send.write_all(&error_bytes).await?;

        // 关闭连接
        connection.close(0u32.into(), b"session_timeout");
        return Ok(());
    }

    // 更新会话活动时间
    ctx.touch_session();
}
```

- [ ] **步骤 1.2：编译验证**

运行编译检查：

```bash
cd agent
cargo check
```

预期：编译成功，无新增错误

- [ ] **步骤 1.3：测试验证**

测试步骤：
1. 临时修改 `SESSION_TIMEOUT_SECS` 为 60（1分钟）
2. 登录后等待超过1分钟
3. 发送请求，验证返回"会话已过期"错误
4. 检查日志：应看到"会话已超时"警告

- [ ] **步骤 1.4：提交**

```bash
git add agent/src/server/quic.rs
git commit -m "feat(auth): integrate session timeout check

- Add session timeout check before processing requests
- Update session activity time on each request
- Record session timeout statistics

Ref: docs/superpowers/specs/2026-07-28-security-monitoring-integration-design.md"
```

---

### 任务 2: 性能监控埋点

**目标：** 在请求处理中添加响应时间记录，并扩展文件传输和终端统计

**文件：**
- 修改：`agent/src/handler.rs:handle_envelope` 函数
- 修改：`agent/src/handler.rs:handle_file_transfer` 函数（文件传输统计）
- 修改：`agent/src/server/quic.rs:handle_pty_output` 函数（终端统计）

#### 步骤

- [ ] **步骤 2.1：在 handler_envelope 中添加响应时间记录**

在 `handle_envelope` 函数开头添加计时：

```rust
// 文件：agent/src/handler.rs
// 位置：handle_envelope 函数开头

use std::time::Instant;

pub async fn handle_envelope(
    envelope: Envelope,
    session: Arc<UserSession>,
    stats_manager: Arc<StatsManager>,
    pty_manager: Arc<Mutex<PtyManager>>,
    subscription_manager: Arc<SubscriptionManager>,
    event_bus: Arc<EventBus>,
) -> Result<Envelope> {
    // 记录请求开始时间
    let start = Instant::now();

    // 处理请求
    let result = match envelope.payload {
        Payload::ReadFile { path } => {
            handle_read_file(path, session).await
        }
        Payload::WriteFile { path, content } => {
            handle_write_file(path, content, session).await
        }
        // ... 其他 Payload 处理
        _ => Err(anyhow::anyhow!("未知的 Payload 类型")),
    };

    // 记录响应时间
    let elapsed = start.elapsed();
    stats_manager.record_api_response_time(elapsed);

    result
}
```

- [ ] **步骤 2.2：在文件传输完成时记录传输量**

在文件传输处理函数中添加统计：

```rust
// 文件：agent/src/handler.rs
// 位置：handle_file_transfer 函数，在传输完成后

async fn handle_file_transfer(
    // ... 参数
    stats_manager: Arc<StatsManager>,
) -> Result<Envelope> {
    // ... 文件传输逻辑

    // 记录传输字节数
    let bytes_transferred = content.len() as u64;
    stats_manager.record_file_transfer_bytes(bytes_transferred);

    // 返回响应
    Ok(Envelope {
        request_id: envelope.request_id,
        payload: Payload::FileTransferResponse { success: true },
    })
}
```

- [ ] **步骤 2.3：在终端输出时记录流量**

在终端输出处理中添加统计：

```rust
// 文件：agent/src/server/quic.rs
// 位置：handle_pty_output 函数，在发送输出后

async fn handle_pty_output(
    // ... 参数
    stats_manager: Arc<StatsManager>,
) -> Result<()> {
    // ... 终端输出逻辑

    // 记录终端输出字节数
    let output_bytes = output.len() as u64;
    stats_manager.record_terminal_bytes(output_bytes);

    Ok(())
}
```

- [ ] **步骤 2.4：编译验证**

```bash
cd agent
cargo check
```

预期：编译成功，无新增错误

- [ ] **步骤 2.5：测试验证**

测试步骤：
1. 执行多次文件读写操作
2. 使用终端执行命令
3. 查询统计数据（需要任务3完成后）
4. 验证响应时间和传输量记录正确

- [ ] **步骤 2.6：提交**

```bash
git add agent/src/handler.rs agent/src/server/quic.rs
git commit -m "feat(monitoring): add performance monitoring instrumentation

- Record API response time in handle_envelope
- Record file transfer bytes in file operations
- Record terminal output bytes in PTY handling

Ref: docs/superpowers/specs/2026-07-28-security-monitoring-integration-design.md"
```

---

### 任务 3: 统计查询 API 实现

**目标：** 添加 GetStats Payload 处理，支持客户端查询统计数据

**文件：**
- 修改：`agent/src/protocol.rs` - 添加 Payload 类型
- 修改：`agent/src/handler.rs` - 添加处理逻辑

#### 步骤

- [ ] **步骤 3.1：添加 Payload 类型定义**

在 `Payload` 枚举中添加新类型：

```rust
// 文件：agent/src/protocol.rs
// 位置：Payload 枚举定义

pub enum Payload {
    // ... 现有 Payload

    /// 统计查询请求
    GetStats {
        /// 统计类型：auth, connection, performance, all
        stats_type: String,
    },

    /// 统计查询响应
    StatsResponse {
        /// 认证统计（可选）
        auth: Option<AuthStatsSnapshot>,
        /// 连接统计
        connection: ConnectionStatsSnapshot,
        /// 性能统计（可选）
        performance: Option<PerformanceStatsSnapshot>,
    },
}
```

- [ ] **步骤 3.2：导入统计快照类型**

在 `handler.rs` 中添加导入：

```rust
// 文件：agent/src/handler.rs
// 位置：文件顶部导入区

use crate::auth::stats::{AuthStatsSnapshot, ConnectionStatsSnapshot, PerformanceStatsSnapshot};
```

- [ ] **步骤 3.3：添加 GetStats 处理逻辑**

在 `handle_envelope` 的 match 分支中添加：

```rust
// 文件：agent/src/handler.rs
// 位置：handle_envelope 函数的 match envelope.payload 分支

Payload::GetStats { stats_type } => {
    // 权限检查：只有 root 用户可以查看全局统计
    if session.uid != 0 && stats_type != "connection" {
        return Ok(Envelope {
            request_id: envelope.request_id,
            payload: Payload::Error {
                code: 403,
                message: "权限不足：只有root用户可以查看此统计".to_string(),
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
                    message: format!("未知的统计类型: {}", stats_type),
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

- [ ] **步骤 3.4：实现 ConnectionStatsSnapshot 默认值**

确保 `ConnectionStatsSnapshot` 实现了 `Default` trait：

```rust
// 文件：agent/src/auth/stats.rs
// 位置：ConnectionStatsSnapshot 结构体定义

impl Default for ConnectionStatsSnapshot {
    fn default() -> Self {
        Self {
            active_connections: 0,
            total_connections: 0,
            normal_disconnects: 0,
            timeout_disconnects: 0,
            error_disconnects: 0,
        }
    }
}
```

- [ ] **步骤 3.5：编译验证**

```bash
cd agent
cargo check
```

预期：编译成功，无新增错误

- [ ] **步骤 3.6：测试验证**

测试步骤：
1. 启动 Agent 服务
2. 使用 root 用户登录
3. 发送 GetStats 请求：
   ```json
   {
     "stats_type": "all"
   }
   ```
4. 验证返回统计数据
5. 使用普通用户登录
6. 发送 GetStats 请求（stats_type: "auth"）
7. 验证返回权限错误

- [ ] **步骤 3.7：提交**

```bash
git add agent/src/protocol.rs agent/src/handler.rs agent/src/auth/stats.rs
git commit -m "feat(api): add GetStats API for querying monitoring data

- Add GetStats and StatsResponse Payload types
- Implement permission control (root vs. normal user)
- Support querying auth, connection, performance stats

Ref: docs/superpowers/specs/2026-07-28-security-monitoring-integration-design.md"
```

---

## 集成测试

所有任务完成后，进行端到端测试：

- [ ] **集成测试 1：会话超时功能**

1. 临时设置超时时间为 60 秒
2. 登录后等待 70 秒
3. 发送任意请求
4. 验证返回"会话已过期"错误
5. 验证统计数据中会话超时计数增加

- [ ] **集成测试 2：性能监控功能**

1. 执行 10 次文件读写操作
2. 使用终端执行多个命令
3. 查询性能统计（GetStats: performance）
4. 验证响应时间和传输量记录

- [ ] **集成测试 3：统计查询功能**

1. root 用户查询所有统计
2. 验证认证、连接、性能数据完整
3. 普通用户查询连接统计
4. 验证只能看到个人数据
5. 普通用户尝试查询认证统计
6. 验证返回权限错误

---

## 回滚计划

如果出现问题，可按以下步骤回滚：

1. **会话超时问题**：
   ```rust
   // 临时禁用会话超时检查
   // if ctx.is_session_timeout() { ... }
   ```

2. **性能监控问题**：
   ```rust
   // 注释掉响应时间记录
   // stats_manager.record_api_response_time(elapsed);
   ```

3. **统计查询问题**：
   ```rust
   // 移除 GetStats 处理分支
   Payload::GetStats { .. } => {
       Ok(Envelope {
           request_id: envelope.request_id,
           payload: Payload::Error {
               code: 501,
               message: "功能未实现".to_string(),
           },
       })
   }
   ```

---

## 成功标准

完成以下所有验证即视为成功：

1. ✅ 编译通过，无新增错误
2. ✅ 会话超时功能正常（超时后拒绝请求）
3. ✅ 性能监控数据正确记录（响应时间、传输量）
4. ✅ 统计查询 API 返回正确数据
5. ✅ 权限控制生效（root vs. 普通用户）
6. ✅ 集成测试全部通过

---

## 后续优化

完成基础集成后，可考虑以下优化：

1. **配置文件支持**：从配置文件读取速率限制参数、会话超时时间
2. **前端集成**：在系统监控应用中展示统计数据
3. **告警系统**：基于统计数据触发告警（如连接数异常）
4. **历史数据**：持久化统计数据，支持历史查询
5. **性能优化**：异步统计记录，避免阻塞主流程