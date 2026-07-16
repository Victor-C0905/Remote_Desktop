# 终端默认工作目录功能实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为终端添加默认工作目录配置和文件管理器地址栏 `shell` 命令支持

**Architecture:** 扩展 Payload 添加可选工作目录参数，Agent 端验证路径并设置工作目录，前端支持配置和命令触发

**Tech Stack:** React + TypeScript + Rust + Tauri 2.x + quinn + nix

---

## 文件结构

**修改文件：**
- `src-tauri/src/connection.rs` - Payload 定义添加字段
- `src-tauri/src/terminal.rs` - Tauri command 添加参数
- `agent/src/pty.rs` - PTY spawn 添加路径验证和设置
- `agent/src/server/quic.rs` - 传递参数到 PTY spawn
- `src/apps/Terminal.tsx` - 读取配置/preloadData，传递参数
- `src/apps/Settings.tsx` - 添加终端默认路径配置项
- `src/apps/FileManager.tsx` - 添加地址栏 `shell` 命令检测

---

## Task 1: 扩展 Payload 定义

**Files:**
- Modify: `src-tauri/src/connection.rs:90`

- [ ] **Step 1: 添加 working_directory 字段到 TerminalSpawnRequest**

在 `src-tauri/src/connection.rs` 文件的 Payload 枚举中，找到 `TerminalSpawnRequest` 定义（约第 90 行），修改为：

```rust
// src-tauri/src/connection.rs:90
TerminalSpawnRequest {
    shell: String,
    cols: u16,
    rows: u16,
    working_directory: Option<String>,  // 新增字段
},
```

- [ ] **Step 2: 验证编译成功**

Run: `cd src-tauri && cargo check`
Expected: 编译成功，无错误

- [ ] **Step 3: 提交**

```bash
git add src-tauri/src/connection.rs
git commit -m "feat(payload): add working_directory field to TerminalSpawnRequest"
```

---

## Task 2: 修改 Tauri command 参数

**Files:**
- Modify: `src-tauri/src/terminal.rs:68-101`

- [ ] **Step 1: 修改 remote_spawn_terminal 函数签名**

在 `src-tauri/src/terminal.rs` 文件的 `remote_spawn_terminal` 函数中（约第 68-75 行），添加参数：

```rust
// src-tauri/src/terminal.rs:68
#[tauri::command]
pub async fn remote_spawn_terminal(
    server_id: String,
    shell: String,
    cols: u16,
    rows: u16,
    working_directory: Option<String>,  // 新增参数
    app: tauri::AppHandle,
) -> Result<RemoteTerminalSession, String> {
```

- [ ] **Step 2: 修改 Envelope 创建部分**

在同一个函数的 Envelope 创建部分（约第 96-101 行），添加字段：

```rust
// src-tauri/src/terminal.rs:96
let envelope = Envelope::new(request_id, Payload::TerminalSpawnRequest {
    shell: shell.clone(),
    cols,
    rows,
    working_directory,  // 新增字段
});
```

- [ ] **Step 3: 验证编译成功**

