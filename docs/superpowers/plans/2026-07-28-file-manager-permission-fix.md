# 文件管理器权限修复实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 移除 Agent 层面的权限检查，让文件管理器完全依赖 Linux 文件系统权限，与终端体验一致。

**Architecture:** 移除 `check_path_permission` 和 `check_allowed_paths` 函数，简化所有请求处理函数，移除 `allowed_paths` 配置项。保留 `UserExecutor` 提供的用户隔离机制。

**Tech Stack:** Rust, Linux 文件系统权限, User Namespace

---

## 文件结构

**修改的文件**：
- `agent/src/handler.rs`：移除权限检查函数和调用（核心修改）
- `agent/src/config.rs`：移除 `allowed_paths` 字段
- `agent/config.toml`：移除 `allowed_paths` 配置项

**不受影响的文件**：
- `agent/src/auth/executor.rs`：UserExecutor 逻辑保持不变
- `agent/src/auth/namespace.rs`：User Namespace 实现保持不变
- 前端代码：无需修改

---

## Task 1: 移除权限检查函数

**Files:**
- Modify: `agent/src/handler.rs:26-78`（删除 check_path_permission 函数）
- Modify: `agent/src/handler.rs:80-110`（删除 check_allowed_paths 函数）

- [ ] **Step 1: 删除 check_path_permission 函数**

在 `agent/src/handler.rs` 中，删除第 26-78 行的 `check_path_permission` 函数：

```rust
// 删除以下代码：
/// 检查路径权限（家目录范围）
///
/// 验证用户是否有权访问指定路径，确保路径在用户的家目录范围内。
///
/// # 参数
/// - `path`: 要检查的路径
/// - `session`: 用户会话信息
///
/// # 返回
/// - `Ok(true)`: 有权限访问
/// - `Ok(false)`: 无权限访问
///
/// # 安全性
/// - 只允许访问用户家目录及其子目录
/// - 防止路径遍历攻击（如 `../`）
/// - root用户(uid=0)拥有整个文件系统的访问权限
fn check_path_permission(path: &str, session: &UserSession) -> Result<bool, String> {
    let path = PathBuf::from(path);

    // 规范化路径，防止路径遍历攻击
    let canonical_path = match path.canonicalize() {
        Ok(p) => p,
        Err(_e) => {
            // 路径不存在时，使用绝对路径检查
            if !path.is_absolute() {
                return Err(format!("路径必须是绝对路径: {}", path.display()));
            }
            path
        }
    };

    // root用户(uid=0)拥有整个文件系统的访问权限
    if session.uid == 0 {
        tracing::debug!(
            "root用户访问: {:?}",
            canonical_path
        );
        return Ok(true);
    }

    // 普通用户：检查是否在用户家目录范围内
    if !canonical_path.starts_with(&session.home_dir) {
        tracing::warn!(
            "权限拒绝: 用户 {} 尝试访问路径 {:?}，家目录为 {:?}",
            session.username,
            canonical_path,
            session.home_dir
        );
        return Ok(false);
    }

    Ok(true)
}
```

- [ ] **Step 2: 删除 check_allowed_paths 函数**

在 `agent/src/handler.rs` 中，删除第 80-110 行的 `check_allowed_paths` 函数：

```rust
// 删除以下代码：
/// 检查路径权限（包括allowed_paths白名单检查）
///
/// # 参数
/// - `path`: 要检查的路径
/// - `cfg`: Agent配置
/// - `session`: 用户会话信息
///
/// # 返回
/// - `Ok(())`: 权限检查通过
/// - `Err(String)`: 权限被拒绝，包含错误信息
fn check_allowed_paths(path: &str, cfg: &AgentConfig, session: &UserSession) -> Result<(), String> {
    // root用户(uid=0)跳过白名单检查
    if session.uid == 0 {
        tracing::trace!("root用户跳过白名单检查: {}", path);
        return Ok(());
    }

    // 普通用户：如果allowed_paths不为空，则检查白名单
    if !cfg.security.allowed_paths.is_empty() {
        let allowed = cfg
            .security
            .allowed_paths
            .iter()
            .any(|prefix| path.starts_with(prefix));
        if !allowed {
            return Err(format!("访问被拒绝: 不在允许的路径列表中 ({})", path));
        }
    }

    Ok(())
}
```

- [ ] **Step 3: 验证编译**

运行编译命令，确保删除函数后代码能够编译：

