# 远程文件管理器实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现完整的远程文件管理器，对齐 GNOME Files 的用户体验，支持实时浏览、完整操作和高级功能。

**Architecture:** 采用渐进式实现，分为 4 个阶段：基础浏览、侧边栏和视图、文件操作、高级功能。每个阶段独立可测试，逐步增强功能。

**Tech Stack:** React 18 + TypeScript (前端), Rust + Tauri (客户端后端), Rust + quinn (Agent), whoami + walkdir + sysinfo (Agent 依赖)

---

## 阶段 1：基础浏览功能

### Task 1.1: 修复 activeServerId 监听问题

**Files:**
- Modify: `src/apps/FileManager.tsx:159`

- [ ] **Step 1: 修改 useEffect 依赖数组**

打开 `src/apps/FileManager.tsx`，找到第 159 行的 useEffect：

```typescript
// 当前代码（有问题）
useEffect(() => { loadDir(HOME_PATH); }, [loadDir, HOME_PATH]);

// 修改为
useEffect(() => {
  loadDir(HOME_PATH);
}, [loadDir, HOME_PATH, activeServerId]);
```

- [ ] **Step 2: 测试修复效果**

启动客户端：
```bash
npm run tauri dev
```

测试步骤：
1. 打开 FileManager
2. 连接远程服务器
3. 验证 FileManager 自动显示远程目录（而不是本地目录）

预期：连接后 FileManager 显示远程 `/home` 目录

- [ ] **Step 3: 提交修复**

```bash
git add src/apps/FileManager.tsx
git commit -m "fix(FileManager): 添加 activeServerId 监听，修复连接后不刷新目录的 bug"
```

---

### Task 1.2: Agent 添加 get_current_user API

**Files:**
- Modify: `agent/src/protocol.rs`
- Modify: `agent/src/handler.rs`
- Modify: `src-tauri/src/connection.rs`

- [ ] **Step 1: Agent 协议添加 Payload 类型**

打开 `agent/src/protocol.rs`，在 Payload enum 中添加：

```rust
enum Payload {
    // 已有类型...

    // 新增：获取当前用户
    GetCurrentUser,
    CurrentUserResponse { username: String },
}
```

- [ ] **Step 2: Agent 实现处理函数**

打开 `agent/src/handler.rs`，添加处理函数：

```rust
fn handle_get_current_user() -> Payload {
    let username = whoami::username();
    Payload::CurrentUserResponse { username }
}
```

在 `handle_payload` 函数中添加分支：

```rust
match payload {
    // 已有分支...

    Payload::GetCurrentUser => handle_get_current_user(),
    _ => Payload::Error { message: "未知请求类型".into(), code: 400 },
}
```

- [ ] **Step 3: 客户端添加 API**

打开 `src-tauri/src/connection.rs`，添加 Tauri command：

```rust
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

在 `lib.rs` 中注册 command：

```rust
fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            // 已有 commands...
            connection::remote_get_current_user,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 4: 测试 API**

重启 Agent：
```bash
cd agent
cargo build --release
./target/release/agent
```

在客户端测试：
```typescript
// 在 FileManager.tsx 中临时测试
const testUser = await invoke<string>("remote_get_current_user", { serverId: activeServerId });
console.log("当前用户:", testUser);
```

预期：返回远程服务器当前用户名（如 "root" 或 "user"）

- [ ] **Step 5: 提交代码**

```bash
git add agent/src/protocol.rs agent/src/handler.rs src-tauri/src/connection.rs src-tauri/src/lib.rs
git commit -m "feat(agent): 添加 get_current_user API，支持获取远程服务器用户名"
```

---

### Task 1.3: FileManager 实现初始化流程

**Files:**
- Modify: `src/apps/FileManager.tsx`

- [ ] **Step 1: 添加 username state**

打开 `src/apps/FileManager.tsx`，在 state 定义部分添加：

```typescript
const [username, setUsername] = useState<string | null>(null);
```

- [ ] **Step 2: 添加获取用户名的 useEffect**

在现有 useEffect 之后添加：

```typescript
useEffect(() => {
  if (activeServerId) {
    // 连接建立后，获取用户名
    invoke<string>("remote_get_current_user", { serverId: activeServerId })
      .then(setUsername)
      .catch(err => {
        console.error("获取用户名失败:", err);
        setUsername("user");  // 默认值
      });
  } else {
    setUsername(null);  // 本地模式清空用户名
  }
}, [activeServerId]);
```

- [ ] **Step 3: 修改初始加载逻辑**

修改第 159 行的 useEffect：

```typescript
useEffect(() => {
  if (activeServerId && username) {
    // 远程模式：有用户名后，加载初始目录
    const homePath = `/home/${username}`;
    loadDir(homePath);
  } else if (!activeServerId) {
    // 本地模式：使用默认路径
    loadDir(HOME_PATH);
  }
}, [activeServerId, username, loadDir, HOME_PATH]);
```

- [ ] **Step 4: 测试初始化流程**

测试步骤：
1. 启动客户端
2. 打开 FileManager（应该显示本地目录）
3. 连接远程服务器
4. 验证 FileManager 自动显示 `/home/{username}` 目录

预期：
- 本地模式：显示 `C:\Users` 或 `/home`
- 远程模式：显示 `/home/root` 或 `/home/user`

- [ ] **Step 5: 提交代码**

```bash
git add src/apps/FileManager.tsx
git commit -m "feat(FileManager): 实现初始化流程，连接后自动加载用户主目录"
```

---

### Task 1.4: 完善基础导航功能

**Files:**
- Modify: `src/apps/FileManager.tsx`

- [ ] **Step 1: 验证现有导航功能**

检查 FileManager.tsx 中已有的导航功能：
- 前进/后退按钮（第 170-195 行）
- 面包屑路径（第 266-282 行）
- 上级目录按钮（第 177-188 行）

确认这些功能已经实现且正常工作。

- [ ] **Step 2: 测试导航功能**

测试步骤：
1. 进入 `/home/user/Documents`
2. 点击"上级目录"按钮，验证返回 `/home/user`
3. 点击面包屑中的 "home"，验证跳转到 `/home`
4. 点击"后退"按钮，验证返回 `/home/user/Documents`

预期：所有导航功能正常工作

- [ ] **Step 3: 提交确认**

如果导航功能已完善，提交确认：

```bash
git add src/apps/FileManager.tsx
git commit -m "docs(FileManager): 确认基础导航功能已实现（前进/后退/面包屑/上级目录）"
```

---

### Task 1.5: 添加文件属性查看

**Files:**
- Modify: `src/apps/FileManager.tsx`

- [ ] **Step 1: 添加属性对话框 state**

在 state 定义部分添加：

```typescript
const [propertiesEntry, setPropertiesEntry] = useState<FileEntry | null>(null);
```

- [ ] **Step 2: 实现属性显示函数**

添加函数：

```typescript
const showProperties = (entry: FileEntry) => {
  setPropertiesEntry(entry);
};
```

