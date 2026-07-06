# 窗口性能优化实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 解决窗口操作卡顿问题，实现窗口 Resize 和拖拽流畅度达到 55-60 FPS，应用内容稳定无闪烁。

**Architecture:** 模拟桌面系统窗口合成器架构，分离窗口框架状态与应用内容状态，使用 requestAnimationFrame 优化 Resize，事件总线精细化广播。

**Tech Stack:** React 18, TypeScript, Tauri 2.x, React.memo, requestAnimationFrame, useRef

---

## 文件结构映射

**核心改动文件：**

| 文件 | 责任 | 改动类型 |
|------|------|---------|
| `src/window-system/WindowManagerContext.tsx` | 全局窗口状态管理 + 选择性事件监听 | 重构 |
| `src/components/window-shell/WindowShell.tsx` | 窗口框架完全独立状态管理 + Resize 优化 | 重构 |
| `src/shell/Desktop.tsx` | 只订阅全局关键状态 + 应用内容 React.memo | 重构 |
| `src/window-system/core/WindowManager.ts` | save() 异步延迟 + focus() 精简 | 优化 |
| `src/window-system/events/WindowEventBus.ts` | 事件分类广播 | 新增 |
| `src/shell/Desktop.css` | z-index 固定规则 | 简化 |

**测试文件：**
- `src/window-system/tests/WindowManagerContext.test.tsx` - 新增
- `src/components/window-shell/tests/WindowShell.test.tsx` - 新增

---

## Phase 1: 核心架构调整（Day 1-3）

### Task 1: WindowManagerContext 重构 - 选择性事件监听

**Files:**
- Modify: `src/window-system/WindowManagerContext.tsx`
- Test: `src/window-system/tests/WindowManagerContext.test.tsx`

- [ ] **Step 1: 编写失败测试 - 验证只监听关键事件**

```tsx
// src/window-system/tests/WindowManagerContext.test.tsx
import { render, screen, act } from '@testing-library/react';
import { WindowManagerProvider, useWindowManager } from '../WindowManagerContext';

function TestComponent() {
  const { globalState } = useWindowManager();
  return (
    <div>
      <span data-testid="window-count">{globalState.windowList.length}</span>
      <span data-testid="active-id">{globalState.activeWindowId || 'none'}</span>
    </div>
  );
}

describe('WindowManagerContext - 事件监听优化', () => {
  test('位置变化事件不触发全局重渲染', async () => {
    const { getByTestId } = render(
      <WindowManagerProvider>
        <TestComponent />
      </WindowManagerProvider>
    );

    const manager = useWindowManager();
    
    // 创建窗口
    await act(async () => {
      await manager.create('files');
    });
    
    const initialCount = getByTestId('window-count').textContent;
    expect(initialCount).toBe('1');
    
    // 模拟位置变化事件（不应触发重渲染）
    act(() => {
      manager.emit({
        type: 'window:position_changed',
        windowId: 'win-files-1',
        timestamp: Date.now()
      });
    });
    
    // 验证渲染次数未增加
    const countAfterPosition = getByTestId('window-count').textContent;
    expect(countAfterPosition).toBe(initialCount);
  });
  
  test('关键事件触发全局重渲染', async () => {
    const { getByTestId } = render(
      <WindowManagerProvider>
        <TestComponent />
      </WindowManagerProvider>
    );

    const manager = useWindowManager();
    
    // 创建窗口（关键事件）
    await act(async () => {
      await manager.create('files');
    });
    
    expect(getByTestId('window-count').textContent).toBe('1');
    
    // 激活窗口（关键事件）
    act(() => {
      manager.focus('win-files-1');
    });
    
    expect(getByTestId('active-id').textContent).toBe('win-files-1');
  });
});
```

- [ ] **Step 2: 运行测试验证失败**

```bash
npm test -- src/window-system/tests/WindowManagerContext.test.tsx
```

Expected: FAIL - "globalState not found in useWindowManager"

- [ ] **Step 3: 修改 WindowManagerContext - 添加 globalState**

```tsx
// src/window-system/WindowManagerContext.tsx
export function WindowManagerProvider({ children }: { children: ReactNode }) {
  const registry = useMemo(() => {
    if (!registryInstance) {
      registryInstance = new WindowRegistry();
      initWindowRegistry(registryInstance);
    }
    return registryInstance;
  }, []);

  const manager = useMemo(() => {
    return new WindowManager(registry);
  }, [registry]);

  // ✅ 新增：全局关键状态
  const [globalState, setGlobalState] = useState({
    windowList: [],
    activeWindowId: null,
  });

  // 加载持久化窗口
  useEffect(() => {
    manager.load();
  }, [manager]);

  // ✅ 新增：选择性事件监听
  useEffect(() => {
    return manager.onAny((event) => {
      const criticalEvents = [
        'window:created',
        'window:closed',
        'window:focused',
        'window:minimized',
        'window:restored'
      ];

      if (criticalEvents.includes(event.type)) {
        setGlobalState({
          windowList: manager.getAll().map(w => ({
            id: w.id,
            appId: w.appId,
            isMinimized: w.minimized
          })),
          activeWindowId: manager.getActive()?.id
        });
      }
    });
  }, [manager]);

  // 保存窗口状态（保持原有逻辑）
  useEffect(() => {
    return manager.onAny(() => {
      manager.save();
    });
  }, [manager]);

  return (
    <WindowManagerContext.Provider value={{
      manager,
      globalState  // ✅ 新增：提供精简全局状态
    }}>
      {children}
    </WindowManagerContext.Provider>
  );
}

// ✅ 新增：返回 globalState 的 hook
export function useWindowManager(): IWindowManager & { globalState: any } {
  const context = useContext(WindowManagerContext);
  if (!context) {
    throw new Error('[useWindowManager] WindowManagerContext not provided');
  }

  // ✅ 移除：不再强制重渲染（改为依赖 globalState）
  return context;
}
```

