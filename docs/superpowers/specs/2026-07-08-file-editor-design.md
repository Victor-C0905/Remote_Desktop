# 文件编辑器设计文档

> 版本: v2.0 | 日期: 2026-07-08
>
> 核心理念：Agent 无状态化 + 客户端有状态 + 流量优化 + 实时同步

***

## 一、需求概述

### 1.1 功能需求

1. **简单文本编辑器**：类似记事本，支持纯文本编辑（无语法高亮、行号等高级功能）
2. **文件类型支持**：所有文件类型（包括二进制文件）
3. **双模式切换**：文本模式和十六进制查看模式
4. **最小功能集**：打开、编辑、保存（保留后续扩展空间，类似 Notepad++）
5. **GNOME 风格窗口**：使用 AppLayout + WindowShell 框架

### 1.2 非功能需求

1. **流量优化**：差异同步 + 分块传输（节省 90%+ 流量）
2. **实时同步**：检测文件被 vim 等外部工具修改，实时推送给客户端
3. **大文件支持**：分块传输 + 虚拟滚动（支持 100MB+ 文件）
4. **冲突检测**：文件锁 + 版本管理（防止并发编辑冲突）
5. **高扩展性**：配置化参数 + 策略模式 + 插件机制

### 1.3 性能指标

| 指标               | 目标值     | 说明                  |
| ---------------- | ------- | ------------------- |
| **文件打开响应时间**     | < 500ms | 小文件（<1MB）全量加载       |
| **分块传输延迟**       | < 100ms | 64KB 分块传输时间         |
| **差异计算时间**       | < 5s    | 10MB 文件差异计算（客户端）    |
| **差异传输大小**       | < 1KB   | 修改一行代码的传输大小         |
| **客户端内存占用**      | < 300MB | 客户端缓存 10 个文件的总内存    |
| **Agent 内存占用**   | < 50MB  | Agent 无状态，只做文件代理    |
| **Agent CPU 占用** | < 5%    | Agent 只负责文件操作，不计算差异 |
| **文件锁超时**        | 依赖 flock | 使用系统级文件锁（flock）    |
| **inotify 事件延迟** | < 500ms | 文件修改到推送通知的延迟        |

***

## 二、架构设计

### 2.1 总体架构

```
┌─ 客户端（React + TypeScript）─────────────────────┐
│                                                     │
│  ┌─ TextEditor 应用 ────────────────────────────┐  │
│  │                                               │  │
│  │  ┌─ HeaderBar ────────────────────────────┐  │  │
│  │  │  文件名 | 保存 | 模式切换 | ⋮         │  │  │
│  │  └────────────────────────────────────────┘  │  │
│  │                                               │  │
│  │  ┌─ 编辑区 ──────────────────────────────┐  │  │
│  │  │  文本模式: <textarea>                 │  │  │
│  │  │  十六进制模式: <HexViewer>            │  │  │
│  │  └────────────────────────────────────────┘  │  │
│  │                                               │  │
│  │  ┌─ StatusBar ───────────────────────────┐  │  │
│  │  │  行: 5 | UTF-8 | 已保存               │  │  │
│  │  └────────────────────────────────────────┘  │  │
│  └───────────────────────────────────────────────┘  │
│                                                     │
└─────────────────────────────────────────────────────┘
                          │
                          │ QUIC Stream
                          │
┌─ Agent (Rust) ─────────────────────────────────────┐
│                                                     │
│  ┌─ 文件管理器 ────────────────────────────────┐  │
│  │  · FileCache (LRU 缓存，最多 10 个文件)     │  │
│  │  · FileLock (文件锁，防止并发编辑)          │  │
│  │  · DiffEngine (差异计算，基于 LCS 算法)     │  │
│  │  · FileWatcher (inotify 监控)               │  │
│  └───────────────────────────────────────────────┘  │
│                                                     │
│  新增 API:                                         │
│  · file_open (锁定文件)                            │
│  · file_close (释放文件锁)                         │
│  · file_chunk_read (分块读取)                      │
│  · file_diff_apply (应用差异)                      │
│  · file_heartbeat (心跳检测)                       │
│                                                     │
└─────────────────────────────────────────────────────┘
                          │
                          │ 文件系统
                          │
┌─ Linux 文件系统 ───────────────────────────────────┐
│                                                     │
│  · 文件内容                                         │
│  · 文件元数据（mtime, permissions）                │
│  · inotify 事件                                     │
│                                                     │
└─────────────────────────────────────────────────────┘
```

