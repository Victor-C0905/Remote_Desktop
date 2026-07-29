# 文件管理器权限修复设计

**日期**: 2026-07-28
**状态**: 设计完成
**优先级**: 高

## 背景

当前文件管理器的权限检查过于严格，导致普通用户无法访问大量系统目录，与终端体验不一致。

### 当前实现问题

1. **家目录限制**：`check_path_permission` 函数限制普通用户只能访问自己的家目录（`/home/{username}`）
2. **白名单检查**：`check_allowed_paths` 函数限制访问范围
3. **用户体验差异**：终端中可以访问的目录（如 `/etc`、`/var/log`、`/tmp` 等），在文件管理器中被阻止

### Linux 终端权限模型

Linux 终端遵循以下权限原则：
- 用户登录后，终端进程以用户身份运行
- 所有文件操作直接通过 Linux 文件系统权限检查（r/w/x 位）
- 没有额外的应用层限制
- 用户能访问的目录 = Linux 权限允许的目录

## 目标

让文件管理器的权限行为与终端完全一致，完全依赖 Linux 文件系统权限。

## 设计方案

### 核心原则

**完全信任 Linux 文件系统权限**，移除所有 Agent 层面的权限检查。

**所有文件操作都遵循 Linux 权限原则**：
- 读文件：需要读权限（r）
- 写文件：需要写权限（w）
- 执行文件：需要执行权限（x）
- 进入目录：需要执行权限（x）
- 列出目录内容：需要读权限（r）
- 创建文件：需要父目录的写权限（w）
- 删除文件：需要父目录的写权限（w）

**权限错误处理与终端一致**：
- 权限不足时，Linux 系统返回 `EACCES`（Permission denied）
- Agent 捕获错误，返回友好的错误信息（如"权限不足: 无法访问文件 'xxx'"）
- 前端显示错误提示，用户可以理解原因

### 现有机制分析

当前 Agent 已经实现了正确的权限隔离机制：

```rust
// UserExecutor: 在用户上下文中执行操作
let executor = UserExecutor::new(session);
executor.execute_as_user(move || {
    // 所有文件操作都在用户上下文中执行
    // Linux 权限自动生效
})
```

**UserExecutor 的工作原理**：
1. 使用 User Namespace 创建隔离环境
2. 通过 `setuid`/`setgid` 切换到用户身份
3. 所有文件操作都在用户上下文中执行
4. Linux 文件系统权限自动生效

这意味着我们不需要 Agent 层面的权限检查，Linux 权限已经能够正确工作。

### 具体修改

#### 1. 移除权限检查函数

**文件**: `agent/src/handler.rs`

移除以下函数：
- `check_path_permission`：家目录范围检查
- `check_allowed_paths`：白名单检查

#### 2. 简化请求处理逻辑

修改所有请求处理函数，移除权限检查调用：

**修改前**（以 `ReadDirRequest` 为例）：
```rust
Payload::ReadDirRequest { path } => {
    // 权限检查：验证用户是否有权访问该路径
    match check_path_permission(path, session) {
        Ok(true) => {
            match handle_read_dir(path, cfg, session) {
                Ok(entries) => { /* ... */ },
                Err(e) => { /* ... */ }
            }
        }
        Ok(false) => {
            error_response(envelope.request_id, &format!("权限不足: {}", path))
        }
        Err(e) => {
            error_response(envelope.request_id, &e)
        }
    }
}
```

**修改后**：
```rust
Payload::ReadDirRequest { path } => {
    match handle_read_dir(path, cfg, session) {
        Ok(entries) => { /* ... */ },
        Err(e) => { /* ... */ }
    }
}
```

#### 3. 修改 handle_read_dir 函数

**修改前**：
```rust
fn handle_read_dir(path: &str, cfg: &AgentConfig, session: &UserSession) -> Result<Vec<FileEntry>, String> {
    // 检查白名单权限（root用户自动跳过）
    check_allowed_paths(path, cfg, session)?;

    // 使用 UserExecutor 在用户上下文中执行操作
    let executor = UserExecutor::new(session);
    // ...
}
```

**修改后**：
```rust
fn handle_read_dir(path: &str, _cfg: &AgentConfig, session: &UserSession) -> Result<Vec<FileEntry>, String> {
    // 使用 UserExecutor 在用户上下文中执行操作
    // Linux 文件系统权限自动生效
    let executor = UserExecutor::new(session);
    // ...
}
```

#### 4. 移除 allowed_paths 配置

**文件**: `agent/src/config.rs`

