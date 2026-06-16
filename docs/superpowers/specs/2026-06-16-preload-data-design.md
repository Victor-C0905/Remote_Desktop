# 数据预加载消除页面闪动 — 设计文档

> 日期: 2026-06-16
> 状态: 待审核
> 策略: 混合策略（父级预加载 + 骨架屏门控）

***

## 一、问题定义

### 1.1 当前现象

用户打开任意应用窗口后，界面先以空白/不完整状态渲染，然后数据异步加载完成后「弹入」，导致视觉闪动（FOUC - Flash of Unstyled Content）。

### 1.2 根因分析

所有组件均采用 `useState(空初始值) → useEffect → async invoke() → setState → 重渲染` 模式。组件在 mount 时立即渲染，此时数据尚未到达。

### 1.3 各组件问题详情

| 组件                 | 初始状态                                                     | 异步依赖链                                                             | 重渲染次数 | 严重度   |
| ------------------ | -------------------------------------------------------- | ----------------------------------------------------------------- | ----- | ----- |
| **FileManager**    | entries=\[] username=null mounts=\[] sidebarSections=\[] | serverId → getUsername → loadDir(home) → getMounts → buildSidebar | 4-5 次 | **高** |
| **Terminal**       | xtermAvailable=false xtermModules=null                   | dynamic import(xterm) → createTerminal → spawnPty → pollOutput    | 2-3 次 | 中     |
| **SystemMonitor**  | metrics=demoData processes=demoData                      | listen(subscription\_event) → 覆盖 demo 数据                          | 1-2 次 | 低-中   |
| **Desktop TopBar** | metrics=null                                             | listen(subscription\_event) → setMetrics                          | 1 次   | 低     |
| **Settings**       | (待确认)                                                    | (待确认)                                                             | -     | 待评估   |

***

## 二、解决方案：混合策略

### 2.1 策略分配

| 组件                 | 策略          | 理由                                                       |
| ------------------ | ----------- | -------------------------------------------------------- |
| **FileManager**    | **父级预加载注入** | 依赖链过长（serverId→username→dir→mounts→sidebar），不适合在组件内部串联等待 |
| **Terminal**       | **内部骨架屏门控** | xterm 动态 import 可接受，终端容器本身适合先展示                          |
| **SystemMonitor**  | **内部骨架屏门控** | 已有 demo 数据占位，加 ready 门控即可                                |
| **Settings**       | **内部骨架屏门控** | 配置数据量小，本地读取快                                             |
| **Desktop TopBar** | **内部骨架屏门控** | metrics 占位即可                                             |

### 2.2 架构总览

```
┌─ Desktop.tsx (父级) ─────────────────────────────────────┐
│                                                          │
│  usePreloader() Hook (新增)                               │
│  ├─ preloadFileManager(serverId)                          │
││   ├─ remote_get_current_user (并行)                      │
││   ├─ remote_read_dir(homePath) (等 username 后)          │
││   └─ remote_get_mounts (并行)                            │
││   → 返回 FileManagerInitialData                          │
││                                                          │
│  WindowState 扩展:                                        │
│  { id, appId, title, minimized, preloadState, data }     │
│                                                          │
│  渲染逻辑:                                                │
│  ├─ preloadState === 'loading' → 骨架屏                   │
│  └─ preloadState === 'ready'  → 注入 data 给子组件        │
│                                                          │
└──────────────────────────────────────────────────────────┘

┌─ 各组件内部 (useDataGate Hook) ──────────────────────────┐
│                                                          │
│  Terminal / Monitor / Settings / TopBar                  │
│                                                          │
│  const { ready, skeleton } = useDataGate(isDataReady);   │
│                                                          │
│  return (                                                │
│    <div>                                                  │
│      <HeaderBar />          ← 始终渲染（纯静态/本地状态） │
│      {ready ? <Content /> : <Skeleton />}  ← 门控切换    │
│      <StatusBar />           ← 始终渲染                 │
│    </div>                                                  │
│  );                                                       │
└──────────────────────────────────────────────────────────┘
```

***

## 三、详细设计

### 3.1 新增 Hook: `usePreloader`

**文件**: `src/hooks/usePreloader.ts`

职责：封装 FileManager 的预加载逻辑，返回可控的加载状态和数据。

```typescript
// 预加载 FileManager 所需的全部数据
interface FileManagerInitialData {
  username: string;
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
```

**预加载流程**:

```
preload(serverId) 被调用:
  │
  ├─ 本地模式 (serverId === null):
  │   └─ 并行执行: read_dir(HOME_PATH)
  │   → 返回 { username: null, currentPath: HOME_PATH, entries, mounts: [], sidebarSections }
  │
  └─ 远程模式 (serverId 存在):
      ├─ Step 1 (并行): remote_get_current_user + remote_get_mounts
      ├─ Step 2 (等 Step 1): remote_read_dir(/home/{username})
      └─ Step 3: 组装 sidebarSections
      → 返回完整 FileManagerInitialData
```

### 3.2 Desktop.tsx 改动