- [ ] **Step 4: 运行测试验证通过**

```bash
npm test -- src/window-system/tests/WindowManagerContext.test.tsx
```

Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add src/window-system/WindowManagerContext.tsx
git add src/window-system/tests/WindowManagerContext.test.tsx
git commit -m "feat(window): WindowManagerContext 选择性事件监听优化

- 添加 globalState 管理窗口列表和激活状态
- 只监听 5 种关键事件（created, closed, focused, minimized, restored）
- 位置/大小变化不再触发全局重渲染
- 新增测试验证事件监听优化"
```

---

### Task 2: WindowShell 完全独立状态管理 - 位置和大小

**Files:**
- Modify: `src/components/window-shell/WindowShell.tsx`
- Test: `src/components/window-shell/tests/WindowShell.test.tsx`

- [ ] **Step 1: 编写失败测试 - 验证本地状态管理**

```tsx
// src/components/window-shell/tests/WindowShell.test.tsx
import { render, screen, act } from '@testing-library/react';
import { WindowShell } from '../WindowShell';

describe('WindowShell - 本地状态管理', () => {
  test('位置变化不触发父组件重渲染', () => {
    let parentRenderCount = 0;
    
    function ParentComponent() {
      parentRenderCount++;
      return (
        <WindowShell
          windowId="test-1"
          appId="files"
          isActive={true}
          mode="standard"
          onClose={() => {}}
          onMinimize={() => {}}
          onMaximize={() => {}}
          onFocus={() => {}}
        >
          <div data-testid="child-content">应用内容</div>
        </WindowShell>
      );
    }
    
    const { getByTestId, container } = render(<ParentComponent />);
    
    const initialCount = parentRenderCount;
    expect(initialCount).toBe(1);
    
    // 模拟拖拽操作（不应触发父组件重渲染）
    const windowElement = container.querySelector('.window-shell');
    act(() => {
      // 触发拖拽事件
      fireEvent.mouseDown(windowElement.querySelector('.window-header-bar'));
      fireEvent.mouseMove(document, { clientX: 200, clientY: 150 });
      fireEvent.mouseUp(document);
    });
    
    // 验证父组件渲染次数未增加
    expect(parentRenderCount).toBe(initialCount);
  });
  
  test('本地位置和大小状态正确初始化', () => {
    const { container } = render(
      <WindowShell
        windowId="test-1"
        appId="files"
        isActive={true}
        mode="standard"
        onClose={() => {}}
        onMinimize={() => {}}
        onMaximize={() => {}}
        onFocus={() => {}}
      >
        <div>应用内容</div>
      </WindowShell>
    );
    
    const windowElement = container.querySelector('.window-shell');
    expect(windowElement.style.width).toMatch(/\d+px/);
    expect(windowElement.style.height).toMatch(/\d+px/);
    expect(windowElement.style.transform).toMatch(/translate\(\d+px, \d+px\)/);
  });
});
```

- [ ] **Step 2: 运行测试验证失败**

```bash
npm test -- src/components/window-shell/tests/WindowShell.test.tsx
```

Expected: FAIL - "position/size props not found"

- [ ] **Step 3: 修改 WindowShell - 本地状态管理**

```tsx
// src/components/window-shell/WindowShell.tsx
export interface WindowShellProps {
  // ❌ 删除：position?: { x: number; y: number };
  // ❌ 删除：size?: { width: number; height: number };
  // ✅ 保留其他 props
  windowId: string;
  appId: string;
  isActive: boolean;
  mode?: 'standard' | 'frameless';
  onClose: () => void;
  onMinimize: () => void;
  onMaximize: () => void;
  onFocus: () => void;
  children: React.ReactNode;
}