移除 `allowed_paths` 字段：
```rust
pub struct SecurityConfig {
    pub blocked_commands: Vec<String>,
    // 移除：pub allowed_paths: Vec<String>,
}
```

#### 5. 更新配置文件

**文件**: `agent/config.toml`

移除 `allowed_paths` 配置项：
```toml
[security]
# 移除：allowed_paths = ["/home", "/tmp"]
blocked_commands = ["rm -rf /", "dd if=/dev/zero"]
```

### 需要修改的函数

#### handler.rs

需要修改以下请求处理（移除权限检查）：

1. `Payload::ReadDirRequest` - 读取目录
2. `Payload::ReadFileRequest` - 读取文件
3. `Payload::WriteFileRequest` - 写入文件
4. `Payload::DeleteRequest` - 删除文件/目录
5. `Payload::MkdirRequest` - 创建目录
6. `Payload::RenameRequest` - 重命名
7. `Payload::CopyRequest` - 复制
8. `Payload::MoveRequest` - 移动
9. `Payload::ApplyDiffRequest` - 应用差异
10. `Payload::FileTransferRequest` - 文件传输
11. `Payload::FileExistsRequest` - 检查文件存在

需要修改以下 handle 函数（移除 check_allowed_paths 调用）：

1. `handle_read_dir`
2. `handle_read_file`
3. `handle_write_file`
4. `handle_delete`
5. `handle_mkdir`
6. `handle_rename`
7. `handle_copy`
8. `handle_move`
9. `handle_apply_diff`
10. `handle_file_transfer_request`
11. `handle_file_exists`

## 权限行为对比

### 修改前

| 操作 | root 用户 | 普通用户 |
|------|-----------|----------|
| 访问 `/home/{username}` | ✅ 允许 | ✅ 允许（家目录范围内） |
| 访问 `/etc` | ✅ 允许 | ❌ 拒绝（不在家目录） |
| 访问 `/var/log` | ✅ 允许 | ❌ 拒绝（不在家目录） |
| 访问 `/tmp` | ✅ 允许 | ❌ 拒绝（不在家目录） |
| 访问 `/usr/local` | ✅ 允许 | ❌ 拒绝（不在家目录） |

### 修改后

| 操作 | root 用户 | 普通用户 |
|------|-----------|----------|
| 访问 `/home/{username}` | ✅ 允许 | ✅ 允许（Linux 权限允许） |
| 访问 `/etc` | ✅ 允许 | ✅ 允许（Linux 权限允许，通常可读） |
| 访问 `/var/log` | ✅ 允许 | ⚠️ 取决于 Linux 权限（通常需要 root） |
| 访问 `/tmp` | ✅ 允许 | ✅ 允许（Linux 权限允许） |
| 访问 `/usr/local` | ✅ 允许 | ✅ 允许（Linux 权限允许，通常可读） |

**关键差异**：
- 修改前：应用层硬性拒绝，即使 Linux 权限允许也无法访问
- 修改后：完全依赖 Linux 权限，与终端行为一致

## 安全性分析

### 为什么是安全的？

1. **UserExecutor 隔离**：所有操作在用户上下文中执行，无法越权
2. **Linux 权限控制**：文件系统权限是最权威的权限控制
3. **root 用户仍受保护**：root 用户虽然可以访问所有路径，但仍受 blocked_commands 限制

### 风险评估

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| 用户误删系统文件 | 低 | blocked_commands 已限制危险命令 |
| 用户访问敏感目录 | 低 | Linux 权限自然保护（如 /root 700 权限） |
| 权限提升攻击 | 无 | User Namespace 隔离，无法越权 |

### 临时提权机制（Future Enhancement）

**当前行为**：
- 文件管理器作为普通用户进程运行，无法直接执行提权操作
- 权限不足时，操作直接失败，返回错误信息
- 这与终端中用户直接运行命令的行为一致（终端中也需要 `sudo` 才能提权）

**重要场景：sudo 免密用户（NOPASSWD）**：

很多生产环境配置了 sudo 免密：
```bash
# /etc/sudoers
username ALL=(ALL) NOPASSWD: ALL
```

这种情况下，文件管理器应该能够自动提权执行操作，而不需要用户输入密码。

**未来可能的提权方案**：

#### 方案 1：智能提权（推荐）✅

自动检测并处理三种情况：

**情况 A：sudo 免密用户（NOPASSWD）**
1. 权限不足时，尝试使用 `sudo -n` 执行操作（-n 表示 non-interactive）
2. 如果 sudo -n 成功，说明用户配置了免密，操作完成
3. 用户无感知，体验流畅

