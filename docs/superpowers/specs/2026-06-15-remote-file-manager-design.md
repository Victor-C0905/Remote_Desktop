# GNOME 远程控制客户端 - 远程文件管理器设计文档

> 版本: v1.0 | 日期: 2026-06-15
>
> 目标: 实现完整的远程文件管理器，对齐 GNOME Files (Nautilus) 的用户体验

---

## 一、需求概述

### 1.1 核心需求

- **实时浏览**：每次进入目录都获取最新状态
- **GNOME 对齐**：初始目录、侧边栏结构、导航功能完全对齐 GNOME Files
- **完整操作**：支持新建文件夹、重命名、删除、复制/移动文件
- **高级功能**：搜索、分页加载、符号链接处理、友好错误处理

### 1.2 用户选择

通过 brainstorming 流程确认的需求：

| 需求项 | 用户选择 | 说明 |
|--------|---------|------|
| 初始目录 | A - 完全对齐 GNOME | 显示 `/home/{username}`，需要从 Agent 获取用户名 |
| 侧边栏结构 | A - 完全对齐 GNOME | 包含位置、设备、其他位置，需要 Agent 提供挂载点 |
| 导航功能 | C - 完整导航 | 前进/后退、面包屑、上级目录、视图切换、搜索 |
| 文件操作 | C - 完整操作 | 新建文件夹、重命名、删除、复制/移动 |
| 错误处理 | D - 完整优化 | 友好错误、分页加载、符号链接处理 |

---

## 二、整体架构设计

### 2.1 核心组件

```
FileManager (React 组件)
    │
    ├── HeaderBar
    │   ├── 导航按钮（前进/后退/上级）
    │   ├── 面包屑路径
    │   ├── 视图切换（列表/图标）
    │   └── 搜索按钮
    │
    ├── Sidebar
    │   ├── 位置（Home、Documents、Downloads 等）
    │   ├── 设备（挂载点列表）
    │   └── 其他位置（网络位置）
    │
    ├── MainArea
    │   ├── 文件列表（列表视图）
    │   ├── 文件网格（图标视图）
    │   ├── 加载状态
    │   └── 错误提示
    │
    └── StatusBar
        └── 文件统计信息

Agent (Rust 后端)
    │
    ├── API 端点
    │   ├── get_current_user() → 返回当前用户名
    │   ├── read_dir(path) → 返回目录内容
    │   ├── get_mounts() → 返回挂载点列表
    │   ├── search_files(query) → 搜索文件
    │   ├── mkdir(path) → 创建目录
    │   ├── rename(old, new) → 重命名
    │   ├── delete(path) → 删除文件/目录
    │   ├── copy(src, dst) → 复制文件
    │   └── move(src, dst) → 移动文件
    │
    └── 权限控制
        └── allowed_paths 白名单检查
```

### 2.2 数据流

```
用户操作 → FileManager → invoke("remote_*") → ConnectionManager → QUIC Stream → Agent
                ↓
            更新状态
                ↓
            重新渲染
```

### 2.3 关键设计点

1. **初始目录获取**：FileManager 打开时，调用 `invoke("remote_get_current_user")` 获取用户名，然后设置初始路径为 `/home/{username}`

2. **连接状态监听**：FileManager 监听 `activeServerId` 变化，当连接建立时自动加载远程目录

3. **实时状态**：每次进入新目录时，调用 `remote_read_dir` 获取最新状态

4. **错误处理**：所有错误通过统一的错误处理机制显示，并提供重试选项

---

## 三、渐进式实现方案

### 3.1 阶段划分

采用渐进式实现，分为 4 个阶段：

| 阶段 | 目标 | 核心功能 | 预计工作量 |
|------|------|---------|-----------|
| 阶段 1 | 基础浏览 | 修复 bug + 基础导航 + 只读浏览 | 1-2 天 |
| 阶段 2 | 侧边栏和视图 | 完整侧边栏 + 视图切换 | 1-2 天 |
| 阶段 3 | 文件操作 | 新建/重命名/删除/复制/移动 | 2-3 天 |
| 阶段 4 | 高级功能 | 搜索/分页/符号链接/错误处理 | 2-3 天 |

