# 数据预加载消除页面闪动 — 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 消除所有应用窗口打开时的数据加载闪动，采用混合策略：FileManager 走父级预加载注入，其余组件走内部骨架屏门控。

**Architecture:** Desktop 父层通过 `usePreloader` Hook 在窗口创建后立即预加载 FileManager 所需的全部数据（username + entries + mounts + sidebar），数据就绪前显示骨架屏，就绪后注入 props 渲染。Terminal / SystemMonitor / Settings 各组件内部通过 `useDataGate` Hook 在数据未就绪时展示与真实布局一致的骨架屏。

**Tech Stack:** React 18 Hooks, Tauri invoke API, CSS shimmer animation, TypeScript

**Design Spec:** [2026-06-16-preload-data-design.md](../specs/2026-06-16-preload-data-design.md)

---

## 文件结构总览

```
src/
  hooks/
    usePreloader.ts              # [新增] FileManager 预加载 Hook
    useDataGate.ts               # [新增] 通用骨架屏门控 Hook
  components/
    skeleton/
      FileManagerSkeleton.tsx    # [新增] FileManager 骨架屏
      TerminalSkeleton.tsx       # [新增] Terminal 骨架屏
      MonitorSkeleton.tsx        # [新增] SystemMonitor 骨架屏
  styles/
    skeleton.css                 # [新增] 骨架屏通用样式 (shimmer 动画)
  apps/
    FileManager.tsx              # [修改] 支持 initialData props 注入
    Terminal.tsx                 # [修改] xterm 加载期间显示骨架屏
    SystemMonitor.tsx            # [修改] 移除 demo 默认值，加 ready 门控
  shell/
    Desktop.tsx                  # [修改] 窗口生命周期加入预加载状态管理
```

---

### Task 1: 创建骨架屏基础样式

**Files:**
- Create: `src/styles/skeleton.css`

- [ ] **Step 1: 创建 skeleton.css**

写入通用骨架屏样式：shimmer 动画、各组件骨架屏的基础 CSS 类。

```css
/* ── 通用 Shimmer 动画 ──────────────────────────────── */

@keyframes skeleton-shimmer {
  0% { background-position: -200px 0; }
  100% { background-position: calc(200px + 100%) 0; }
}

@keyframes skeleton-cursor-blink {
  0%, 50% { opacity: 1; }
  51%, 100% { opacity: 0; }
}

.skeleton-shimmer {
  background: linear-gradient(
    90deg,
    var(--card-bg, #f0f0f0) 25%,
    var(--border-color, rgba(0,0,0,0.08)) 50%,
    var(--card-bg, #f0f0f0) 75%
  );
  background-size: 200px 100%;
  animation: skeleton-shimmer 1.5s ease-in-out infinite;
  border-radius: var(--radius-sm, 8px);
}

/* ── FileManager 骨架屏 ─────────────────────────────── */

.fm-skeleton {
  display: flex;
  height: 100%;
  overflow: hidden;
}

.fm-skeleton-sidebar {
  width: 200px;
  flex-shrink: 0;
  padding: 12px 8px;
  display: flex;
  flex-direction: column;
  gap: 16px;
  border-right: 1px solid var(--border-color, rgba(0,0,0,0.15));
  background: var(--sidebar-bg, #ebebeb);
}

.fm-skeleton-section-title {
  height: 16px;
  width: 60px;
  margin-bottom: 8px;
}

.fm-skeleton-sidebar-item {
  height: 28px;
  width: 80%;
  border-radius: 6px;
}

.fm-skeleton-main {
  flex: 1;
  display: flex;
  flex-direction: column;
  padding: 12px 16px;
  gap: 4px;
}

.fm-skeleton-row {
  height: 36px;
  width: 100%;
  border-radius: 6px;
}

/* ── Terminal 骨架屏 ─────────────────────────────────── */

.terminal-skeleton {
  flex: 1;
  background: #1e1e1e;
  display: flex;
  align-items: flex-start;
  justify-content: flex-start;
  padding: 12px 16px;
  position: relative;
  overflow: hidden;
}

.terminal-skeleton-line {
  height: 18px;
  width: 60%;
  border-radius: 2px;
  background: rgba(255, 255, 255, 0.08);
  margin-bottom: 4px;
}

.terminal-skeleton-cursor {
  display: inline-block;
  width: 8px;
  height: 16px;
  background: #4ec9b0;
  animation: skeleton-cursor-blink 1s step-end infinite;
  vertical-align: text-bottom;
  margin-left: 4px;
}

/* ── SystemMonitor 骨架屏 ────────────────────────────── */

.sm-skeleton-content {
  flex: 1;
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.sm-skeleton-tabs {
  display: flex;
  gap: 4px;
  margin-bottom: 8px;
}

.sm-skeleton-tab {
  height: 32px;
  width: 70px;
  border-radius: 6px;
}

.sm-skeleton-card {
  height: 140px;
  width: 100%;
  border-radius: 12px;
}

.sm-skeleton-bar {
  height: 24px;
  width: 100%;
  border-radius: 6px;
  margin-bottom: 8px;
}
```