**情况 B：需要密码的 sudoer**
1. `sudo -n` 失败，返回需要密码的错误
2. 弹出密码输入框
3. 用户输入密码后，使用 sudo 执行操作

**情况 C：非 sudoer 用户**
1. `sudo -n` 失败，用户不在 sudoers 列表
2. 提示用户切换用户或联系管理员

**实现逻辑**：
```rust
// 伪代码
fn handle_write_with_elevation(path: &str, content: &str, session: &UserSession) -> Result<(), String> {
    // 1. 尝试普通用户权限执行
    match execute_as_user(|| fs::write(path, content)) {
        Ok(_) => return Ok(()),
        Err(e) if e.kind() == PermissionDenied => {
            // 2. 权限不足，尝试 sudo 提权
            if session.is_sudoer() {
                // 3. 尝试 sudo -n（免密模式）
                match execute_with_sudo(|| fs::write(path, content)) {
                    Ok(_) => return Ok(()),
                    Err(SudoNeedsPassword) => {
                        // 4. 需要密码，通知前端弹出密码框
                        return Err("需要 sudo 密码".into());
                    }
                    Err(e) => return Err(e),
                }
            } else {
                // 5. 非 sudoer，提示权限不足
                return Err("权限不足，请联系管理员".into());
            }
        }
        Err(e) => return Err(e),
    }
}
```

**优点**：
- 自动适配 sudo 免密用户，无感知提权
- 对于需要密码的 sudoer，提供友好的密码输入界面
- 对于非 sudoer，提供清晰的错误提示
- 符合 Linux 终端体验

**缺点**：
- 需要实现 sudo 执行机制
- 需要与前端配合实现密码输入框

#### 方案 2：用户切换

提供用户切换功能：
1. 用户点击"切换用户"按钮
2. 输入 root 用户名和密码
3. Agent 建立新的用户会话（root）
4. 文件管理器以 root 身份运行

**优点**：
- 实现简单，复用现有认证机制
- 安全性高，遵循现有权限模型
- 用户可以随时切换回普通用户

**缺点**：
- 需要用户主动切换，不如方案 1 自动化
- 无法利用 sudo 免密配置

#### 方案 3：混合模式（最佳方案）✅

结合方案 1 和方案 2：
- **默认使用智能提权**（方案 1）：自动处理 sudo 免密用户和需要密码的 sudoer
- **提供用户切换**（方案 2）：作为备用方案，用户可以手动切换用户

### 有感知的提权设计（推荐）✅

**设计目标**：
- 用户明确知道当前操作的权限状态
- 用户可以控制是否使用提权
- 提供视觉反馈和安全审计

#### 1. 权限状态可视化

**顶部状态栏显示**：
```
当前用户: vic (普通用户) | sudo: 可用（免密）
```

**状态类型**：
| 状态 | 显示 | 说明 |
|------|------|------|
| 普通用户 | `vic (普通用户)` | 当前以普通用户身份运行 |
| sudo 免密 | `sudo: 可用（免密）` | 用户配置了 NOPASSWD，可自动提权 |
| sudo 需密码 | `sudo: 可用（需密码）` | 用户是 sudoer，但需要密码 |
| 非 sudoer | `sudo: 不可用` | 用户不是 sudoer |
| root 用户 | `root (管理员)` | 当前以 root 身份运行 |

**颜色编码**：
- 普通用户：默认颜色（灰色）
- sudo 可用：提示色（蓝色）
- root 用户：警告色（红色）

#### 2. 提权操作确认

**场景 A：sudo 免密用户（推荐配置）**

用户尝试需要提权的操作时：
```
┌─────────────────────────────────────┐
│ ⚠️  需要管理员权限                  │
├─────────────────────────────────────┤
│ 操作：写入文件 /etc/test.conf       │
│                                     │
│ 当前用户权限不足，需要使用 sudo 提权 │
│                                     │
│ ☑️ 记住选择（本次会话）             │
│                                     │
│     [取消]  [使用 sudo 执行]        │
└─────────────────────────────────────┘
```

**选项说明**：
- **取消**：放弃操作
- **使用 sudo 执行**：确认后执行（sudo 免密用户无需密码）
- **记住选择**：本次会话内，后续类似操作自动使用 sudo

**场景 B：需要密码的 sudoer**

```
┌─────────────────────────────────────┐
│ 🔐  需要 sudo 密码                   │
├─────────────────────────────────────┤
│ 操作：写入文件 /etc/test.conf       │
│                                     │
│ 当前用户权限不足，需要使用 sudo 提权 │
│                                     │
│ 密码：[________________]            │
│                                     │
│ ☑️ 记住密码（本次会话）             │
│                                     │
│     [取消]  [确认执行]               │
└─────────────────────────────────────┘
```

