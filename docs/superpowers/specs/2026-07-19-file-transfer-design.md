# 文件传输功能设计文档

> 版本: v1.0 | 日期: 2026-07-19
>
> 基于 GNOME 远程控制客户端架构，设计文件传输功能，支持本地与远程服务器之间的双向文件传输。

---

## 一、需求概述

### 1.1 使用场景

- **双向传输**：既需要上传（本地 → 远程），也需要下载（远程 → 本地）
- **混合文件大小**：支持小文件（<10MB）、中等文件（10-100MB）、大文件（>100MB）
- **批量传输**：支持多文件同时上传/下载，带队列管理

### 1.2 交互方式

- **右键菜单**：文件右键 → "下载"，空白区域右键 → "上传文件"
- **拖拽上传**：支持从本地文件管理器拖拽文件到 FileManager 窗口
- **文件选择对话框**：上传和下载都使用 Windows 原生文件选择对话框（通过 `@tauri-apps/plugin-dialog`）

### 1.3 进度显示

- **独立通知弹窗**：GNOME 风格的通知卡片，显示传输进度
- **实时信息**：进度条、文件名、传输速度、剩余时间
- **队列管理**：支持暂停、继续、取消、重试操作

---

## 二、技术方案

### 2.1 方案选择

选择 **方案 A：扩展现有协议 + 流式传输**

**理由：**
- 与现有 QUIC 协议无缝集成，不引入额外服务器
- QUIC 多路复用，性能最优
- 完全用 Rust 实现（除 UI 前端），符合项目约束
- 支持进度推送、断点续传、大文件传输

### 2.2 架构分层

```
┌─ 前端层（React/TypeScript）───────────────────────┐
│ FileManager.tsx                                     │
│  ├─ 右键菜单：添加"下载"/"上传文件"菜单项           │
│  └─ 拖拽监听：监听文件拖拽事件                       │
│                                                     │
│ TransferNotification.tsx (新增)                     │
│  ├─ 传输进度通知卡片（GNOME 风格）                   │
│  ├─ 实时进度条、速度、剩余时间                       │
│  └─ 暂停/继续/取消控制                               │
└─────────────────────────────────────────────────────┘

┌─ Tauri 后端层（Rust）─────────────────────────────┐
│ lib.rs                                              │
│  ├─ transfer_file() Command                         │
│  ├─ cancel_transfer() Command                       │
│  └─ get_transfer_progress() Command                 │
│                                                     │
│ transfer.rs (新增)                                   │
│  ├─ TransferManager：管理所有传输任务                │
│  ├─ TransferTask：单个传输任务状态                   │
│  └─ TransferProgress：进度推送事件                   │
└─────────────────────────────────────────────────────┘

┌─ QUIC 传输层─────────────────────────────────────┐
│ 协议扩展：                                           │
│  ├─ FileTransferRequest {direction, path}           │
│  ├─ FileTransferAccept {session_id}                 │
│  ├─ FileChunk {session_id, seq, data}               │
│  └─ FileTransferComplete {success, mtime}           │
│                                                     │
│ 传输策略：                                           │
│  ├─ 小文件 (<10MB)：一次性传输                       │
│  ├─ 中等文件 (10-100MB)：分块传输 + 进度推送         │
│  └─ 大文件 (>100MB)：分块 + 断点续传支持             │
└─────────────────────────────────────────────────────┘

┌─ Agent 层（Rust）────────────────────────────────┐
│ handler.rs                                          │
│  ├─ handle_file_transfer()                          │
│  ├─ handle_file_chunk()                             │
│  └─ handle_transfer_complete()                      │
│                                                     │
│ file_stream.rs (新增)                               │
│  ├─ FileStreamReader：分块读取文件                   │
│  ├─ FileStreamWriter：分块写入文件                   │
│  └─ ProgressTracker：进度跟踪                        │
└─────────────────────────────────────────────────────┘
```

---

## 三、协议设计

### 3.1 新增协议消息类型

在现有的 `Payload` 枚举中添加以下类型：

