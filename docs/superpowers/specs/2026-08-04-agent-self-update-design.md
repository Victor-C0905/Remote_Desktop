# Agent 自更新机制设计文档

> **创建日期**: 2026-08-04
> **状态**: 设计阶段
> **参考**: SSH (OpenSSH) 二进制更新机制

## 目标

实现客户端通过 QUIC 协议触发 agent 二进制更新，无需 SSH 登录服务器。

参考 SSH 的成熟实践：
- SSH 二进制更新通过 `systemctl restart` 实现（连接断开，客户端自动重连）
- 备份旧二进制以支持回滚
- 失败时保留旧版本运行

**设计原则**：对齐 SSH 的简洁性，只做必要的更新逻辑。

## 职责划分

```
传输层（现有）：              更新层（新增）：
├─ 分块传输二进制            ├─ 备份旧二进制
├─ 完整性校验（如有）        ├─ 替换二进制
└─ 重传机制（如有）          └─ 触发 restart
                              （不关心数据怎么来的）
```

**不在本次范围**：
- Manager 架构集成（FD 转移、Worker 路由、reload 热更新）
- 配置热重载（reload 机制）
- 自动下载源（GitHub Releases / apt 仓库）
- 客户端自动重连实现（由客户端项目负责）
- 传输完整性校验（由传输层/QUIC 协议保证）

## 架构

```
┌──────────────────────────────────────────────────────────┐
│  客户端                                                  │
│  ├─ 发起更新请求（UpdateAgentRequest）                   │
│  ├─ 分块上传二进制数据（复用现有 FileChunk 协议）        │
│  ├─ 收到 success 响应后断开                              │
│  └─ 自动重连                                             │
└──────────────────────────────────────────────────────────┘
                         │
                         ▼ (QUIC, Serde 协议)
┌──────────────────────────────────────────────────────────┐
│  Agent 主进程                                            │
│  ├─ handle_stream 接收 UpdateAgentRequest                │
│  ├─ 权限检查（session.uid == 0）                         │
│  ├─ 接收 FileChunk 数据，写入 /tmp/agent.new            │
│  ├─ 备份当前二进制（cp agent agent.bak）                 │
│  ├─ 替换二进制（mv /tmp/agent.new agent）                │
│  ├─ 设置权限（chmod 755）                                │
│  ├─ 返回 success 响应                                    │
│  └─ spawn: sh -c "sleep 1 && systemctl restart ..."     │
└──────────────────────────────────────────────────────────┘
                         │
                         ▼
              服务重启，客户端自动重连
```

## 组件

### 1. 协议扩展（src/protocol/serde.rs）

新增两个 Payload variants，复用现有 `FileChunk` 传输数据：

```rust
/// Agent 更新请求（客户端 → Agent）
#[serde(rename = "update_agent_request")]
UpdateAgentRequest {
    file_size: u64,         // 二进制文件总大小（字节）
},

/// Agent 更新响应（Agent → 客户端）
#[serde(rename = "update_agent_response")]
UpdateAgentResponse {
    success: bool,           // 是否成功
    message: String,         // 详细信息
},
```

**不新增** `UpdateAgentChunk`：复用现有 `FileChunk` 协议传输数据块，`session_id` 固定为 `"agent_update"`。

### 2. 自更新模块（src/updater.rs）

新增独立模块，职责单一：接收临时文件路径和当前二进制路径，执行更新流程。

```rust
/// 执行更新流程
///
/// # 参数
/// - `temp_binary`: 已接收的临时文件路径（/tmp/agent.new）
/// - `current_binary`: 当前二进制路径（/usr/local/bin/quireld）
///
/// # 流程
/// 1. 备份当前二进制到 .bak
/// 2. 替换二进制
/// 3. 设置权限 755
/// 4. spawn 触发 systemctl restart
pub async fn perform_update(
    temp_binary: &Path,
    current_binary: &Path,
) -> Result<UpdateResult>
```

```rust
pub struct UpdateResult {
    pub success: bool,
    pub message: String,
}
```

**核心实现**：

```rust
pub async fn perform_update(
    temp_binary: &Path,
    current_binary: &Path,
) -> Result<UpdateResult> {
    let backup_path = format!("{}.bak", current_binary.display());
    
    // 1. 备份当前二进制
    fs::copy(current_binary, &backup_path).await?;
    
    // 2. 替换二进制
    fs::rename(temp_binary, current_binary).await?;
    
    // 3. 设置权限 755
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(current_binary, PermissionsExt::from_mode(0o755)).await?;
    }
    
    // 4. 延迟 1 秒触发 restart（给响应发送时间）
    tokio::process::Command::new("sh")
        .arg("-c")
        .arg(format!("sleep 1 && systemctl restart {}", SERVICE_NAME))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    
    Ok(UpdateResult {
        success: true,
        message: "Update successful, restarting...".to_string(),
    })
}
```

### 3. 权限检查（内联在 quic.rs）

不新增独立函数，直接在 handle_stream 中检查：

```rust
Payload::UpdateAgentRequest { file_size } => {
    // 权限检查：只允许 root 用户更新
    if session.uid != 0 {
        return Envelope::new(request_id, Payload::UpdateAgentResponse {
            success: false,
            message: "Permission denied: only root can update agent".to_string(),
        });
    }
    // ... 后续流程
}
```

### 4. QUIC 请求处理（src/server/quic.rs）

在 `handle_stream` 中添加新分支，复用现有 `FileChunk` 接收逻辑：