**安全考虑**：
- 密码仅在本次会话内缓存（内存中，不写入磁盘）
- 会话结束后自动清除
- 支持"记住密码"选项，提升用户体验

#### 3. 提权操作审计日志

**Agent 日志**：
```log
[2026-07-28 10:30:15] INFO  用户 vic 使用 sudo 提权写入文件: /etc/test.conf
[2026-07-28 10:30:20] INFO  用户 vic 使用 sudo 提权删除文件: /var/log/old.log
[2026-07-28 10:30:25] WARN  用户 vic sudo 提权失败: 密码错误
```

**前端通知**：
- 操作完成后，状态栏显示提示：`已使用 sudo 权限执行操作（点击查看详情）`
- 点击后显示审计日志面板

#### 4. 用户可控的提权策略

**配置文件**（`agent/config.toml`）：
```toml
[privilege_escalation]
# 提权策略：always_ask（总是询问）| auto_sudo（自动 sudo）| disabled（禁用）
strategy = "always_ask"

# 记住选择：session（会话内）| never（从不）
remember_choice = "session"
```

**策略说明**：
- `always_ask`（推荐）：每次提权操作都需要用户确认
- `auto_sudo`：sudo 免密用户自动提权，无需确认（适合信任环境）
- `disabled`：禁用提权功能，仅允许普通用户操作

#### 5. 实现优先级

**Phase 1（核心功能）**：
1. 权限状态可视化（顶部状态栏）
2. sudo 免密用户的确认对话框
3. Agent 日志记录

**Phase 2（完善体验）**：
1. 密码输入框（需要密码的 sudoer）
2. "记住选择"功能
3. 审计日志面板

**Phase 3（高级功能）**：
1. 用户可控的提权策略配置
2. 密码缓存（会话级）

**当前实现建议**：
- **Phase 1**：实现有感知的提权（推荐配置）
  - sudo 免密用户：显示确认对话框
  - 权限状态可视化
  - 操作审计日志
- **Phase 2**：完善需要密码的场景

## 测试方案

### 测试用例

#### 1. 普通用户访问系统目录

**前置条件**：普通用户登录

**测试步骤**：
1. 文件管理器导航到 `/etc`
2. 验证可以列出目录内容（Linux 权限允许）
3. 尝试写入 `/etc/test.conf`
4. 验证操作被拒绝（Linux 权限不允许）

**预期结果**：与终端行为一致

#### 2. 普通用户访问其他用户目录

**前置条件**：用户 A 登录，用户 B 的家目录权限为 700

**测试步骤**：
1. 文件管理器导航到 `/home/userB`
2. 验证访问被拒绝（Linux 权限不允许）

**预期结果**：与终端行为一致

#### 3. root 用户全局访问

**前置条件**：root 用户登录

**测试步骤**：
1. 文件管理器导航到 `/etc`、`/var/log`、`/root`
2. 验证所有目录可访问

**预期结果**：root 用户可以访问所有路径

### 回归测试

- 运行现有测试套件，确保没有破坏现有功能
- 特别关注文件操作相关的测试

## 实现步骤

### Phase 1: 移除权限检查函数

1. 移除 `check_path_permission` 函数
2. 移除 `check_allowed_paths` 函数
3. 移除所有权限检查调用

### Phase 2: 简化配置

1. 移除 `allowed_paths` 配置项
2. 更新配置文件
3. 更新文档

### Phase 3: 测试验证

1. 运行单元测试
2. 手动测试各种场景
3. 验证与终端行为一致性

## 影响范围

### 修改的文件

- `agent/src/handler.rs`：移除权限检查函数和调用
- `agent/src/config.rs`：移除 allowed_paths 字段
- `agent/config.toml`：移除配置项

### 不受影响的部分

- `agent/src/auth/executor.rs`：UserExecutor 逻辑保持不变
- `agent/src/auth/namespace.rs`：User Namespace 实现保持不变
- 前端代码：无需修改

## 结论

通过完全移除 Agent 层面的权限检查，让文件管理器完全依赖 Linux 文件系统权限，可以：

1. **与终端体验完全一致**：用户能访问的目录 = Linux 权限允许的目录
2. **简化代码**：移除复杂的权限检查逻辑
3. **提高灵活性**：用户可以访问任何 Linux 权限允许的目录
4. **保持安全性**：UserExecutor 提供的隔离机制仍然有效

这是最符合 Linux 权限模型的设计方案。