**WindowState 扩展**:

```typescript
interface WindowState {
  id: string;
  appId: string;
  title: string;
  minimized: boolean;
  // 新增字段
  preloadState: 'idle' | 'loading' | 'ready';
  preloadData: any;  // 按 appId 类型区分
}
```

**openApp 流程改动**:

```typescript
const openApp = useCallback(async (appId: string) => {
  // 1. 先创建窗口框架（显示骨架屏）
  const newWindow: WindowState = {
    id: `win-${appId}-${Date.now()}`,
    appId,
    title: titles[appId],
    minimized: false,
    preloadState: 'loading',  // 初始为 loading
    preloadData: null,
  };
  setWindows(ws => [...ws, newWindow]);
  setActiveWindowId(newWindow.id);

  // 2. FileManager 走预加载通道
  if (appId === 'files') {
    const { serverId } = ...; // 从 ServerManager 获取
    try {
      const data = await preloader.preload(serverId);
      setWindows(ws => ws.map(w =>
        w.id === newWindow.id
          ? { ...w, preloadState: 'ready', preloadData: data }
          : w
      ));
    } catch (err) {
      setWindows(ws => ws.map(w =>
        w.id === newWindow.id
          ? { ...w, preloadState: 'error', preloadData: null }
          : w
      ));
    }
  }
  // 其他 appId: preloadState 直接设为 ready（由组件内部处理）
  else {
    setWindows(ws => ws.map(w =>
      w.id === newWindow.id ? { ...w, preloadState: 'ready' } : w
    ));
  }
}, [windows, preloader]);
```

**renderAppContent 改动**:

```typescript
const renderAppContent = (appId: string, win: WindowState) => {
  switch (appId) {
    case 'files':
      return win.preloadState === 'ready' ? (
        <FileManager initialData={win.preloadData} />
      ) : (
        <FileManagerSkeleton />  // 骨架屏
      );
    case 'terminal':
      return <TerminalApp />;  // 内部自行处理骨架屏
    case 'monitor':
      return <SystemMonitor />;  // 内部自行处理骨架屏
    case 'settings':
      return <Settings />;  // 内部自行处理骨架屏
  }
};
```

### 3.3 FileManager 改为受控模式

**新增 props 接口**:

```typescript
interface FileManagerProps {
  initialData?: {
    username: string | null;
    currentPath: string;
    entries: FileEntry[];
    mounts: MountInfo[];
    sidebarSections: SidebarSection[];
  } | null;  // null = 需要自己加载（兜底）
}
```

**行为变更**:

* 当 `initialData` 存在时：直接用其初始化所有 state，**跳过**首次加载的 useEffect

* 当 `initialData` 为 null 时：保持原有自加载逻辑（兜底/独立使用场景）

* 后续导航操作（点击目录、前进后退）不受影响，仍走原有 `loadDir()` 逻辑

**需要移除/条件化的 useEffect**:

1. ~~获取用户名的 useEffect~~ → initialData 中已包含 username
2. ~~初始加载的 useEffect~~ → initialData 中已包含 entries + currentPath
3. ~~获取挂载点的 useEffect~~ → initialData 中已包含 mounts
4. ~~生成侧边栏的 useEffect~~ → initialData 中已包含 sidebarSections

改为：

```typescript
// 初始化：优先使用注入的数据
useEffect(() => {
  if (initialData) {
    setUsername(initialData.username);
    setCurrentPath(initialData.currentPath);
    setEntries(initialData.entries);
    setMounts(initialData.mounts);
    setSidebarSections(initialData.sidebarSections);
    setHistory([initialData.currentPath]);
  }
}, [initialData]);

// 原有加载逻辑仅在无 initialData 时生效
useEffect(() => {
  if (!initialData) {
    /* 保持原有加载逻辑作为兜底 */
  }
}, [activeServerId, username, /* ... */]);
```

### 3.4 新增 Hook: `useDataGate`

**文件**: `src/hooks/useDataGate.ts`

通用骨架屏门控 Hook，供 Terminal / SystemMonitor / Settings 使用。

```typescript
interface UseDataGateOptions {
  /** 数据是否就绪 */
  ready: boolean;
  /** 骨架屏类型 */
  variant: 'terminal' | 'monitor' | 'settings' | 'topbar-metrics';
}

interface UseDataGateReturn {
  /** 是否应展示真实内容 */
  showContent: boolean;
  /** 骨架屏 JSX */
  skeleton: React.ReactNode;
}

function useDataGate(options: UseDataGateOptions): UseDataGateReturn;
```

**骨架屏规格**（匹配各组件真实布局尺寸）:

