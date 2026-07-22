# 文件传输清理与资源管理修复计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复文件传输系统中停止下载/关闭程序/断开连接时的资源泄漏、数据丢失和状态不一致问题，确保系统在异常情况下能够正确清理资源。

**Architecture:** 通过分层清理机制（连接层 → 任务层 → 文件层）实现完整的资源生命周期管理，使用 Drop trait 和 Tauri 生命周期钩子确保资源释放，添加状态持久化支持断点续传。

**Tech Stack:** Rust (Tauri 2.x, tokio, quinn), TypeScript (React 18), localStorage API

---

## 问题汇总

### 🔴 高危问题（立即修复）
1. **连接断开时传输任务未被清理** - 内存泄漏、僵尸任务、资源泄漏
2. **部分下载的文件残留** - 磁盘空间浪费、用户困惑
3. **部分上传的文件残留** - Agent 端临时文件
4. **文件句柄泄漏** - Windows 文件锁定、资源耗尽
5. **缺少优雅关闭机制** - 数据丢失、传输中断
6. **QUIC Stream 未关闭** - Agent 端资源泄漏

### 🟡 中危问题（中期优化）
7. **任务列表无大小限制** - 内存增长、性能下降
8. **已完成任务未被自动清理** - 需要手动调用
9. **前端任务列表不持久化** - 状态丢失、无法恢复
10. **断点续传依赖内存** - 重启失效
11. **事件监听器管理** - 已正确处理（无需修复）

### 🟢 低危问题（长期改进）
12. **QUIC 连接超时机制** - 僵死连接
13. **心跳机制缺失** - 延迟发现断线

---

## 文件结构规划

### 后端（Rust）
- **修改**: `src-tauri/src/connection.rs` - 添加连接断开清理逻辑
- **修改**: `src-tauri/src/transfer.rs` - 添加任务清理、文件清理、优雅关闭
- **修改**: `src-tauri/src/lib.rs` - 添加应用生命周期管理
- **新建**: `src-tauri/src/cleanup.rs` - 统一清理模块

### 前端（TypeScript）
- **修改**: `src/hooks/useTransferProgress.ts` - 添加 localStorage 持久化
- **修改**: `src/App.tsx` - 添加 beforeunload 事件处理
- **新建**: `src/utils/transferStorage.ts` - 传输状态持久化工具

---

## Task 1: 添加连接断开时的任务清理

**Files:**
- Modify: `src-tauri/src/connection.rs:550-565`
- Modify: `src-tauri/src/transfer.rs:58-70`
- Test: 手动测试断开连接

- [ ] **Step 1: 在 TransferManager 中添加按连接 ID 清理任务的方法**

```rust
// src-tauri/src/transfer.rs

impl TransferManager {
    /// 清理指定连接的所有任务
    pub async fn cleanup_by_connection(&self, connection_id: &str) {
        let mut tasks = self.tasks.lock().await;
        let ids_to_remove: Vec<String> = tasks
            .iter()
            .filter(|(_, task)| task.connection_id == connection_id)
            .map(|(id, _)| id.clone())
            .collect();

        for id in ids_to_remove {
            if let Some(task) = tasks.remove(&id) {
                // 发送取消事件
                let _ = self.emit_progress(&task);
            }
        }

        eprintln!("[TransferManager] 已清理连接 {} 的 {} 个任务", connection_id, tasks.len());
    }

    /// 取消指定任务
    pub async fn cancel_task(&self, task_id: &str) -> Result<(), String> {
        let mut tasks = self.tasks.lock().await;
        if let Some(task) = tasks.get_mut(task_id) {
            task.status = "cancelled".to_string();
            let task_clone = task.clone();
            drop(tasks);
            self.emit_progress(&task_clone)?;
            Ok(())
        } else {
            Err(format!("任务 {} 不存在", task_id))
        }
    }
}
```

- [ ] **Step 2: 修改 remote_disconnect 函数，调用清理方法**

```rust
// src-tauri/src/connection.rs

#[tauri::command]
pub async fn remote_disconnect(server_id: String, app: tauri::AppHandle) -> Result<(), String> {
    eprintln!("[Connection] 断开连接: {}", server_id);

    // 1. 清理传输任务
    let transfer_manager = app.state::<TransferManager>();
    transfer_manager.cleanup_by_connection(&server_id).await;

    // 2. 清理终端会话（如果有）
    // TODO: 添加终端会话清理

    // 3. 断开连接
    let manager = app.state::<ConnectionManager>();
    let tx = {
        let mut conns = manager.connections.lock().unwrap();
        conns.remove(&server_id).map(|c| c.tx)
    };

    if let Some(tx) = tx {
        let _ = tx.send(ClientRequest::Disconnect).await;
        eprintln!("[Connection] 连接已断开: {}", server_id);
        Ok(())
    } else {
        Err("未找到该服务器的连接".into())
    }
}
```