### 2.2 核心原则

1. **Agent 无状态化**（性能优化）：
   - **无文件缓存**：不维护文件内容缓存，减少内存占用
   - **无差异计算**：客户端计算差异，Agent 只负责应用
   - **无版本管理**：使用文件系统 mtime 作为版本标识
   - **无锁状态**：使用系统级 flock，Agent 不维护锁状态
   - **文件代理**：只负责"代理"文件系统操作（读取、写入、锁定）

2. **客户端有状态**（业务逻辑）：
   - **文件缓存**：客户端缓存文件内容（最多 10 个文件）
   - **差异计算**：客户端使用 LCS 算法计算差异
   - **版本管理**：基于文件系统 mtime 检测冲突
   - **冲突检测**：客户端本地检测版本冲突

3. **流量优化**（保持不变）：
   - 初次打开：传输整个文件（小文件）或分块（大文件）
   - 保存修改：只传输差异部分（节省 90%+ 流量）
   - 十六进制查看：分页加载（每次只传输 64KB）

### 2.3 性能优化对比

| 操作 | 原架构 | 新架构（无状态） |
|------|--------|-----------------|
| **文件缓存** | Agent 缓存 10 个文件（300MB） | 客户端缓存 10 个文件（300MB） |
| **差异计算** | Agent CPU 计算（5 秒） | 客户端 CPU 计算（5 秒） |
| **版本管理** | Agent 维护版本号 | 使用文件系统 mtime |
| **文件锁** | Agent 维护锁状态 | 使用系统级 flock |
| **Agent 内存占用** | 300MB | < 50MB |
| **Agent CPU 占用** | 高（差异计算） | 低（只做文件代理） |

### 2.4 客户端数据结构

```typescript
// 文件状态（客户端缓存）
interface FileState {
  path: string;              // 文件路径
  mtime: number;             // 文件修改时间（Unix timestamp，作为版本标识）
  checksum: string;          // 内容校验和（MD5）
  content: string;           // 文本模式内容
  binaryData?: Uint8Array;   // 十六进制模式数据（可选）
  isLocked: boolean;         // 是否被当前客户端锁定
  lockedBy?: string;         // 锁定者 ID（其他客户端）
}

// 文件变更
interface FileChange {
  type: 'replace' | 'insert' | 'delete';  // 变更类型
  line: number;            // 行号（从 1 开始）
  old: string;             // 原内容
  new: string;             // 新内容
}

// 编辑器模式
type EditorMode = 'text' | 'hex';

// 编辑器状态
interface EditorState {
  fileState: FileState | null;      // 文件状态
  editorMode: EditorMode;           // 编辑器模式
  isLoading: boolean;               // 是否正在加载
  isSaving: boolean;                // 是否正在保存
  hasUnsavedChanges: boolean;       // 是否有未保存的修改
}
```

### 2.5 Agent API（无状态，只做文件代理）

```rust
// 新增 Agent API（简化版）
Payload::FileReadRequest {
    path: String,
}

Payload::FileReadResponse {
    content: String,
    mtime: u64,  // 文件修改时间（Unix timestamp）
    size: u64,
}

Payload::FileWriteRequest {
    path: String,
    content: String,
}

Payload::FileWriteResponse {
    mtime: u64,  // 新的修改时间
}

Payload::FileDiffApplyRequest {
    path: String,
    base_mtime: u64,  // 基于哪个 mtime
    diff: Vec<FileChange>,
}

Payload::FileDiffApplyResponse {
    mtime: u64,  // 新的修改时间
}

Payload::FileLockRequest {
    path: String,
}

Payload::FileLockResponse {
    success: bool,
}

Payload::FileUnlockRequest {
    path: String,
}

Payload::FileUnlockResponse {
    success: bool,
}

Payload::FileWatchRequest {
    path: String,
    subscribe: bool,  // true=订阅, false=取消订阅
}

Payload::FileWatchResponse {
    success: bool,
}

Payload::FileChangedEvent {
    path: String,
    mtime: u64,  // 新的修改时间
}
```