```rust
Payload::UpdateAgentRequest { file_size } => {
    // 1. 权限检查
    if session.uid != 0 {
        let response = Envelope::new(request_id, Payload::UpdateAgentResponse {
            success: false,
            message: "Permission denied: only root can update agent".to_string(),
        });
        send_response(&mut stream, &response).await?;
        continue;
    }
    
    // 2. 响应接受
    let accept = Envelope::new(request_id, Payload::UpdateAgentResponse {
        success: true,
        message: format!("Ready to receive {} bytes", file_size),
    });
    send_response(&mut stream, &accept).await?;
    
    // 3. 接收 FileChunk 数据
    let temp_path = Path::new("/tmp/quireld.new");
    let mut file = tokio::fs::File::create(&temp_path).await?;
    let mut received = 0u64;
    
    while received < file_size {
        let chunk_envelope = Envelope::decode(&read_len_prefix(&mut stream).await?)?;
        if let Payload::FileChunk { data, size, .. } = chunk_envelope.payload {
            file.write_all(&data[..size as usize]).await?;
            received += size as u64;
        }
    }
    file.flush().await?;
    drop(file);
    
    // 4. 执行更新
    let current_binary = Path::new("/usr/local/bin/quireld");
    let result = updater::perform_update(&temp_path, current_binary).await;
    
    // 5. 返回结果
    let response = match result {
        Ok(update_result) => Envelope::new(request_id, Payload::UpdateAgentResponse {
            success: update_result.success,
            message: update_result.message,
        }),
        Err(e) => Envelope::new(request_id, Payload::UpdateAgentResponse {
            success: false,
            message: format!("Update failed: {}", e),
        }),
    };
    send_response(&mut stream, &response).await?;
}
```

## 数据流

```
客户端                              Agent
  │                                  │
  │── UpdateAgentRequest ──────────►│
  │   {file_size}                   │── if session.uid != 0 → error
  │◄── UpdateAgentResponse ─────────│
  │   {success: true, "Ready..."}   │
  │                                  │
  │── FileChunk ───────────────────►│
  │   {session_id: "agent_update",  │── 写入 /tmp/agent.new
  │    seq: 1, data, size}          │
  │                                  │
  │── FileChunk ───────────────────►│
  │   {seq: 2, data, size}          │── 写入 /tmp/agent.new
  │                                  │
  │   ... (重复直到传完)            │
  │                                  │
  │                                  │── cp agent agent.bak
  │                                  │── mv /tmp/agent.new agent
  │                                  │── chmod 755 agent
  │                                  │
  │◄── UpdateAgentResponse ─────────│
  │   {success: true,               │
  │    message: "restarting..."}    │
  │                                  │── spawn: sh -c "sleep 1 && systemctl restart"
  │                                  │
  │ X── 连接断开 ───────────────────│X  服务重启
  │                                  │
  │── 自动重连 ────────────────────►│
  │   ✅ 新版本运行                  │
```

## 错误处理与回滚

| 失败阶段 | 处理方式 | 服务影响 |
|---------|---------|---------|
| 权限检查失败 | 返回错误 | 无影响 |
| 数据传输中断 | 删除临时文件，返回错误 | 无影响 |
| 备份失败 | 返回错误，不替换 | 无影响 |
| 替换失败 | 返回错误，备份仍在 | 无影响，旧版本运行 |
| chmod 失败 | 返回警告，二进制已替换 | 下次 restart 可恢复 |
| systemctl restart 失败 | 二进制已替换，手动 restart 可恢复 | 需手动恢复 |

**关键原则**：任何阶段失败都保留旧版本运行，服务不中断。

## 文件结构

```
agent/src/
├── protocol/
│   └── serde.rs          # 修改：添加 2 个 Payload variants
├── server/
│   └── quic.rs           # 修改：handle_stream 添加 UpdateAgent 分支
├── updater.rs            # 新增：自更新核心逻辑（备份/替换/restart）
└── main.rs               # 不修改
```

## 不修改的部分

- main.rs（不集成 Manager 架构）
- Manager/Worker 相关代码（本次不使用）
- WebSocket 服务器
- 现有的 PTY 管理、文件操作、命令执行
- systemd service 文件（继续用 restart，不启用 reload）
- config.rs（路径硬编码，不新增配置项）

## 测试策略

### 单元测试（src/updater.rs）

```rust
#[cfg(test)]
mod tests {
    use tempfile::tempdir;
    
    #[tokio::test]
    async fn test_backup_current_binary() {
        // 创建临时目录模拟
        // 验证备份文件生成
    }
    
    #[tokio::test]
    async fn test_replace_binary() {
        // 验证文件替换成功
    }
    
    #[tokio::test]
    async fn test_replace_binary_failure_preserves_original() {
        // 替换失败时，原文件应保留
    }
}
```

### 集成测试（tests/update_test.rs）

```rust
#[tokio::test]
async fn test_update_agent_permission_denied() {
    // 非特权用户发起更新 → 被拒绝
}

#[tokio::test]
async fn test_update_agent_full_flow() {
    // 模拟客户端发送更新请求
    // 验证临时文件创建、备份、替换
}
```

## 验证标准

1. **功能验证**：
   - 客户端发送更新请求 + 数据块 → agent 接收、备份、替换、restart
   - 客户端自动重连后，新版本运行

2. **安全验证**：
   - 非 root 用户发起更新被拒绝
   - 任何阶段失败，服务不中断

3. **回滚验证**：
   - 替换失败时，旧二进制仍在运行
   - 备份文件存在

4. **编译验证**：
   - `cargo build --release` 零错误
   - `cargo test` 全部通过