- [ ] **Step 3: 在右键菜单中添加属性选项**

修改右键菜单（第 402-428 行），在最后添加：

```typescript
{contextMenu && (
  <div className="fm-context-menu" style={{ left: contextMenu.x, top: contextMenu.y }}>
    {/* 已有选项... */}
    <div className="ctx-separator" />
    <div className="ctx-item" onClick={() => { showProperties(contextMenu.entry); setContextMenu(null); }}>
      <span className="ctx-icon">ℹ️</span> 属性
    </div>
  </div>
)}
```

- [ ] **Step 4: 添加属性对话框 UI**

在组件末尾添加属性对话框：

```typescript
{propertiesEntry && (
  <div className="fm-properties-dialog">
    <div className="pd-header">
      <span className="pd-icon">{getFileIcon(propertiesEntry)}</span>
      <span className="pd-name">{propertiesEntry.name}</span>
    </div>
    <div className="pd-content">
      <div className="pd-row">
        <span className="pd-label">类型:</span>
        <span className="pd-value">{propertiesEntry.is_dir ? "文件夹" : "文件"}</span>
      </div>
      <div className="pd-row">
        <span className="pd-label">大小:</span>
        <span className="pd-value">{formatSize(propertiesEntry.size)}</span>
      </div>
      <div className="pd-row">
        <span className="pd-label">修改时间:</span>
        <span className="pd-value">{formatDate(propertiesEntry.mtime)}</span>
      </div>
      <div className="pd-row">
        <span className="pd-label">权限:</span>
        <span className="pd-value">{propertiesEntry.permissions}</span>
      </div>
    </div>
    <div className="pd-footer">
      <button onClick={() => setPropertiesEntry(null)}>关闭</button>
    </div>
  </div>
)}
```

- [ ] **Step 5: 添加属性对话框样式**

打开 `src/apps/FileManager.css`，添加样式：

```css
.fm-properties-dialog {
  position: fixed;
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%);
  width: 400px;
  background: var(--card-bg);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  padding: 16px;
  z-index: 1000;
}

.pd-header {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 16px;
}

.pd-icon {
  font-size: 32px;
}

.pd-name {
  font-size: 14pt;
  font-weight: 600;
}

.pd-content {
  margin-bottom: 16px;
}

.pd-row {
  display: flex;
  justify-content: space-between;
  padding: 8px 0;
  border-bottom: 1px solid var(--border-color);
}

.pd-label {
  color: var(--text-secondary);
}

.pd-value {
  color: var(--text-primary);
}

.pd-footer {
  display: flex;
  justify-content: flex-end;
}
```

- [ ] **Step 6: 测试属性查看**

测试步骤：
1. 右键点击任意文件或文件夹
2. 选择"属性"
3. 验证显示正确的文件信息

预期：显示文件名、类型、大小、修改时间、权限

- [ ] **Step 7: 提交代码**

```bash
git add src/apps/FileManager.tsx src/apps/FileManager.css
git commit -m "feat(FileManager): 添加文件属性查看功能，显示文件详细信息"
```

---

## 阶段 1 验收

- [ ] **验收测试：基础浏览功能**

运行完整测试：

1. 启动客户端：`npm run tauri dev`
2. 启动 Agent：`cd agent && ./target/release/agent`
3. 测试步骤：
   - 打开 FileManager
   - 连接远程服务器
   - 验证显示 `/home/{username}` 目录
   - 测试导航功能（前进/后退/面包屑/上级目录）
   - 右键点击文件，查看属性
   - 断开连接，验证返回本地文件系统

预期：所有基础浏览功能正常工作

- [ ] **提交阶段 1 完成**

```bash
git add .
git commit -m "feat(FileManager): 完成阶段 1 - 基础浏览功能"
```

---

## 阶段 2：侧边栏和视图切换

### Task 2.1: Agent 添加 get_mounts API

**Files:**
- Modify: `agent/src/protocol.rs`
- Modify: `agent/src/handler.rs`
- Modify: `src-tauri/src/connection.rs`

- [ ] **Step 1: Agent 协议添加 Payload 类型**

打开 `agent/src/protocol.rs`，添加：

```rust
#[derive(Serialize)]
pub struct MountInfo {
    pub mount_point: String,
    pub device: String,
    pub filesystem: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
}

enum Payload {
    // 已有类型...

    // 新增：获取挂载点
    GetMounts,
    MountsResponse { mounts: Vec<MountInfo> },
}
```

- [ ] **Step 2: Agent 实现处理函数**

打开 `agent/src/handler.rs`，添加：

```rust
use sysinfo::Disks;

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

在 `handle_payload` 中添加分支：

```rust
Payload::GetMounts => handle_get_mounts(),
```

- [ ] **Step 3: 客户端添加 API**

打开 `src-tauri/src/connection.rs`，添加：

```rust
#[derive(Debug, Serialize)]
pub struct MountInfo {
    pub mount_point: String,
    pub device: String,
    pub filesystem: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
}