***

## 三、关键技术设计

### 3.1 大文件传输方案

**问题**：10MB 文件一次性传输会导致网络延迟高、内存占用大、首屏渲染慢。

**解决方案**：分块传输 + 虚拟滚动

```
客户端 ─── file_chunk_read(offset=0, len=64KB) ───→ Agent
Agent ─── FileChunkResponse(data[0..64KB], total=10MB) ───→ 客户端

客户端渲染首屏（64KB，约 1000 行）

用户滚动到底部 ─── file_chunk_read(offset=64KB) ───→ Agent
Agent ─── FileChunkResponse(data[64KB..128KB]) ───→ 客户端

客户端追加渲染（虚拟滚动）
```

**技术细节**：

* 分块大小：64KB（平衡网络延迟和渲染性能）

* 虚拟滚动：只渲染可见行（约 50 行），内存占用恒定

* 预加载：滚动时预加载下一块（提升体验）

### 3.2 实时检测其他改动（inotify）

**问题**：文件被 vim、nano、其他编辑器修改时，如何感知？

**解决方案**：Agent 使用 inotify 监控文件系统

```
Agent 使用 notify crate 监控 /home/user/projects/

检测到 config.yaml 被 vim 修改
  │
  ├── 计算差异（DiffEngine）
  │    old_content: "server:\n  host: prod"
  │    new_content: "server:\n  host: dev"
  │    diff: [
  │      { type: "replace", line: 2, old: "  host: prod", new: "  host: dev" }
  │    ]
  │
  └── 推送给所有订阅的客户端
       event: { 
         type: "file_modified", 
         path: "/home/user/projects/config.yaml",
         diff: [...]
       }
```

### 3.3 最小传输更改（差异同步，客户端计算）

**问题**：修改一行代码，如何避免传输整个文件（10MB）？

**解决方案**：客户端计算差异，Agent 只负责验证和应用

```
场景：用户编辑 config.yaml

1. 初次打开
   客户端 ─── file_read(path="/home/config.yaml") ───→ Agent
   Agent ─── FileReadResponse(content="...", mtime=1704067200) ───→ 客户端
   客户端缓存：{ mtime: 1704067200, content: "..." }

2. 用户编辑（修改第 5 行）
   客户端本地修改：第 5 行 "host: prod" → "host: dev"
   
3. 保存文件（客户端计算差异）
   客户端：
   - 计算差异（LCS 算法）：diff = [{ line: 5, old: "host: prod", new: "host: dev" }]
   
   客户端 ─── file_diff_apply(base_mtime=1704067200, diff=[...]) ───→ Agent
   
   Agent：
   - 读取文件当前 mtime：current_mtime = 1704067200
   - 验证 mtime：current_mtime == base_mtime ✓（无冲突）
   - 应用差异：修改第 5 行
   - 写入文件
   - 获取新 mtime：new_mtime = 1704067260
   
   Agent ─── FileDiffApplyResponse(mtime=1704067260) ───→ 客户端
   客户端更新缓存：{ mtime: 1704067260, content: "..." }
```

**流量对比**：
- 全量传输：10MB（传输整个文件）
- 差异同步：约 100 字节（只传输修改的行）

**客户端差异计算算法（LCS）**：