Run: `cd src-tauri && cargo check`
Expected: 编译成功，无错误

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/terminal.rs
git commit -m "feat(terminal): add working_directory parameter to remote_spawn_terminal"
```

---

## Task 3: Agent 端修改 PTY spawn 函数

**Files:**
- Modify: `agent/src/pty.rs:32-97`
- Modify: `agent/src/pty.rs:211-217`

- [ ] **Step 1: 修改 PtySession::spawn 函数签名**

在 `agent/src/pty.rs` 文件的 `PtySession::spawn` 函数定义（约第 32 行），添加参数：

```rust
// agent/src/pty.rs:32
pub fn spawn(shell: &str, cols: u16, rows: u16, working_directory: Option<&str>) -> Result<Self> {
```

- [ ] **Step 2: 在子进程部分添加工作目录设置逻辑**

在同一个函数的 `ForkptyResult::Child` 分支（约第 76-95 行），在 `let mut cmd = std::process::Command::new(&shell);` 之后，添加工作目录设置：

```rust
// agent/src/pty.rs:76
nix::pty::ForkptyResult::Child => {
    // 子进程：执行 shell
    use std::os::unix::process::CommandExt;
    let mut cmd = std::process::Command::new(&shell);

    // ── 设置工作目录 ─────────────────────────────────────────────
    // 获取 home 目录（跨平台）
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| "/".to_string());

    if let Some(path) = working_directory {
        // 验证路径是否存在且是目录
        if std::path::Path::new(path).is_dir() {
            cmd.current_dir(path);
            info!("使用指定工作目录: {}", path);
        } else {
            cmd.current_dir(&home);
            warn!("路径不存在或不是目录，回退到 home: {} -> {}", path, home);
        }
    } else {
        cmd.current_dir(&home);
        info!("使用默认工作目录: {}", home);
    }
    // ──────────────────────────────────────────────────────────────

    cmd.env("TERM", "xterm-256color")
        .env("COLORTERM", "truecolor")
        .env("COLUMNS", cols.to_string())
        .env("LINES", rows.to_string())
        // 设置 locale 以支持 Unicode 字符（如 htop 边框）
        // 注意：只设置 LANG，不设置 LC_ALL（避免覆盖系统默认）
        // 大多数 Linux 系统都支持 en_US.UTF-8 或 C.UTF-8
        .env("LANG", "en_US.UTF-8")
        // 如果系统不支持 en_US.UTF-8，尝试 C.UTF-8（大写）
        .env("LC_CTYPE", "C.UTF-8");

    // 使用 exec 替换当前进程
    let err = cmd.exec();
    warn!("Shell 执行失败: {}", err);
    std::process::exit(1);
}
```

- [ ] **Step 3: 修改 PtyManager::spawn 函数签名和调用**

在 `agent/src/pty.rs` 文件的 `PtyManager::spawn` 函数（约第 211 行），修改函数签名和调用：

```rust
// agent/src/pty.rs:211
pub async fn spawn(&self, shell: &str, cols: u16, rows: u16, working_directory: Option<&str>) -> Result<String> {
    let session = PtySession::spawn(shell, cols, rows, working_directory)?;
    let session_id = format!("pty-{}", uuid::Uuid::new_v4());

    let mut sessions = self.sessions.lock().await;
    sessions.insert(session_id.clone(), session);

    info!("PTY 会话创建: id={}, shell={}, cwd={:?}",
        session_id, shell, working_directory);
    Ok(session_id)
}
```

- [ ] **Step 4: 验证编译成功**

Run: `cd agent && cargo check`
Expected: 编译成功，无错误

- [ ] **Step 5: 提交**

```bash
git add agent/src/pty.rs
git commit -m "feat(agent): add working directory support to PTY spawn"
```

---

## Task 4: Agent 端修改 QUIC 请求处理

**Files:**
- Modify: `agent/src/server/quic.rs:213-217`

- [ ] **Step 1: 修改 TerminalSpawnRequest 处理逻辑**

在 `agent/src/server/quic.rs` 文件的 `Payload::TerminalSpawnRequest` 处理部分（约第 213-217 行），修改参数传递：

```rust
// agent/src/server/quic.rs:213
Payload::TerminalSpawnRequest { shell, cols, rows } => {
    tracing::info!("终端创建请求: shell={}, cols={}, rows={}, cwd={:?}",
        shell, cols, rows, working_directory);

    // 创建 PTY 会话
    let session_id = pty_manager.spawn(&shell, *cols, *rows, working_directory.as_deref()).await?;
```

**注意：** 需要解构 Payload 时添加 `working_directory` 字段：

```rust
// 修改 Payload match 语句（约第 213 行）
Payload::TerminalSpawnRequest { shell, cols, rows, working_directory } => {
    // ... existing code ...
}
```

- [ ] **Step 2: 验证编译成功**

Run: `cd agent && cargo check`
Expected: 编译成功，无错误

- [ ] **Step 3: 提交**

```bash
git add agent/src/server/quic.rs
git commit -m "feat(agent): pass working_directory to PTY spawn in QUIC handler"
```

---

## Task 5: Terminal 组件支持工作目录参数

**Files:**
- Modify: `src/apps/Terminal.tsx:287-292`

- [ ] **Step 1: 修改 TerminalAppProps 接口定义**

在 `src/apps/Terminal.tsx` 文件顶部，找到 TerminalAppProps 接口（约第 435 行附近），添加 preloadData 支持：

```typescript
// src/apps/Terminal.tsx:435
interface TerminalAppProps {
  windowId: string;
  preloadData?: {
    workingDirectory?: string;
  };
}
```

- [ ] **Step 2: 在组件中读取配置和 preloadData**

在 `TerminalApp` 函数组件开头（约第 440 行），添加配置读取逻辑：

```typescript
// src/apps/Terminal.tsx:440
export function TerminalApp({ windowId, preloadData }: TerminalAppProps) {
  // ── 窗口系统集成 ────────────────────────────────────────────────────
  const { activeServer } = useServerManager();
  const activeServerId = activeServer?.id || null;

  // ── 工作目录配置 ────────────────────────────────────────────────────
  // 优先使用 preloadData（从文件管理器打开），否则使用用户配置的默认路径
  const [configuredDefaultPath] = useState(() => {
    return localStorage.getItem("terminal-default-path") || null;
  });

  const workingDirectory = preloadData?.workingDirectory || configuredDefaultPath;
```

- [ ] **Step 3: 修改 remote_spawn_terminal 调用**

在 `connectRemotePty` 函数中（约第 287-292 行），修改 invoke 调用：

```typescript
// src/apps/Terminal.tsx:287
const result = await invoke<{ session_id: string }>('remote_spawn_terminal', {
  serverId: serverId,
  shell: '',  // 使用默认 shell
  cols: terminal.cols,
  rows: terminal.rows,
  workingDirectory,  // 传递工作目录参数
});
```

- [ ] **Step 4: 验证 TypeScript 类型检查**

Run: `npm run type-check`
Expected: 无类型错误

- [ ] **Step 5: 提交**

```bash
git add src/apps/Terminal.tsx
git commit -m "feat(terminal): support working directory configuration and preloadData"
```

---

## Task 6: Settings 添加终端默认路径配置

**Files:**
- Modify: `src/apps/Settings.tsx:632-669`

- [ ] **Step 1: 添加状态管理**

在 `src/apps/Settings.tsx` 文件的 `Settings` 组件中，找到 "terminal" case 部分（约第 632 行），添加状态管理：

```typescript
// src/apps/Settings.tsx:632
case "terminal":
  const [terminalDefaultPath, setTerminalDefaultPath] = useState(() => {
    return localStorage.getItem("terminal-default-path") || "";
  });

  const handleTerminalPathChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const value = e.target.value;
    setTerminalDefaultPath(value);
    localStorage.setItem("terminal-default-path", value);
  };

  return (
    <div className="st-section">
      <div className="st-section-title">终端设置</div>

      {/* 默认工作目录 */}
      <div className="st-card">
        <div className="st-card-header text-title">默认工作目录</div>
        <div className="st-option-row">
          <input
            type="text"
            className="st-input"
            value={terminalDefaultPath}
            onChange={handleTerminalPathChange}
            placeholder="留空使用用户主目录 (~)"
            spellCheck={false}
          />
        </div>
        <div className="st-hint" style={{ marginTop: '8px', color: 'var(--text-secondary)', fontSize: '13px' }}>
          💡 在文件管理器地址栏输入 <code style={{ fontFamily: 'monospace' }}>shell</code> 可在当前目录打开终端
        </div>
      </div>

      {/* 配色方案 */}
      <div className="st-card">
        <div className="st-card-header text-title">配色方案</div>
        <div className="st-theme-toggle">
          <button className="st-theme-btn active">
            <span className="st-theme-icon">⬛</span>
            <span className="st-theme-label">暗色</span>
          </button>
          <button className="st-theme-btn">
            <span className="st-theme-icon">⬜</span>
            <span className="st-theme-label">亮色</span>
          </button>
          <button className="st-theme-btn">
            <span className="st-theme-icon">🔲</span>
            <span className="st-theme-label">白底</span>
          </button>
        </div>
      </div>

      {/* 终端选项 */}
      <div className="st-card">
        <div className="st-card-header text-title">终端选项</div>
        <div className="st-option-row">
          <span className="st-option-label text-body">光标样式</span>
          <select className="st-select">
            <option>方块</option>
            <option>竖线</option>
            <option>下划线</option>
          </select>
        </div>
        <div className="st-option-row">
          <span className="st-option-label text-body">光标闪烁</span>
          <input type="checkbox" className="st-checkbox" defaultChecked />
        </div>
      </div>
    </div>
  );
```

- [ ] **Step 2: 添加 CSS 样式（如果不存在）**

检查 `src/apps/Settings.css` 是否包含以下样式类，如果不存在则添加：

```css
/* src/apps/Settings.css */
.st-hint {
  font-size: 13px;
  color: var(--text-secondary);
  margin-top: 8px;
  line-height: 1.5;
}

.st-hint code {
  background: var(--card-bg);
  padding: 2px 6px;
  border-radius: 4px;
  font-family: 'Source Code Pro', monospace;
}
```

- [ ] **Step 3: 验证 UI 渲染**

Run: `npm run tauri dev`
Expected: 设置界面正常显示，输入框可以输入，localStorage 正常保存

- [ ] **Step 4: 提交**

```bash
git add src/apps/Settings.tsx src/apps/Settings.css
git commit -m "feat(settings): add terminal default path configuration"
```

---

## Task 7: FileManager 添加地址栏 `shell` 命令支持

**Files:**
- Modify: `src/apps/FileManager.tsx:504-553`

- [ ] **Step 1: 导入 WindowManager hook**

在 `src/apps/FileManager.tsx` 文件顶部，确保导入了 WindowManager：

```typescript
// src/apps/FileManager.tsx:顶部导入
import { useWindowManager } from '../window-system/hooks/useWindowManager';
```

- [ ] **Step 2: 在组件中获取 WindowManager 实例**

在 `FileManager` 函数组件开头，添加 WindowManager hook：

```typescript
// src/apps/FileManager.tsx:组件开头
const manager = useWindowManager();
```

- [ ] **Step 3: 添加 openTerminalAtCurrentPath 函数**

在 `FileManager` 组件中，添加打开终端的函数（约第 504 行之前）：

```typescript
// src/apps/FileManager.tsx:504 之前
const openTerminalAtCurrentPath = useCallback(() => {
  manager.create('terminal', {
    preloadData: {
      workingDirectory: currentPath
    }
  });
  // 恢复地址栏显示为当前路径
  setPathInput(currentPath);
  pathInputRef.current?.blur();
}, [manager, currentPath]);
```

- [ ] **Step 4: 修改 handlePathInputKeyDown 函数**

在 `handlePathInputKeyDown` 函数的 Enter 键处理部分（约第 509 行），添加 `shell` 命令检测：

```typescript
// src/apps/FileManager.tsx:509
} else if (e.key === "Enter") {
  e.preventDefault();
  setShowSuggestions(false);

  // 新增：检测 shell 命令
  if (pathInput.trim() === 'shell') {
    openTerminalAtCurrentPath();
    return;
  }

  // 注意：不在此处设置 setIsEditingPath(false)
  // 过早设为 false 会导致 useEffect 在异步导航期间覆盖用户输入，造成删除异常
  // 统一在导航完成后通过 blur() 触发 handlePathInputBlur 清理

  // 格式化路径
  const cleanPath = normalizePath(pathInput);

  try {
    await invoke<ReadDirResponse>("remote_read_dir", {
      serverId: activeServerId,
      path: cleanPath,
    });
    // 路径存在：正常导航（navigateTo → loadDir → setCurrentPath 更新完毕）
    navigateTo(cleanPath);
    // 导航完成后主动失焦，由 handlePathInputBlur 统一清理（同步 pathInput + 关闭编辑模式）
    pathInputRef.current?.blur();
  } catch {
    // 路径不存在：弹窗提示，恢复为当前路径（正确格式）
    setPathErrorDialog(`路径不存在: ${cleanPath}`);
    // 同样通过失焦统一处理，blur handler 会将 pathInput 恢复为 currentPath
    pathInputRef.current?.blur();
  }
}
```

- [ ] **Step 5: 验证功能**

Run: `npm run tauri dev`
测试步骤：
1. 打开文件管理器
2. 浏览到某个目录（如 `/home/user`）
3. 地址栏输入 `shell`
4. 按 Enter
5. 验证：新终端窗口打开，工作目录为当前目录

Expected: 终端在当前目录打开，地址栏恢复正常显示

- [ ] **Step 6: 提交**

```bash
git add src/apps/FileManager.tsx
git commit -m "feat(file-manager): add 'shell' command support in path input"
```

---

## Task 8: 集成测试和验证

**Files:**
- Test: 整体功能验证

- [ ] **Step 1: 编译所有组件**

Run: `npm run tauri build`
Expected: 编译成功，无错误

- [ ] **Step 2: 测试默认路径配置功能**

测试步骤：
1. 打开设置 → 终端设置
2. 在"默认工作目录"输入 `/home/user/projects`
3. 保存设置
4. 创建新终端
5. 在终端中执行 `pwd`

Expected: 显示 `/home/user/projects`

- [ ] **Step 3: 测试无效路径回退**

测试步骤：
1. 在设置中输入不存在的路径（如 `/nonexistent/path`）
2. 创建新终端
3. 执行 `pwd`

Expected: 显示用户主目录 `~`

- [ ] **Step 4: 测试文件管理器 `shell` 命令**

测试步骤：
1. 文件管理器浏览到 `/var/log`
2. 地址栏输入 `shell`
3. 按 Enter
4. 新终端中执行 `pwd`

Expected: 显示 `/var/log`

- [ ] **Step 5: 测试空配置**

测试步骤：
1. 清空设置中的"默认工作目录"
2. 创建新终端
3. 执行 `pwd`

Expected: 显示用户主目录

- [ ] **Step 6: 提交测试验证**

```bash
git add -A
git commit -m "test: verify terminal working directory feature"
```

---

## Task 9: 文档更新

**Files:**
- Create: `docs/features/terminal-working-directory.md`

- [ ] **Step 1: 创建用户文档**

创建文件 `docs/features/terminal-working-directory.md`：

```markdown
# 终端工作目录功能

