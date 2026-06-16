# 阶段 3：文件操作功能实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现完整的文件操作功能，包括新建文件夹、重命名、删除、复制/移动文件，对齐 GNOME Files 的用户体验。

**Architecture:** Agent 提供 5 个文件操作 API（mkdir、rename、delete、copy、move），FileManager 通过右键菜单和快捷键触发操作，所有操作都经过权限检查。

**Tech Stack:** Rust (Agent) + TypeScript/React (FileManager) + Tauri invoke API

---

## 文件结构

**Agent 端：**
- `agent/src/protocol.rs` - 新增 5 个 Payload 类型
- `agent/src/handler.rs` - 新增 5 个 handler 函数
- `agent/src/security.rs` - 新增权限检查函数（可选，当前依赖 Linux 权限）

**客户端端：**
- `src-tauri/src/connection.rs` - 新增 5 个 Tauri command
- `src/apps/FileManager.tsx` - 新增文件操作 UI 和右键菜单
- `src/apps/FileManager.css` - 新增右键菜单样式

---

## Task 1: Agent 扩展协议类型

**Files:**
- Modify: `agent/src/protocol.rs:10-50`

- [ ] **Step 1: 添加新的 Payload 类型**

```rust
// agent/src/protocol.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Payload {
    // 已有类型...
    ReadDirRequest { path: String },
    ReadDirResponse { path: String, entries: Vec<FileEntry> },

    // 新增：文件操作类型
    MkdirRequest { path: String },
    MkdirResponse { success: bool, path: String },

    RenameRequest { old_path: String, new_path: String },
    RenameResponse { success: bool, old_path: String, new_path: String },

    DeleteRequest { path: String },
    DeleteResponse { success: bool, path: String },

    CopyRequest { src: String, dst: String },
    CopyResponse { success: bool, src: String, dst: String },

    MoveRequest { src: String, dst: String },
    MoveResponse { success: bool, src: String, dst: String },

    // 错误类型
    Error { message: String, code: u32 },
}
```

- [ ] **Step 2: 编译验证**

Run: `cd agent && cargo check`
Expected: 编译成功，无错误

- [ ] **Step 3: Commit**

```bash
git add agent/src/protocol.rs
git commit -m "feat(agent): add file operation payload types"
```

---

## Task 2: Agent 实现 mkdir handler

**Files:**
- Modify: `agent/src/handler.rs:100-150`

- [ ] **Step 1: 实现 handle_mkdir 函数**

```rust
// agent/src/handler.rs
fn handle_mkdir(path: String) -> Payload {
    // 注意：当前依赖 Linux 文件系统权限，不再检查 allowed_paths
    // 如果需要限制，可以在 agent.toml 中配置 allowed_paths

    match fs::create_dir_all(&path) {
        Ok(_) => {
            println!("[Agent] mkdir 成功: {}", path);
            Payload::MkdirResponse { success: true, path }
        }
        Err(e) => {
            let error_msg = if e.kind() == std::io::ErrorKind::PermissionDenied {
                format!("权限不足，无法创建目录 '{}'（需要相应的 Linux 用户权限）", path)
            } else {
                format!("无法创建目录 '{}': {}", path, e)
            };
            println!("[Agent] mkdir 失败: {}", error_msg);
            Payload::Error { message: error_msg, code: 500 }
        }
    }
}
```

- [ ] **Step 2: 在 handle_payload 中添加路由**

```rust
// agent/src/handler.rs
pub fn handle_payload(payload: Payload) -> Payload {
    match payload {
        // 已有路由...
        Payload::ReadDirRequest { path } => handle_read_dir(path),

        // 新增路由
        Payload::MkdirRequest { path } => handle_mkdir(path),

        _ => Payload::Error { message: "未知请求类型".into(), code: 400 },
    }
}
```

- [ ] **Step 3: 编译验证**

Run: `cd agent && cargo check`
Expected: 编译成功，无错误

- [ ] **Step 4: Commit**