---

## 四、阶段 1 详细设计：基础浏览功能

### 4.1 目标

修复当前 bug + 实现基础浏览功能，让 FileManager 可以正常工作。

### 4.2 具体任务

#### 4.2.1 修复 `activeServerId` 监听问题

**当前问题：** FileManager 的 `useEffect` 缺少对 `activeServerId` 的依赖，导致连接远程服务器后不会自动刷新目录。

**修复方案：**

```typescript
// FileManager.tsx
useEffect(() => {
  loadDir(HOME_PATH);
}, [loadDir, HOME_PATH, activeServerId]);  // 添加 activeServerId
```

#### 4.2.2 Agent 添加 `get_current_user` API

**Agent 协议：**

```rust
// agent/src/protocol.rs
enum Payload {
    GetCurrentUser,
    CurrentUserResponse { username: String },
}
```

**Agent 实现：**

```rust
// agent/src/handler.rs
fn handle_get_current_user() -> Payload {
    let username = whoami::username();  // 使用 whoami crate
    Payload::CurrentUserResponse { username }
}
```

**客户端 API：**

```rust
// src-tauri/src/connection.rs
#[tauri::command]
pub async fn remote_get_current_user(server_id: String, app: tauri::AppHandle) -> Result<String, String> {
    let resp = remote_send(server_id, Payload::GetCurrentUser, app).await?;
    match resp.payload {
        Payload::CurrentUserResponse { username } => Ok(username),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}
```

#### 4.2.3 FileManager 初始化流程

```typescript
// FileManager.tsx
const [username, setUsername] = useState<string | null>(null);

useEffect(() => {
  if (activeServerId) {
    // 连接建立后，获取用户名
    invoke<string>("remote_get_current_user", { serverId: activeServerId })
      .then(setUsername)
      .catch(err => {
        console.error("获取用户名失败:", err);
        setUsername("user");  // 默认值
      });
  }
}, [activeServerId]);

useEffect(() => {
  if (activeServerId && username) {
    // 有用户名后，加载初始目录
    const homePath = `/home/${username}`;
    loadDir(homePath);
  } else if (!activeServerId) {
    // 本地模式，使用默认路径
    loadDir(HOME_PATH);
  }
}, [activeServerId, username, loadDir, HOME_PATH]);
```

#### 4.2.4 基础导航功能

- **前进/后退**：使用历史数组管理
- **面包屑**：显示当前路径的层级结构
- **上级目录**：计算父目录路径

#### 4.2.5 文件属性查看

右键菜单添加"属性"选项，显示：
- 文件名
- 类型（文件/目录）
- 大小
- 修改时间
- 权限

### 4.3 验收标准

- ✅ 连接远程服务器后，FileManager 自动显示 `/home/{username}` 目录
- ✅ 可以通过导航按钮浏览目录
- ✅ 可以查看文件属性
- ✅ 断开连接后，FileManager 显示本地文件系统

---

## 五、阶段 2 详细设计：侧边栏和视图切换

### 5.1 目标

实现完整侧边栏和视图切换功能，对齐 GNOME Files 的用户体验。

### 5.2 具体任务

#### 5.2.1 Agent 添加 `get_mounts` API

**Agent 实现：**

```rust
// agent/src/handler.rs
#[derive(Serialize)]
pub struct MountInfo {
    pub mount_point: String,
    pub device: String,      // 设备名称（如 /dev/sda1）
    pub filesystem: String,  // 文件系统类型（如 ext4）
    pub total_bytes: u64,
    pub used_bytes: u64,
}

fn handle_get_mounts() -> Payload {
    let mounts = Disks::new_with_refreshed_list()
        .iter()
        .map(|disk| MountInfo {
            mount_point: disk.mount_point().to_string_lossy().to_string(),
            device: disk.name().to_string_lossy().to_string(),
            filesystem: disk.file_system().to_string_lossy().to_string(),
            total_bytes: disk.total_space(),
            used_bytes: disk.total_space() - disk.available_space(),
        })
        .collect();

    Payload::MountsResponse { mounts }
}
```

