# 终端默认工作目录功能设计

> **版本:** v1.0
> **日期:** 2026-07-16
> **状态:** 已批准

---

## 目标

为远程终端应用添加工作目录控制功能，支持：
1. 默认终端使用配置的工作目录（或 `~`）
2. 文件管理器地址栏输入 `shell` 命令快速打开当前目录的终端
3. 路径验证和回退机制

---

## 架构设计

### 数据流

```
┌─ 前端层 ───────────────────────────────────────────────┐
│ Settings.tsx: 配置默认路径 (localStorage)              │
│ Terminal.tsx: 读取配置/preloadData → 传递参数          │
│ FileManager.tsx: 检测 "shell" 命令 → 创建终端         │
└─────────────────────────────────────────────────────────┘
                          ↓ invoke("remote_spawn_terminal")
┌─ Tauri 后端层 ─────────────────────────────────────────┐
│ terminal.rs: 接收参数，转发到 Agent                    │
│ connection.rs: Payload 添加 working_directory 字段     │
└─────────────────────────────────────────────────────────┘
                          ↓ QUIC Stream
┌─ Agent 层 ──────────────────────────────────────────────┐
│ quic.rs: 接收请求，传递参数到 PTY 管理器               │
│ pty.rs: 验证路径，设置工作目录，启动 shell             │
└─────────────────────────────────────────────────────────┘
```

### 核心组件

#### 1. Payload 扩展

```rust
// src-tauri/src/connection.rs
pub enum Payload {
    // ...
    TerminalSpawnRequest {
        shell: String,
        cols: u16,
        rows: u16,
        working_directory: Option<String>,  // 新增字段
    },
    // ...
}
```

#### 2. Agent 端路径处理

```rust
// agent/src/pty.rs
impl PtySession {
    pub fn spawn(shell: &str, cols: u16, rows: u16, working_directory: Option<&str>) -> Result<Self> {
        // ...

        match result {
            ForkptyResult::Child => {
                let mut cmd = std::process::Command::new(&shell);

                // 设置工作目录
                let home = std::env::var("HOME")
                    .or_else(|_| std::env::var("USERPROFILE"))
                    .unwrap_or_else(|_| "/".to_string());

                if let Some(path) = working_directory {
                    // 验证路径是否存在且是目录
                    if std::path::Path::new(path).is_dir() {
                        cmd.current_dir(path);
                    } else {
                        cmd.current_dir(&home);
                    }
                } else {
                    cmd.current_dir(&home);
                }

                // 设置环境变量
                cmd.env("TERM", "xterm-256color")
                    .env("COLORTERM", "truecolor")
                    .env("COLUMNS", cols.to_string())
                    .env("LINES", rows.to_string())
                    .env("LANG", "en_US.UTF-8")
                    .env("LC_CTYPE", "C.UTF-8");

                let err = cmd.exec();
                warn!("Shell 执行失败: {}", err);
                std::process::exit(1);
            }
        }
    }
}
```

#### 3. 前端配置管理

```typescript
// Settings.tsx
const TerminalSettings = () => {
  const [defaultPath, setDefaultPath] = useState(
    localStorage.getItem("terminal-default-path") || ""
  );

  const handleSave = () => {
    localStorage.setItem("terminal-default-path", defaultPath);
  };

  return (
    <div className="st-card">
      <div className="st-card-header text-title">默认工作目录</div>
      <div className="st-option-row">
        <input
          type="text"
          className="st-input"
          value={defaultPath}
          onChange={(e) => setDefaultPath(e.target.value)}
          placeholder="留空使用用户主目录 (~)"
        />
      </div>
      <div className="st-hint">
        💡 在文件管理器地址栏输入 shell 可在当前目录打开终端
      </div>
    </div>
  );
};
```

#### 4. Terminal 组件参数传递

```typescript
// Terminal.tsx
interface TerminalAppProps {
  windowId: string;
  preloadData?: {
    workingDirectory?: string;
  };
}

export function TerminalApp({ windowId, preloadData }: TerminalAppProps) {
  // 优先使用 preloadData，否则使用配置的默认路径
  const defaultPath = localStorage.getItem("terminal-default-path") || null;
  const workingDirectory = preloadData?.workingDirectory || defaultPath;

  const result = await invoke<{ session_id: string }>('remote_spawn_terminal', {
    serverId: serverId,
    shell: '',
    cols: terminal.cols,
    rows: terminal.rows,
    workingDirectory,  // 传递给后端
  });
}
```