```bash
git add agent/src/handler.rs
git commit -m "feat(agent): implement mkdir handler"
```

---

## Task 3: Agent 实现 rename handler

**Files:**
- Modify: `agent/src/handler.rs:150-200`

- [ ] **Step 1: 实现 handle_rename 函数**

```rust
// agent/src/handler.rs
fn handle_rename(old_path: String, new_path: String) -> Payload {
    match fs::rename(&old_path, &new_path) {
        Ok(_) => {
            println!("[Agent] rename 成功: {} -> {}", old_path, new_path);
            Payload::RenameResponse { success: true, old_path, new_path }
        }
        Err(e) => {
            let error_msg = if e.kind() == std::io::ErrorKind::PermissionDenied {
                format!("权限不足，无法重命名 '{}'（需要相应的 Linux 用户权限）", old_path)
            } else {
                format!("无法重命名 '{}': {}", old_path, e)
            };
            println!("[Agent] rename 失败: {}", error_msg);
            Payload::Error { message: error_msg, code: 500 }
        }
    }
}
```

- [ ] **Step 2: 在 handle_payload 中添加路由**

```rust
// agent/src/handler.rs
pub fn handle_payload(payload: Payload) -> Payload {
    match payload {
        // 已有路由...
        Payload::MkdirRequest { path } => handle_mkdir(path),

        // 新增路由
        Payload::RenameRequest { old_path, new_path } => handle_rename(old_path, new_path),

        _ => Payload::Error { message: "未知请求类型".into(), code: 400 },
    }
}
```

- [ ] **Step 3: 编译验证**

Run: `cd agent && cargo check`
Expected: 编译成功，无错误

- [ ] **Step 4: Commit**

```bash
git add agent/src/handler.rs
git commit -m "feat(agent): implement rename handler"
```

---

## Task 4: Agent 实现 delete handler

**Files:**
- Modify: `agent/src/handler.rs:200-250`

- [ ] **Step 1: 实现 handle_delete 函数**

```rust
// agent/src/handler.rs
fn handle_delete(path: String) -> Payload {
    // 检查路径是否存在
    let metadata = match fs::metadata(&path) {
        Ok(m) => m,
        Err(e) => {
            let error_msg = if e.kind() == std::io::ErrorKind::NotFound {
                format!("路径 '{}' 不存在", path)
            } else {
                format!("无法访问 '{}': {}", path, e)
            };
            return Payload::Error { message: error_msg, code: 404 };
        }
    };

    // 根据类型删除
    let result = if metadata.is_dir() {
        fs::remove_dir_all(&path)
    } else {
        fs::remove_file(&path)
    };

    match result {
        Ok(_) => {
            println!("[Agent] delete 成功: {}", path);
            Payload::DeleteResponse { success: true, path }
        }
        Err(e) => {
            let error_msg = if e.kind() == std::io::ErrorKind::PermissionDenied {
                format!("权限不足，无法删除 '{}'（需要相应的 Linux 用户权限）", path)
            } else {
                format!("无法删除 '{}': {}", path, e)
            };
            println!("[Agent] delete 失败: {}", error_msg);
            Payload::Error { message: error_msg, code: 500 }
        }
    }
}
```

- [ ] **Step 2: 在 handle_payload 中添加路由**

```rust
// agent/src/handler.rs
pub fn handle_payload(payload: Payload) -> Payload {
    match payload {
        // 已有路由...
        Payload::RenameRequest { old_path, new_path } => handle_rename(old_path, new_path),

        // 新增路由
        Payload::DeleteRequest { path } => handle_delete(path),

        _ => Payload::Error { message: "未知请求类型".into(), code: 400 },
    }
}
```

- [ ] **Step 3: 编译验证**

Run: `cd agent && cargo check`
Expected: 编译成功，无错误

- [ ] **Step 4: Commit**

```bash
git add agent/src/handler.rs
git commit -m "feat(agent): implement delete handler"
```

---

## Task 5: Agent 实现 copy handler