#### 5.2.2 侧边栏结构设计

```typescript
// FileManager.tsx
interface SidebarSection {
  title: string;
  items: SidebarItem[];
}

interface SidebarItem {
  icon: string;
  label: string;
  path: string;
  type: 'bookmark' | 'mount' | 'network';
}

// 动态生成侧边栏
const [sidebarSections, setSidebarSections] = useState<SidebarSection[]>([]);

useEffect(() => {
  if (activeServerId && username) {
    // 远程模式：生成远程侧边栏
    generateRemoteSidebar();
  } else {
    // 本地模式：使用本地侧边栏
    setSidebarSections(LOCAL_SIDEBAR);
  }
}, [activeServerId, username]);

async function generateRemoteSidebar() {
  // 获取挂载点
  const mounts = await invoke<MountInfo[]>("remote_get_mounts", { serverId: activeServerId });

  const sections: SidebarSection[] = [
    {
      title: "位置",
      items: [
        { icon: "🏠", label: "主目录", path: `/home/${username}`, type: "bookmark" },
        { icon: "📄", label: "文档", path: `/home/${username}/Documents`, type: "bookmark" },
        { icon: "⬇️", label: "下载", path: `/home/${username}/Downloads`, type: "bookmark" },
        { icon: "🖼️", label: "图片", path: `/home/${username}/Pictures`, type: "bookmark" },
        { icon: "🎵", label: "音乐", path: `/home/${username}/Music`, type: "bookmark" },
        { icon: "🎬", label: "视频", path: `/home/${username}/Videos`, type: "bookmark" },
      ]
    },
    {
      title: "设备",
      items: mounts.map(m => ({
        icon: "💾",
        label: m.mount_point,
        path: m.mount_point,
        type: "mount"
      }))
    },
    {
      title: "其他位置",
      items: [
        { icon: "🌐", label: "网络", path: "/network", type: "network" }
      ]
    }
  ];

  setSidebarSections(sections);
}
```

#### 5.2.3 视图切换功能

```typescript
// FileManager.tsx
type ViewMode = "list" | "grid";

const [viewMode, setViewMode] = useState<ViewMode>("list");

// HeaderBar 中添加视图切换按钮
<div className="view-toggle">
  <button
    className={viewMode === "list" ? "active" : ""}
    onClick={() => setViewMode("list")}
    title="列表视图"
  >☰</button>
  <button
    className={viewMode === "grid" ? "active" : ""}
    onClick={() => setViewMode("grid")}
    title="图标视图"
  >⊞</button>
</div>

// 根据视图模式渲染不同的内容
{viewMode === "list" ? (
  <FileListView entries={entries} />
) : (
  <FileGridView entries={entries} />
)}
```

#### 5.2.4 GNOME 风格的侧边栏样式

```css
/* FileManager.css */
.fm-sidebar {
  width: 220px;
  background: var(--sidebar-bg);
  border-right: 1px solid var(--sidebar-border);
  overflow-y: auto;
}

.sidebar-section {
  margin-bottom: 12px;
}

.sidebar-section-title {
  padding: 8px 12px;
  font-size: 9pt;
  color: var(--text-secondary);
  font-weight: 600;
}

.sidebar-item {
  display: flex;
  align-items: center;
  padding: 6px 12px;
  gap: 8px;
  cursor: pointer;
  border-radius: 6px;
  margin: 2px 6px;

  &:hover {
    background: var(--card-bg);
  }

  &.active {
    background: var(--accent-bg);
    color: var(--accent-fg);
  }
}

.si-icon {
  font-size: 16px;
}

.si-label {
  font-size: 10pt;
}
```

### 5.3 验收标准