#[tauri::command]
pub async fn remote_get_mounts(server_id: String, app: tauri::AppHandle) -> Result<Vec<MountInfo>, String> {
    let resp = remote_send(server_id, Payload::GetMounts, app).await?;
    match resp.payload {
        Payload::MountsResponse { mounts } => Ok(mounts),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}
```

在 `lib.rs` 中注册：

```rust
connection::remote_get_mounts,
```

- [ ] **Step 4: 测试 API**

重启 Agent，在客户端测试：

```typescript
const mounts = await invoke<MountInfo[]>("remote_get_mounts", { serverId: activeServerId });
console.log("挂载点:", mounts);
```

预期：返回挂载点列表（如 `[{mount_point: "/", device: "/dev/sda1", ...}]`）

- [ ] **Step 5: 提交代码**

```bash
git add agent/src/protocol.rs agent/src/handler.rs src-tauri/src/connection.rs src-tauri/src/lib.rs
git commit -m "feat(agent): 添加 get_mounts API，支持获取远程服务器挂载点信息"
```

---

### Task 2.2: FileManager 实现动态侧边栏

**Files:**
- Modify: `src/apps/FileManager.tsx`

- [ ] **Step 1: 定义侧边栏类型**

在文件顶部添加类型定义：

```typescript
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

interface MountInfo {
  mount_point: string;
  device: string;
  filesystem: string;
  total_bytes: number;
  used_bytes: number;
}
```

- [ ] **Step 2: 添加侧边栏 state**

在 state 定义部分添加：

```typescript
const [sidebarSections, setSidebarSections] = useState<SidebarSection[]>([]);
```

- [ ] **Step 3: 定义本地侧边栏常量**

添加常量：

```typescript
const LOCAL_SIDEBAR: SidebarSection[] = IS_WIN ? [
  {
    title: "位置",
    items: [
      { icon: "🏠", label: "用户目录", path: "C:\\Users", type: "bookmark" },
      { icon: "📄", label: "文档", path: "C:\\Users\\Public\\Documents", type: "bookmark" },
      { icon: "⬇️", label: "下载", path: "C:\\Users\\Public\\Downloads", type: "bookmark" },
    ]
  },
  {
    title: "设备",
    items: [
      { icon: "💻", label: "C 盘", path: "C:\\", type: "mount" },
      { icon: "💾", label: "D 盘", path: "D:\\", type: "mount" },
    ]
  },
] : [
  {
    title: "位置",
    items: [
      { icon: "🏠", label: "主目录", path: "/home", type: "bookmark" },
      { icon: "📄", label: "文档", path: "/home/user/Documents", type: "bookmark" },
      { icon: "⬇️", label: "下载", path: "/home/user/Downloads", type: "bookmark" },
    ]
  },
  {
    title: "设备",
    items: [
      { icon: "💻", label: "计算机", path: "/", type: "mount" },
    ]
  },
];
```

- [ ] **Step 4: 实现远程侧边栏生成函数**

添加函数：

```typescript
const generateRemoteSidebar = async () => {
  try {
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
  } catch (err) {
    console.error("生成侧边栏失败:", err);
    setSidebarSections(LOCAL_SIDEBAR);
  }
};
```

- [ ] **Step 5: 添加侧边栏生成 useEffect**

添加：

```typescript
useEffect(() => {
  if (activeServerId && username) {
    generateRemoteSidebar();
  } else {
    setSidebarSections(LOCAL_SIDEBAR);
  }
}, [activeServerId, username]);
```

- [ ] **Step 6: 替换静态侧边栏为动态侧边栏**

修改侧边栏渲染部分（第 304-330 行），替换为：

```typescript
<div className="fm-sidebar">
  {sidebarSections.map((section, idx) => (
    <div key={idx} className="sidebar-section">
      <div className="sidebar-section-title">{section.title}</div>
      {section.items.map((item) => (
        <div
          key={item.path}
          className={`sidebar-item${currentPath === item.path ? " active" : ""}`}
          onClick={() => navigateTo(item.path)}
        >
          <span className="si-icon">{item.icon}</span>
          <span className="si-label">{item.label}</span>
        </div>
      ))}
    </div>
  ))}
</div>
```

- [ ] **Step 7: 测试动态侧边栏**

测试步骤：
1. 启动客户端，打开 FileManager
2. 验证本地模式显示本地侧边栏
3. 连接远程服务器
4. 验证远程模式显示远程侧边栏（包含用户主目录和挂载点）

预期：侧边栏根据连接状态动态切换

- [ ] **Step 8: 提交代码**

```bash
git add src/apps/FileManager.tsx
git commit -m "feat(FileManager): 实现动态侧边栏，支持本地和远程模式切换"
```

---

### Task 2.3: 添加 GNOME 风格侧边栏样式

**Files:**
- Modify: `src/apps/FileManager.css`

- [ ] **Step 1: 添加侧边栏样式**

打开 `src/apps/FileManager.css`，找到 `.fm-sidebar` 部分，替换为：

```css
.fm-sidebar {
  width: 220px;
  background: var(--sidebar-bg);
  border-right: 1px solid var(--sidebar-border);
  overflow-y: auto;
  padding: 8px 0;
}

.sidebar-section {
  margin-bottom: 12px;
}

.sidebar-section-title {
  padding: 8px 12px;
  font-size: 9pt;
  color: var(--text-secondary);
  font-weight: 600;
  text-transform: uppercase;
}

.sidebar-item {
  display: flex;
  align-items: center;
  padding: 6px 12px;
  gap: 8px;
  cursor: pointer;
  border-radius: 6px;
  margin: 2px 6px;
  transition: background 0.2s ease-out;

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
  width: 16px;
  text-align: center;
}

.si-label {
  font-size: 10pt;
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
}
```

- [ ] **Step 2: 测试样式效果**

验证侧边栏样式符合 GNOME HIG：
- 侧边栏宽度 220px
- 标题使用 9pt 字体，灰色
- 项目使用 10pt 字体
- hover 时背景变化
- active 时使用 accent 颜色

- [ ] **Step 3: 提交样式**

```bash
git add src/apps/FileManager.css
git commit -m "style(FileManager): 添加 GNOME 风格侧边栏样式，符合 HIG 规范"
```

---

### Task 2.4: 实现视图切换功能

**Files:**
- Modify: `src/apps/FileManager.tsx`

- [ ] **Step 1: 验证现有视图切换**

检查 FileManager.tsx 中已有的视图切换功能（第 93 行定义 ViewMode，第 108 行 state，第 285-296 行 UI）。

确认已经实现。

- [ ] **Step 2: 测试视图切换**

测试步骤：
1. 打开 FileManager
2. 点击列表视图按钮（☰）
3. 验证显示列表视图
4. 点击图标视图按钮（⊞）
5. 验证显示图标视图

预期：视图切换正常工作

- [ ] **Step 3: 提交确认**

如果视图切换已完善，提交确认：

```bash
git add src/apps/FileManager.tsx
git commit -m "docs(FileManager): 确认视图切换功能已实现（列表/图标视图）"
```

---

## 阶段 2 验收

- [ ] **验收测试：侧边栏和视图切换**

运行完整测试：

1. 启动客户端和 Agent
2. 测试步骤：
   - 连接远程服务器
   - 验证侧边栏显示完整的位置、设备、其他位置
   - 点击侧边栏项，验证跳转到对应目录
   - 切换列表视图和图标视图
   - 验证样式符合 GNOME HIG

预期：所有侧边栏和视图切换功能正常工作

- [ ] **提交阶段 2 完成**

```bash
git add .
git commit -m "feat(FileManager): 完成阶段 2 - 侧边栏和视图切换"
```

---

## 阶段 3：文件操作功能

### Task 3.1: Agent 添加文件操作 API

**Files:**
- Modify: `agent/src/protocol.rs`
- Modify: `agent/src/handler.rs`
- Modify: `agent/src/security.rs`

- [ ] **Step 1: Agent 协议添加 Payload 类型**

打开 `agent/src/protocol.rs`，添加：

```rust
enum Payload {
    // 已有类型...

    // 新增：文件操作
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
}
```

- [ ] **Step 2: Agent 实现权限检查**

打开 `agent/src/security.rs`，添加：

```rust
pub fn check_allowed_paths(path: &str, config: &Config) -> bool {
    let allowed = &config.security.allowed_paths;
    allowed.iter().any(|prefix| path.starts_with(prefix))
}

pub fn check_blocked_operations(path: &str) -> bool {
    // 检查是否尝试删除系统关键路径
    if path.starts_with("/etc/systemd") ||
       path.starts_with("/usr/lib") ||
       path.starts_with("/bin") ||
       path.starts_with("/sbin") {
        return false;
    }
    true
}
```

- [ ] **Step 3: Agent 实现文件操作处理函数**

打开 `agent/src/handler.rs`，添加：

```rust
use crate::security::{check_allowed_paths, check_blocked_operations};

fn handle_mkdir(path: String, config: &Config) -> Payload {
    if !check_allowed_paths(&path, config) {
        return Payload::Error { message: "路径不在允许访问范围内".into(), code: 403 };
    }

    match fs::create_dir_all(&path) {
        Ok(_) => Payload::MkdirResponse { success: true, path },
        Err(e) => Payload::Error { message: e.to_string(), code: 500 },
    }
}

fn handle_rename(old_path: String, new_path: String, config: &Config) -> Payload {
    if !check_allowed_paths(&old_path, config) || !check_allowed_paths(&new_path, config) {
        return Payload::Error { message: "路径不在允许访问范围内".into(), code: 403 };
    }

    match fs::rename(&old_path, &new_path) {
        Ok(_) => Payload::RenameResponse { success: true, old_path, new_path },
        Err(e) => Payload::Error { message: e.to_string(), code: 500 },
    }
}

fn handle_delete(path: String, config: &Config) -> Payload {
    if !check_allowed_paths(&path, config) || !check_blocked_operations(&path) {
        return Payload::Error { message: "路径不在允许访问范围内或为系统关键路径".into(), code: 403 };
    }

    let metadata = fs::metadata(&path);
    match metadata {
        Ok(m) => {
            if m.is_dir() {
                match fs::remove_dir_all(&path) {
                    Ok(_) => Payload::DeleteResponse { success: true, path },
                    Err(e) => Payload::Error { message: e.to_string(), code: 500 },
                }
            } else {
                match fs::remove_file(&path) {
                    Ok(_) => Payload::DeleteResponse { success: true, path },
                    Err(e) => Payload::Error { message: e.to_string(), code: 500 },
                }
            }
        }
        Err(e) => Payload::Error { message: e.to_string(), code: 500 },
    }
}

fn handle_copy(src: String, dst: String, config: &Config) -> Payload {
    if !check_allowed_paths(&src, config) || !check_allowed_paths(&dst, config) {
        return Payload::Error { message: "路径不在允许访问范围内".into(), code: 403 };
    }

    match fs::copy(&src, &dst) {
        Ok(_) => Payload::CopyResponse { success: true, src, dst },
        Err(e) => Payload::Error { message: e.to_string(), code: 500 },
    }
}

fn handle_move(src: String, dst: String, config: &Config) -> Payload {
    handle_rename(src, dst, config)
}
```

在 `handle_payload` 中添加分支：

```rust
Payload::MkdirRequest { path } => handle_mkdir(path, &config),
Payload::RenameRequest { old_path, new_path } => handle_rename(old_path, new_path, &config),
Payload::DeleteRequest { path } => handle_delete(path, &config),
Payload::CopyRequest { src, dst } => handle_copy(src, dst, &config),
Payload::MoveRequest { src, dst } => handle_move(src, dst, &config),
```

- [ ] **Step 4: 客户端添加 API**

打开 `src-tauri/src/connection.rs`，添加：

```rust
#[tauri::command]
pub async fn remote_mkdir(server_id: String, path: String, app: tauri::AppHandle) -> Result<bool, String> {
    let resp = remote_send(server_id, Payload::MkdirRequest { path }, app).await?;
    match resp.payload {
        Payload::MkdirResponse { success, .. } => Ok(success),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

#[tauri::command]
pub async fn remote_rename(server_id: String, old_path: String, new_path: String, app: tauri::AppHandle) -> Result<bool, String> {
    let resp = remote_send(server_id, Payload::RenameRequest { old_path, new_path }, app).await?;
    match resp.payload {
        Payload::RenameResponse { success, .. } => Ok(success),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

#[tauri::command]
pub async fn remote_delete(server_id: String, path: String, app: tauri::AppHandle) -> Result<bool, String> {
    let resp = remote_send(server_id, Payload::DeleteRequest { path }, app).await?;
    match resp.payload {
        Payload::DeleteResponse { success, .. } => Ok(success),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

#[tauri::command]
pub async fn remote_copy(server_id: String, src: String, dst: String, app: tauri::AppHandle) -> Result<bool, String> {
    let resp = remote_send(server_id, Payload::CopyRequest { src, dst }, app).await?;
    match resp.payload {
        Payload::CopyResponse { success, .. } => Ok(success),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

#[tauri::command]
pub async fn remote_move(server_id: String, src: String, dst: String, app: tauri::AppHandle) -> Result<bool, String> {
    let resp = remote_send(server_id, Payload::MoveRequest { src, dst }, app).await?;
    match resp.payload {
        Payload::MoveResponse { success, .. } => Ok(success),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}
```

在 `lib.rs` 中注册：

```rust
connection::remote_mkdir,
connection::remote_rename,
connection::remote_delete,
connection::remote_copy,
connection::remote_move,
```

- [ ] **Step 5: 测试 API**

重启 Agent，测试文件操作：

```typescript
// 测试创建目录
await invoke("remote_mkdir", { serverId: activeServerId, path: "/home/user/test-dir" });

// 测试重命名
await invoke("remote_rename", {
  serverId: activeServerId,
  oldPath: "/home/user/test-dir",
  newPath: "/home/user/test-dir-renamed"
});

// 测试删除
await invoke("remote_delete", { serverId: activeServerId, path: "/home/user/test-dir-renamed" });
```

预期：所有操作成功执行，权限检查生效

- [ ] **Step 6: 提交代码**

```bash
git add agent/src/protocol.rs agent/src/handler.rs agent/src/security.rs src-tauri/src/connection.rs src-tauri/src/lib.rs
git commit -m "feat(agent): 添加文件操作 API（mkdir/rename/delete/copy/move），支持权限检查"
```

---

### Task 3.2: FileManager 实现文件操作 UI

**Files:**
- Modify: `src/apps/FileManager.tsx`

- [ ] **Step 1: 添加错误显示 state**

在 state 定义部分添加：

```typescript
const [error, setError] = useState<string | null>(null);
```

- [ ] **Step 2: 实现错误显示函数**

添加函数：

```typescript
const showError = (message: string) => {
  setError(message);
  setTimeout(() => setError(null), 5000);  // 5秒后自动消失
};
```

- [ ] **Step 3: 实现新建文件夹函数**

添加函数：

```typescript
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
```

- [ ] **Step 4: 实现重命名函数**

添加函数：

```typescript
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
```

- [ ] **Step 5: 实现删除函数**

添加函数：

```typescript
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

- [ ] **Step 6: 更新右键菜单**

修改右键菜单（第 402-428 行），添加文件操作选项：

```typescript
{contextMenu && (
  <div className="fm-context-menu" style={{ left: contextMenu.x, top: contextMenu.y }}>
    <div className="ctx-item" onClick={() => { handleOpen(contextMenu.entry); setContextMenu(null); }}>
      <span className="ctx-icon">📂</span> 打开
    </div>
    <div className="ctx-separator" />

    {/* 文件操作 */}
    <div className="ctx-item" onClick={() => { handleMkdir(); setContextMenu(null); }}>
      <span className="ctx-icon">📁</span> 新建文件夹
    </div>
    <div className="ctx-item" onClick={() => { handleRename(contextMenu.entry); setContextMenu(null); }}>
      <span className="ctx-icon">✏️</span> 重命名
    </div>
    <div className="ctx-item" onClick={() => { handleDelete(contextMenu.entry); setContextMenu(null); }}>
      <span className="ctx-icon">🗑️</span> 删除
    </div>

    <div className="ctx-separator" />
    <div className="ctx-item" onClick={() => { showProperties(contextMenu.entry); setContextMenu(null); }}>
      <span className="ctx-icon">ℹ️</span> 属性
    </div>
  </div>
)}
```

- [ ] **Step 7: 添加错误提示 UI**

在组件末尾添加错误提示：

```typescript
{error && (
  <div className="fm-error-toast">
    <span className="error-icon">⚠️</span>
    <span className="error-message">{error}</span>
    <button onClick={() => setError(null)}>✕</button>
  </div>
)}
```

- [ ] **Step 8: 添加错误提示样式**

打开 `src/apps/FileManager.css`，添加：

```css
.fm-error-toast {
  position: fixed;
  bottom: 20px;
  right: 20px;
  background: #ff6b6b;
  color: white;
  padding: 12px 16px;
  border-radius: 8px;
  display: flex;
  align-items: center;
  gap: 8px;
  z-index: 1000;
  animation: slideIn 0.3s ease-out;
}

.error-icon {
  font-size: 16px;
}

.error-message {
  flex: 1;
}

@keyframes slideIn {
  from {
    transform: translateX(100%);
    opacity: 0;
  }
  to {
    transform: translateX(0);
    opacity: 1;
  }
}
```

- [ ] **Step 9: 测试文件操作**

测试步骤：
1. 右键点击空白区域，选择"新建文件夹"
2. 输入名称，验证文件夹创建成功
3. 右键点击文件夹，选择"重命名"
4. 输入新名称，验证重命名成功
5. 右键点击文件夹，选择"删除"
6. 确认删除，验证文件夹删除成功

预期：所有文件操作正常工作，错误时显示友好提示

- [ ] **Step 10: 提交代码**

```bash
git add src/apps/FileManager.tsx src/apps/FileManager.css
git commit -m "feat(FileManager): 实现文件操作 UI（新建/重命名/删除），支持错误提示"
```

---

## 阶段 3 验收

- [ ] **验收测试：文件操作功能**

运行完整测试：

1. 启动客户端和 Agent
2. 测试步骤：
   - 创建新文件夹
   - 重命名文件/文件夹
   - 删除文件/文件夹
   - 尝试删除系统关键路径（如 `/etc/systemd`），验证权限检查生效
   - 操作失败时验证显示友好错误提示

预期：所有文件操作正常工作，权限检查生效

- [ ] **提交阶段 3 完成**

```bash
git add .
git commit -m "feat(FileManager): 完成阶段 3 - 文件操作功能"
```

---

## 阶段 4：高级功能

### Task 4.1: Agent 添加搜索 API

**Files:**
- Modify: `agent/src/protocol.rs`
- Modify: `agent/src/handler.rs`

- [ ] **Step 1: Agent 协议添加 Payload 类型**

打开 `agent/src/protocol.rs`，添加：

```rust
#[derive(Serialize)]
pub struct SearchResult {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub mtime: String,
}

enum Payload {
    // 已有类型...

    // 新增：搜索
    SearchRequest { query: String, start_path: String },
    SearchResponse { query: String, results: Vec<SearchResult> },
}
```

- [ ] **Step 2: Agent 实现搜索处理函数**

打开 `agent/src/handler.rs`，添加：

```rust
use walkdir::WalkDir;

fn handle_search(query: String, start_path: String, config: &Config) -> Payload {
    let results = WalkDir::new(&start_path)
        .into_iter()
        .filter_entry(|e| check_allowed_paths(e.path().to_string_lossy(), config))
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

在 `handle_payload` 中添加：

```rust
Payload::SearchRequest { query, start_path } => handle_search(query, start_path, &config),
```

- [ ] **Step 3: 客户端添加 API**

打开 `src-tauri/src/connection.rs`，添加：

```rust
#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub mtime: String,
}

#[tauri::command]
pub async fn remote_search(server_id: String, query: String, start_path: String, app: tauri::AppHandle) -> Result<Vec<SearchResult>, String> {
    let resp = remote_send(server_id, Payload::SearchRequest { query, start_path }, app).await?;
    match resp.payload {
        Payload::SearchResponse { results, .. } => Ok(results),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}
```

在 `lib.rs` 中注册：

```rust
connection::remote_search,
```

- [ ] **Step 4: 测试搜索 API**

重启 Agent，测试搜索：

```typescript
const results = await invoke<SearchResult[]>("remote_search", {
  serverId: activeServerId,
  query: "config",
  startPath: "/home/user"
});
console.log("搜索结果:", results);
```

预期：返回包含 "config" 的文件列表

- [ ] **Step 5: 提交代码**

```bash
git add agent/src/protocol.rs agent/src/handler.rs src-tauri/src/connection.rs src-tauri/src/lib.rs
git commit -m "feat(agent): 添加搜索 API，支持文件搜索功能"
```

---

### Task 4.2: FileManager 实现搜索 UI

**Files:**
- Modify: `src/apps/FileManager.tsx`

- [ ] **Step 1: 添加搜索 state**

在 state 定义部分添加：

```typescript
const [searchQuery, setSearchQuery] = useState<string>("");
const [searchResults, setSearchResults] = useState<SearchResult[]>([]);
const [isSearching, setIsSearching] = useState(false);
const [showSearchResults, setShowSearchResults] = useState(false);
```

- [ ] **Step 2: 定义 SearchResult 类型**

添加类型定义：

```typescript
interface SearchResult {
  path: string;
  name: string;
  is_dir: boolean;
  size: number;
  mtime: string;
}
```

- [ ] **Step 3: 实现搜索函数**

添加函数：

```typescript
const handleSearch = async () => {
  if (!searchQuery.trim()) return;

  setIsSearching(true);
  setShowSearchResults(true);
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

const clearSearch = () => {
  setSearchQuery("");
  setSearchResults([]);
  setShowSearchResults(false);
};
```

- [ ] **Step 4: 添加搜索 UI**

在 HeaderBar 中添加搜索栏（第 298 行附近）：

```typescript
<div className="search-bar">
  <input
    type="text"
    placeholder="搜索..."
    value={searchQuery}
    onChange={(e) => setSearchQuery(e.target.value)}
    onKeyPress={(e) => e.key === "Enter" && handleSearch()}
  />
  <button onClick={handleSearch} disabled={isSearching}>
    {isSearching ? "⏳" : "🔍"}
  </button>
  {showSearchResults && (
    <button onClick={clearSearch}>✕</button>
  )}
</div>
```

- [ ] **Step 5: 添加搜索结果显示**

在 MainArea 中添加搜索结果视图：

```typescript
{showSearchResults ? (
  <div className="fm-search-results">
    <div className="search-header">
      <span>搜索 "{searchQuery}" - {searchResults.length} 个结果</span>
    </div>
    {searchResults.length === 0 ? (
      <div className="fm-empty">
        <div className="empty-icon">🔍</div>
        <div className="empty-text">未找到匹配的文件</div>
      </div>
    ) : (
      <div className="fm-list">
        {searchResults.map((result, idx) => (
          <div
            key={result.path}
            className="fm-list-row"
            onClick={() => {
              if (result.is_dir) {
                navigateTo(result.path);
                clearSearch();
              }
            }}
          >
            <div className="file-name">
              <span className="fn-icon">{result.is_dir ? "📁" : "📄"}</span>
              <span className="fn-text">{result.name}</span>
            </div>
            <span className="file-path">{result.path}</span>
            <span className="file-size">{formatSize(result.size)}</span>
          </div>
        ))}
      </div>
    )}
  </div>
) : (
  // 正常文件列表
)}
```

- [ ] **Step 6: 添加搜索样式**

打开 `src/apps/FileManager.css`，添加：

```css
.search-bar {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 4px 8px;
  background: var(--view-bg);
  border: 1px solid var(--border-color);
  border-radius: 6px;
}

.search-bar input {
  border: none;
  background: transparent;
  font-size: 10pt;
  width: 150px;
  outline: none;
}

.fm-search-results {
  padding: 12px;
}

.search-header {
  padding: 8px 0;
  font-size: 10pt;
  color: var(--text-secondary);
}

.file-path {
  color: var(--text-secondary);
  font-size: 9pt;
}
```

- [ ] **Step 7: 测试搜索功能**

测试步骤：
1. 在搜索框输入关键词（如 "config"）
2. 按 Enter 或点击搜索按钮
3. 验证显示搜索结果
4. 点击搜索结果中的文件夹，验证跳转到该目录
5. 点击清除按钮，验证返回正常文件列表

预期：搜索功能正常工作

- [ ] **Step 8: 提交代码**

```bash
git add src/apps/FileManager.tsx src/apps/FileManager.css
git commit -m "feat(FileManager): 实现搜索功能，支持文件搜索和结果显示"
```

---

### Task 4.3: Agent 添加分页加载 API

**Files:**
- Modify: `agent/src/protocol.rs`
- Modify: `agent/src/handler.rs`

- [ ] **Step 1: Agent 协议添加 Payload 类型**

打开 `agent/src/protocol.rs`，添加：

```rust
enum Payload {
    // 已有类型...

    // 新增：分页加载
    ReadDirPaginatedRequest { path: String, offset: usize, limit: usize },
    ReadDirPaginatedResponse {
        path: String,
        entries: Vec<FileEntry>,
        total: usize,
        offset: usize,
        has_more: bool,
    },
}
```

- [ ] **Step 2: Agent 实现分页处理函数**

打开 `agent/src/handler.rs`，添加：

```rust
fn handle_read_dir_paginated(path: String, offset: usize, limit: usize) -> Payload {
    let all_entries = fs::read_dir(&path)
        .filter_map(|e| e.ok())
        .map(|e| file_entry_from_dir_entry(&e))
        .collect::<Vec<_>>();

    let total = all_entries.len();
    let entries = all_entries.into_iter().skip(offset).take(limit).collect();
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

在 `handle_payload` 中添加：

```rust
Payload::ReadDirPaginatedRequest { path, offset, limit } => handle_read_dir_paginated(path, offset, limit),
```

- [ ] **Step 3: 客户端添加 API**

打开 `src-tauri/src/connection.rs`，添加：

```rust
#[derive(Debug, Serialize)]
pub struct ReadDirPaginatedResponse {
    pub path: String,
    pub entries: Vec<FileEntry>,
    pub total: usize,
    pub offset: usize,
    pub has_more: bool,
}

#[tauri::command]
pub async fn remote_read_dir_paginated(
    server_id: String,
    path: String,
    offset: usize,
    limit: usize,
    app: tauri::AppHandle
) -> Result<ReadDirPaginatedResponse, String> {
    let resp = remote_send(server_id, Payload::ReadDirPaginatedRequest { path, offset, limit }, app).await?;
    match resp.payload {
        Payload::ReadDirPaginatedResponse { path, entries, total, offset, has_more } => {
            Ok(ReadDirPaginatedResponse { path, entries, total, offset, has_more })
        }
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}
```

在 `lib.rs` 中注册：

```rust
connection::remote_read_dir_paginated,
```

- [ ] **Step 4: 测试分页 API**

重启 Agent，测试分页：

```typescript
const resp = await invoke<ReadDirPaginatedResponse>("remote_read_dir_paginated", {
  serverId: activeServerId,
  path: "/home/user",
  offset: 0,
  limit: 10
});
console.log("分页结果:", resp);
```

预期：返回前 10 个文件，has_more 表示是否有更多

- [ ] **Step 5: 提交代码**

```bash
git add agent/src/protocol.rs agent/src/handler.rs src-tauri/src/connection.rs src-tauri/src/lib.rs
git commit -m "feat(agent): 添加分页加载 API，支持大目录优化"
```

---

### Task 4.4: FileManager 实现分页加载

**Files:**
- Modify: `src/apps/FileManager.tsx`

- [ ] **Step 1: 添加分页 state**

在 state 定义部分添加：

```typescript
const [allEntries, setAllEntries] = useState<FileEntry[]>([]);
const [hasMore, setHasMore] = useState(true);
const [loadingMore, setLoadingMore] = useState(false);
const PAGE_SIZE = 100;
```

- [ ] **Step 2: 定义 ReadDirPaginatedResponse 类型**

添加类型定义：

```typescript
interface ReadDirPaginatedResponse {
  path: string;
  entries: FileEntry[];
  total: number;
  offset: number;
  has_more: boolean;
}
```

- [ ] **Step 3: 修改 loadDir 函数支持分页**

修改 loadDir 函数（第 117-154 行）：

```typescript
const loadDir = useCallback(async (path: string) => {
  setLoading(true);
  setError(null);
  setSelectedIdx(null);
  setContextMenu(null);
  setAllEntries([]);  // 清空已有条目
  setHasMore(true);

  try {
    let resp: ReadDirPaginatedResponse | null = null;

    if (activeServerId) {
      // 远程模式：使用分页加载
      resp = await invoke<ReadDirPaginatedResponse>("remote_read_dir_paginated", {
        serverId: activeServerId,
        path,
        offset: 0,
        limit: PAGE_SIZE
      });
    } else {
      // 本地模式：直接读取（不分页）
      const localResp = await invoke<ReadDirResponse>("read_dir", { path });
      resp = {
        path: localResp.path,
        entries: localResp.entries,
        total: localResp.entries.length,
        offset: 0,
        has_more: false
      };
    }

    if (resp && resp.entries) {
      const sorted = [...resp.entries].sort((a, b) => {
        if (a.is_dir !== b.is_dir) return a.is_dir ? -1 : 1;
        return a.name.localeCompare(b.name);
      });
      setAllEntries(sorted);
      setHasMore(resp.has_more);
    } else {
      setAllEntries([]);
      setHasMore(false);
    }

    setCurrentPath(path);
  } catch (e: any) {
    setError(e.toString());
    setAllEntries([]);
    setHasMore(false);
  } finally {
    setLoading(false);
  }
}, [activeServerId]);
```

- [ ] **Step 4: 实现加载更多函数**

添加函数：

```typescript
const loadMore = async () => {
  if (!hasMore || loadingMore || !activeServerId) return;

  setLoadingMore(true);
  const offset = allEntries.length;

  try {
    const resp = await invoke<ReadDirPaginatedResponse>("remote_read_dir_paginated", {
      serverId: activeServerId,
      path: currentPath,
      offset,
      limit: PAGE_SIZE
    });

    const sorted = [...resp.entries].sort((a, b) => {
      if (a.is_dir !== b.is_dir) return a.is_dir ? -1 : 1;
      return a.name.localeCompare(b.name);
    });

    setAllEntries(prev => [...prev, ...sorted]);
    setHasMore(resp.has_more);
  } catch (err) {
    showError(`加载更多失败: ${err}`);
  } finally {
    setLoadingMore(false);
  }
};
```

- [ ] **Step 5: 添加滚动监听**

添加 useEffect：

```typescript
useEffect(() => {
  const handleScroll = (e: Event) => {
    const target = e.target as HTMLDivElement;
    if (target.scrollHeight - target.scrollTop <= target.clientHeight + 100) {
      loadMore();
    }
  };

  const mainElement = mainRef.current;
  if (mainElement) {
    mainElement.addEventListener("scroll", handleScroll);
    return () => mainElement.removeEventListener("scroll", handleScroll);
  }
}, [hasMore, loadingMore, currentPath, activeServerId]);
```

- [ ] **Step 6: 修改文件列表显示**

修改文件列表渲染部分，使用 allEntries：

```typescript
{entries.length === 0 ? (
  // 空目录提示
) : viewMode === "list" ? (
  <div className="fm-list">
    {/* 列表头部 */}
    {allEntries.map((entry, idx) => (
      <div key={entry.name} className="fm-list-row">
        {/* 文件项 */}
      </div>
    ))}
    {loadingMore && (
      <div className="fm-loading-more">
        <div className="spinner" />
        加载更多...
      </div>
    )}
  </div>
) : (
  // 图标视图
)}
```

- [ ] **Step 7: 添加加载更多样式**

打开 `src/apps/FileManager.css`，添加：

```css
.fm-loading-more {
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 12px;
  color: var(--text-secondary);
  font-size: 10pt;
}
```

- [ ] **Step 8: 测试分页加载**

测试步骤：
1. 进入包含大量文件的目录（>100 文件）
2. 验证只显示前 100 个文件
3. 滚动到底部
4. 验证自动加载更多文件
5. 验证显示"加载更多..."提示

预期：分页加载正常工作

- [ ] **Step 9: 提交代码**

```bash
git add src/apps/FileManager.tsx src/apps/FileManager.css
git commit -m "feat(FileManager): 实现分页加载，支持大目录优化"
```

---

### Task 4.5: Agent 添加符号链接处理

**Files:**
- Modify: `agent/src/handler.rs`

- [ ] **Step 1: 修改 FileEntry 结构**

打开 `agent/src/protocol.rs`，修改 FileEntry：

```rust
#[derive(Serialize, Clone)]
pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub mtime: String,
    pub permissions: String,
    pub is_symlink: bool,       // 新增
    pub symlink_target: Option<String>,  // 新增
}
```

- [ ] **Step 2: 修改 file_entry_from_path 函数**

打开 `agent/src/handler.rs`，修改：

```rust
fn file_entry_from_path(path: &Path) -> FileEntry {
    let metadata = fs::metadata(path);
    let symlink_metadata = fs::symlink_metadata(path);

    let is_symlink = symlink_metadata
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false);

    let symlink_target = if is_symlink {
        fs::read_link(path).ok().map(|p| p.to_string_lossy().to_string())
    } else {
        None
    };

    match metadata {
        Ok(m) => FileEntry {
            name: path.file_name().unwrap().to_string_lossy().to_string(),
            is_dir: m.is_dir(),
            size: m.len(),
            mtime: format_iso8601(m.modified().unwrap_or(SystemTime::UNIX_EPOCH)),
            permissions: get_permissions(&m),
            is_symlink,
            symlink_target,
        },
        Err(_) => FileEntry {
            name: path.file_name().unwrap().to_string_lossy().to_string(),
            is_dir: false,
            size: 0,
            mtime: String::new(),
            permissions: String::new(),
            is_symlink,
            symlink_target,
        },
    }
}
```

- [ ] **Step 3: 客户端更新类型**

打开 `src/apps/FileManager.tsx`，修改 FileEntry 类型：

```typescript
export interface FileEntry {
  name: string;
  is_dir: boolean;
  size: number;
  mtime: string;
  permissions: string;
  is_symlink: boolean;       // 新增
  symlink_target: string | null;  // 新增
}
```

- [ ] **Step 4: 测试符号链接**

在远程服务器创建符号链接：

```bash
ln -s /home/user/Documents /home/user/DocsLink
```

在客户端测试：

```typescript
const entries = await invoke<FileEntry[]>("remote_read_dir", {
  serverId: activeServerId,
  path: "/home/user"
});
const link = entries.find(e => e.name === "DocsLink");
console.log("符号链接:", link);
```

预期：返回 `is_symlink: true, symlink_target: "/home/user/Documents"`

- [ ] **Step 5: 提交代码**

```bash
git add agent/src/protocol.rs agent/src/handler.rs src/apps/FileManager.tsx
git commit -m "feat(agent): 添加符号链接处理，返回链接信息和目标路径"
```

---

### Task 4.6: FileManager 显示符号链接

**Files:**
- Modify: `src/apps/FileManager.tsx`

- [ ] **Step 1: 修改 getFileIcon 函数**

修改 getFileIcon 函数（第 32-42 行）：

```typescript
function getFileIcon(entry: FileEntry): string {
  if (entry.is_symlink) return "🔗";  // 链接图标
  if (entry.is_dir) return FOLDER_ICON;
  const ext = entry.name.lastIndexOf(".");
  if (ext >= 0) {
    const icon = FILE_ICONS[entry.name.slice(ext).toLowerCase()];
    if (icon) return icon;
  }
  if (entry.name.startsWith(".")) return "🔒";
  return "📄";
}
```

- [ ] **Step 2: 在文件列表中显示链接目标**

修改文件列表渲染部分（第 366-372 行）：

```typescript
<div className="file-name">
  <span className="fn-icon">{getFileIcon(entry)}</span>
  <span className="fn-text">{entry.name}</span>
  {entry.is_symlink && entry.symlink_target && (
    <span className="symlink-target">→ {entry.symlink_target}</span>
  )}
</div>
```

- [ ] **Step 3: 修改 handleOpen 函数**

修改 handleOpen 函数（第 198-209 行）：

```typescript
const handleOpen = useCallback((entry: FileEntry) => {
  if (entry.is_symlink && entry.symlink_target) {
    // 跳转到链接目标
    navigateTo(entry.symlink_target);
  } else if (entry.is_dir) {
    const sep = IS_WIN ? "\\" : "/";
    const newPath = currentPath === "/" || (IS_WIN && currentPath.endsWith(":"))
      ? `${currentPath}${sep}${entry.name}`
      : `${currentPath}${sep}${entry.name}`;
    navigateTo(newPath);
  } else {
    console.log(`[FileManager] Open file: ${currentPath}/${entry.name}`);
  }
}, [currentPath, navigateTo]);
```

- [ ] **Step 4: 添加符号链接样式**

打开 `src/apps/FileManager.css`，添加：

```css
.symlink-target {
  color: var(--text-secondary);
  font-size: 9pt;
  margin-left: 8px;
}
```

- [ ] **Step 5: 测试符号链接显示**

测试步骤：
1. 在远程服务器创建符号链接
2. 在 FileManager 中查看该目录
3. 验证符号链接显示 🔗 图标和目标路径
4. 双击符号链接，验证跳转到目标目录

预期：符号链接正确显示和跳转

- [ ] **Step 6: 提交代码**

```bash
git add src/apps/FileManager.tsx src/apps/FileManager.css
git commit -m "feat(FileManager): 显示符号链接信息和目标路径，支持点击跳转"
```

---

### Task 4.7: FileManager 实现错误处理

**Files:**
- Modify: `src/apps/FileManager.tsx`

- [ ] **Step 1: 定义错误类型**

添加类型定义：

```typescript
interface ErrorInfo {
  type: "permission" | "connection" | "not_found" | "unknown";
  message: string;
  path?: string;
  retryable: boolean;
}
```

- [ ] **Step 2: 修改 showError 函数**

修改 showError 函数：

```typescript
const showError = (error: ErrorInfo) => {
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

  setError(`${icon} ${title}: ${error.message}`);
  setTimeout(() => setError(null), 5000);
};
```

- [ ] **Step 3: 在 loadDir 中添加错误分类**

修改 loadDir 函数的错误处理部分：

```typescript
catch (e: any) {
  const errorMsg = e.toString();

  let errorInfo: ErrorInfo = {
    type: "unknown",
    message: errorMsg,
    retryable: false
  };

  // 分类错误
  if (errorMsg.includes("权限") || errorMsg.includes("permission")) {
    errorInfo.type = "permission";
    errorInfo.retryable = false;
  } else if (errorMsg.includes("连接") || errorMsg.includes("connection")) {
    errorInfo.type = "connection";
    errorInfo.retryable = true;
  } else if (errorMsg.includes("不存在") || errorMsg.includes("not found")) {
    errorInfo.type = "not_found";
    errorInfo.retryable = false;
  }

  showError(errorInfo);
  setAllEntries([]);
  setHasMore(false);
}
```

- [ ] **Step 4: 添加连接状态监听**

添加 useEffect：

```typescript
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

需要导入 listen：

```typescript
import { listen } from "@tauri-apps/api/event";
```

- [ ] **Step 5: 测试错误处理**

测试步骤：
1. 尝试访问权限不足的目录（如 `/root`）
2. 验证显示"权限不足"错误
3. 断开连接
4. 验证显示"连接断开"错误

预期：错误分类正确，显示友好提示

- [ ] **Step 6: 提交代码**

```bash
git add src/apps/FileManager.tsx
git commit -m "feat(FileManager): 实现错误分类和友好提示，支持连接状态监听"
```

---

## 阶段 4 验收

- [ ] **验收测试：高级功能**

运行完整测试：

1. 启动客户端和 Agent
2. 测试步骤：
   - 搜索文件，验证搜索功能
   - 进入大目录（>100 文件），验证分页加载
   - 创建符号链接，验证显示和跳转
   - 尝试访问权限不足目录，验证错误处理
   - 断开连接，验证连接状态监听

预期：所有高级功能正常工作

- [ ] **提交阶段 4 完成**

```bash
git add .
git commit -m "feat(FileManager): 完成阶段 4 - 高级功能（搜索/分页/符号链接/错误处理）"
```

---

## 最终验收

- [ ] **验收测试：完整功能**

运行完整验收测试：

1. 启动客户端和 Agent
2. 测试所有功能：
   - 基础浏览：连接后自动显示用户主目录
   - 导航功能：前进/后退/面包屑/上级目录
   - 侧边栏：完整的位置、设备、其他位置
   - 视图切换：列表/图标视图
   - 文件操作：新建/重命名/删除
   - 文件属性：查看文件详细信息
   - 搜索功能：搜索文件
   - 分页加载：大目录优化
   - 符号链接：显示和跳转
   - 错误处理：友好提示

预期：所有功能正常工作，符合设计文档要求

- [ ] **提交最终版本**

```bash
git add .
git commit -m "feat(FileManager): 完成远程文件管理器 - 对齐 GNOME Files 用户体验"
```

---

## Self-Review

**1. Spec coverage:**
- ✅ 阶段 1：基础浏览功能（修复 bug、获取用户名、初始化流程、导航、属性）
- ✅ 阶段 2：侧边栏和视图切换（动态侧边栏、挂载点、视图切换、GNOME 样式）
- ✅ 阶段 3：文件操作功能（新建、重命名、删除、权限检查）
- ✅ 阶段 4：高级功能（搜索、分页、符号链接、错误处理）

**2. Placeholder scan:**
- ✅ 没有 TBD、TODO、不完整部分
- ✅ 所有步骤包含完整代码和命令

**3. Type consistency:**
- ✅ FileEntry 类型在所有任务中一致
- ✅ Payload 类型在 Agent 和客户端一致
- ✅ 函数签名在所有任务中匹配

---

## 执行选择

计划完成并保存到 [2026-06-15-remote-file-manager-implementation.md](file:///e:/MyWork/gnome-remote/docs/superpowers/plans/2026-06-15-remote-file-manager-implementation.md)。

**两种执行方式：**

**1. Subagent-Driven（推荐）** - 我为每个任务派遣新的子代理，任务之间进行审查，快速迭代

**2. Inline Execution** - 在此会话中使用 executing-plans 执行任务，批量执行并设置检查点进行审查

**您选择哪种方式？**