| 组件             | 骨架屏描述                                   |
| -------------- | --------------------------------------- |
| Terminal       | 深色背景 (#1e1e1e) + 标签栏骨架 + 终端区域闪烁光标动画     |
| SystemMonitor  | HeaderBar + tab 栏 + 内容区卡片骨架（shimmer 效果） |
| Settings       | HeaderBar + 侧边栏项骨架 + 右侧面板骨架             |
| TopBar Metrics | "CPU --% MEM --.-G / --.-G" 文字占位        |

### 3.5 Terminal 内部改造

Terminal 的改造重点：

1. xterm 动态 import 期间显示终端形状的骨架屏（而非 fallback textarea）
2. `xtermAvailable` 变为 `true` 后才渲染终端实例
3. 骨架屏与真实终端外观一致（深色背景 + 光标闪烁动画）

```typescript
// Terminal 内部
const terminalReady = xtermAvailable && xtermModules !== null;

return (
  <div className="terminal-app">
    <div className="terminal-tab-bar">{/* 标签栏始终渲染 */}</div>
    <div className="terminal-container">
      {terminalReady ? (
        /* 真实 xterm 实例 */
      ) : (
        <div className="terminal-skeleton">
          <div className="skeleton-cursor" />  {/* 闪烁光标 */}
        </div>
      )}
    </div>
    <div className="terminal-status-bar">{/* 始终渲染 */}</div>
  </div>
);
```

### 3.6 SystemMonitor 内部改造

1. 移除 `generateDemoMetrics()` / `generateDemoProcesses()` 作为默认初始值
2. 改为 `metrics = null`, `processes = []` 作为初始值
3. 增加 `dataReady` state（metrics 不为 null 或已明确加载完成）
4. 未就绪时显示骨架屏

```typescript
// 改造前
const [metrics, setMetrics] = useState<MetricsSnapshot>(generateDemoMetrics());

// 改造后
const [metrics, setMetrics] = useState<MetricsSnapshot | null>(null);
const [dataReady, setDataReady] = useState(false);

// 远程模式：等事件订阅收到第一条数据
// 本地模式：直接标记 ready（或也用 demo 数据但标记为 placeholder）
```

### 3.7 CSS 骨架屏样式

**新增文件**: `src/styles/skeleton.css`

```css
/* 通用 shimmer 动画 */
@keyframes shimmer {
  0% { background-position: -200px 0; }
  100% { background-position: calc(200px + 100%) 0; }
}

.skeleton-shimmer {
  background: linear-gradient(90deg, var(--card-bg) 25%, var(--border-color) 50%, var(--card-bg) 75%);
  background-size: 200px 100%;
  animation: shimmer 1.5s ease-in-out infinite;
}

/* FileManager 骨架屏 */
.fm-skeleton-sidebar { /* 侧边栏骨架 */ }
.fm-skeleton-list { /* 列表行骨架 */ }

/* Terminal 骨架屏 */
.terminal-skeleton { /* 深色背景 + 光标 */ }

/* Monitor 骨架屏 */
.sm-skeleton-card { /* 卡片骨架 */ }
```

***

## 四、不变的部分

以下内容**不在本次改动范围内**:

1. **窗口拖拽/最小化/关闭/聚焦逻辑** — 不变
2. **各组件的业务功能** — 文件导航、终端输入、监控图表等不变
3. **Tauri 后端命令** — 不变
4. **ServerManager 连接管理** — 不变
5. **订阅系统 (subscription)** — 不变，仍由各组件自行监听事件
6. **后续导航触发的数据加载** — FileManager 点击目录后的 loadDir() 不变

***

## 五、错误处理

| 场景                          | 处理方式                                                     |
| --------------------------- | -------------------------------------------------------- |
| 预加载网络超时 (>5s)               | 显示错误骨架屏 + 重试按钮                                           |
| 预加载返回错误                     | 窗口内容区显示错误提示，保留 HeaderBar 和重试入口                           |
| 部分数据失败                      | 尽可能渲染已有数据（如 username 成功但 mounts 失败 → 正常显示文件列表，侧边栏设备列表为空） |
| 组件独立使用（无 initialData props） | 降级为原有的自加载模式                                              |

***

## 六、文件清单

| 操作     | 文件路径                                              | 说明                         |
| ------ | ------------------------------------------------- | -------------------------- |
| **新增** | `src/hooks/usePreloader.ts`                       | FileManager 预加载 Hook       |
| **新增** | `src/hooks/useDataGate.ts`                        | 通用骨架屏门控 Hook               |
| **新增** | `src/components/skeleton/FileManagerSkeleton.tsx` | FileManager 骨架屏组件          |
| **新增** | `src/components/skeleton/TerminalSkeleton.tsx`    | Terminal 骨架屏组件             |
| **新增** | `src/components/skeleton/MonitorSkeleton.tsx`     | Monitor 骨架屏组件              |
| **新增** | `src/styles/skeleton.css`                         | 骨架屏通用样式 (shimmer 动画)       |
| **修改** | `src/shell/Desktop.tsx`                           | 窗口生命周期加入预加载状态管理            |
| **修改** | `src/apps/FileManager.tsx`                        | 支持初始数据注入 props，跳过首次加载      |
| **修改** | `src/apps/Terminal.tsx`                           | xterm 加载期间显示骨架屏替代 fallback |
| **修改** | `src/apps/SystemMonitor.tsx`                      | 移除 demo 数据默认值，加 ready 门控   |