**Files:**
- Modify: `agent/src/handler.rs:250-300`

- [ ] **Step 1: 实现 handle_copy 函数**

```rust
// agent/src/handler.rs
fn handle_copy(src: String, dst: String) -> Payload {
    // 检查源文件是否存在
    let metadata = match fs::metadata(&src) {
        Ok(m) => m,
        Err(e) => {
            let error_msg = if e.kind() == std::io::ErrorKind::NotFound {
                format!("源文件 '{}' 不存在", src)
            } else {
                format!("无法访问源文件 '{}': {}", src, e)
            };
            return Payload::Error { message: error_msg, code: 404 };
        }
    };

    // 只支持文件复制，不支持目录复制（目录复制需要递归）
    if metadata.is_dir() {
        return Payload::Error {
            message: format!("不支持复制目录 '{}'（请使用移动功能）", src),
            code: 400,
        };
    }

    match fs::copy(&src, &dst) {
        Ok(_) => {
            println!("[Agent] copy 成功: {} -> {}", src, dst);
            Payload::CopyResponse { success: true, src, dst }
        }
        Err(e) => {
            let error_msg = if e.kind() == std::io::ErrorKind::PermissionDenied {
                format!("权限不足，无法复制 '{}'（需要相应的 Linux 用户权限）", src)
            } else {
                format!("无法复制 '{}': {}", src, e)
            };
            println!("[Agent] copy 失败: {}", error_msg);
            Payload::Error { message: error_msg, code: 500 }
        }
    }
}
```

- [ ] **Step 2: 在 handle_payload 中添加路由**

```rust
// agent/src/handler.rs
pub fn handle_payload(payload: Payload) -> Payload {
    match payload {
        // 已有路由...
        Payload::DeleteRequest { path } => handle_delete(path),

        // 新增路由
        Payload::CopyRequest { src, dst } => handle_copy(src, dst),

        _ => Payload::Error { message: "未知请求类型".into(), code: 400 },
    }
}
```

- [ ] **Step 3: 编译验证**

Run: `cd agent && cargo check`
Expected: 编译成功，无错误

- [ ] **Step 4: Commit**

```bash
git add agent/src/handler.rs
git commit -m "feat(agent): implement copy handler"
```

---

## Task 6: Agent 实现 move handler

**Files:**
- Modify: `agent/src/handler.rs:300-350`

- [ ] **Step 1: 实现 handle_move 函数**

```rust
// agent/src/handler.rs
fn handle_move(src: String, dst: String) -> Payload {
    // move 本质上是 rename（跨文件系统移动需要先 copy 再 delete）
    // 这里简化实现，直接使用 rename
    handle_rename(src, dst)
}
```

- [ ] **Step 2: 在 handle_payload 中添加路由**

```rust
// agent/src/handler.rs
pub fn handle_payload(payload: Payload) -> Payload {
    match payload {
        // 已有路由...
        Payload::CopyRequest { src, dst } => handle_copy(src, dst),

        // 新增路由
        Payload::MoveRequest { src, dst } => handle_move(src, dst),

        _ => Payload::Error { message: "未知请求类型".into(), code: 400 },
    }
}
```

- [ ] **Step 3: 编译验证**

Run: `cd agent && cargo check`
Expected: 编译成功，无错误

- [ ] **Step 4: Commit**

```bash
git add agent/src/handler.rs
git commit -m "feat(agent): implement move handler"
```

---

## Task 7: 客户端添加 Tauri commands

**Files:**
- Modify: `src-tauri/src/connection.rs:200-300`

- [ ] **Step 1: 实现 remote_mkdir command**

```rust
// src-tauri/src/connection.rs
#[tauri::command]
pub async fn remote_mkdir(server_id: String, path: String, app: tauri::AppHandle) -> Result<bool, String> {
    println!("[Connection] remote_mkdir: server_id={}, path={}", server_id, path);

    let resp = remote_send(server_id, Payload::MkdirRequest { path }, app).await?;

    match resp.payload {
        Payload::MkdirResponse { success, .. } => Ok(success),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}
```