```typescript
// 客户端计算差异
function calculateDiff(oldContent: string, newContent: string): FileChange[] {
  const oldLines = oldContent.split('\n');
  const newLines = newContent.split('\n');
  
  // 使用 LCS 算法计算最长公共子序列
  const lcs = longestCommonSubsequence(oldLines, newLines);
  
  // 基于 LCS 生成差异
  const diff = generateDiff(oldLines, newLines, lcs);
  
  return diff;
}

function longestCommonSubsequence(a: string[], b: string[]): string[] {
  // DP 表
  const dp: number[][] = Array(a.length + 1).fill(0).map(() => 
    Array(b.length + 1).fill(0)
  );
  
  // 填充 DP 表
  for (let i = 1; i <= a.length; i++) {
    for (let j = 1; j <= b.length; j++) {
      if (a[i - 1] === b[j - 1]) {
        dp[i][j] = dp[i - 1][j - 1] + 1;
      } else {
        dp[i][j] = Math.max(dp[i][j - 1], dp[i - 1][j]);
      }
    }
  }
  
  // 回溯找 LCS
  const lcs: string[] = [];
  let i = a.length, j = b.length;
  
  while (i > 0 && j > 0) {
    if (a[i - 1] === b[j - 1]) {
      lcs.push(a[i - 1]);
      i--;
      j--;
    } else if (dp[i - 1][j] > dp[i][j - 1]) {
      i--;
    } else {
      j--;
    }
  }
  
  return lcs.reverse();
}
```

### 3.4 冲突检测与解决（基于 mtime）

**问题**：两个客户端同时编辑同一文件，如何避免冲突？

**解决方案**：乐观锁 + 文件锁 + mtime 验证

```
场景：两个客户端同时编辑 config.yaml

客户端 A 打开文件获取锁（使用 flock）
客户端 B 尝试打开文件 → 拒绝（文件已被 A 锁定）

客户端 A 编辑：第 5 行
客户端 A 保存：
  - 客户端计算差异
  - 发送 file_diff_apply(base_mtime=1704067200, diff=[...])
  - Agent 验证 mtime ✓（无冲突）
  - Agent 应用差异，返回新 mtime=1704067260
  - 客户端 A 更新缓存：{ mtime: 1704067260 }

Agent 推送事件给客户端 B：文件已修改（mtime=1704067260）

客户端 B 应用补丁（更新本地缓存）

客户端 B 编辑：第 10 行
客户端 B 保存：
  - 客户端计算差异
  - 发送 file_diff_apply(base_mtime=1704067260, diff=[...])
  - Agent 验证 mtime ✓（无冲突）
  - Agent 应用差异，返回新 mtime=1704067320
```

**mtime 冲突检测逻辑**：

```rust
// Agent 验证 mtime
pub fn apply_diff(
    path: &str,
    base_mtime: u64,
    diff: Vec<FileChange>,
) -> Result<u64, String> {
    // 1. 读取文件当前 mtime
    let metadata = std::fs::metadata(path)?;
    let current_mtime = metadata.modified()?.duration_since(UNIX_EPOCH)?.as_secs();
    
    // 2. 验证 mtime（乐观锁）
    if current_mtime != base_mtime {
        return Err("mtime 冲突：文件已被其他客户端修改".to_string());
    }
    
    // 3. 应用差异
    let content = std::fs::read_to_string(path)?;
    let new_content = apply_diff_to_content(&content, &diff);
    
    // 4. 写入文件
    std::fs::write(path, &new_content)?;
    
    // 5. 返回新 mtime
    let metadata = std::fs::metadata(path)?;
    let new_mtime = metadata.modified()?.duration_since(UNIX_EPOCH)?.as_secs();
    
    Ok(new_mtime)
}
```

***

## 四、风险与应对

### 4.1 高风险问题

| 风险               | 解决方案                      |
| ---------------- | ------------------------- |
| **文件锁死锁**（客户端崩溃） | 锁超时机制（5 分钟）+ 心跳检测（每 30 秒） |
| **版本不一致**（网络中断）  | 重连时同步 + 三方合并              |
| **内存爆炸**（版本历史）   | 版本历史限制（最近 3 个）+ LRU 缓存    |