- [ ] **Step 3: 测试清理逻辑**

测试步骤：
1. 启动应用，连接到远程服务器
2. 开始下载一个文件
3. 在下载过程中断开连接
4. 检查任务列表是否被清空
5. 检查控制台输出，确认清理日志

Expected: 任务列表清空，控制台输出 "已清理连接 xxx 的 N 个任务"

---

## Task 2: 清理部分下载的临时文件

**Files:**
- Modify: `src-tauri/src/transfer.rs:854-856`
- Modify: `src-tauri/src/transfer.rs:454-467`
- Test: 手动测试下载中断

- [ ] **Step 1: 在 FileWriter 中添加临时文件标记**

```rust
// src-tauri/src/transfer.rs

pub struct FileWriter {
    file: BufWriter<File>,
    file_size: u64,
    transferred: u64,
    is_temporary: bool,  // 新增：标记是否为临时文件
    path: PathBuf,       // 新增：保存文件路径
}

impl FileWriter {
    pub fn new(path: &str, file_size: u64) -> Result<Self, String> {
        let path = PathBuf::from(path);
        let file = File::create(&path)
            .map_err(|e| format!("无法创建文件: {}", e))?;

        Ok(Self {
            file: BufWriter::new(file),
            file_size,
            transferred: 0,
            is_temporary: true,  // 默认为临时文件
            path,
        })
    }

    /// 标记文件为已完成（不是临时文件）
    pub fn mark_completed(&mut self) {
        self.is_temporary = false;
    }
}

impl Drop for FileWriter {
    fn drop(&mut self) {
        // 如果是临时文件且未完成，删除文件
        if self.is_temporary && self.transferred < self.file_size {
            eprintln!("[FileWriter] 清理临时文件: {:?}", self.path);
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
```

- [ ] **Step 2: 在传输完成时标记为已完成**

```rust
// src-tauri/src/transfer.rs (在下载完成的地方)

// 下载完成
if transferred >= file_size {
    writer.mark_completed();  // 标记为已完成
    task_clone.status = "completed".to_string();
    let _ = manager.emit_progress(&task_clone);
}
```

- [ ] **Step 3: 测试临时文件清理**

测试步骤：
1. 开始下载一个大文件（如 100MB）
2. 在下载过程中手动终止程序（Ctrl+C）
3. 检查文件是否被删除
4. 重新启动程序，确认文件不存在

Expected: 临时文件被自动删除

---

## Task 3: 添加优雅关闭机制

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/main.rs`
- Test: 手动测试程序关闭

- [ ] **Step 1: 在 lib.rs 中添加关闭钩子**

```rust
// src-tauri/src/lib.rs

use std::sync::Arc;
use tokio::sync::Mutex;

/// 全局关闭标志
pub static SHUTDOWN_FLAG: once_cell::sync::Lazy<Arc<Mutex<bool>>> =
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(false)));

/// 注册关闭钩子
pub fn setup_shutdown_hook(app: &tauri::AppHandle) {
    let app_handle = app.clone();

    // Ctrl+C 处理
    ctrlc::set_handler(move || {
        eprintln!("[Shutdown] 收到关闭信号");
        let app = app_handle.clone();
        tokio::spawn(async move {
            graceful_shutdown(app).await;
        });
    }).expect("无法设置 Ctrl+C 处理器");
}

/// 优雅关闭
async fn graceful_shutdown(app: tauri::AppHandle) {
    eprintln!("[Shutdown] 开始优雅关闭...");

    // 1. 设置关闭标志
    *SHUTDOWN_FLAG.lock().await = true;

    // 2. 取消所有传输任务
    let transfer_manager = app.state::<TransferManager>();
    let tasks = transfer_manager.list_tasks().await;
    for task in tasks {
        if task.status == "pending" || task.status == "transferring" {
            let _ = transfer_manager.cancel_task(&task.id).await;
        }
    }

    // 3. 断开所有连接
    let connection_manager = app.state::<ConnectionManager>();
    let conns = connection_manager.connections.lock().unwrap().clone();
    for (server_id, _) in conns {
        let _ = remote_disconnect(server_id, app.clone()).await;
    }

    eprintln!("[Shutdown] 优雅关闭完成");
    std::process::exit(0);
}
```

- [ ] **Step 2: 在 main.rs 中注册钩子**

```rust
// src-tauri/src/main.rs

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            // 注册关闭钩子
            gnome_remote::setup_shutdown_hook(&app.handle());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 3: 测试优雅关闭**