- ✅ 侧边栏显示完整的位置、设备、其他位置
- ✅ 设备部分显示远程服务器的挂载点
- ✅ 点击侧边栏项可以跳转到对应目录
- ✅ 可以切换列表视图和图标视图
- ✅ 侧边栏样式符合 GNOME HIG

---

## 六、阶段 3 详细设计：文件操作功能

### 6.1 目标

实现文件系统修改功能，包括新建文件夹、重命名、删除、复制/移动。

### 6.2 具体任务

#### 6.2.1 Agent 添加文件操作 API

```rust
// agent/src/handler.rs

// 创建目录
fn handle_mkdir(path: String) -> Payload {
    // 检查权限白名单
    if !check_allowed_paths(&path) {
        return Payload::Error { message: "路径不在允许访问范围内".into(), code: 403 };
    }

    match fs::create_dir_all(&path) {
        Ok(_) => Payload::MkdirResponse { success: true, path },
        Err(e) => Payload::Error { message: e.to_string(), code: 500 },
    }
}

// 重命名
fn handle_rename(old_path: String, new_path: String) -> Payload {
    if !check_allowed_paths(&old_path) || !check_allowed_paths(&new_path) {
        return Payload::Error { message: "路径不在允许访问范围内".into(), code: 403 };
    }

    match fs::rename(&old_path, &new_path) {
        Ok(_) => Payload::RenameResponse { success: true, old_path, new_path },
        Err(e) => Payload::Error { message: e.to_string(), code: 500 },
    }
}

// 删除
fn handle_delete(path: String) -> Payload {
    if !check_allowed_paths(&path) {
        return Payload::Error { message: "路径不在允许访问范围内".into(), code: 403 };
    }

    // 检查是否是目录
    let metadata = fs::metadata(&path)?;
    if metadata.is_dir() {
        fs::remove_dir_all(&path)?;
    } else {
        fs::remove_file(&path)?;
    }

    Payload::DeleteResponse { success: true, path }
}

// 复制文件
fn handle_copy(src: String, dst: String) -> Payload {
    if !check_allowed_paths(&src) || !check_allowed_paths(&dst) {
        return Payload::Error { message: "路径不在允许访问范围内".into(), code: 403 };
    }

    fs::copy(&src, &dst)?;
    Payload::CopyResponse { success: true, src, dst }
}

// 移动文件
fn handle_move(src: String, dst: String) -> Payload {
    handle_rename(src, dst)  // 移动本质上是重命名
}
```

#### 6.2.2 FileManager 实现文件操作 UI

```typescript
// FileManager.tsx

// 右键菜单操作
const handleMkdir = async () => {
  const name = prompt("新建文件夹名称:");
  if (!name) return;

  const newPath = `${currentPath}/${name}`;
  try {
    await invoke("remote_mkdir", { serverId: activeServerId, path: newPath });
    loadDir(currentPath);  // 刷新当前目录
  } catch (err) {
    showError(`创建文件夹失败: ${err}`);
  }
};

const handleRename = async (entry: FileEntry) => {
  const newName = prompt("新名称:", entry.name);
  if (!newName || newName === entry.name) return;

  const oldPath = `${currentPath}/${entry.name}`;
  const newPath = `${currentPath}/${newName}`;

  try {
    await invoke("remote_rename", {
      serverId: activeServerId,
      oldPath,
      newPath
    });
    loadDir(currentPath);
  } catch (err) {
    showError(`重命名失败: ${err}`);
  }
};

const handleDelete = async (entry: FileEntry) => {
  const confirmed = confirm(`确定删除 "${entry.name}"?`);
  if (!confirmed) return;

  const path = `${currentPath}/${entry.name}`;
  try {
    await invoke("remote_delete", { serverId: activeServerId, path });
    loadDir(currentPath);
  } catch (err) {
    showError(`删除失败: ${err}`);
  }
};
```

#### 6.2.3 权限控制和安全检查

**Agent 配置：**