#### 5. FileManager 地址栏命令检测

```typescript
// FileManager.tsx
const handlePathInputKeyDown = async (e: React.KeyboardEvent<HTMLInputElement>) => {
  if (e.key === "Enter") {
    e.preventDefault();
    setShowSuggestions(false);

    // 检测 shell 命令
    if (pathInput.trim() === 'shell') {
      openTerminalAtCurrentPath();
      return;
    }

    // 原有逻辑：路径导航
    const cleanPath = normalizePath(pathInput);
    // ...
  }
};

const openTerminalAtCurrentPath = useCallback(() => {
  const manager = useWindowManager();
  manager.create('terminal', {
    preloadData: {
      workingDirectory: currentPath
    }
  });
  setPathInput(currentPath);
  pathInputRef.current?.blur();
}, [currentPath]);
```

---

## 用户体验

### 场景 A：直接创建终端

**操作流程：**
1. 用户点击 Dock/桌面图标/快捷键创建终端
2. 系统读取配置的默认路径
3. 如果路径有效 → 使用该路径
4. 如果路径无效或未配置 → 使用 `~`

**配置界面：**
```
┌─ 终端设置 ──────────────────────────────────┐
│ 默认工作目录                                │
│ ┌───────────────────────────────────────┐  │
│ │ /home/user/projects                   │  │
│ └───────────────────────────────────────┘  │
│ 💡 在文件管理器地址栏输入 shell 可在当前目录打开终端 │
└──────────────────────────────────────────────┘
```

### 场景 B：文件管理器打开终端

**操作流程：**
1. 文件管理器浏览到 `/home/user/projects`
2. 地址栏输入 `shell`
3. 按 Enter
4. 创建新终端窗口，工作目录为 `/home/user/projects`
5. 地址栏恢复显示 `/home/user/projects`

---

## 错误处理

| 错误场景 | 处理方式 |
|---------|---------|
| 配置路径不存在 | 自动回退到 `~`，不显示错误 |
| 配置路径是文件而非目录 | 自动回退到 `~` |
| 无权限访问路径 | shell 启动失败，显示错误信息 |
| Agent 端路径验证失败 | 使用 home 目录 |

---

## 技术栈

- **前端：** React + TypeScript
- **后端：** Rust + Tauri 2.x
- **Agent：** Rust + quinn (QUIC) + nix (PTY)
- **配置存储：** localStorage (前端)

---

## 兼容性

- Payload 新增字段为 `Option<String>`，默认为 `None`
- 前端不传递参数时，Agent 默认使用 `~`
- 现有终端实例不受影响
- 向后兼容旧版本 Agent（忽略未知字段）

---

## 测试计划

### 功能测试

1. **默认路径配置**
   - 配置有效路径 → 终端在该目录启动
   - 配置无效路径 → 终端在 `~` 启动
   - 留空配置 → 终端在 `~` 启动

2. **地址栏命令**
   - 输入 `shell` → 打开终端（当前目录）
   - 输入 `shell` + 空格 → 不触发命令
   - 输入 `shell` + 其他字符 → 不触发命令

3. **路径验证**
   - Agent 无权限路径 → 回退到 `~`
   - 符号链接路径 → 使用真实路径
   - 路径包含特殊字符 → 正确处理

### 兼容性测试

1. **向后兼容**
   - 旧 Agent + 新客户端 → 默认 `~`
   - 新 Agent + 旧客户端 → 默认 `~`

2. **多实例**
   - 不同窗口实例使用不同工作目录
   - 配置变更不影响已打开的终端

---

## 实施优先级

| 优先级 | 组件 | 说明 |
|--------|------|------|
| P0 | Payload 扩展 | 基础数据结构 |
| P0 | Agent PTY 修改 | 核心功能实现 |
| P1 | Terminal 参数传递 | 前端集成 |
| P1 | FileManager 命令检测 | 地址栏功能 |
| P2 | Settings 配置界面 | 用户配置入口 |

---

## 参考文档

- [GNOME Terminal 文档](https://help.gnome.org/users/gnome-terminal/stable/)
- [PTY 编程指南](https://man7.org/linux/man-pages/man7/pty.7.html)
- [Tauri IPC 文档](https://tauri.app/v2/guide/inter-process-communication/)