测试步骤：
1. 开始下载文件
2. 按 Ctrl+C 关闭程序
3. 检查控制台输出，确认优雅关闭日志
4. 检查文件是否被正确清理

Expected: 控制台输出优雅关闭日志，临时文件被删除

---

## Task 4: 添加任务列表自动清理机制

**Files:**
- Modify: `src-tauri/src/transfer.rs:58-70`
- Test: 自动清理测试

- [ ] **Step 1: 添加定期清理任务**

```rust
// src-tauri/src/transfer.rs

impl TransferManager {
    /// 启动定期清理任务
    pub fn start_cleanup_task(&self) {
        let tasks = self.tasks.clone();
        let app_handle = self.app_handle.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(300)); // 每5分钟清理一次

            loop {
                interval.tick().await;

                // 检查关闭标志
                if *crate::SHUTDOWN_FLAG.lock().await {
                    break;
                }

                // 清理已完成的任务（保留最近10个）
                let mut tasks_guard = tasks.lock().await;
                let completed_ids: Vec<String> = tasks_guard
                    .iter()
                    .filter(|(_, task)| {
                        task.status == "completed" || task.status == "failed" || task.status == "cancelled"
                    })
                    .map(|(id, _)| id.clone())
                    .collect();

                // 保留最近10个已完成的任务
                if completed_ids.len() > 10 {
                    let to_remove = completed_ids.len() - 10;
                    for id in completed_ids.into_iter().take(to_remove) {
                        tasks_guard.remove(&id);
                    }
                    eprintln!("[TransferManager] 自动清理了 {} 个已完成任务", to_remove);
                }
            }
        });
    }
}
```

- [ ] **Step 2: 在 TransferManager 初始化时启动清理任务**

```rust
// src-tauri/src/transfer.rs

pub fn new(app_handle: AppHandle) -> Self {
    let manager = Self {
        app_handle,
        tasks: Arc::new(Mutex::new(HashMap::new())),
    };

    // 启动定期清理任务
    manager.start_cleanup_task();

    manager
}
```

- [ ] **Step 3: 测试自动清理**

测试步骤：
1. 创建 15 个传输任务
2. 等待 5 分钟
3. 检查任务列表，确认只保留最近 10 个已完成的任务

Expected: 5 分钟后，任务列表只保留最近 10 个

---

## Task 5: 添加前端任务持久化

**Files:**
- Create: `src/utils/transferStorage.ts`
- Modify: `src/hooks/useTransferProgress.ts:84-155`
- Test: 刷新页面测试

- [ ] **Step 1: 创建传输状态持久化工具**

```typescript
// src/utils/transferStorage.ts

import { TransferTask } from '../hooks/useTransferProgress';

const STORAGE_KEY = 'gnome-remote-transfers';

export const transferStorage = {
    save(transfers: TransferTask[]): void {
        try {
            localStorage.setItem(STORAGE_KEY, JSON.stringify(transfers));
        } catch (error) {
            console.error('保存传输状态失败:', error);
        }
    },

    load(): TransferTask[] {
        try {
            const data = localStorage.getItem(STORAGE_KEY);
            return data ? JSON.parse(data) : [];
        } catch (error) {
            console.error('加载传输状态失败:', error);
            return [];
        }
    },

    clear(): void {
        localStorage.removeItem(STORAGE_KEY);
    }
};
```

- [ ] **Step 2: 修改 useTransferProgress Hook，添加持久化**

```typescript
// src/hooks/useTransferProgress.ts

export function useTransferProgress(connectionId?: string) {
    // 从 localStorage 加载初始状态
    const [transfers, setTransfers] = useState<TransferTask[]>(() => {
        return transferStorage.load().filter(task => {
            // 只保留最近24小时的任务
            const age = Date.now() - task.start_time;
            return age < 24 * 60 * 60 * 1000;
        });
    });

    // 监听变化并保存
    useEffect(() => {
        transferStorage.save(transfers);
    }, [transfers]);

    // ... 其他代码
}
```

- [ ] **Step 3: 测试持久化**

测试步骤：
1. 开始下载文件
2. 刷新页面
3. 检查任务列表是否保留

Expected: 刷新后任务列表保留

---

## Task 6: 添加前端关闭事件处理

**Files:**
- Modify: `src/App.tsx`
- Test: 关闭窗口测试