```toml
# agent.toml
[security]
allowed_paths = ["/home", "/var/log", "/opt", "/tmp"]
blocked_commands = ["rm -rf /", "dd if=", "mkfs."]
```

**Agent 实现：**

```rust
// agent/src/security.rs
pub fn check_allowed_paths(path: &str) -> bool {
    let allowed = config.security.allowed_paths;
    allowed.iter().any(|prefix| path.starts_with(prefix))
}

pub fn check_blocked_operations(op: &FileOperation) -> bool {
    // 检查是否尝试删除系统关键路径
    if op.path.starts_with("/etc/systemd") ||
       op.path.starts_with("/usr/lib") {
        return false;
    }
    true
}
```

#### 6.2.4 右键菜单更新

```typescript
// FileManager.tsx 右键菜单
<div className="fm-context-menu">
  <div className="ctx-item" onClick={() => handleOpen(entry)}>
    <span className="ctx-icon">📂</span> 打开
  </div>
  <div className="ctx-separator" />

  {/* 新增操作 */}
  <div className="ctx-item" onClick={handleMkdir}>
    <span className="ctx-icon">📁</span> 新建文件夹
  </div>
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

### 6.3 验收标准

- ✅ 可以创建新文件夹
- ✅ 可以重命名文件/文件夹
- ✅ 可以删除文件/文件夹
- ✅ Agent 正确执行权限检查（只允许白名单路径）
- ✅ 操作失败时显示友好的错误消息

---

## 七、阶段 4 详细设计：高级功能

### 7.1 目标

实现搜索、分页加载、符号链接处理、错误处理等高级功能。

### 7.2 具体任务

#### 7.2.1 搜索功能

**Agent 添加搜索 API：**

```rust
// agent/src/handler.rs
#[derive(Serialize)]
pub struct SearchResult {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub mtime: String,
}

fn handle_search(query: String, start_path: String) -> Payload {
    let results = walkdir::WalkDir::new(&start_path)
        .into_iter()
        .filter_entry(|e| check_allowed_paths(e.path().to_string_lossy()))
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().contains(&query))
        .take(100)  // 限制结果数量
        .map(|e| SearchResult {
            path: e.path().to_string_lossy().to_string(),
            name: e.file_name().to_string_lossy().to_string(),
            is_dir: e.file_type().is_dir(),
            size: e.metadata().map(|m| m.len()).unwrap_or(0),
            mtime: e.metadata()
                .and_then(|m| m.modified())
                .map(|t| format_iso8601(t))
                .unwrap_or_default(),
        })
        .collect();

    Payload::SearchResponse { query, results }
}
```

**FileManager 搜索 UI：**

```typescript
// FileManager.tsx
const [searchQuery, setSearchQuery] = useState<string>("");
const [searchResults, setSearchResults] = useState<SearchResult[]>([]);
const [isSearching, setIsSearching] = useState(false);

const handleSearch = async () => {
  if (!searchQuery.trim()) return;

  setIsSearching(true);
  try {
    const results = await invoke<SearchResult[]>("remote_search", {
      serverId: activeServerId,
      query: searchQuery,
      startPath: currentPath
    });
    setSearchResults(results);
  } catch (err) {
    showError(`搜索失败: ${err}`);
  } finally {
    setIsSearching(false);
  }
};

// HeaderBar 搜索栏
<div className="search-bar">
  <input
    type="text"
    placeholder="搜索..."
    value={searchQuery}
    onChange={(e) => setSearchQuery(e.target.value)}
    onKeyPress={(e) => e.key === "Enter" && handleSearch()}
  />
  <button onClick={handleSearch}>🔍</button>
</div>
```

#### 7.2.2 分页加载（大目录优化）

**Agent 支持分页：**

```rust
// agent/src/handler.rs
#[derive(Serialize)]
pub struct ReadDirPaginatedResponse {
    pub path: String,
    pub entries: Vec<FileEntry>,
    pub total: usize,
    pub offset: usize,
    pub has_more: bool,
}