```rust
pub enum Payload {
    // ... 现有类型 ...

    /// 文件传输请求（客户端 → Agent）
    #[serde(rename = "file_transfer")]
    FileTransferRequest {
        direction: String,        // "upload" 或 "download"
        path: String,             // 远程文件路径
        file_size: Option<u64>,   // 文件大小（上传时提供）
        chunk_size: Option<u32>,  // 建议的分块大小（可选，默认 64KB）
    },

    /// 文件传输接受响应（Agent → 客户端）
    #[serde(rename = "file_transfer_accept")]
    FileTransferAccept {
        session_id: String,       // 传输会话 ID
        file_size: u64,           // 文件总大小
        chunk_size: u32,          // 确认的分块大小（字节）
        mtime: Option<u64>,       // 文件修改时间（下载时提供）
    },

    /// 文件数据块（双向传输）
    #[serde(rename = "file_chunk")]
    FileChunk {
        session_id: String,       // 传输会话 ID
        seq: u32,                 // 块序号（从 1 开始）
        data: String,             // base64 编码的文件数据
        size: u32,                // 实际数据大小（字节）
    },

    /// 文件传输完成（双向传输）
    #[serde(rename = "file_transfer_complete")]
    FileTransferComplete {
        session_id: String,       // 传输会话 ID
        success: bool,            // 是否成功
        mtime: Option<u64>,       // 文件修改时间（上传成功后返回）
        error: Option<String>,    // 错误信息（失败时）
    },

    /// 文件传输进度（Agent → 客户端，主动推送）
    #[serde(rename = "file_transfer_progress")]
    FileTransferProgress {
        session_id: String,       // 传输会话 ID
        transferred: u64,         // 已传输字节数
        total: u64,               // 总字节数
        speed_bps: u64,           // 传输速度（字节/秒）
        eta_secs: u64,            // 预计剩余时间（秒）
    },

    /// 取消文件传输（客户端 → Agent）
    #[serde(rename = "cancel_file_transfer")]
    CancelFileTransfer {
        session_id: String,       // 传输会话 ID
    },

    /// 取消文件传输响应（Agent → 客户端）
    #[serde(rename = "cancel_file_transfer_resp")]
    CancelFileTransferResponse {
        session_id: String,       // 传输会话 ID
        success: bool,            // 是否成功取消
    },
}
```

### 3.2 传输流程

**上传流程（本地 → 远程）：**
```
用户触发上传（右键菜单或拖拽）
→ 打开 Windows 文件选择对话框（多选）
→ 用户选择文件
→ 弹出 TransferNotification
→ Tauri transfer_file() 创建任务
→ TransferManager 创建 QUIC Stream
→ 发送 FileTransferRequest {upload, "/remote/path/file.txt"}
→ Agent 验证权限，返回 FileTransferAccept {session_id}
→ 客户端分块读取本地文件
→ 发送多个 FileChunk {session_id, seq, base64_data}
→ Agent 分块写入远程文件
→ 发送 FileTransferComplete {success, mtime}
→ 通知推送进度更新
→ FileManager 刷新目录
```

**下载流程（远程 → 本地）：**
```
用户右键文件 → "下载"
→ 打开 Windows 保存对话框
→ 用户选择保存位置
→ 弹出 TransferNotification
→ Tauri transfer_file() 创建任务
→ TransferManager 创建 QUIC Stream
→ 发送 FileTransferRequest {download, "/remote/path/file.txt"}
→ Agent 验证权限，返回 FileTransferAccept {session_id, file_size}
→ Agent 分块读取远程文件
→ 发送多个 FileChunk {session_id, seq, base64_data}
→ 客户端分块写入本地文件
→ 发送 FileTransferComplete {success}
→ 通知推送进度更新
```

### 3.3 智能分块策略

```rust
fn calculate_chunk_size(file_size: u64) -> u32 {
    if file_size < 10 * 1024 * 1024 {
        // 小文件 (<10MB)：一次性传输
        file_size as u32
    } else if file_size < 100 * 1024 * 1024 {
        // 中等文件 (10-100MB)：64KB 分块
        64 * 1024
    } else {
        // 大文件 (>100MB)：256KB 分块
        256 * 1024
    }
}
```

---

## 四、前端实现

### 4.1 文件选择对话框

使用 `@tauri-apps/plugin-dialog` 的 API：

**上传：**
```typescript
import { open } from '@tauri-apps/plugin-dialog';

const selectedFiles = await open({
    multiple: true,  // 支持多选
    directory: false, // 选择文件（不是文件夹）
    title: '选择要上传的文件',
    filters: [
        { name: '所有文件', extensions: ['*'] },
    ],
});

// selectedFiles 是字符串数组（多选）或字符串（单选）
const files = Array.isArray(selectedFiles) ? selectedFiles : [selectedFiles];
```