- [ ] **Step 2: 实现 remote_rename command**

```rust
// src-tauri/src/connection.rs
#[tauri::command]
pub async fn remote_rename(
    server_id: String,
    old_path: String,
    new_path: String,
    app: tauri::AppHandle
) -> Result<bool, String> {
    println!("[Connection] remote_rename: old={}, new={}", old_path, new_path);

    let resp = remote_send(server_id, Payload::RenameRequest { old_path, new_path }, app).await?;

    match resp.payload {
        Payload::RenameResponse { success, .. } => Ok(success),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}
```

- [ ] **Step 3: 实现 remote_delete command**

```rust
// src-tauri/src/connection.rs
#[tauri::command]
pub async fn remote_delete(server_id: String, path: String, app: tauri::AppHandle) -> Result<bool, String> {
    println!("[Connection] remote_delete: path={}", path);

    let resp = remote_send(server_id, Payload::DeleteRequest { path }, app).await?;

    match resp.payload {
        Payload::DeleteResponse { success, .. } => Ok(success),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}
```

- [ ] **Step 4: 实现 remote_copy command**

```rust
// src-tauri/src/connection.rs
#[tauri::command]
pub async fn remote_copy(
    server_id: String,
    src: String,
    dst: String,
    app: tauri::AppHandle
) -> Result<bool, String> {
    println!("[Connection] remote_copy: src={}, dst={}", src, dst);

    let resp = remote_send(server_id, Payload::CopyRequest { src, dst }, app).await?;

    match resp.payload {
        Payload::CopyResponse { success, .. } => Ok(success),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}
```

- [ ] **Step 5: 实现 remote_move command**

```rust
// src-tauri/src/connection.rs
#[tauri::command]
pub async fn remote_move(
    server_id: String,
    src: String,
    dst: String,
    app: tauri::AppHandle
) -> Result<bool, String> {
    println!("[Connection] remote_move: src={}, dst={}", src, dst);

    let resp = remote_send(server_id, Payload::MoveRequest { src, dst }, app).await?;

    match resp.payload {
        Payload::MoveResponse { success, .. } => Ok(success),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}
```

- [ ] **Step 6: 在 lib.rs 中注册 commands**

```rust
// src-tauri/src/lib.rs
fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            // 已有 commands...
            remote_connect,
            remote_disconnect,
            remote_read_dir,
            remote_get_current_user,
            remote_get_mounts,

            // 新增 commands
            remote_mkdir,
            remote_rename,
            remote_delete,
            remote_copy,
            remote_move,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 7: 编译验证**

Run: `cd src-tauri && cargo check`
Expected: 编译成功，无错误

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/connection.rs src-tauri/src/lib.rs
git commit -m "feat(client): add file operation tauri commands"
```

---

## Task 8: FileManager 实现新建文件夹 UI

**Files:**
- Modify: `src/apps/FileManager.tsx:300-400`

- [ ] **Step 1: 添加 handleMkdir 函数**

```typescript
// src/apps/FileManager.tsx
const handleMkdir = async () => {
  // 使用简单的 prompt（后续可以改为对话框）
  const name = prompt("新建文件夹名称:");
  if (!name || name.trim() === "") return;

  // 构建新路径
  const newPath = IS_WIN
    ? `${currentPath}\\${name.trim()}`
    : `${currentPath}/${name.trim()}`;

  console.log("[FileManager] mkdir:", newPath);

  try {
    if (activeServerId) {
      await invoke("remote_mkdir", { serverId: activeServerId, path: newPath });
    } else {
      // 本地模式（暂不支持）
      alert("本地模式暂不支持文件操作");
      return;
    }

    // 刷新当前目录
    loadDir(currentPath);
  } catch (err) {
    console.error("[FileManager] mkdir 失败:", err);
    alert(`创建文件夹失败: ${err}`);
  }
};
```