export function WindowShell({
  windowId,
  appId,
  isActive,
  mode = 'standard',
  onClose,
  onMinimize,
  onMaximize,
  onFocus,
  children,
}: WindowShellProps) {
  // ✅ 新增：获取 WindowManager
  const { manager } = useWindowManager();
  const windowObj = manager.getById(windowId);

  // ✅ 新增：本地状态管理
  const [localPosition, setLocalPosition] = useState(
    windowObj?.position || { x: 100, y: 100 }
  );
  const [localSize, setLocalSize] = useState(
    windowObj?.size || { width: 800, height: 600 }
  );
  const [localMinimized, setLocalMinimized] = useState(
    windowObj?.minimized || false
  );

  // 拖拽状态（保持原有逻辑）
  const [isDragging, setIsDragging] = useState(false);
  const dragStartPos = useRef({ x: 0, y: 0 });
  const dragPositionRef = useRef(localPosition);
  const rafIdRef = useRef<number | null>(null);

  // Resize 状态（保持原有逻辑）
  const [isResizing, setIsResizing] = useState(false);
  const [resizeDirection, setResizeDirection] = useState<string | null>(null);
  const resizeStartPos = useRef({
    x: 0,
    y: 0,
    width: localSize.width,
    height: localSize.height,
    posX: localPosition.x,
    posY: localPosition.y,
  });
  const resizeDataRef = useRef({
    x: localPosition.x,
    y: localPosition.y,
    width: localSize.width,
    height: localSize.height,
  });

  // ✅ 新增：状态同步节流函数
  const syncToManager = useMemo(() => {
    let lastSyncTime = 0;
    const THROTTLE_MS = 500;

    return (data: { x: number; y: number; width: number; height: number }) => {
      const now = Date.now();
      if (now - lastSyncTime > THROTTLE_MS) {
        lastSyncTime = now;

        const windowObj = manager.getById(windowId);
        if (windowObj) {
          windowObj.setPosition({ x: data.x, y: data.y });
          windowObj.setSize({ width: data.width, height: data.height });
        }
      }
    };
  }, [manager, windowId]);

  // ✅ 拖拽逻辑更新：使用本地状态
  useEffect(() => {
    if (!isDragging) return;

    const handleMouseMove = (e: MouseEvent) => {
      const newX = e.clientX - dragStartPos.current.x;
      const newY = e.clientY - dragStartPos.current.y;

      // 边界计算
      const parentEl = windowRef.current?.parentElement;
      const parentWidth = parentEl?.clientWidth || window.innerWidth;
      const parentHeight = parentEl?.clientHeight || window.innerHeight;

      const maxX = parentWidth - localSize.width;
      const maxY = parentHeight - localSize.height;
      const minX = 0;
      const minY = 0;

      const boundedX = Math.max(minX, Math.min(newX, maxX));
      const boundedY = Math.max(minY, Math.min(newY, maxY));

      dragPositionRef.current = { x: boundedX, y: boundedY };

      if (rafIdRef.current !== null) {
        cancelAnimationFrame(rafIdRef.current);
      }

      rafIdRef.current = requestAnimationFrame(() => {
        if (windowRef.current) {
          windowRef.current.style.transform = `translate(${boundedX}px, ${boundedY}px)`;
        }
      });
    };

    const handleMouseUp = () => {
      if (rafIdRef.current !== null) {
        cancelAnimationFrame(rafIdRef.current);
        rafIdRef.current = null;
      }

      setIsDragging(false);

      // ✅ 更新本地状态
      setLocalPosition(dragPositionRef.current);

      // ✅ 同步到 WindowManager（节流）
      syncToManager({
        x: dragPositionRef.current.x,
        y: dragPositionRef.current.y,
        width: localSize.width,
        height: localSize.height
      });
    };

    document.addEventListener('mousemove', handleMouseMove, { passive: true });
    document.addEventListener('mouseup', handleMouseUp);

    return () => {
      document.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('mouseup', handleMouseUp);
      if (rafIdRef.current !== null) {
        cancelAnimationFrame(rafIdRef.current);
        rafIdRef.current = null;
      }
    };
  }, [isDragging, localSize, syncToManager]);

  // ✅ Resize 逻辑更新：使用本地状态 + requestAnimationFrame
  useEffect(() => {
    if (!isResizing || !resizeDirection) return;

    const handleMouseMove = (e: MouseEvent) => {
      const deltaX = e.clientX - resizeStartPos.current.x;
      const deltaY = e.clientY - resizeStartPos.current.y;

      let newWidth = resizeStartPos.current.width;
      let newHeight = resizeStartPos.current.height;
      let newX = resizeStartPos.current.posX;
      let newY = resizeStartPos.current.posY;

      // 边界计算
      const parentEl = windowRef.current?.parentElement;
      const parentWidth = parentEl?.clientWidth || window.innerWidth;
      const parentHeight = parentEl?.clientHeight || window.innerHeight;

      const minWidth = 400;
      const minHeight = 300;

      // Resize 方向计算（保持原有逻辑）
      if (resizeDirection.includes('e')) {
        const maxWidth = parentWidth - newX;
        newWidth = Math.max(minWidth, Math.min(maxWidth, resizeStartPos.current.width + deltaX));
      }
      if (resizeDirection.includes('w')) {
        const widthDelta = Math.min(deltaX, resizeStartPos.current.width - minWidth);
        newWidth = resizeStartPos.current.width - widthDelta;
        const newXCandidate = resizeStartPos.current.posX + widthDelta;
        newX = Math.max(0, newXCandidate);
      }
      if (resizeDirection.includes('s')) {
        const maxHeight = parentHeight - newY;
        newHeight = Math.max(minHeight, Math.min(maxHeight, resizeStartPos.current.height + deltaY));
      }
      if (resizeDirection.includes('n')) {
        const heightDelta = Math.min(deltaY, resizeStartPos.current.height - minHeight);
        newHeight = resizeStartPos.current.height - heightDelta;
        const newYCandidate = resizeStartPos.current.posY + heightDelta;
        newY = Math.max(0, newYCandidate);
      }

      // ✅ 存储到 ref
      resizeDataRef.current = { x: newX, y: newY, width: newWidth, height: newHeight };

      // ✅ 使用 requestAnimationFrame 更新视觉
      if (rafIdRef.current !== null) {
        cancelAnimationFrame(rafIdRef.current);
      }

      rafIdRef.current = requestAnimationFrame(() => {
        if (windowRef.current) {
          windowRef.current.style.width = `${newWidth}px`;
          windowRef.current.style.height = `${newHeight}px`;
          windowRef.current.style.transform = `translate(${newX}px, ${newY}px)`;
        }
      });
    };

    const handleMouseUp = () => {
      if (rafIdRef.current !== null) {
        cancelAnimationFrame(rafIdRef.current);
        rafIdRef.current = null;
      }

      setIsResizing(false);
      setResizeDirection(null);

      // ✅ 更新本地状态
      setLocalPosition({ x: resizeDataRef.current.x, y: resizeDataRef.current.y });
      setLocalSize({
        width: resizeDataRef.current.width,
        height: resizeDataRef.current.height
      });

      // ✅ 同步到 WindowManager（节流）
      syncToManager(resizeDataRef.current);
    };

    // ✅ 添加 passive: true
    document.addEventListener('mousemove', handleMouseMove, { passive: true });
    document.addEventListener('mouseup', handleMouseUp);

    return () => {
      document.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('mouseup', handleMouseUp);
      if (rafIdRef.current !== null) {
        cancelAnimationFrame(rafIdRef.current);
      }
    };
  }, [isResizing, resizeDirection, syncToManager]);

  const windowRef = useRef<HTMLDivElement>(null);

  // 拖拽开始（保持原有逻辑）
  const handleDragStart = useCallback(
    (e: React.MouseEvent) => {
      if (mode === 'frameless') return;

      e.preventDefault();
      e.stopPropagation();

      onFocus();

      setIsDragging(true);
      dragStartPos.current = {
        x: e.clientX - localPosition.x,
        y: e.clientY - localPosition.y,
      };
      dragPositionRef.current = localPosition;
    },
    [localPosition, onFocus, mode]
  );

  // Resize 开始（保持原有逻辑）
  const handleResizeStart = useCallback(
    (e: React.MouseEvent, direction: string) => {
      e.preventDefault();
      e.stopPropagation();

      setIsResizing(true);
      setResizeDirection(direction);

      resizeStartPos.current = {
        x: e.clientX,
        y: e.clientY,
        width: localSize.width,
        height: localSize.height,
        posX: localPosition.x,
        posY: localPosition.y,
      };

      resizeDataRef.current = {
        x: localPosition.x,
        y: localPosition.y,
        width: localSize.width,
        height: localSize.height,
      };
    },
    [localSize, localPosition]
  );

  // 窗口聚焦（保持原有逻辑）
  const handleWindowMouseDown = (e: React.MouseEvent) => {
    e.stopPropagation();
    onFocus();
  };

  // ✅ 窗口打开动画（使用本地状态）
  const [isOpening, setIsOpening] = useState(true);
  useEffect(() => {
    const timer = setTimeout(() => setIsOpening(false), 200);
    return () => clearTimeout(timer);
  }, []);

  return (
    <div
      ref={windowRef}
      className={`window-shell${isActive ? ' active' : ''}${isDragging ? ' dragging' : ''}`}
      data-window-id={windowId}
      style={{
        position: 'absolute',
        left: 0,
        top: 0,
        width: localSize.width,  // ✅ 使用本地状态
        height: localSize.height,  // ✅ 使用本地状态
        zIndex: undefined,  // ✅ 由 CSS 控制
        display: localMinimized ? 'none' : 'flex',  // ✅ 使用本地状态
        flexDirection: 'column',
        cursor: isDragging ? 'move' : 'default',
        transform: `translate(${localPosition.x}px, ${localPosition.y}px) scale(${isOpening ? 0.96 : 1})`,  // ✅ 使用本地状态
        willChange: isDragging ? 'transform' : 'auto',
        opacity: isOpening ? 0 : 1,
        transition: isOpening
          ? 'opacity 200ms cubic-bezier(0.25, 0, 0, 1), transform 200ms cubic-bezier(0.25, 0, 0, 1)'
          : 'opacity 0.2s ease-out',
      }}
      onMouseDown={handleWindowMouseDown}
    >
      {/* HeaderBar（保持原有逻辑） */}
      {mode === 'standard' && (
        <div
          className="window-header-bar"
          onMouseDown={handleDragStart}
          style={{
            cursor: isDragging ? 'move' : 'move',
            userSelect: 'none',
          }}
        >
          <div className="window-title">{/* 应用标题 */}</div>
          <WindowControls onClose={onClose} onMinimize={onMinimize} onMaximize={onMaximize} />
        </div>
      )}

      {/* ContentFrame */}
      <div className="window-content-frame">{children}</div>

      {/* Resize Handles（保持原有逻辑） */}
      {/* ... */}
    </div>
  );
}
```

- [ ] **Step 4: 运行测试验证通过**

```bash
npm test -- src/components/window-shell/tests/WindowShell.test.tsx
```

Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add src/components/window-shell/WindowShell.tsx
git add src/components/window-shell/tests/WindowShell.test.tsx
git commit -m "feat(window): WindowShell 完全独立状态管理

- 移除 position/size props，改用本地 state
- Resize 使用 requestAnimationFrame 优化
- 状态同步添加 500ms 节流
- Resize 事件监听器添加 passive: true
- 新增测试验证本地状态管理"
```