**下载：**
```typescript
import { save } from '@tauri-apps/plugin-dialog';

const localPath = await save({
    defaultPath: entry.name,  // 默认文件名
    title: '保存文件',
    filters: [
        { name: '所有文件', extensions: ['*'] },
    ],
});
```

### 4.2 TransferNotification 组件

**组件结构：**
```typescript
interface TransferTask {
    id: string;
    direction: 'upload' | 'download';
    fileName: string;
    remotePath: string;
    localPath?: string;
    fileSize: number;
    transferred: number;
    speed: number;  // 字节/秒
    eta: number;    // 秒
    status: 'pending' | 'active' | 'paused' | 'completed' | 'error';
    error?: string;
    progress: number;  // 0-100
}
```

**GNOME 风格样式：**
- 使用项目现有的 CSS 变量（`--ovelis-*`）
- 毛玻璃背景（`backdrop-filter: blur(20px)`）
- 圆角（`--ovelis-radius-lg`）
- 固定在窗口右下角（`position: fixed; bottom: 20px; right: 20px`）

### 4.3 右键菜单集成

在 FileManager 的右键菜单中添加：

**文件右键菜单：**
- "下载"（仅文件，非文件夹）

**空白区域右键菜单：**
- "上传文件"

---

## 五、Tauri 后端实现（Rust）

### 5.1 模块结构

```
src-tauri/src/
├── lib.rs              # Tauri Command 注册
├── connection.rs       # 现有的连接管理
├── terminal.rs         # 现有的终端管理
└── transfer.rs         # 新增：文件传输管理
    ├── TransferManager    # 传输任务管理器
    ├── TransferTask       # 单个传输任务
    ├── FileStreamReader   # 文件分块读取（上传）
    └── FileStreamWriter   # 文件分块写入（下载）
```

### 5.2 Tauri Commands

```rust
#[tauri::command]
async fn transfer_file(
    server_id: String,
    direction: String,  // "upload" 或 "download"
    remote_path: String,
    local_path: String,
) -> Result<String, String>;  // 返回 task_id

#[tauri::command]
async fn pause_transfer(task_id: String) -> Result<(), String>;

#[tauri::command]
async fn cancel_transfer(task_id: String) -> Result<(), String>;

#[tauri::command]
async fn get_transfer_progress() -> Result<Vec<TransferProgressPayload>, String>;
```

### 5.3 进度推送

使用 Tauri 事件系统推送进度更新：

```rust
self.app_handle.emit("transfer-progress", TransferProgressPayload {
    task_id: task.id.clone(),
    session_id: task.session_id.clone(),
    direction: match task.direction {
        TransferDirection::Upload => "upload",
        TransferDirection::Download => "download",
    },
    file_size: task.file_size,
    transferred: task.transferred,
    progress: calculate_progress(task),
    speed_bps: calculate_speed(task),
    eta_secs: calculate_eta(task),
    status: task.status.to_string(),
});
```

---

## 六、Agent 后端实现（Rust）

### 6.1 模块结构

```
agent/src/
├── handler.rs          # 扩展：处理文件传输请求
├── protocol.rs         # 扩展：协议定义
├── file_stream.rs      # 新增：文件流处理
│   ├── FileStreamReader   # 分块读取文件
│   ├── FileStreamWriter   # 分块写入文件
│   └── ProgressTracker    # 进度跟踪
└── transfer_session.rs # 新增：传输会话管理
    └── TransferSession    # 会话状态管理
```

### 6.2 文件流处理

**FileStreamReader（下载）：**
```rust
pub struct FileStreamReader {
    file: BufReader<File>,
    file_size: u64,
    transferred: u64,
    chunk_size: u32,
}

impl FileStreamReader {
    pub fn read_next_chunk(&mut self) -> Result<Option<Vec<u8>>, String>;
    pub fn progress(&self) -> u32;
}
```

**FileStreamWriter（上传）：**
```rust
pub struct FileStreamWriter {
    file: BufWriter<File>,
    file_size: u64,
    transferred: u64,
    temp_path: String,
    final_path: String,
}

impl FileStreamWriter {
    pub fn write_chunk(&mut self, data: &[u8]) -> Result<(), String>;
    pub fn finish(&mut self) -> Result<(), String>;  // 重命名临时文件
    pub fn abort(&mut self);  // 删除临时文件
}
```

### 6.3 安全检查

