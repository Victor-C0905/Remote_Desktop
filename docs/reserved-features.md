# Agent 预留功能文档

## 概述

本文档记录 Agent 项目中已实现但当前未使用的预留功能。这些功能是系统设计的核心组件，为未来扩展和特定场景提供支持。

---

## 预留功能列表

### 1. UserNamespace 用户命名空间隔离

**位置：** `agent/src/auth/namespace.rs`

**状态：** ✅ 已集成到 spawn_process

**功能描述：**
- 创建 Linux User Namespace 实现用户隔离
- UID/GID 映射，限制权限范围
- 禁用 setgroups 防止权限提升攻击

**实现详情：**
```rust
pub struct UserNamespace {
    inner_uid: u32,  // 容器内的UID
    inner_gid: u32,  // 容器内的GID
}

impl UserNamespace {
    pub fn new(inner_uid: u32, inner_gid: u32) -> Self;
    pub fn create_and_switch(&self) -> Result<()>;
}
```

**使用方式：**
```bash
# 通过环境变量启用 User Namespace
export USE_USER_NAMESPACE=1
cargo run --release
```

**适用场景：**
- 高安全级别的多用户环境
- 需要严格隔离的用户操作
- 符合安全规范的进程隔离

**降级机制：**
- 如果 User Namespace 创建失败，自动降级到 setuid/setgid
- 保证功能可用性的同时提供可选的安全增强

---

### 2. spawn_process 进程生成功能

**位置：** `agent/src/auth/executor.rs:158`

**状态：** 📋 预留（已实现，待集成）

**功能描述：**
- 在指定用户上下文中生成子进程
- 支持 User Namespace 隔离（Linux）
- 通过 setuid/setgid 切换用户身份

**实现详情：**
```rust
#[cfg(unix)]
#[allow(dead_code)]
pub fn spawn_process(&self, program: &str, args: &[&str]) -> Result<i32>
```

**能力：**
- ✅ 以目标用户身份启动进程
- ✅ User Namespace 隔离（可选）
- ✅ setuid/setgid 用户切换
- ✅ 工作目录设置为用户家目录

**潜在用途：**
1. **脚本执行**
   ```rust
   let executor = UserExecutor::new(&session);
   let pid = executor.spawn_process("/bin/bash", &["-c", "script.sh"])?;
   ```

2. **文件处理进程**
   ```rust
   // 在用户上下文中执行文件压缩
   executor.spawn_process("tar", &["-czf", "archive.tar.gz", "files/"])?;
   ```

3. **后台任务处理**
   ```rust
   // 启动后台数据处理进程
   executor.spawn_process("python", &["process_data.py", "--input", "data.json"])?;
   ```

**集成方案（待定）：**

**场景 1：文件操作增强**
- 在需要外部工具处理文件时使用
- 例如：压缩/解压、格式转换

**场景 2：脚本执行功能**
- 为用户提供脚本执行能力
- 在安全的用户上下文中运行

**场景 3：后台任务处理**
- 异步任务执行
- 定时任务调度

**注意事项：**
- 需要谨慎处理进程生命周期
- 需要实现进程监控和清理机制
- 需要考虑资源限制和超时控制

---

### 3. PtySession::spawn 和 PtyManager::spawn

**位置：** `agent/src/pty.rs:39, 359`

**状态：** 📋 预留（基础方法）

**功能描述：**
- 创建基础的 PTY（伪终端）会话
- 无用户隔离的普通终端会话

**实现详情：**
```rust
// PtySession::spawn
#[allow(dead_code)]
pub fn spawn(shell: &str, cols: u16, rows: u16, working_directory: Option<&str>) -> Result<Self>

// PtyManager::spawn
#[allow(dead_code)]
pub async fn spawn(&self, shell: &str, cols: u16, rows: u16, working_directory: Option<&str>) -> Result<String>
```

**对比实际使用的方法：**

| 方法 | 用户隔离 | 实际使用 | 调用位置 |
|------|---------|---------|---------|
| `PtySession::spawn` | ❌ 无 | ❌ 未使用 | 无 |
| `PtyManager::spawn` | ❌ 无 | ❌ 未使用 | 无 |
| `PtySession::spawn_as_user` | ✅ 有 | ✅ 使用中 | pty.rs:122 |
| `PtyManager::spawn_as_user` | ✅ 有 | ✅ 使用中 | quic.rs:1145 |

**为什么需要保留基础方法：**

1. **API 完整性**
   - 提供完整的 PTY API 层次结构
   - 基础方法 → 增强方法 → 业务方法

2. **未来场景**
   - 系统级终端会话（不涉及用户切换）
   - 管理员维护终端
   - 调试和诊断工具

3. **测试和开发**
   - 单元测试中可能需要简单的基础方法
   - 开发调试时的快速验证

**潜在用途（待定）：**

**场景 1：系统维护终端**
```rust
// 管理员维护终端（不需要用户隔离）
let session_id = pty_manager.spawn("/bin/bash", 120, 40, None).await?;
```

**场景 2：调试终端**
```rust
// 快速创建调试终端
let session = PtySession::spawn("/bin/sh", 80, 24, None)?;
```

**场景 3：批量操作**
```rust
// 批量执行系统命令（无需用户上下文）
for cmd in commands {
    let session_id = pty_manager.spawn("/bin/sh", 80, 24, None).await?;
    // 执行命令...
}
```

---

## 设计原则

### 为什么保留这些功能？

1. **架构完整性**
   - 这些功能是系统设计的核心组件
   - 提供 API 的完整性和一致性

2. **未来扩展性**
   - 为未来功能扩展预留接口
   - 避免重复开发

3. **安全增强选项**
   - UserNamespace 提供可选的安全增强
   - 灵活的安全策略配置

4. **开发便利性**
   - 基础方法用于测试和开发
   - 降低开发复杂度

---

## 维护指南

### 如何处理这些功能？

1. **不要删除代码**
   - 这些功能已正确实现
   - 使用 `#[allow(dead_code)]` 注解抑制警告

2. **保持文档更新**
   - 记录功能的用途和状态
   - 更新潜在的应用场景

3. **定期评审**
   - 定期评估是否需要集成
   - 更新集成方案

### 集成时的注意事项

1. **spawn_process 集成**
   - 确定具体的业务场景
   - 实现进程生命周期管理
   - 添加资源限制和超时控制

2. **PtySession::spawn 集成**
   - 确认确实不需要用户隔离
   - 评估安全风险
   - 添加适当的权限检查

---

## 版本历史

| 版本 | 日期 | 变更说明 |
|------|------|---------|
| 1.0 | 2026-07-28 | 初始文档，记录预留功能 |

---

## 相关文档

- [SSH兼容认证设计](./superpowers/specs/2026-07-24-ssh-compatible-authentication-design.md)
- [安全功能集成计划](./superpowers/plans/2026-07-28-security-monitoring-integration.md)
- [项目规范](../.trae/rules/项目规范.md)