## 功能概述

终端支持自定义默认工作目录，并可通过文件管理器快速在当前目录打开终端。

## 使用方法

### 配置默认工作目录

1. 打开设置面板
2. 选择"终端"设置
3. 在"默认工作目录"输入框中输入路径（如 `/home/user/projects`）
4. 留空则使用用户主目录 `~`

### 从文件管理器打开终端

1. 在文件管理器中浏览到目标目录
2. 在地址栏输入 `shell`
3. 按 Enter 键
4. 新终端窗口将以当前目录作为工作目录打开

## 工作原理

### 数据流

```
用户配置 → localStorage → Terminal.tsx → Tauri Command → Agent PTY
```

### 路径验证

- Agent 端验证路径是否存在且是目录
- 无效路径自动回退到用户主目录
- 不显示错误提示（符合用户期望）

## 注意事项

- 配置变更不影响已打开的终端窗口
- 文件管理器打开的终端优先使用当前目录
- Agent 必须有权限访问配置的路径

## 故障排除

| 问题 | 解决方法 |
|------|---------|
| 终端总是在 `~` 启动 | 检查配置路径是否存在且有权限 |
| `shell` 命令无响应 | 确保输入的是纯 `shell`（无空格或其他字符） |
| 配置保存后无效 | 检查 localStorage 是否正常工作 |
```

- [ ] **Step 2: 提交文档**

```bash
git add docs/features/terminal-working-directory.md
git commit -m "docs: add terminal working directory feature documentation"
```

---

## Self-Review 检查清单

**1. Spec coverage:**
- ✅ Payload 扩展 - Task 1
- ✅ Tauri command 参数 - Task 2
- ✅ Agent PTY 修改 - Task 3
- ✅ Agent QUIC 处理 - Task 4
- ✅ Terminal 参数传递 - Task 5
- ✅ Settings 配置界面 - Task 6
- ✅ FileManager 命令检测 - Task 7
- ✅ 集成测试 - Task 8
- ✅ 文档 - Task 9

**2. Placeholder scan:**
- ✅ 无 TBD、TODO、incomplete sections
- ✅ 所有代码步骤包含完整实现
- ✅ 所有测试步骤包含验证命令

**3. Type consistency:**
- ✅ `working_directory: Option<String>` 在所有文件中一致
- ✅ `preloadData` 接口在 Terminal 和 FileManager 中一致
- ✅ `localStorage` key 一致（`terminal-default-path`）

---

## 完成标志

- [ ] 所有 Task 完成并通过测试
- [ ] 代码已提交到 Git
- [ ] 文档已更新
- [ ] 功能已集成测试验证