```rust
// 1. 路径白名单检查
if !cfg.security.allowed_paths.is_empty() {
    let allowed = cfg.security.allowed_paths
        .iter()
        .any(|prefix| path.starts_with(prefix));
    if !allowed {
        return Err(format!("访问被拒绝: {}", path));
    }
}

// 2. 文件大小限制
const MAX_FILE_SIZE: u64 = 1024 * 1024 * 1024;  // 1GB
if file_size > MAX_FILE_SIZE {
    return Err(format!("文件太大: {} MB", file_size / (1024 * 1024)));
}

// 3. 磁盘空间检查（上传）
let available_space = get_available_space(&path)?;
if available_space < file_size {
    return Err(format!("磁盘空间不足"));
}
```

---

## 七、错误处理

### 7.1 常见错误场景

| 错误场景 | 处理方式 |
|---------|---------|
| 文件权限不足 | 返回错误消息："权限不足: 需要 Linux 用户权限" |
| 磁盘空间不足 | 返回错误消息："磁盘空间不足: 可用 X MB, 需要 Y MB" |
| 文件已存在 | 前端弹出确认对话框："文件已存在，是否覆盖？" |
| 网络中断 | 保存续传信息，下次连接时提示恢复传输 |
| 文件大小超限 | 返回错误消息："文件太大: X MB (限制 Y MB)" |
| 传输超时 | 1小时无活动自动取消传输，清理临时文件 |

### 7.2 并发限制

```rust
const MAX_CONCURRENT_TRANSFERS: usize = 3;

// 超过限制时，新任务进入排队状态（Pending）
```

---

## 八、实现计划

### 8.1 阶段划分

**阶段 1：核心功能（1-2 周）**
- 协议扩展（FileTransferRequest/Accept/Chunk/Complete）
- Agent 文件流处理（FileStreamReader/Writer）
- Tauri TransferManager
- 前端右键菜单 + 文件选择对话框
- 基本的传输进度显示

**阶段 2：优化和体验（1 周）**
- 传输速度和剩余时间计算
- 批量传输队列管理
- 错误处理和重试机制
- 文件存在时的覆盖确认

**阶段 3：高级功能（可选）**
- 断点续传
- 文件夹递归传输
- 传输历史记录

### 8.2 依赖项

**前端：**
- `@tauri-apps/plugin-dialog`：已安装（tauri-plugin-dialog = "2"）

**后端：**
- 无需额外依赖，使用现有的 `std::fs`、`tokio`、`serde`

---

## 九、技术约束

1. **除 UI 前端外，全部用 Rust 实现**（符合项目规范）
2. **使用现有的 QUIC 协议**，不引入额外的服务器或协议
3. **遵循 GNOME HIG 设计规范**，UI 组件使用项目现有的 CSS 变量
4. **权限检查依赖 Linux 文件系统权限**，Agent 以普通用户身份运行

---

## 十、附录：代码示例

### A. 前端调用示例

```typescript
// 上传文件
const handleUpload = async () => {
    const files = await open({ multiple: true });
    if (!files) return;

    for (const localPath of Array.isArray(files) ? files : [files]) {
        const fileName = localPath.split(/[\\/]/).pop();
        const remotePath = `${currentPath}/${fileName}`;
        
        await invoke('transfer_file', {
            serverId: activeServerId,
            direction: 'upload',
            remotePath,
            localPath,
        });
    }
};

// 下载文件
const handleDownload = async (entry: FileEntry) => {
    const localPath = await save({ defaultPath: entry.name });
    if (!localPath) return;

    const remotePath = `${currentPath}/${entry.name}`;
    
    await invoke('transfer_file', {
        serverId: activeServerId,
        direction: 'download',
        remotePath,
        localPath,
    });
};
```

### B. Tauri 进度监听

```typescript
// 监听传输进度事件
import { listen } from '@tauri-apps/api/event';

const unlisten = await listen<TransferProgressPayload>('transfer-progress', (event) => {
    const { task_id, progress, speed_bps, eta_secs, status } = event.payload;
    
    // 更新 TransferNotification 组件状态
    setTransfers(prev => prev.map(t => 
        t.id === task_id 
            ? { ...t, progress, speed: speed_bps, eta: eta_secs, status }
            : t
    ));
});
```

---

## 十一、总结

本设计文档详细描述了文件传输功能的架构、协议、实现方案和错误处理机制。核心思路是扩展现有 QUIC 协议，使用分块传输支持大文件，通过 Tauri 事件系统推送实时进度，所有后端逻辑用 Rust 实现，前端 UI 遵循 GNOME 设计规范。

下一步：使用 `writing-plans` 技能编写详细的实现计划。