```bash
cd agent && cargo check
```

Expected: 编译成功，无错误（可能有未使用的导入警告）

- [ ] **Step 4: 提交更改**

```bash
git add agent/src/handler.rs
git commit -m "refactor: 移除权限检查函数（check_path_permission 和 check_allowed_paths）"
```

---

## Task 2: 简化请求处理逻辑（读取目录）

**Files:**
- Modify: `agent/src/handler.rs:136-168`（简化 ReadDirRequest 处理）
- Modify: `agent/src/handler.rs:734-779`（简化 handle_read_dir 函数）

- [ ] **Step 1: 简化 ReadDirRequest 处理**

修改 `agent/src/handler.rs` 中的 `Payload::ReadDirRequest` 处理逻辑：

**修改前**（第 136-168 行）：
```rust
Payload::ReadDirRequest { path } => {
    tracing::info!("读取目录请求: {}", path);

    // 权限检查：验证用户是否有权访问该路径
    match check_path_permission(path, session) {
        Ok(true) => {
            match handle_read_dir(path, cfg, session) {
                Ok(entries) => {
                    tracing::info!("读取目录成功: {} ({} 个文件)", path, entries.len());
                    Envelope::new(
                        envelope.request_id,
                        Payload::ReadDirResponse {
                            path: path.clone(),
                            entries,
                        },
                    )
                }
                Err(e) => {
                    tracing::error!("读取目录失败: {} - {}", path, e);
                    error_response(envelope.request_id, &e)
                }
            }
        }
        Ok(false) => {
            tracing::warn!("权限拒绝: 用户 {} 无权访问路径 {}", session.username, path);
            error_response(envelope.request_id, &format!("权限不足: 无法访问路径 '{}'", path))
        }
        Err(e) => {
            tracing::error!("权限检查失败: {}", e);
            error_response(envelope.request_id, &e)
        }
    }
}
```

**修改后**：
```rust
Payload::ReadDirRequest { path } => {
    tracing::info!("读取目录请求: {}", path);

    match handle_read_dir(path, cfg, session) {
        Ok(entries) => {
            tracing::info!("读取目录成功: {} ({} 个文件)", path, entries.len());
            Envelope::new(
                envelope.request_id,
                Payload::ReadDirResponse {
                    path: path.clone(),
                    entries,
                },
            )
        }
        Err(e) => {
            tracing::error!("读取目录失败: {} - {}", path, e);
            error_response(envelope.request_id, &e)
        }
    }
}
```

- [ ] **Step 2: 简化 handle_read_dir 函数**

修改 `agent/src/handler.rs` 中的 `handle_read_dir` 函数：

**修改前**（第 734-779 行）：
```rust
fn handle_read_dir(path: &str, cfg: &AgentConfig, session: &UserSession) -> Result<Vec<FileEntry>, String> {
    // 检查白名单权限（root用户自动跳过）
    check_allowed_paths(path, cfg, session)?;

    // 使用 UserExecutor 在用户上下文中执行操作
    let executor = UserExecutor::new(session);
    let path = path.to_string(); // 克隆为 String,以便移动到闭包
    executor.execute_as_user(move || {
        // ...
    }).map_err(|e| e.to_string())
}
```

**修改后**：
```rust
fn handle_read_dir(path: &str, _cfg: &AgentConfig, session: &UserSession) -> Result<Vec<FileEntry>, String> {
    // 使用 UserExecutor 在用户上下文中执行操作
    // Linux 文件系统权限自动生效
    let executor = UserExecutor::new(session);
    let path = path.to_string(); // 克隆为 String,以便移动到闭包
    executor.execute_as_user(move || {
        // 尝试读取目录，依赖 Linux 文件系统权限
        let entries = fs::read_dir(&path)
            .map_err(|e| {
                let error_msg = e.to_string();
                if error_msg.contains("Permission denied") {
                    anyhow::anyhow!("权限不足: 无法访问目录 '{}' (需要相应的 Linux 用户权限)", path)
                } else {
                    anyhow::anyhow!("无法读取目录 '{}': {}", path, e)
                }
            })?
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let metadata = entry.metadata().ok()?;
                let name = entry.file_name().to_string_lossy().to_string();

                let mtime = metadata
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| {
                        let dt = chrono::DateTime::<chrono::Utc>::from(SystemTime::UNIX_EPOCH + d);
                        dt.to_rfc3339()
                    })
                    .unwrap_or_default();

                Some(FileEntry {
                    name,
                    is_dir: metadata.is_dir(),
                    size: if metadata.is_dir() { 0 } else { metadata.len() },
                    mtime,
                    permissions: format_permissions(&metadata),
                })
            })
            .collect();

        Ok(entries)
    }).map_err(|e| e.to_string())
}
```