---

### Task 3: Desktop 组件调整 - 只订阅全局状态

**Files:**
- Modify: `src/shell/Desktop.tsx`

- [ ] **Step 1: 修改 Desktop - 使用 globalState**

```tsx
// src/shell/Desktop.tsx
function DesktopContent() {
  const [overviewVisible, setOverviewVisible] = useState(false);
  const [notificationOpen, setNotificationOpen] = useState(false);
  const unreadNotifications = 2;
  const criticalNotifications = 1;
  const [metrics, setMetrics] = useState<MetricsSnapshot | null>(null);
  const [clock, setClock] = useState("");

  const { activeServer } = useServerManager();
  const { wallpaper } = useWallpaper();
  
  // ✅ 修改：只获取 manager 和 globalState
  const { manager, globalState } = useWindowManager();
  
  const { themeId, accentColorId } = useSettingsStore();
  useTheme(themeId, accentColorId);

  // Clock（保持原有逻辑）
  useEffect(() => {
    const update = () => {
      const now = new Date();
      setClock(
        now.toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" })
      );
    };
    update();
    const id = setInterval(update, 10000);
    return () => clearInterval(id);
  }, []);

  // Metrics 监听（保持原有逻辑）
  useEffect(() => {
    if (!activeServer?.id) return;

    const setupListener = async () => {
      const unlisten = await listen<{ server_id: string; event_type: string; data: MetricsSnapshot }>(
        'subscription_event',
        (event) => {
          if (event.payload.server_id === activeServer.id && event.payload.event_type === 'metrics') {
            setMetrics(event.payload.data);
          }
        }
      );
      return unlisten;
    };

    let unlistenFn: (() => void) | undefined;
    setupListener().then((fn) => {
      unlistenFn = fn;
    });

    return () => {
      if (unlistenFn) {
        unlistenFn();
      }
    };
  }, [activeServer?.id]);

  // 断连时清空 metrics（保持原有逻辑）
  useEffect(() => {
    if (!activeServer?.id) {
      setMetrics(null);
    }
  }, [activeServer?.id]);

  const openApp = useCallback(async (appId: string) => {
    setOverviewVisible(false);

    const existingWindows = manager.getByAppId(appId);
    if (existingWindows.length > 0) {
      const existing = existingWindows[0];
      manager.focus(existing.id);
      return;
    }

    await manager.create(appId, { serverId: activeServer?.id });
  }, [manager, activeServer?.id, setOverviewVisible]);

  // Global shortcuts（保持原有逻辑）
  const shortcuts = createAppShortcuts(
    openApp,
    () => setOverviewVisible(v => !v),
    () => setOverviewVisible(false),
    () => setNotificationOpen(false)
  );
  useGlobalShortcuts(shortcuts);

  // ✅ 新增：应用内容组件（React.memo）
  const MemoizedAppContent = React.memo(function AppContent({ appId, windowId }: { appId: string, windowId: string }) {
    const manager = useWindowManager();
    const win = manager.getById(windowId);

    switch (appId) {
      case 'files':
        return <FileManager windowId={windowId} preloadData={win?.preloadData} />;
      case 'terminal':
        return <TerminalApp windowId={windowId} />;
      case 'monitor':
        return <SystemMonitor windowId={windowId} />;
      case 'settings':
        return <Settings windowId={windowId} />;
      default:
        return <div>Unknown app</div>;
    }
  }, (prevProps, nextProps) => {
    return prevProps.appId === nextProps.appId && prevProps.windowId === nextProps.windowId;
  });

  // ✅ 修改：只使用 globalState
  const { windowList, activeWindowId } = globalState;

  return (
    <div className="shell">
      {/* Top Bar（保持原有逻辑） */}
      <div className="top-bar" data-tauri-drag-region>
        {/* ... */}
      </div>

      {/* Desktop Area */}
      <div className="desktop-area" style={getWallpaperStyle(wallpaper)}>
        {/* Desktop Icons（保持原有逻辑） */}
        <div className="desktop-icons">
          {DESKTOP_APPS.map((app) => (
            <div
              key={app.id}
              className="desktop-icon"
              onDoubleClick={() => openApp(app.id)}
            >
              <div className="icon">{app.icon}</div>
              <div className="label">{app.label}</div>
            </div>
          ))}
        </div>

        {/* ✅ Application Windows - 使用 globalState */}
        {windowList.map((win) => {
          return (
            <WindowShell
              key={win.id}
              windowId={win.id}
              appId={win.appId}
              isActive={win.id === activeWindowId}
              isMinimized={win.isMinimized}
              mode="standard"
              onClose={() => manager.close(win.id)}
              onMinimize={() => manager.minimize(win.id)}
              onMaximize={() => {
                console.log('maximize:', win.id);
              }}
              onFocus={() => manager.focus(win.id)}
            >
              {/* ✅ 应用内容使用 React.memo */}
              <MemoizedAppContent appId={win.appId} windowId={win.id} />
            </WindowShell>
          );
        })}

        {/* Dock（保持原有逻辑） */}
        <div className="dock-container">
          <div className="dock">
            {DOCK_APPS.map((app) => {
              const isOpen = manager.getByAppId(app.id).length > 0;
              return (
                <div
                  key={app.id}
                  className={`dock-item${isOpen ? " running" : ""}`}
                  onClick={() => openApp(app.id)}
                >
                  <div className="icon">{app.icon}</div>
                  <div className="label">{app.label}</div>
                  {isOpen && <div className="dock-indicator" />}
                </div>
              );
            })}
          </div>
        </div>
      </div>

      {/* Overview Overlay（保持原有逻辑） */}
      {overviewVisible && (
        <div className="overlay" onClick={(e) => {
          if (e.target === e.currentTarget) setOverviewVisible(false);
        }}>
          {/* ... */}
        </div>
      )}

      {/* Notification Center（保持原有逻辑） */}
      <NotificationCenter
        isOpen={notificationOpen}
        onClose={() => setNotificationOpen(false)}
      />
    </div>
  );
}
```