fn handle_read_dir_paginated(path: String, offset: usize, limit: usize) -> Payload {
    let mut entries = fs::read_dir(&path)?
        .filter_map(|e| e.ok())
        .skip(offset)
        .take(limit)
        .map(|e| file_entry_from_dir_entry(&e))
        .collect::<Vec<_>>();

    let total = fs::read_dir(&path)?.count();
    let has_more = offset + limit < total;

    Payload::ReadDirPaginatedResponse {
        path,
        entries,
        total,
        offset,
        has_more,
    }
}
```

**FileManager 实现无限滚动：**

```typescript
// FileManager.tsx
const [allEntries, setAllEntries] = useState<FileEntry[]>([]);
const [hasMore, setHasMore] = useState(true);
const [loadingMore, setLoadingMore] = useState(false);
const PAGE_SIZE = 100;

const loadMore = async () => {
  if (!hasMore || loadingMore) return;

  setLoadingMore(true);
  const offset = allEntries.length;

  try {
    const resp = await invoke<ReadDirPaginatedResponse>("remote_read_dir_paginated", {
      serverId: activeServerId,
      path: currentPath,
      offset,
      limit: PAGE_SIZE
    });

    setAllEntries(prev => [...prev, ...resp.entries]);
    setHasMore(resp.has_more);
  } catch (err) {
    showError(`加载更多失败: ${err}`);
  } finally {
    setLoadingMore(false);
  }
};

// 监听滚动事件
useEffect(() => {
  const handleScroll = (e: Event) => {
    const target = e.target as HTMLDivElement;
    if (target.scrollHeight - target.scrollTop <= target.clientHeight + 100) {
      loadMore();
    }
  };

  mainRef.current?.addEventListener("scroll", handleScroll);
  return () => mainRef.current?.removeEventListener("scroll", handleScroll);
}, [hasMore, loadingMore]);
```

#### 7.2.3 符号链接处理

**Agent 返回链接信息：**

```rust
// agent/src/handler.rs
#[derive(Serialize)]
pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub mtime: String,
    pub permissions: String,
    pub is_symlink: bool,       // 新增
    pub symlink_target: Option<String>,  // 新增
}

fn file_entry_from_path(path: &Path) -> FileEntry {
    let metadata = fs::metadata(path)?;
    let symlink_metadata = fs::symlink_metadata(path);

    let is_symlink = symlink_metadata
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false);

    let symlink_target = if is_symlink {
        fs::read_link(path).ok().map(|p| p.to_string_lossy().to_string())
    } else {
        None
    };

    FileEntry {
        name: path.file_name().unwrap().to_string_lossy().to_string(),
        is_dir: metadata.is_dir(),
        size: metadata.len(),
        mtime: format_iso8601(metadata.modified()?),
        permissions: get_permissions(&metadata),
        is_symlink,
        symlink_target,
    }
}
```

**FileManager 显示链接：**

```typescript
// FileManager.tsx
const getFileIcon = (entry: FileEntry): string => {
  if (entry.is_symlink) return "🔗";  // 链接图标
  if (entry.is_dir) return "📁";
  // ... 其他图标
};

// 文件列表中显示链接目标
<div className="file-name">
  <span className="fn-icon">{getFileIcon(entry)}</span>
  <span className="fn-text">{entry.name}</span>
  {entry.is_symlink && entry.symlink_target && (
    <span className="symlink-target">→ {entry.symlink_target}</span>
  )}
</div>

// 点击链接时跳转到目标
const handleOpen = (entry: FileEntry) => {
  if (entry.is_symlink && entry.symlink_target) {
    // 跳转到链接目标
    navigateTo(entry.symlink_target);
  } else if (entry.is_dir) {
    navigateTo(`${currentPath}/${entry.name}`);
  }
};
```

#### 7.2.4 错误处理和重试机制

```typescript
// FileManager.tsx
interface ErrorInfo {
  type: "permission" | "connection" | "not_found" | "unknown";
  message: string;
  path?: string;
  retryable: boolean;
}