- [ ] **Step 2: 在右键菜单中添加"新建文件夹"选项**

```typescript
// src/apps/FileManager.tsx
// 在右键菜单的空白区域点击时显示
<div className="fm-context-menu">
  <div className="ctx-item" onClick={handleMkdir}>
    <span className="ctx-icon">📁</span> 新建文件夹
  </div>
  <div className="ctx-separator" />
  {/* 其他选项 */}
</div>
```

- [ ] **Step 3: 编译验证**

Run: `npm run typecheck`
Expected: 类型检查通过，无错误

- [ ] **Step 4: Commit**

```bash
git add src/apps/FileManager.tsx
git commit -m "feat(filemanager): add mkdir UI"
```

---

## Task 9: FileManager 实现重命名 UI

**Files:**
- Modify: `src/apps/FileManager.tsx:400-500`

- [ ] **Step 1: 添加 handleRename 函数**

```typescript
// src/apps/FileManager.tsx
const handleRename = async (entry: FileEntry) => {
  // 使用简单的 prompt
  const newName = prompt("新名称:", entry.name);
  if (!newName || newName.trim() === "" || newName === entry.name) return;

  // 构建路径
  const oldPath = IS_WIN
    ? `${currentPath}\\${entry.name}`
    : `${currentPath}/${entry.name}`;

  const newPath = IS_WIN
    ? `${currentPath}\\${newName.trim()}`
    : `${currentPath}/${newName.trim()}`;

  console.log("[FileManager] rename:", oldPath, "->", newPath);

  try {
    if (activeServerId) {
      await invoke("remote_rename", {
        serverId: activeServerId,
        oldPath,
        newPath
      });
    } else {
      alert("本地模式暂不支持文件操作");
      return;
    }

    // 刷新当前目录
    loadDir(currentPath);
  } catch (err) {
    console.error("[FileManager] rename 失败:", err);
    alert(`重命名失败: ${err}`);
  }
};
```

- [ ] **Step 2: 在右键菜单中添加"重命名"选项**

```typescript
// src/apps/FileManager.tsx
// 在文件项的右键菜单中显示
<div className="fm-context-menu">
  <div className="ctx-item" onClick={() => handleOpen(entry)}>
    <span className="ctx-icon">📂</span> 打开
  </div>
  <div className="ctx-separator" />

  <div className="ctx-item" onClick={() => handleRename(entry)}>
    <span className="ctx-icon">✏️</span> 重命名
  </div>

  {/* 其他选项 */}
</div>
```

- [ ] **Step 3: 编译验证**

Run: `npm run typecheck`
Expected: 类型检查通过，无错误

- [ ] **Step 4: Commit**

```bash
git add src/apps/FileManager.tsx
git commit -m "feat(filemanager): add rename UI"
```

---

## Task 10: FileManager 实现删除 UI

**Files:**
- Modify: `src/apps/FileManager.tsx:500-600`

- [ ] **Step 1: 添加 handleDelete 函数**

```typescript
// src/apps/FileManager.tsx
const handleDelete = async (entry: FileEntry) => {
  // 使用简单的 confirm
  const confirmed = confirm(`确定删除 "${entry.name}"?\n\n${entry.is_dir ? "这将删除文件夹及其所有内容。" : "此操作无法撤销。"}`);
  if (!confirmed) return;

  // 构建路径
  const path = IS_WIN
    ? `${currentPath}\\${entry.name}`
    : `${currentPath}/${entry.name}`;

  console.log("[FileManager] delete:", path);

  try {
    if (activeServerId) {
      await invoke("remote_delete", { serverId: activeServerId, path });
    } else {
      alert("本地模式暂不支持文件操作");
      return;
    }

    // 刷新当前目录
    loadDir(currentPath);
  } catch (err) {
    console.error("[FileManager] delete 失败:", err);
    alert(`删除失败: ${err}`);
  }
};
```

- [ ] **Step 2: 在右键菜单中添加"删除"选项**