- [ ] **Step 2: 验证 Desktop 渲染正确**

手动测试：
- 打开应用窗口，验证窗口列表正确显示
- 点击窗口激活，验证 activeWindowId 正确更新
- 拖动窗口，验证 Desktop 不重渲染

- [ ] **Step 3: 提交**

```bash
git add src/shell/Desktop.tsx
git commit -m "feat(window): Desktop 只订阅全局关键状态

- 使用 globalState 替代 getAll()
- 删除 position/size props 传递
- 应用内容使用 React.memo 包裹
- 窗口操作不触发 Desktop 重渲染"
```

---

## Phase 2: 事件处理优化（Day 4-5）

### Task 4: WindowEventBus 精细化

**Files:**
- Create: `src/window-system/events/WindowEventBus.ts`

- [ ] **Step 1: 创建 WindowEventBus - 事件分类**

```tsx
// src/window-system/events/WindowEventBus.ts
import { WindowEvent, WindowEventType } from '../types';

type EventHandler = (event: WindowEvent) => void;

export class WindowEventBus {
  private listeners: Map<WindowEventType, Set<EventHandler>> = new Map();
  private onAnyListeners: Set<EventHandler> = new Set();
  
  // ✅ 新增：关键事件集合
  private criticalEvents = new Set<WindowEventType>([
    'window:created',
    'window:closed',
    'window:focused',
    'window:minimized',
    'window:restored'
  ]);

  on(eventType: WindowEventType, handler: EventHandler): () => void {
    if (!this.listeners.has(eventType)) {
      this.listeners.set(eventType, new Set());
    }
    
    this.listeners.get(eventType)!.add(handler);
    
    return () => {
      this.listeners.get(eventType)?.delete(handler);
    };
  }

  onAny(handler: EventHandler): () => void {
    this.onAnyListeners.add(handler);
    
    return () => {
      this.onAnyListeners.delete(handler);
    };
  }

  emit(event: WindowEvent): void {
    // ✅ 新增：只广播关键事件到 onAny 监听器
    if (this.criticalEvents.has(event.type)) {
      this.onAnyListeners.forEach(handler => handler(event));
    }

    // 所有事件都广播到特定监听器（保持原有逻辑）
    const handlers = this.listeners.get(event.type);
    if (handlers) {
      handlers.forEach(handler => handler(event));
    }
  }

  // ✅ 新增：判断是否为关键事件
  isCriticalEvent(eventType: WindowEventType): boolean {
    return this.criticalEvents.has(eventType);
  }
}
```