- [ ] **Step 3: 验证编译**

```bash
cd agent && cargo check
```

Expected: 编译成功

- [ ] **Step 4: 提交更改**

```bash
git add agent/src/handler.rs
git commit -m "refactor: 简化 ReadDirRequest 处理，移除权限检查"
```

---

## Task 3: 简化其他请求处理逻辑

**Files:**
- Modify: `agent/src/handler.rs`（多个请求处理函数）

由于篇幅限制，这里只展示部分关键函数的修改。其他函数（ReadFile、WriteFile、Delete、Mkdir、Rename、Copy、Move、ApplyDiff、FileTransfer、FileExists）的修改模式相同。

- [ ] **Step 1: 简化 ReadFileRequest 处理**

修改 `Payload::ReadFileRequest` 处理（第 170-198 行）：

**修改前**：
```rust
Payload::ReadFileRequest { path } => {
    tracing::info!("读取文件请求: {}", path);

    // 权限检查：验证用户是否有权访问该路径
    match check_path_permission(path, session) {
        Ok(true) => {
            match handle_read_file(path, cfg, session) {
                // ...
            }
        }
        Ok(false) => {
            tracing::warn!("权限拒绝: 用户 {} 无权访问路径 {}", session.username, path);
            error_response(envelope.request_id, &format!("权限不足: 无法访问路径 '{}'", path))
        }
        Err(e) => {
            tracing::error!("权限检查失败: {}", e);
            error_response(envelope.request_id, &e)
        }
    }
}
```

**修改后**：
```rust
Payload::ReadFileRequest { path } => {
    tracing::info!("读取文件请求: {}", path);

    match handle_read_file(path, cfg, session) {
        Ok((content, mtime, size)) => Envelope::new(
            envelope.request_id,
            Payload::ReadFileResponse {
                path: path.clone(),
                content,
                mtime,
                size,
            },
        ),
        Err(e) => error_response(envelope.request_id, &e),
    }
}
```

- [ ] **Step 2: 简化 handle_read_file 函数**

修改 `handle_read_file` 函数（第 781-832 行）：

**修改前**：
```rust
fn handle_read_file(path: &str, cfg: &AgentConfig, session: &UserSession) -> Result<(String, u64, u64), String> {
    tracing::debug!("[handle_read_file] path={}", path);
    // 检查白名单权限（root用户自动跳过）
    check_allowed_paths(path, cfg, session)?;

    // 使用 UserExecutor 在用户上下文中执行操作
    let executor = UserExecutor::new(session);
    // ...
}
```

**修改后**：
```rust
fn handle_read_file(path: &str, _cfg: &AgentConfig, session: &UserSession) -> Result<(String, u64, u64), String> {
    tracing::debug!("[handle_read_file] path={}", path);
    // 使用 UserExecutor 在用户上下文中执行操作
    // Linux 文件系统权限自动生效
    let executor = UserExecutor::new(session);
    // ...（其余代码保持不变）
}
```

- [ ] **Step 3: 批量简化其他请求处理**

使用相同的模式简化以下请求处理：
- `Payload::WriteFileRequest`（第 200-227 行）
- `Payload::DeleteRequest`（第 229-252 行）
- `Payload::MkdirRequest`（第 254-286 行）
- `Payload::RenameRequest`（第 288-328 行）
- `Payload::CopyRequest`（第 330-370 行）
- `Payload::MoveRequest`（第 372-412 行）
- `Payload::ApplyDiffRequest`（第 437-479 行）
- `Payload::FileTransferRequest`（第 520-543 行）
- `Payload::FileExistsRequest`（第 570-593 行）

**通用修改模式**：
1. 移除 `check_path_permission` 调用
2. 直接调用 handle 函数
3. 将 handle 函数的 `cfg` 参数改为 `_cfg`（如果不再使用）

- [ ] **Step 4: 批量简化其他 handle 函数**