### 4.2 中风险问题

| 风险               | 解决方案                        |
| ---------------- | --------------------------- |
| **差异计算性能**       | 文件大小限制（>50MB 禁用）+ 超时机制（5 秒） |
| **inotify 事件风暴** | 事件防抖（500ms）+ 批量推送（每秒最多 1 次） |
| **分块传输一致性**      | 版本号绑定 + 原子性读取               |

### 4.3 安全考虑

#### 4.3.1 文件访问控制

**问题**：防止恶意客户端访问敏感文件。

**解决方案**：

* **路径白名单**：只允许访问特定目录（如 `/home/user/`、`/var/log/`）

* **路径黑名单**：禁止访问系统敏感路径（如 `/etc/shadow`、`/root/`）

* **权限检查**：Agent 以低权限用户运行（非 root），依赖文件系统权限

```toml
# agent.toml
[security]
allowed_paths = ["/home/user/", "/var/log/"]
blocked_paths = ["/etc/shadow", "/root/", "/proc/"]
```

#### 4.3.2 文件锁定滥用

**问题**：恶意客户端锁定大量文件，导致 DoS 攻击。

**解决方案**：

* **单客户端锁定限制**：最多锁定 5 个文件

* **锁超时**：5 分钟无操作自动释放

* **管理员监控**：可查看所有锁定文件，强制释放

#### 4.3.3 差异计算 DoS

**问题**：恶意客户端频繁修改大文件，消耗 Agent CPU。

**解决方案**：

* **差异计算并发限制**：最多 3 个并发差异计算

* **差异计算超时**：5 秒超时，回退到全量传输

* **大文件禁用差异**：>50MB 禁用差异同步

#### 4.3.4 敏感信息泄露

**问题**：编辑器显示敏感文件内容（如密钥、密码）。

**解决方案**：

* **文件名过滤**：检测敏感文件名（如 `.ssh/id_rsa`），显示警告

* **内容脱敏**：不提供自动脱敏功能，依赖用户判断

* **审计日志**：记录所有文件访问、编辑操作

```rust
// 敏感文件名检测
fn is_sensitive_file(path: &str) -> bool {
    let sensitive_patterns = [
        ".ssh/id_rsa",
        ".ssh/id_ed25519",
        ".pgp/",
        ".gnupg/",
        "credentials",
        "password",
    ];
    
    sensitive_patterns.iter().any(|p| path.contains(p))
}
```

***

## 五、高扩展性设计

### 5.1 配置化参数

所有参数可通过 `agent.toml` 调整：

```toml
[file_editor.lock]
timeout_secs = 300  # 5 分钟超时
heartbeat_interval_secs = 30  # 心跳间隔
max_locks_per_client = 5  # 单客户端最多锁定 5 个文件

[file_editor.diff]
timeout_secs = 5  # 差异计算超时
large_file_threshold_mb = 50  # 大文件阈值

[file_editor.chunk]
chunk_size_kb = 64  # 分块大小
dynamic_adjustment = true  # 动态调整分块大小

[file_editor.watcher]
debounce_ms = 500  # 事件防抖
use_inotify = true  # 使用 inotify 监控
```

### 5.2 策略模式（可替换实现）

* **差异计算策略**：行级差异（默认）、字节级差异、禁用差异

* **文件锁策略**：内存锁（默认）、系统锁（flock）

* **文件监控策略**：inotify（默认）、轮询（网络文件系统）

### 5.3 插件机制（可扩展功能）

```rust
/// 文件操作插件（可扩展）
pub trait FilePlugin: Send + Sync {
    fn name(&self) -> &str;
    fn on_file_open(&self, path: &str, mode: &str) -> Result<(), String>;
    fn on_file_save(&self, path: &str, content: &[u8]) -> Result<(), String>;
    fn on_file_close(&self, path: &str);
}
```

**内置插件**：

* FileSizeCheckPlugin：文件大小检查

* PermissionCheckPlugin：文件权限检查

* BackupPlugin：保存前自动备份