- [ ] **Step 1: 添加 beforeunload 事件处理**

```typescript
// src/App.tsx

import { useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';

function App() {
    useEffect(() => {
        const handleBeforeUnload = (e: BeforeUnloadEvent) => {
            // 通知后端准备关闭
            invoke('prepare_shutdown').catch(console.error);
        };

        window.addEventListener('beforeunload', handleBeforeUnload);

        return () => {
            window.removeEventListener('beforeunload', handleBeforeUnload);
        };
    }, []);

    // ... 其他代码
}
```

- [ ] **Step 2: 在后端添加 prepare_shutdown 命令**

```rust
// src-tauri/src/lib.rs

#[tauri::command]
async fn prepare_shutdown(app: tauri::AppHandle) -> Result<(), String> {
    eprintln!("[Frontend] 收到关闭通知");
    // 前端会在 beforeunload 时调用此命令
    // 可以在这里执行一些快速清理操作
    Ok(())
}
```

- [ ] **Step 3: 测试关闭事件**

测试步骤：
1. 开始下载文件
2. 关闭窗口
3. 检查控制台输出，确认关闭通知

Expected: 控制台输出 "收到关闭通知"

---

## Task 7: 添加心跳机制

**Files:**
- Modify: `src-tauri/src/connection.rs`
- Test: 网络中断测试

- [ ] **Step 1: 在 Connection 中添加心跳逻辑**

```rust
// src-tauri/src/connection.rs

use tokio::time::{interval, Duration};

pub struct Connection {
    pub tx: mpsc::Sender<ClientRequest>,
    last_ping: Arc<Mutex<Instant>>,
}

async fn heartbeat_loop(conn: Connection) {
    let mut interval = interval(Duration::from_secs(30));

    loop {
        interval.tick().await;

        // 发送心跳包
        if let Err(e) = conn.tx.send(ClientRequest::Ping).await {
            eprintln!("[Heartbeat] 心跳失败: {}", e);
            break;
        }

        // 检查上次响应时间
        let last_ping = conn.last_ping.lock().await;
        if last_ping.elapsed() > Duration::from_secs(60) {
            eprintln!("[Heartbeat] 连接超时，断开连接");
            break;
        }
    }
}
```

- [ ] **Step 2: 在连接建立时启动心跳循环**

```rust
// src-tauri/src/connection.rs

pub async fn remote_connect(/* ... */) -> Result<(), String> {
    // ... 连接建立

    // 启动心跳循环
    tokio::spawn(heartbeat_loop(connection.clone()));

    Ok(())
}
```

- [ ] **Step 3: 测试心跳超时**

测试步骤：
1. 连接到远程服务器
2. 等待 60 秒（不操作）
3. 检查连接是否自动断开

Expected: 60 秒后连接自动断开

---

## Task 8: 添加 QUIC Stream 超时机制

**Files:**
- Modify: `src-tauri/src/transfer.rs:691-692`
- Test: 超时测试

- [ ] **Step 1: 在 Stream 操作中添加超时**

```rust
// src-tauri/src/transfer.rs

use tokio::time::{timeout, Duration};

async fn open_stream_with_timeout(conn: &quinn::Connection) -> Result<(quinn::SendStream, quinn::RecvStream), String> {
    timeout(Duration::from_secs(10), conn.open_bi())
        .await
        .map_err(|_| "打开 Stream 超时".to_string())?
        .map_err(|e| format!("打开 Stream 失败: {}", e))
}

// 使用
let (mut send, mut recv) = open_stream_with_timeout(&conn)?;
```

- [ ] **Step 2: 测试超时机制**

测试步骤：
1. 开始下载文件
2. 手动阻塞网络
3. 检查是否超时

Expected: 10 秒后超时

---

## Agent 端修复任务

### Task 9: Agent 端 QUIC Stream 关闭清理

**Files:**
- Modify: `agent/src/server/quic.rs`
- Modify: `agent/src/handler.rs`
- Test: 手动测试连接断开

- [ ] **Step 1: 在 Agent 连接处理中添加 Stream 清理**

```rust
// agent/src/server/quic.rs

use tokio::sync::broadcast;

pub struct Connection {
    pub connection: quinn::Connection,
    shutdown_tx: broadcast::Sender<()>,
}

impl Connection {
    pub fn new(connection: quinn::Connection) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        Self {
            connection,
            shutdown_tx,
        }
    }

    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(());
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        // 发送关闭信号
        self.shutdown();
        // 关闭所有 Stream
        self.connection.close(0u32.into(), b"Connection closed");
    }
}
```