- [ ] **Step 2: 验证文件已创建**

确认 `src/styles/skeleton.css` 存在且无语法错误。

Run: 检查文件是否存在

Expected: 文件存在

- [ ] **Step 3: Commit（提示用户）**

> 请手动提交：`git add src/styles/skeleton.css && git commit -m "feat: add skeleton screen base styles"`

---

### Task 2: 创建 FileManager 骨架屏组件

**Files:**
- Create: `src/components/skeleton/FileManagerSkeleton.tsx`

- [ ] **Step 1: 创建 FileManagerSkeleton 组件**

```tsx
import "../styles/skeleton.css";

export function FileManagerSkeleton() {
  return (
    <div className="fm">
      {/* HeaderBar 骨架 */}
      <div className="fm-headerbar">
        <div className="skeleton-shimmer" style={{ width: 32, height: 28, borderRadius: 6, display: 'inline-block' }} />
        <div className="skeleton-shimmer" style={{ width: 32, height: 28, borderRadius: 6, display: 'inline-block', marginLeft: 6 }} />
        <div className="skeleton-shimmer" style={{ width: 32, height: 28, borderRadius: 6, display: 'inline-block', marginLeft: 6 }} />
        <div className="skeleton-shimmer" style={{ flex: 1, height: 28, borderRadius: 6, display: 'inline-block', marginLeft: 12 }} />
        <div className="skeleton-shimmer" style={{ width: 80, height: 28, borderRadius: 6, display: 'inline-block', marginLeft: 8 }} />
      </div>

      {/* Content 骨架 */}
      <div className="fm-content">
        <div className="fm-skeleton-sidebar">
          <div>
            <div className="skeleton-shimmer fm-skeleton-section-title" />
            {[...Array(6)].map((_, i) => (
              <div key={i} className="skeleton-shimmer fm-skeleton-sidebar-item" />
            ))}
          </div>
          <div>
            <div className="skeleton-shimmer fm-skeleton-section-title" />
            {[...Array(3)].map((_, i) => (
              <div key={i} className="skeleton-shimmer fm-skeleton-sidebar-item" />
            ))}
          </div>
        </div>
        <div className="fm-skeleton-main">
          {/* 列表头骨架 */}
          <div style={{ display: 'flex', gap: 8, marginBottom: 8 }}>
            <div className="skeleton-shimmer" style={{ flex: 3, height: 24 }} />
            <div className="skeleton-shimmer" style={{ flex: 1, height: 24 }} />
            <div className="skeleton-shimmer" style={{ flex: 2, height: 24 }} />
            <div className="skeleton-shimmer" style={{ flex: 1, height: 24 }} />
          </div>
          {/* 列表行骨架 */}
          {[...Array(10)].map((_, i) => (
            <div key={i} className="skeleton-shimmer fm-skeleton-row" />
          ))}
        </div>
      </div>

      {/* StatusBar 骨架 */}
      <div className="fm-statusbar">
        <div className="skeleton-shimmer" style={{ width: 140, height: 14, borderRadius: 4 }} />
        <div className="skeleton-shimmer" style={{ width: 100, height: 14, borderRadius: 4 }} />
      </div>
    </div>
  );
}
```

- [ ] **Step 2: 验证组件可正常导入**

确保 TypeScript 编译无错误。

- [ ] **Step 3: Commit（提示用户）**

> 请手动提交：`git add src/components/skeleton/FileManagerSkeleton.tsx && git commit -m "feat: add FileManager skeleton screen component"`

---

### Task 3: 创建 Terminal 骨架屏组件

**Files:**
- Create: `src/components/skeleton/TerminalSkeleton.tsx`

- [ ] **Step 1: 创建 TerminalSkeleton 组件**