- [ ] **Step 2: 提交**

```bash
git add src/window-system/events/WindowEventBus.ts
git commit -m "feat(window): WindowEventBus 精细化事件广播

- 新增 criticalEvents 集合
- onAny 只广播关键事件
- 新增 isCriticalEvent 方法"
```

---

### Task 5: WindowManager save() 优化

**Files:**
- Modify: `src/window-system/core/WindowManager.ts`

- [ ] **Step 1: 修改 WindowManager - save() 异步延迟**

```tsx
// src/window-system/core/WindowManager.ts
export class WindowManager implements IWindowManager {
  private windows: WindowCollection;
  private registry: WindowRegistry;
  private eventBus: WindowEventBus;
  private activeWindowId: string | null = null;
  
  // ✅ 新增：save 定时器
  private saveTimer: NodeJS.Timeout | null = null;

  constructor(registry: WindowRegistry) {
    this.windows = new WindowCollection();
    this.registry = registry;
    this.eventBus = new WindowEventBus();
  }

  // ... 其他方法保持不变

  // ✅ 修改：save() 异步延迟
  save(): void {
    // 清除之前的定时器
    if (this.saveTimer) {
      clearTimeout(this.saveTimer);
    }

    // 延迟 1 秒后执行
    this.saveTimer = setTimeout(() => {
      try {
        const data = this.windows.getAll().map(w => w.serialize());
        localStorage.setItem('gnome-remote-windows', JSON.stringify(data));
      } catch (error) {
        console.error('[WindowManager] Failed to save window states', error);
        
        // 重试机制
        try {
          localStorage.removeItem('gnome-remote-windows');
          localStorage.setItem('gnome-remote-windows', JSON.stringify(data));
        } catch (retryError) {
          console.error('[WindowManager] Retry save failed', retryError);
        }
      }
      
      this.saveTimer = null;
    }, 1000);
  }

  // ✅ 修改：focus() 精简
  focus(windowId: string): void {
    const window = this.windows.get(windowId);
    if (!window) return;

    if (window.minimized) {
      window.setMinimized(false);
    }

    window.activate();
    this.activeWindowId = windowId;

    // 只广播激活事件
    this.eventBus.emit({
      type: 'window:focused',
      windowId,
      timestamp: Date.now(),
    });
  }

  // ✅ 新增：清理定时器（卸载时）
  cleanup(): void {
    if (this.saveTimer) {
      clearTimeout(this.saveTimer);
      this.saveTimer = null;
    }
  }
}
```