```typescript
// src/apps/FileManager.tsx
<div className="fm-context-menu">
  {/* 已有选项... */}
  <div className="ctx-item" onClick={() => handleRename(entry)}>
    <span className="ctx-icon">✏️</span> 重命名
  </div>

  <div className="ctx-item" onClick={() => handleDelete(entry)}>
    <span className="ctx-icon">🗑️</span> 删除
  </div>

  <div className="ctx-separator" />
  <div className="ctx-item" onClick={() => showProperties(entry)}>
    <span className="ctx-icon">ℹ️</span> 属性
  </div>
</div>
```

- [ ] **Step 3: 编译验证**

Run: `npm run typecheck`
Expected: 类型检查通过，无错误

- [ ] **Step 4: Commit**

```bash
git add src/apps/FileManager.tsx
git commit -m "feat(filemanager): add delete UI"
```

---

## Task 11: FileManager 实现复制/移动 UI（可选）

**Files:**
- Modify: `src/apps/FileManager.tsx:600-700`

**注意：** 复制/移动功能需要选择目标路径，实现较为复杂。本任务标记为可选，可以在后续阶段实现。

- [ ] **Step 1: 添加剪贴板状态管理**

```typescript
// src/apps/FileManager.tsx
const [clipboard, setClipboard] = useState<{
  type: "copy" | "move";
  entries: FileEntry[];
} | null>(null);

const handleCopy = (entries: FileEntry[]) => {
  setClipboard({ type: "copy", entries });
};

const handleCut = (entries: FileEntry[]) => {
  setClipboard({ type: "move", entries });
};
```

- [ ] **Step 2: 添加粘贴函数**

```typescript
// src/apps/FileManager.tsx
const handlePaste = async () => {
  if (!clipboard) return;

  for (const entry of clipboard.entries) {
    const srcPath = IS_WIN
      ? `${currentPath}\\${entry.name}`
      : `${currentPath}/${entry.name}`;

    const dstPath = IS_WIN
      ? `${currentPath}\\${entry.name}`
      : `${currentPath}/${entry.name}`;

    try {
      if (clipboard.type === "copy") {
        await invoke("remote_copy", { serverId: activeServerId, src: srcPath, dst: dstPath });
      } else {
        await invoke("remote_move", { serverId: activeServerId, src: srcPath, dst: dstPath });
      }
    } catch (err) {
      alert(`粘贴失败: ${err}`);
    }
  }

  // 清空剪贴板并刷新
  setClipboard(null);
  loadDir(currentPath);
};
```

- [ ] **Step 3: 在右键菜单中添加复制/剪切/粘贴选项**

```typescript
// src/apps/FileManager.tsx
<div className="fm-context-menu">
  {/* 已有选项... */}
  <div className="ctx-item" onClick={() => handleCopy([entry])}>
    <span className="ctx-icon">📋</span> 复制
  </div>
  <div className="ctx-item" onClick={() => handleCut([entry])}>
    <span className="ctx-icon">✂️</span> 剪切
  </div>
  {clipboard && (
    <div className="ctx-item" onClick={handlePaste}>
      <span className="ctx-icon">📄</span> 粘贴
    </div>
  )}
</div>
```

- [ ] **Step 4: 编译验证**

Run: `npm run typecheck`
Expected: 类型检查通过，无错误

- [ ] **Step 5: Commit**

```bash
git add src/apps/FileManager.tsx
git commit -m "feat(filemanager): add copy/move UI (optional)"
```

---

## Task 12: FileManager 添加右键菜单样式

**Files:**
- Modify: `src/apps/FileManager.css:200-300`

- [ ] **Step 1: 添加右键菜单样式**

```css
/* src/apps/FileManager.css */
.fm-context-menu {
  position: fixed;
  background: var(--card-bg);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  padding: 4px 0;
  min-width: 200px;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
  z-index: 1000;
}

.ctx-item {
  display: flex;
  align-items: center;
  padding: 8px 12px;
  gap: 8px;
  cursor: pointer;
  font-size: 10pt;

  &:hover {
    background: var(--accent-bg);
    color: var(--accent-fg);
  }
}

.ctx-icon {
  font-size: 16px;
}

.ctx-separator {
  height: 1px;
  background: var(--border-color);
  margin: 4px 0;
}
```