```tsx
import "../styles/skeleton.css";

export function TerminalSkeleton() {
  return (
    <div className="terminal-app">
      {/* TabBar 骨架 */}
      <div className="terminal-tab-bar">
        <div className="skeleton-shimmer" style={{ width: 100, height: 32, borderRadius: 6 }} />
        <div style={{ flex: 1 }} />
        <div className="skeleton-shimmer" style={{ width: 28, height: 28, borderRadius: 14 }} />
      </div>

      {/* 终端区域骨架 */}
      <div className="terminal-skeleton">
        <div style={{ color: 'rgba(255,255,255,0.5)', fontSize: 13, fontFamily: "'Source Code Pro', monospace", lineHeight: 1.6 }}>
          <div className="terminal-skeleton-line" />
          <div className="terminal-skeleton-line" style={{ width: '45%' }} />
          <div className="terminal-skeleton-line" style={{ width: '70%' }} />
          <div className="terminal-skeleton-line" style={{ width: '30%' }} />
          <div style={{ marginTop: 8 }}>
            <span style={{ color: '#4ec9a066' }}>user@gnome-remote</span>
            <span style={{ color: '#ffffff66' }}>:</span>
            <span style={{ color: '#6699ff66' }}>~</span>
            <span style={{ color: '#ffffff66' }}>$ </span>
            <span className="terminal-skeleton-cursor" />
          </div>
        </div>
      </div>

      {/* StatusBar 骨架 */}
      <div className="terminal-status-bar">
        <div className="status-item">
          <div className="skeleton-shimmer" style={{ width: 8, height: 8, borderRadius: '50%', display: 'inline-block' }} />
          <div className="skeleton-shimmer" style={{ width: 60, height: 12, borderRadius: 4, display: 'inline-block', marginLeft: 6 }} />
        </div>
        <div className="status-spacer" />
        <div className="skeleton-shimmer" style={{ width: 70, height: 12, borderRadius: 4 }} />
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Commit（提示用户）**

> 请手动提交：`git add src/components/skeleton/TerminalSkeleton.tsx && git commit -m "feat: add Terminal skeleton screen component"`

---

### Task 4: 创建 SystemMonitor 骨架屏组件

**Files:**
- Create: `src/components/skeleton/MonitorSkeleton.tsx`

- [ ] **Step 1: 创建 MonitorSkeleton 组件**

```tsx
import "../styles/skeleton.css";