- [ ] **Step 2: 验证 save 不阻塞 UI**

手动测试：
- 快速拖动窗口多次，验证 localStorage 不频繁写入
- 查看 localStorage，验证窗口状态正确保存

- [ ] **Step 3: 提交**

```bash
git add src/window-system/core/WindowManager.ts
git commit -m "feat(window): WindowManager save() 异步优化

- save() 延迟 1 秒执行，避免频繁写入
- focus() 精简，只广播激活事件
- 新增 cleanup() 方法清理定时器
- 添加错误处理和重试机制"
```

---

### Task 6: CSS z-index 固定规则

**Files:**
- Modify: `src/shell/Desktop.css`

- [ ] **Step 1: 添加 CSS z-index 固定规则**

```css
/* src/shell/Desktop.css */

/* ... 其他样式保持不变 ... */

/* ✅ 新增：窗口 z-index 固定规则 */
.window-shell {
  /* 基础 z-index：10 */
  z-index: 10;
}

.window-shell.active {
  /* 活动窗口：固定 90 */
  z-index: 90 !important;
}

.window-shell.minimized {
  /* 最小化窗口：隐藏 */
  display: none;
}
```

- [ ] **Step 2: 验证 z-index 正确**

手动测试：
- 打开多个窗口，验证活动窗口在最上层
- 切换窗口激活，验证 z-index 自动切换

- [ ] **Step 3: 提交**

```bash
git add src/shell/Desktop.css
git commit -m "feat(window): CSS z-index 固定规则

- 基础窗口 z-index: 10
- 活动窗口 z-index: 90
- 删除 JavaScript 动态计算"
```

---

## Phase 3: 测试与验证（Day 6-7）

### Task 7: 性能测试

**Files:**
- Create: `src/tests/performance/window-performance.test.tsx`

- [ ] **Step 1: 编写性能测试**

```tsx
// src/tests/performance/window-performance.test.tsx
import { render, screen, act } from '@testing-library/react';
import { Desktop } from '../../shell/Desktop';

describe('窗口性能测试', () => {
  test('拖拽窗口 FPS > 55', async () => {
    const { container } = render(<Desktop />);
    
    // 创建窗口
    await act(async () => {
      // 模拟创建窗口
    });
    
    const window = container.querySelector('.window-shell');
    const startTime = performance.now();
    
    // 模拟拖拽（100 次）
    for (let i = 0; i < 100; i++) {
      fireEvent.mouseDown(window.querySelector('.window-header-bar'));
      fireEvent.mouseMove(document, { clientX: 100 + i * 2, clientY: 50 + i });
      fireEvent.mouseUp(document);
    }
    
    const endTime = performance.now();
    const duration = endTime - startTime;
    const fps = 100 / (duration / 1000);
    
    expect(fps).toBeGreaterThan(55);
  });
  
  test('Resize 窗口 FPS > 55', async () => {
    // 类似拖拽测试
  });
  
  test('应用内容不因窗口移动而重渲染', () => {
    let fileManagerRenderCount = 0;
    
    const { container } = render(<Desktop />);
    const window = container.querySelector('.window-shell');
    
    // 模拟拖拽
    fireEvent.mouseDown(window.querySelector('.window-header-bar'));
    fireEvent.mouseMove(document, { clientX: 200, clientY: 150 });
    fireEvent.mouseUp(document);
    
    // 验证 FileManager 渲染次数未增加
    expect(fileManagerRenderCount).toBe(0);
  });
});
```