const showError = (error: ErrorInfo) => {
  // 显示友好的错误消息
  const errorDiv = document.createElement("div");
  errorDiv.className = "fm-error-overlay";

  let icon = "⚠️";
  let title = "错误";

  switch (error.type) {
    case "permission":
      icon = "🔒";
      title = "权限不足";
      break;
    case "connection":
      icon = "🔌";
      title = "连接断开";
      break;
    case "not_found":
      icon = "📂";
      title = "目录不存在";
      break;
  }

  errorDiv.innerHTML = `
    <div class="error-icon">${icon}</div>
    <div class="error-title">${title}</div>
    <div class="error-message">${error.message}</div>
    ${error.retryable ? '<button onclick="retry()">重试</button>' : ''}
  `;

  document.body.appendChild(errorDiv);
};

// 连接状态监听
useEffect(() => {
  const unsubscribe = listen("connection-lost", (event) => {
    showError({
      type: "connection",
      message: "与服务器的连接已断开",
      retryable: true
    });
  });

  return unsubscribe;
}, []);
```

### 7.3 验收标准

- ✅ 可以搜索文件（输入关键词后显示匹配结果）
- ✅ 大目录（>100 文件）支持分页加载，滚动时自动加载更多
- ✅ 符号链接显示特殊图标和目标路径，点击跳转到目标
- ✅ 权限不足时显示友好提示，而不是崩溃
- ✅ 连接断开时显示提示，并提供重连选项
- ✅ 所有错误都有明确的错误类型和友好提示

---

## 八、技术栈和依赖

### 8.1 新增依赖

| 依赖 | 用途 | 版本 |
|------|------|------|
| `whoami` | Agent 获取当前用户名 | 1.4.0 |
| `walkdir` | Agent 搜索文件 | 2.3.3 |
| `sysinfo` | Agent 获取挂载点信息 | 已有 |

### 8.2 协议扩展

需要在 Agent 协议中新增以下 Payload 类型：

```rust
// agent/src/protocol.rs
enum Payload {
    // 已有类型...

    // 新增类型
    GetCurrentUser,
    CurrentUserResponse { username: String },
    GetMounts,
    MountsResponse { mounts: Vec<MountInfo> },
    SearchRequest { query: String, start_path: String },
    SearchResponse { query: String, results: Vec<SearchResult> },
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
    ReadDirPaginatedRequest { path: String, offset: usize, limit: usize },
    ReadDirPaginatedResponse { path: String, entries: Vec<FileEntry>, total: usize, offset: usize, has_more: bool },
}
```

---

## 九、风险和注意事项

### 9.1 安全风险

- **文件删除**：需要严格的权限检查，防止删除系统关键文件
- **路径遍历**：防止恶意路径（如 `../../../etc/passwd`）
- **符号链接**：防止链接指向敏感路径

### 9.2 性能风险

- **大目录加载**：需要分页加载，避免一次性加载数千文件
- **搜索性能**：需要限制搜索结果数量，避免搜索整个文件系统
- **实时状态**：每次进入目录都重新获取，可能增加网络负载

### 9.3 兼容性风险

- **Windows 路径**：需要正确处理 Windows 路径分隔符
- **符号链接**：不同系统的符号链接行为可能不同
- **挂载点**：不同系统的挂载点命名可能不同

---

## 十、总结

本设计文档详细描述了远程文件管理器的完整实现方案，采用渐进式开发策略，分为 4 个阶段：

1. **阶段 1**：修复基础 bug，实现基础浏览功能
2. **阶段 2**：实现完整侧边栏和视图切换
3. **阶段 3**：实现文件操作功能
4. **阶段 4**：实现高级功能（搜索、分页、符号链接、错误处理）

每个阶段都有明确的目标、具体任务和验收标准，确保开发过程可控、可验证。

通过本设计，FileManager 将完全对齐 GNOME Files 的用户体验，提供完整的远程文件管理功能。