- [ ] **Step 2: 验证样式**

Run: `npm run dev`
Expected: 右键菜单样式符合 GNOME HIG

- [ ] **Step 3: Commit**

```bash
git add src/apps/FileManager.css
git commit -m "feat(filemanager): add context menu styles"
```

---

## Task 13: 测试和验收

**Files:**
- Test: 手动测试所有文件操作功能

- [ ] **Step 1: 测试新建文件夹**

测试步骤：
1. 连接远程服务器
2. 在 FileManager 中右键点击空白区域
3. 选择"新建文件夹"
4. 输入名称（例如 "test-folder"）
5. 验证文件夹创建成功，目录列表自动刷新

Expected: 文件夹创建成功，显示在列表中

- [ ] **Step 2: 测试重命名**

测试步骤：
1. 右键点击任意文件或文件夹
2. 选择"重命名"
3. 输入新名称
4. 验证重命名成功，目录列表自动刷新

Expected: 重命名成功，显示新名称

- [ ] **Step 3: 测试删除**

测试步骤：
1. 右键点击刚创建的 "test-folder"
2. 选择"删除"
3. 确认删除对话框
4. 验证删除成功，目录列表自动刷新

Expected: 删除成功，文件不再显示

- [ ] **Step 4: 测试权限不足场景**

测试步骤：
1. 尝试在 `/etc` 目录下创建文件夹
2. 验证显示权限不足错误

Expected: 显示友好错误消息："权限不足，无法创建目录..."

- [ ] **Step 5: 测试 Agent 和客户端日志**

测试步骤：
1. 查看 Agent 日志：`tail -f /var/log/gnome-remote-agent.log`
2. 查看客户端日志：浏览器开发者工具 Console
3. 验证所有操作都有清晰的日志输出

Expected: 日志清晰，包含操作类型、路径、成功/失败状态

- [ ] **Step 6: 验收标准检查**

验收标准：
- ✅ 可以创建新文件夹
- ✅ 可以重命名文件/文件夹
- ✅ 可以删除文件/文件夹
- ✅ Agent 正确执行操作（依赖 Linux 权限）
- ✅ 操作失败时显示友好的错误消息
- ✅ 操作成功后自动刷新目录列表

---

## Task 14: 文档更新

**Files:**
- Modify: `docs/superpowers/plans/2026-06-15-remote-file-manager-implementation.md`

- [ ] **Step 1: 更新实施计划文档**

添加阶段3完成记录：
```markdown
## 阶段 3：文件操作功能（已完成）

**完成时间：** 2026-06-15

**实现内容：**
- Agent 添加 5 个文件操作 API（mkdir、rename、delete、copy、move）
- FileManager 实现文件操作 UI（右键菜单）
- 所有操作依赖 Linux 文件系统权限
- 操作失败时显示友好错误消息

**验收标准：**
- ✅ 可以创建新文件夹
- ✅ 可以重命名文件/文件夹
- ✅ 可以删除文件/文件夹
- ✅ Agent 正确执行操作
- ✅ 操作失败时显示友好错误消息
```

- [ ] **Step 2: Commit**

```bash
git add docs/superpowers/plans/2026-06-15-remote-file-manager-implementation.md
git commit -m "docs: update phase 3 completion status"
```

---

## 执行选项

计划已完成并保存到 `docs/superpowers/plans/2026-06-15-file-operations-phase3.md`。

**两种执行方式：**

**1. Subagent-Driven (推荐)** - 我为每个任务派发新的子代理，任务间进行审查，快速迭代

**2. Inline Execution** - 在此会话中使用 executing-plans 执行，批量执行并在检查点审查

**选择哪种方式？**