- [ ] **Step 2: 运行性能测试**

```bash
npm test -- src/tests/performance/window-performance.test.tsx
```

Expected: PASS

- [ ] **Step 3: 提交**

```bash
git add src/tests/performance/window-performance.test.tsx
git commit -m "test(window): 添加性能测试

- 拖拽 FPS > 55
- Resize FPS > 55
- 应用内容重渲染次数验证"
```

---

### Task 8: 功能回归测试

**Files:**
- 无需修改文件，手动测试

- [ ] **Step 1: 窗口操作测试清单**

```markdown
## 窗口操作测试
- [ ] 拖拽窗口移动流畅度（快速拖动无卡顿）
- [ ] 窗口边界限制（不能移出桌面区域）
- [ ] 拖拽结束后位置同步（恢复后位置正确）

## Resize 测试
- [ ] Resize 流畅度（实时跟随鼠标）
- [ ] 最小窗口尺寸限制（不能缩小到 400x300 以下）
- [ ] Resize 边界检查（不能超出桌面区域）
- [ ] Resize 结束后尺寸同步

## 窗口激活测试
- [ ] 点击窗口激活（立即置顶）
- [ ] 多窗口 z-index 正确（活动窗口在最上层）
- [ ] 激活时不影响其他窗口位置

## 窗口最小化/恢复测试
- [ ] 最小化窗口立即隐藏
- [ ] 恢复窗口保持原位置和尺寸
- [ ] Dock 点击恢复最小化窗口

## 应用内容稳定性测试
- [ ] FileManager 文件列表不因窗口移动而重置
- [ ] Terminal 输出内容不因窗口 resize 而清空
- [ ] SystemMonitor 数据不因窗口操作而中断
- [ ] Settings 设置状态不因窗口激活而改变

## 多窗口测试
- [ ] 同时打开 10 个窗口流畅运行
- [ ] 快速切换窗口无延迟
- [ ] 关闭窗口不影响其他窗口
- [ ] localStorage 持久化正确（重启后恢复）
```

- [ ] **Step 2: 手动测试并记录结果**

运行项目：
```bash
npm run dev
```

逐一测试清单项，记录结果。

- [ ] **Step 3: 提交测试报告**

```bash
git add docs/superpowers/test-reports/2026-07-06-window-performance-test-report.md
git commit -m "test(window): 功能回归测试报告

- 所有窗口操作功能正常
- 性能指标达标
- 应用内容稳定"
```

---

## 最终验收

### Task 9: 性能指标验证

**验收标准：**
- ✅ Resize FPS > 55，拖拽 FPS > 55
- ✅ 窗口激活延迟 < 50ms
- ✅ 应用内容无闪烁/重渲染
- ✅ 10+ 窗口流畅运行
- ✅ 所有功能测试通过
- ✅ 错误处理机制有效

- [ ] **Step 1: 使用 Chrome Performance Monitor 测试**

1. 打开 Chrome DevTools → Performance Monitor
2. 拖动窗口，观察 FPS 和 CPU 占用
3. Resize 窗口，观察 FPS 和 CPU 占用
4. 记录结果

Expected:
- FPS: 55-60
- CPU: < 10%

- [ ] **Step 2: 使用 React DevTools 验证渲染次数**

1. 打开 React DevTools → Profiler
2. 拖动窗口，录制渲染次数
3. 验证 FileManager/Terminal 等应用内容不重渲染

Expected:
- Desktop 渲染次数：1（创建时）
- WindowShell 渲染次数：少量（拖拽结束时）
- 应用内容渲染次数：0（窗口操作时）

- [ ] **Step 3: 最终提交**

```bash
git add docs/superpowers/test-reports/2026-07-06-performance-metrics-report.md
git commit -m "feat(window): 窗口性能优化完成

- Resize FPS: 55-60
- 拖拽 FPS: 55-60
- CPU 占用: < 10%
- 应用内容稳定无闪烁
- 所有功能测试通过"
```

---

## 实施完成标记

- [ ] **Phase 1 完成**：核心架构调整（Day 1-3）
- [ ] **Phase 2 完成**：事件处理优化（Day 4-5）
- [ ] **Phase 3 完成**：测试与验证（Day 6-7）
- [ ] **所有验收标准达成**
- [ ] **Git 标签创建**：`v1.0-window-performance-optimized`

---

**计划状态**：Draft → Pending Execution → Completed

**下一步**：选择执行方式（Subagent-Driven 或 Inline Execution）