- [ ] **Step 2: 在请求处理中响应关闭信号**

```rust
// agent/src/handler.rs

pub async fn handle_connection(conn: Connection) {
    let mut shutdown_rx = conn.shutdown_tx.subscribe();

    loop {
        tokio::select! {
            // 正常请求处理
            result = conn.accept_bi() => {
                match result {
                    Ok((send, recv)) => {
                        tokio::spawn(handle_request(send, recv));
                    }
                    Err(e) => {
                        eprintln!("[Agent] 接收请求失败: {}", e);
                        break;
                    }
                }
            }

            // 关闭信号
            _ = shutdown_rx.recv() => {
                eprintln!("[Agent] 收到关闭信号，清理资源");
                break;
            }
        }
    }
}
```

- [ ] **Step 3: 测试 Agent 端清理**

测试步骤：
1. 启动 Agent，客户端连接
2. 开始上传文件
3. 客户端断开连接
4. 检查 Agent 端日志，确认清理
5. 检查 Agent 端是否有资源泄漏

Expected: Agent 端正确清理所有资源

---

### Task 10: Agent 端临时文件清理

**Files:**
- Modify: `agent/src/transfer_session.rs`
- Modify: `agent/src/file_stream.rs`
- Test: 手动测试上传中断

- [ ] **Step 1: 在文件接收器中添加临时文件管理**

```rust
// agent/src/file_stream.rs

pub struct FileReceiver {
    file: BufWriter<File>,
    file_size: u64,
    received: u64,
    is_temporary: bool,
    path: PathBuf,
}

impl FileReceiver {
    pub fn new(path: &str, file_size: u64) -> Result<Self, String> {
        let path = PathBuf::from(path);
        let file = File::create(&path)
            .map_err(|e| format!("无法创建文件: {}", e))?;

        Ok(Self {
            file: BufWriter::new(file),
            file_size,
            received: 0,
            is_temporary: true,
            path,
        })
    }

    pub fn mark_completed(&mut self) {
        self.is_temporary = false;
    }
}

impl Drop for FileReceiver {
    fn drop(&mut self) {
        if self.is_temporary && self.received < self.file_size {
            eprintln!("[FileReceiver] 清理临时文件: {:?}", self.path);
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
```

- [ ] **Step 2: 在上传完成时标记**

```rust
// agent/src/transfer_session.rs

if received >= file_size {
    receiver.mark_completed();
    session.status = "completed".to_string();
}
```

- [ ] **Step 3: 测试 Agent 端文件清理**

测试步骤：
1. 开始上传大文件
2. 中断上传（网络断开或客户端关闭）
3. 检查 Agent 端文件是否被删除

Expected: Agent 端自动删除未完成的临时文件

---

### Task 11: Agent 端会话超时检测

**Files:**
- Modify: `agent/src/server/quic.rs`
- Modify: `agent/src/handler.rs`
- Test: 手动测试超时断开

- [ ] **Step 1: 添加会话超时检测**

```rust
// agent/src/server/quic.rs

use tokio::time::{timeout, Duration};

pub async fn handle_connection_with_timeout(conn: Connection) {
    let result = timeout(
        Duration::from_secs(300), // 5分钟超时
        handle_connection(conn)
    ).await;

    match result {
        Ok(_) => eprintln!("[Agent] 连接正常结束"),
        Err(_) => eprintln!("[Agent] 连接超时，强制关闭"),
    }
}
```

- [ ] **Step 2: 添加心跳检测（可选）**

```rust
// agent/src/handler.rs

pub async fn handle_request(send: quinn::SendStream, recv: quinn::RecvStream) {
    // 处理心跳包
    if is_heartbeat(&recv) {
        send_heartbeat_response(send).await;
        return;
    }

    // 处理正常请求
    // ...
}
```

- [ ] **Step 3: 测试超时机制**

测试步骤：
1. 连接 Agent 后不发送任何请求
2. 等待 5 分钟
3. 检查 Agent 是否自动断开连接

Expected: Agent 在超时后自动断开连接

---

## 更新后的执行建议

建议按以下顺序执行：

1. **Phase 1（核心修复）**: Task 1, 2, 3 - 解决客户端高危问题 ✅ 已完成
2. **Phase 2（持久化）**: Task 4, 5, 6 - 提升用户体验
3. **Phase 3（优化）**: Task 7, 8 - 客户端长期改进
4. **Phase 4（Agent 端修复）**: Task 9, 10, 11 - 解决 Agent 端问题

每个 Phase 完成后进行集成测试，确保系统稳定性。