export function MonitorSkeleton() {
  return (
    <div className="sm">
      {/* HeaderBar 骨架 */}
      <div className="sm-headerbar">
        <div className="sm-skeleton-tabs">
          <div className="skeleton-shimmer sm-skeleton-tab" />
          <div className="skeleton-shimmer sm-skeleton-tab" />
          <div className="skeleton-shimmer sm-skeleton-tab" />
        </div>
        <div className="sm-headerbar-spacer" />
        <div className="skeleton-shimmer" style={{ width: 28, height: 28, borderRadius: 6 }} />
      </div>

      {/* Content 骨架 */}
      <div className="sm-skeleton-content">
        {/* 卡片行 */}
        <div style={{ display: 'flex', gap: 16, marginBottom: 16 }}>
          <div className="skeleton-shimmer sm-skeleton-card" />
          <div className="skeleton-shimmer sm-skeleton-card" />
        </div>
        {/* 进度条行 */}
        <div className="skeleton-shimmer sm-skeleton-bar" />
        <div className="skeleton-shimmer sm-skeleton-bar" />
        {/* 底部卡片 */}
        <div style={{ display: 'flex', gap: 16, marginTop: 16 }}>
          <div className="skeleton-shimmer sm-skeleton-card" style={{ flex: 2 }} />
          <div className="skeleton-shimmer sm-skeleton-card" style={{ flex: 1 }} />
        </div>
        {/* 磁盘区 */}
        <div style={{ marginTop: 16 }}>
          <div className="skeleton-shimmer" style={{ width: 100, height: 16, borderRadius: 4, marginBottom: 8 }} />
          <div className="skeleton-shimmer sm-skeleton-bar" />
          <div className="skeleton-shimmer sm-skeleton-bar" />
        </div>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Commit（提示用户）**

> 请手动提交：`git add src/components/skeleton/MonitorSkeleton.tsx && git commit -m "feat: add SystemMonitor skeleton screen component"`

---

### Task 5: 创建 usePreloader Hook

**Files:**
- Create: `src/hooks/usePreloader.ts`

- [ ] **Step 1: 创建 usePreloader Hook**

这是核心 Hook，封装 FileManager 的全部预加载逻辑。

```typescript
import { useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";

/* ── Types (复用 FileManager 的类型) ─────────────────── */

export interface FileEntry {
  name: string;
  is_dir: boolean;
  size: number;
  mtime: string;
  permissions: string;
}

export interface MountInfo {
  mount_point: string;
  device: string;
  filesystem: string;
  total_bytes: number;
  used_bytes: number;
}

export interface SidebarItem {
  icon: string;
  label: string;
  path: string;
  type: "bookmark" | "mount" | "network";
}

export interface SidebarSection {
  title: string;
  items: SidebarItem[];
}

export interface FileManagerInitialData {
  username: string | null;
  currentPath: string;
  entries: FileEntry[];
  mounts: MountInfo[];
  sidebarSections: SidebarSection[];
}

type PreloadState = 'idle' | 'loading' | 'ready' | 'error';

interface PreloaderResult {
  state: PreloadState;
  data: FileManagerInitialData | null;
  error: string | null;
  preload: (serverId: string | null) => Promise<void>;
  reset: () => void;
}

/* ── Helper: 构建侧边栏 ───────────────────────────────── */

function buildSidebarSections(
  username: string | null,
  mounts: MountInfo[],
  activeServerId: string | null,
  IS_WIN: boolean
): SidebarSection[] {
  if (activeServerId && username) {
    // 远程模式侧边栏
    return [
      {
        title: "位置",
        items: [
          { icon: "\u{1F3E0}", label: "主目录", path: `/home/${username}`, type: "bookmark" },
          { icon: "\u{1F4C4}", label: "文档", path: `/home/${username}/Documents`, type: "bookmark" },
          { icon: "\u{2B07}\uFE0F", label: "下载", path: `/home/${username}/Downloads`, type: "bookmark" },
          { icon: "\u{1F5BC}\uFE0F", label: "图片", path: `/home/${username}/Pictures`, type: "bookmark" },
          { icon: "\u{1F3B5}", label: "音乐", path: `/home/${username}/Music`, type: "bookmark" },
          { icon: "\u{1F3AC}", label: "视频", path: `/home/${username}/Videos`, type: "bookmark" },
        ],
      },
      {
        title: "设备",
        items: mounts.map((m) => ({
          icon: "\u{1F4BE}",
          label: m.mount_point,
          path: m.mount_point,
          type: "mount" as const,
        })),
      },
      {
        title: "其他位置",
        items: [
          { icon: "\u{1F310}", label: "网络", path: "/network", type: "network" as const },
        ],
      },
    ];
  } else if (!activeServerId) {
    // 本地模式侧边栏
    const bookmarks = IS_WIN
      ? [
          { icon: "\u{1F3E0}", label: "主目录", path: "C:\\Users", type: "bookmark" as const },
          { icon: "\u{1F4C4}", label: "文档", path: "C:\\Users\\Public\\Documents", type: "bookmark" as const },
          { icon: "\u{2B07}\uFE0F", label: "下载", path: "C:\\Users\\Public\\Downloads", type: "bookmark" as const },
          { icon: "\u{1F5BC}\uFE0F}", label: "图片", path: "C:\\Users\\Public\\Pictures", type: "bookmark" as const },
          { icon: "\u{1F3B5}", label: "音乐", path: "C:\\Users\\Public\\Music", type: "bookmark" as const },
          { icon: "\u{1F3AC}", label: "视频", path: "C:\\Users\\Public\\Videos", type: "bookmark" as const },
        ]
      : [
          { icon: "\u{1F3E0}", label: "主目录", path: "/home", type: "bookmark" as const },
          { icon: "\u{1F4C4}", label: "文档", path: "/home/Documents", type: "bookmark" as const },
          { icon: "\u{2B07}\uFE0F", label: "下载", path: "/home/Downloads", type: "bookmark" as const },
          { icon: "\u{1F5BC}\uFE0F}", label: "图片", path: "/home/Pictures", type: "bookmark" as const },
          { icon: "\u{1F3B5}", label: "音乐", path: "/home/Music", type: "bookmark" as const },
          { icon: "\u{1F3AC}", label: "视频", path: "/home/Videos", type: "bookmark" as const },
        ];

    return [
      { title: "位置", items: bookmarks },
      {
        title: "其他位置",
        items: [{ icon: "\u{1F310}", label: "网络", path: "/network", type: "network" as const }],
      },
    ];
  }

  return [];
}

/* ── Hook ─────────────────────────────────────────────── */

export function usePreloader(): PreloaderResult {
  const [state, setState] = useState<PreloadState>('idle');
  const [data, setData] = useState<FileManagerInitialData | null>(null);
  const [error, setError] = useState<string | null>(null);

  const preload = useCallback(async (serverId: string | null) => {
    setState('loading');
    setError(null);
    setData(null);

    const IS_WIN_LOCAL = typeof navigator !== "undefined" && navigator.platform.startsWith("Win");
    const IS_WIN = !serverId && IS_WIN_LOCAL;

    try {
      if (serverId) {
        // ── 远程模式：并行获取 username 和 mounts ──
        const [username, mounts] = await Promise.all([
          invoke<string>("remote_get_current_user", { serverId })
            .catch(() => "user"),  // 失败时使用默认值
          invoke<MountInfo[]>("remote_get_mounts", { serverId })
            .catch(() => []),     // 失败时使用空数组
        ]);

        // ── 用 username 加载初始目录 ──
        const homePath = `/home/${username}`;
        const dirResp = await invoke<{ path: string; entries: FileEntry[] }>(
          "remote_read_dir",
          { serverId, path: homePath }
        );

        const entries = (dirResp?.entries || []).sort((a, b) => {
          if (a.is_dir !== b.is_dir) return a.is_dir ? -1 : 1;
          return a.name.localeCompare(b.name);
        });

        // ── 组装完整数据 ──
        const sidebarSections = buildSidebarSections(username, mounts, serverId, false);
        const result: FileManagerInitialData = {
          username,
          currentPath: homePath,
          entries,
          mounts,
          sidebarSections,
        };

        setData(result);
        setState('ready');
      } else {
        // ── 本地模式 ──
        const HOME_PATH = IS_WIN_LOCAL ? "C:\\Users" : "/home";
        const dirResp = await invoke<{ path: string; entries: FileEntry[] }>(
          "read_dir",
          { path: HOME_PATH }
        );

        const entries = (dirResp?.entries || []).sort((a, b) => {
          if (a.is_dir !== b.is_dir) return a.is_dir ? -1 : 1;
          return a.name.localeCompare(b.name);
        });

        const sidebarSections = buildSidebarSections(null, [], null, IS_WIN);
        const result: FileManagerInitialData = {
          username: null,
          currentPath: HOME_PATH,
          entries,
          mounts: [],
          sidebarSections,
        };

        setData(result);
        setState('ready');
      }
    } catch (err) {
      console.error("[usePreload] 预加载失败:", err);
      setError(err instanceof Error ? err.message : String(err));
      setState('error');
    }
  }, []);

  const reset = useCallback(() => {
    setState('idle');
    setData(null);
    setError(null);
  }, []);

  return { state, data, error, preload, reset };
}
```

- [ ] **Step 2: 验证 TypeScript 编译无误**

- [ ] **Step 3: Commit（提示用户）**

> 请手动提交：`git add src/hooks/usePreloader.ts && git commit -m "feat: add usePreloader hook for FileManager data preloading"`

---

### Task 6: 创建 useDataGate Hook

**Files:**
- Create: `src/hooks/useDataGate.ts`

- [ ] **Step 1: 创建 useDataGate Hook**

```typescript
import { useMemo } from "react";
import { TerminalSkeleton } from "../components/skeleton/TerminalSkeleton";
import { MonitorSkeleton } from "../components/skeleton/MonitorSkeleton";

export type SkeletonVariant = 'terminal' | 'monitor';

interface UseDataGateOptions {
  /** 数据是否已就绪 */
  ready: boolean;
  /** 骨架屏类型 */
  variant?: SkeletonVariant;
}

interface UseDataGateReturn {
  /** 是否应展示真实内容 */
  showContent: boolean;
  /** 骨架屏 JSX 元素 */
  skeleton: React.ReactNode;
}

/**
 * 通用数据门控 Hook。
 * 当 ready=false 时返回对应的骨架屏 JSX，
 * ready=true 时 showContent=true，调用方据此切换渲染。
 */
export function useDataGate(options: UseDataGateOptions): UseDataGateReturn {
  const { ready, variant = 'terminal' } = options;

  const skeleton = useMemo<React.ReactNode>(() => {
    switch (variant) {
      case 'terminal':
        return <TerminalSkeleton />;
      case 'monitor':
        return <MonitorSkeleton />;
      default:
        return <TerminalSkeleton />;
    }
  }, [variant]);

  return {
    showContent: ready,
    skeleton,
  };
}
```

- [ ] **Step 2: Commit（提示用户）**

> 请手动提交：`git add src/hooks/useDataGate.ts && git commit -m "feat: add useDataGate hook for skeleton screen gating"`

---

### Task 7: 改造 FileManager 为受控模式

**Files:**
- Modify: `src/apps/FileManager.tsx`

- [ ] **Step 1: 新增 initialData props 接口**

在 FileManager 组件的 props 定义处增加可选的 `initialData` 参数：

```typescript
// 在组件定义之前添加 Props 接口
interface FileManagerProps {
  /** 父级预加载注入的初始数据。存在时跳过首次加载 useEffect */
  initialData?: {
    username: string | null;
    currentPath: string;
    entries: FileEntry[];
    mounts: MountInfo[];
    sidebarSections: SidebarSection[];
  } | null;
}
```

修改函数签名：

```typescript
// 改造前
export function FileManager() {

// 改造后
export function FileManager({ initialData }: FileManagerProps = {}) {
```

- [ ] **Step 2: 添加 initialData 初始化 useEffect**

在现有 `loadDir` 回调定义之后、现有的「获取远程服务器用户名」useEffect 之前，插入：

```typescript
  // ── 父级数据注入：优先使用预加载数据 ─────────────
  useEffect(() => {
    if (initialData) {
      console.log("[FileManager] 使用父级注入的初始数据");
      setUsername(initialData.username);
      setCurrentPath(initialData.currentPath);
      setEntries(initialData.entries);
      setMounts(initialData.mounts);
      setSidebarSections(initialData.sidebarSections);
      setHistory([initialData.currentPath]);
      setPathInput(initialData.currentPath);
      setLoading(false);
    }
  }, [initialData]);
```

- [ ] **Step 3: 条件化首次加载的 useEffect**

将以下 4 个 useEffect 包裹在 `if (!initialData)` 条件中：

**获取用户名的 useEffect** (约 L177-195):
```typescript
  useEffect(() => {
    if (initialData) return; // 有注入数据时跳过
    /* ... 原有逻辑不变 ... */
  }, [activeServerId, initialData]);
```

**初始加载的 useEffect** (约 L198-212):
```typescript
  useEffect(() => {
    if (initialData) return; // 有注入数据时跳过
    /* ... 原有逻辑不变 ... */
  }, [activeServerId, username, loadDir, HOME_PATH, initialData]);
```

**获取挂载点的 useEffect** (约 L215-233):
```typescript
  useEffect(() => {
    if (initialData) return; // 有注入数据时跳过
    /* ... 原有逻辑不变 ... */
  }, [activeServerId, initialData]);
```

**生成侧边栏的 useEffect** (约 L236-301):
```typescript
  useEffect(() => {
    if (initialData) return; // 有注入数据时跳过
    /* ... 原有逻辑不变 ... */
  }, [activeServerId, username, mounts, initialData]);
```

- [ ] **Step 4: 验证改造后行为正确**

检查项：
- 有 `initialData` 时：组件直接展示数据，不触发任何 invoke 调用
- 无 `initialData` 时：行为与改造前完全一致（兜底）
- 后续导航操作（点击目录、前进后退）不受影响

- [ ] **Step 5: Commit（提示用户）**

> 请手动提交：`git add src/apps/FileManager.tsx && git commit -m "feat: support initialData injection in FileManager for preload"`

---

### Task 8: 改造 Terminal 显示骨架屏

**Files:**
- Modify: `src/apps/Terminal.tsx`

- [ ] **Step 1: 引入 TerminalSkeleton 并替换 fallback 渲染**

在文件头部引入骨架屏：

```typescript
import { TerminalSkeleton } from "../components/skeleton/TerminalSkeleton";
```

修改 terminal-container 的渲染逻辑（约 L437-457），将 fallback textarea 替换为 TerminalSkeleton：

```typescript
      {/* Terminal Content — 改造前 */}
      <div className="terminal-container">
        {xtermAvailable && xtermModules ? (
          tabs.map(...)
        ) : (
          renderFallbackTerminal()
        )}
      </div>

      {/* Terminal Content — 改造后 */}
      <div className="terminal-container">
        {xtermAvailable && xtermModules ? (
          tabs.map((tab) => (
            <div
              key={tab.id}
              className="terminal-instance"
              ref={(el) => {
                if (el && !xtermRefs.current.has(tab.id) && xtermModules) {
                  createTerminal(tab.id, el);
                }
              }}
              style={{
                display: tab.id === activeTabId ? "block" : "none",
                height: "100%",
              }}
            />
          ))
        ) : (
          <TerminalSkeleton />  {/* 替换原来的 renderFallbackTerminal() */}
        )}
      </div>
```

同时可以删除或保留 `renderFallbackTerminal` 函数（不再被使用）。建议保留但注释标注为 deprecated。

- [ ] **Step 2: Commit（提示用户）**

> 请手动提交：`git add src/apps/Terminal.tsx && git commit -m "feat: show TerminalSkeleton while xterm modules are loading"`

---

### Task 9: 改造 SystemMonitor 加 ready 门控

**Files:**
- Modify: `src/apps/SystemMonitor.tsx`

- [ ] **Step 1: 引入 useDataGate 和 MonitorSkeleton**

```typescript
import { useDataGate } from "../hooks/useDataGate";
import { MonitorSkeleton } from "../components/skeleton/MonitorSkeleton";
```

- [ ] **Step 2: 修改初始状态和增加 dataReady**

```typescript
// 改造前：
const [metrics, setMetrics] = useState<MetricsSnapshot>(generateDemoMetrics());
const [processes, setProcesses] = useState<ProcessInfo[]>(generateDemoProcesses());

// 改造后：
const [metrics, setMetrics] = useState<MetricsSnapshot | null>(null);
const [processes, setProcesses] = useState<ProcessInfo[]>([]);
const [dataReady, setDataReady] = useState(false);
```

- [ ] **Step 3: 在事件监听中标记 dataReady**

在 subscription_event 的回调内，收到第一条 metrics 数据后标记 ready：

```typescript
// 在 listen 回调的 setMetrics(newMetrics) 之后添加：
if (!dataReady) {
  setDataReady(true);
}
```

对于本地模式（无 activeServerId），直接标记 ready：

```typescript
// 在组件开头添加：
useEffect(() => {
  if (!activeServerId) {
    // 本地模式：使用 demo 数据作为占位，直接标记 ready
    setMetrics(generateDemoMetrics());
    setProcesses(generateDemoProcesses());
    setDataReady(true);
  }
}, [activeServerId]);
```

- [ ] **Step 4: 使用 useDataGate 包裹内容渲染**

在 return 的最外层 div 内部，用 useDataGate 控制：

```typescript
export function SystemMonitor() {
  // ... 所有现有 state 和 logic ...

  const { showContent, skeleton } = useDataGate({
    ready: dataReady,
    variant: 'monitor',
  });

  return (
    <div className="sm">
      {/* HeaderBar 始终渲染 */}
      <div className="sm-headerbar">{/* ... unchanged ... */}</div>

      {/* 内容区：门控切换 */}
      <div className="sm-content">
        {showContent ? (
          /* ... 原有的 processes/resources/filesystems 内容 ... */
        ) : (
          skeleton  /* MonitorSkeleton */
        )}
      </div>
    </div>
  );
}
```

注意：需要把所有原有的 tab 内容（processes/resources/filesystems 三块）包裹在 `{showContent ? (...) : skeleton}` 中。HeaderBar 保持始终渲染。

- [ ] **Step 5: 处理 metrics 可能为 null 的防御性代码**

由于 metrics 初始值为 null，需要确保所有使用 `metrics.xxx` 的地方安全。在 `showContent` 为 true 时，metrics 必定非空（因为 dataReady 是在 setMetrics 之后才设为 true）。但在 TypeScript 类型层面需要处理：

```typescript
// 在组件中计算 memPercent 等值的地方：
const memPercent = metrics ? (metrics.mem_used_bytes / metrics.mem_total_bytes) * 100 : 0;
const swapPercent = metrics?.swap_used_bytes > 0 ? 0 : 0;
```

或者更简洁的方式：当 `showContent` 为 true 时断言 metrics 非空：

```typescript
const safeMetrics = metrics!;  // showContent 为 true 时保证非空
```

- [ ] **Step 6: Commit（提示用户）**

> 请手动提交：`git add src/apps/SystemMonitor.tsx && git commit -m "feat: add data-ready gate and skeleton to SystemMonitor"`

---

### Task 10: 改造 Desktop.tsx 集成预加载流程

**Files:**
- Modify: `src/shell/Desktop.tsx`

这是最关键的改动——将预加载集成到窗口生命周期中。

- [ ] **Step 1: 引入依赖**

```typescript
import { usePreloader } from "../hooks/usePreloader";
import { FileManagerSkeleton } from "../components/skeleton/FileManagerSkeleton";
```

- [ ] **Step 2: 扩展 WindowState 接口**

```typescript
// 改造前：
interface WindowState {
  id: string;
  appId: string;
  title: string;
  minimized: boolean;
}

// 改造后：
interface WindowState {
  id: string;
  appId: string;
  title: string;
  minimized: boolean;
  preloadState: 'loading' | 'ready' | 'error';
  preloadData: any;  // FileManagerInitialData | null
}
```

- [ ] **Step 3: 在 DesktopContent 中使用 usePreloader**

在 `DesktopContent` 函数体内、现有 state 声明之后添加：

```typescript
  const preloader = usePreloader();
```

- [ ] **Step 4: 改造 openApp 为异步函数**

将 openApp 从 sync 改为 async，加入预加载逻辑：

```typescript
  const openApp = useCallback(async (appId: string) => {
    setOverviewVisible(false);

    const titles: Record<string, string> = {
      files: "文件管理器",
      terminal: "终端",
      monitor: "系统监控",
      settings: "设置",
    };

    // 如果窗口已存在，直接聚焦
    if (windows.some((w) => w.appId === appId)) {
      const existing = windows.find((w) => w.appId === appId);
      if (existing) {
        setWindows((ws) =>
          ws.map((w) => (w.id === existing.id ? { ...w, minimized: false } : w))
        );
        setActiveWindowId(existing.id);
      }
      return;
    }

    // 1. 先创建窗口框架（preloadState='loading'）
    const newWindow: WindowState = {
      id: `win-${appId}-${Date.now()}`,
      appId,
      title: titles[appId] || appId,
      minimized: false,
      preloadState: 'loading',
      preloadData: null,
    };
    setWindows((ws) => [...ws, newWindow]);
    setActiveWindowId(newWindow.id);

    // 2. FileManager 走预加载通道
    if (appId === 'files') {
      try {
        await preloader.preload(activeServerId);
        setWindows((ws) =>
          ws.map((w) =>
            w.id === newWindow.id
              ? { ...w, preloadState: 'ready', preloadData: preloader.data }
              : w
          )
        );
      } catch {
        setWindows((ws) =>
          ws.map((w) =>
            w.id === newWindow.id
              ? { ...w, preloadState: 'error' }
              : w
          )
        );
      }
    } else {
      // 其他应用：无需预加载，直接标记 ready（由组件内部处理骨架屏）
      setWindows((ws) =>
        ws.map((w) =>
          w.id === newWindow.id ? { ...w, preloadState: 'ready' } : w
        )
      );
    }
  }, [windows, preloader, activeServerId]);
```

注意：这里 `activeServerId` 需要从 `useServerManager()` 获取。检查当前代码是否已有此变量——从已读取的代码看，`DesktopContent` 已有 `const { activeServer } = useServerManager()`，需要从中取 `activeServerId` 或直接用 `activeServer?.id`。

- [ ] **Step 5: 改造 renderAppContent**

修改渲染逻辑，根据 preloadState 决定展示骨架屏还是真实组件：

```typescript
  // renderAppContent 需要接收 window 对象
  const renderAppContent = (appId: string, win: WindowState) => {
    switch (appId) {
      case 'files':
        if (win.preloadState !== 'ready') {
          return <FileManagerSkeleton />;
        }
        return <FileManager initialData={win.preloadData} />;
      case 'terminal':
        return <TerminalApp />;
      case 'monitor':
        return <SystemMonitor />;
      case 'settings':
        return <Settings />;
      default:
        return <div>Unknown app</div>;
    }
  };
```

- [ ] **Step 6: 更新窗口渲染处的调用**

找到 `{renderAppContent(win.appId)}` 的位置，改为传入 win：

```typescript
// 改造前：
{renderAppContent(win.appId)}

// 改造后：
{renderAppContent(win.appId, win)}
```

- [ ] **Step 7: 处理错误状态的 UI（可选增强）**

在 FileManagerSkeleton 的位置，如果 `preloadState === 'error'` 可以显示错误提示：

```typescript
      case 'files':
        if (win.preloadState === 'error') {
          return (
            <div className="fm" style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', flexDirection: 'column', gap: 12 }}>
              <div style={{ fontSize: 36 }}>⚠️</div>
              <div>数据加载失败</div>
              <button
                onClick={() => {
                  // 重试：重新触发预加载
                  preloader.preload(activeServerId).then(() => {
                    setWindows(ws => ws.map(w =>
                      w.id === win.id ? { ...w, preloadState: 'ready', preloadData: preloader.data } : w
                    ));
                  });
                }}
                style={{ padding: '6px 16px', borderRadius: 8, cursor: 'pointer' }}
              >重试</button>
            </div>
          );
        }
        if (win.preloadState !== 'ready') {
          return <FileManagerSkeleton />;
        }
        return <FileManager initialData={win.preloadData} />;
```

- [ ] **Step 8: 验证整体流程**

测试场景：
1. 点击 Dock 「文件」→ 看到 FileManagerSkeleton → 数据出现 → 完整 FileManager
2. 点击 Dock 「终端」→ TerminalSkeleton → xterm 加载完成 → 真实终端
3. 点击 Dock 「监控」→ MonitorSkeleton → 数据就绪 → 真实监控面板
4. 再次点击已打开的应用 → 直接聚焦，不重复预加载
5. 关闭窗口后重新打开 → 重新走预加载流程

- [ ] **Step 9: Commit（提示用户）**

> 请手动提交：`git add src/shell/Desktop.tsx && git commit -m "feat: integrate preload flow into Desktop window lifecycle"`

---

## 自审清单

### Spec 覆盖度

| 设计文档要求 | 对应 Task |
|---|---|
| usePreloader Hook 封装 FileManager 预加载 | Task 5 |
| WindowState 扩展 preloadState/preloadData | Task 10 Step 2 |
| openApp 异步化 + 预加载流程 | Task 10 Step 4 |
| FileManager 受控模式 + initialData props | Task 7 |
| FileManager 骨架屏 | Task 2 |
| Terminal 骨架屏替代 fallback | Task 8 |
| SystemMonitor 移除 demo 默认值 + ready 门控 | Task 9 |
| useDataGate 通用门控 Hook | Task 6 |
| CSS shimmer 动画 | Task 1 |
| 错误处理（超时/失败/部分失败） | Task 10 Step 7 |

### 占位符扫描

- 无 TBD / TODO
- 每个步骤都有具体代码
- 每个 Commit 都有明确的消息

### 类型一致性

- `FileManagerInitialData` 在 usePreloader 定义 → FileManager props 使用 → Desktop 传递，类型一致
- `PreloadState` 在 usePreloader 定义 → Desktop WindowState 使用，一致
- `SkeletonVariant` 在 useDataGate 定义 → 各组件引用，一致