使用相同的模式简化以下 handle 函数：
- `handle_write_file`（第 834-876 行）
- `handle_delete`（第 965-1003 行）
- `handle_mkdir`（第 1005-1025 行）
- `handle_rename`（第 1027-1049 行）
- `handle_copy`（第 1051-1094 行）
- `handle_move`（第 1096-1099 行）
- `handle_apply_diff`（第 890-963 行）
- `handle_file_transfer_request`（第 1186-1391 行）
- `handle_file_exists`（第 1583-1637 行）

**通用修改模式**：
1. 移除 `check_allowed_paths(path, cfg, session)?;` 调用
2. 将 `cfg` 参数改为 `_cfg`（如果不再使用）

- [ ] **Step 5: 验证编译**

```bash
cd agent && cargo check
```

Expected: 编译成功

- [ ] **Step 6: 提交更改**

```bash
git add agent/src/handler.rs
git commit -m "refactor: 批量简化请求处理逻辑，移除权限检查"
```

---

## Task 4: 移除 allowed_paths 配置

**Files:**
- Modify: `agent/src/config.rs`（移除 allowed_paths 字段）
- Modify: `agent/config.toml`（移除配置项）

- [ ] **Step 1: 移除 SecurityConfig 中的 allowed_paths 字段**

修改 `agent/src/config.rs` 中的 `SecurityConfig` 结构体：

**修改前**：
```rust
#[derive(Debug, Clone, Deserialize)]
pub struct SecurityConfig {
    pub allowed_paths: Vec<String>,
    pub blocked_commands: Vec<String>,
}
```

**修改后**：
```rust
#[derive(Debug, Clone, Deserialize)]
pub struct SecurityConfig {
    // 移除：pub allowed_paths: Vec<String>,
    pub blocked_commands: Vec<String>,
}
```

- [ ] **Step 2: 移除配置文件中的 allowed_paths 项**

修改 `agent/config.toml`：

**修改前**：
```toml
[security]
allowed_paths = ["/home", "/tmp", "/etc", "/var/log"]
blocked_commands = ["rm -rf /", "dd if=/dev/zero"]
```

**修改后**：
```toml
[security]
# 移除：allowed_paths = ["/home", "/tmp", "/etc", "/var/log"]
blocked_commands = ["rm -rf /", "dd if=/dev/zero"]
```

- [ ] **Step 3: 验证编译**

```bash
cd agent && cargo check
```

Expected: 编译成功

- [ ] **Step 4: 提交更改**

```bash
git add agent/src/config.rs agent/config.toml
git commit -m "refactor: 移除 allowed_paths 配置项"
```

---

## Task 5: 测试基本文件操作

**Files:**
- Test: 手动测试（文件管理器界面）

- [ ] **Step 1: 编译 Agent**

```bash
cd agent && cargo build --release
```

Expected: 编译成功

- [ ] **Step 2: 启动 Agent**

在 WSL 中启动 Agent：

```bash
cd agent
sudo ./target/release/agent
```

Expected: Agent 正常启动，监听端口

- [ ] **Step 3: 启动客户端并测试**

在 Windows 中启动客户端：

```bash
cd ..
npm run tauri dev
```

Expected: 客户端正常启动

- [ ] **Step 4: 测试普通用户访问系统目录**

测试步骤：
1. 使用普通用户登录（非 root）
2. 打开文件管理器
3. 导航到 `/etc`
4. 验证可以列出目录内容（Linux 权限允许）
5. 尝试创建文件 `/etc/test.txt`
6. 验证操作被拒绝（Linux 权限不允许）

Expected: 与终端行为一致

- [ ] **Step 5: 测试 root 用户全局访问**

测试步骤：
1. 使用 root 用户登录
2. 打开文件管理器
3. 导航到 `/etc`、`/var/log`、`/root`
4. 验证所有目录可访问

Expected: root 用户可以访问所有路径

- [ ] **Step 6: 提交测试结果**

无需提交代码，记录测试结果即可。

---

## 实现总结

完成以上任务后，文件管理器将：

1. **完全依赖 Linux 文件系统权限**：移除所有 Agent 层面的权限检查
2. **与终端体验一致**：用户能访问的目录 = Linux 权限允许的目录
3. **代码简化**：移除复杂的权限检查逻辑
4. **安全性保持**：UserExecutor 提供的隔离机制仍然有效

**后续工作（Phase 2）**：
- 实现有感知的提权功能
- 添加权限状态可视化
- 实现 sudo 免密用户的支持