### 5.4 监控和告警（可观测性）

```rust
pub struct Metrics {
    pub file_locks: usize,  // 文件锁数量
    pub cached_files: usize,  // 缓存文件数量
    pub diff_calculations: u64,  // 差异计算次数
    pub avg_diff_time_ms: f64,  // 差异计算平均时间
    pub watcher_events: u64,  // 文件监控事件数量
}
```

***

## 六、错误处理策略

### 6.1 错误分类

```rust
pub enum FileEditorError {
    // 文件锁相关
    FileLocked { path: String, locked_by: String },
    LockTimeout { path: String, timeout_secs: u64 },
    
    // 版本冲突相关
    VersionConflict { path: String, base_version: u64, current_version: u64 },
    
    // 文件操作相关
    FileNotFound { path: String },
    FileTooLarge { path: String, size: u64, max_size: u64 },
    PermissionDenied { path: String, operation: String },
    
    // 网络相关
    NetworkInterrupted { message: String },
    ConnectionLost { server_id: String },
    Timeout { operation: String, timeout_secs: u64 },
}
```

### 6.2 重试机制

* **可重试错误**：LockTimeout、NetworkInterrupted、Timeout 等

* **重试策略**：指数退避（1s → 2s → 4s → 8s），最多 3 次

### 6.3 错误通知系统（可扩展）

```rust
/// 通知渠道接口
pub trait NotificationChannel: Send + Sync {
    fn name(&self) -> &str;
    fn send(&self, notification: &ErrorNotification) -> Result<(), String>;
    fn supports_level(&self, level: NotificationLevel) -> bool;
}
```

**内置渠道**：

* ClientNotificationCenter：推送到客户端通知栏

* LogChannel：记录到日志文件

* EmailChannel：发送邮件告警（仅 Critical 级别）

* WebhookChannel：发送到外部系统（Slack、钉钉等）

***

## 七、测试策略

### 7.1 单元测试

* 测试文件锁机制（锁定、超时、释放）

* 测试差异计算（LCS 算法）

* 测试版本冲突检测

### 7.2 集成测试

* 测试完整工作流（打开 → 编辑 → 保存 → 关闭）

* 测试网络中断恢复

* 测试并发编辑冲突

### 7.3 性能测试

* 大文件性能（100MB 文件分块读取 < 100ms）

* 差异计算性能（10MB 文件差异计算 < 5s）

***

## 八、实施计划

### 阶段 1：基础功能（1-2 周）

**目标**：最小可用版本

* 使用现有 Agent API（ReadFile + WriteFile）

* 全量传输（暂无差异同步）

* 无文件锁（支持手动刷新）

* 实现文本编辑和十六进制查看（双模式）

### 阶段 2：流量优化（1 周）

**目标**：差异同步

* 新增差异计算 API

* 实现版本管理

* 实现文件锁机制

### 阶段 3：实时监控（1 周）

**目标**：实时同步

* 新增 inotify 监控

* 实现文件变更事件推送

* 实现心跳检测和锁超时

### 阶段 4：大文件支持（1 周）

**目标**：大文件优化

* 新增分块读取 API

* 实现虚拟滚动

* 实现网络中断恢复

***

## 九、技术栈

| 层        | 组件                    | 选型           |
| -------- | --------------------- | ------------ |
| 客户端框架    | React 18 + TypeScript | WebView      |
| 设计系统     | Adwaita (CSS 迁移)      | CSS 变量       |
| 终端前端     | xterm.js              | Canvas       |
| Agent 框架 | Tauri 2.x + Rust      | quinn (QUIC) |
| 文件监控     | notify crate          | inotify      |
| 差异计算     | 自研 LCS 算法             | -            |

***

## 十、参考资料

* GNOME HIG：<https://developer.gnome.org/hig/>

* Tauri 文档：<https://tauri.app/v2/guides/>

* notify crate：<https://docs.rs/notify/latest/notify/>

* inotify 文档：<https://man7.org/linux/man-pages/man7/inotify.7.html>

