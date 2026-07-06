# 窗口性能优化设计文档

> 版本: v1.0 | 日期: 2026-07-06
> 状态: Draft
> 作者: Assistant

---

## 一、背景与目标

### 1.1 问题分析

当前项目窗口操作（移动、Resize、激活、最小化）存在明显卡顿，主要问题：

| 问题分类 | 严重度 | 具体表现 |
|---------|--------|---------|
| **P0（极高）** | 全局重渲染机制 + Resize 缺少优化 | 每次窗口事件触发全树重渲染，Resize 每帧 60+ 次 state 更新 |
| **P1（高）** | 状态同步连锁反应 + 缺少组件 memoization | 拖拽/Resize 触发 save + 重渲染，所有应用内容随窗口移动重渲染 |

详细分析见：[窗口操作卡顿原因分析总结](#附录a窗口操作卡顿原因分析总结)

### 1.2 优化目标

**主要目标**：
- 窗口 Resize 流畅度：从 FPS 20-30 → FPS 55-60
- 窗口拖拽流畅度：保持现有 FPS 40-50，提升至 FPS 55-60
- 整体响应速度：窗口激活、最小化延迟 < 50ms
- 应用内容稳定性：窗口操作时应用内容无闪烁/重渲染

**次要目标**：
- 降低 CPU 占用：窗口操作时 CPU < 10%
- 支持更多窗口：10+ 窗口流畅运行
- localStorage 写入优化：避免频繁阻塞 UI

---

## 二、设计原则

### 2.1 核心原则

参考桌面系统窗口管理架构（Windows DWM、macOS Quartz、GNOME Mutter），采用 **模拟合成器架构（轻量级）**：

1. **状态分离原则**：窗口框架状态与应用内容状态完全分离
2. **局部优化原则**：窗口位置/大小变化只影响窗口本身，不触发父组件重渲染
3. **事件精细化原则**：只广播关键事件，避免全局重渲染
4. **性能优先原则**：使用 requestAnimationFrame + ref 优化，优先保证流畅度

### 2.2 架构约束

- **不引入新依赖**：使用 React 原生能力（useState、useRef、useMemo、React.memo）
- **保持现有功能**：所有窗口操作功能不变，只优化性能
- **向后兼容**：现有 API 和事件订阅机制不变
- **改动可控**：核心改动 5-8 个文件，避免大规模重构

---

## 三、架构设计

### 3.1 模拟窗口合成器架构

```
┌─ Desktop（全局容器）─────────────────────────────┐
│                                                    │
│  ┌─ WindowManager（合成器）───────────────────┐   │
│  │  管理窗口框架状态：                          │   │
│  │  - 窗口列表（只含 ID、appId、激活状态）      │   │
│  │  - z-index 计算                             │   │
│  │  - 事件总线（只广播关键事件）                │   │
│  │                                              │   │
│  │  ❌ 不管理：位置、大小、最小化状态           │   │
│  └─────────────────────────────────────────────┘   │
│                                                    │
│  ┌─ WindowShell（窗口框架）────────────────────┐  │
│  │  完全独立管理窗口状态：                      │  │
│  │  - position（位置）- useState                │  │
│  │  - size（大小）- useState                    │  │
│  │  - minimized（最小化）- useState             │  │
│  │  - 拖拽/resize 状态 - useRef                 │  │
│  │                                              │  │
│  │  ✅ 位置/大小变化不触发父组件重渲染          │  │
│  │  ✅ 使用 requestAnimationFrame 优化          │  │
│  │                                              │  │
│  │  ┌─ AppContent（应用内容）──────────────┐   │  │
│  │  │  React.memo 包裹                        │   │  │
│  │  │  独立管理应用状态和数据                  │   │  │
│  │  │  不受窗口框架状态变化影响                │   │  │
│  │  └─────────────────────────────────────────┘   │  │
│  └─────────────────────────────────────────────┘   │
│                                                    │
└────────────────────────────────────────────────────┘
```

### 3.2 状态分离策略

**WindowManager（合成器）状态**：
- `globalState`: `{ windowList: [{id, appId, isMinimized}], activeWindowId }`
- 不包含位置/大小数据（精简状态）
- 只监听 5 种关键事件（减少触发频率）

**WindowShell（窗口框架）状态**：
- `localPosition`: `{x, y}`（完全独立）
- `localSize`: `{w, h}`（完全独立）
- `localMinimized`: `boolean`（完全独立）
- 拖拽/Resize 状态使用 ref，不触发重渲染

**AppContent（应用内容）状态**：
- 完全独立，不受父组件影响
- 使用 React.memo 包裹

---

## 四、组件设计

### 4.1 WindowManagerContext 重构

**核心改动**：选择性事件监听 + 状态精简

```tsx
// WindowManagerContext.tsx
export function WindowManagerProvider({ children }) {
  const manager = useMemo(() => new WindowManager(registry), []);

  // ✅ 只存储全局关键状态
  const [globalState, setGlobalState] = useState({
    windowList: [],          // 只含 ID、appId、isMinimized
    activeWindowId: null,
  });

  // ✅ 只监听关键事件
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

  return (
    <WindowManagerContext.Provider value={{
      manager,
      globalState
    }}>
      {children}
    </WindowManagerContext.Provider>
  );
}
```

**关键点**：
- ❌ 位置/大小变化不再触发 `setGlobalState`
- ✅ 窗口列表不含位置/大小数据（精简状态）
- ✅ 只监听 5 种关键事件（减少触发频率）

### 4.2 WindowShell 完全独立状态管理

**核心改动**：位置/大小由 WindowShell 自己管理，不再依赖 props

```tsx
// WindowShell.tsx
export function WindowShell({ windowId, appId, isActive, mode, onClose, onMinimize, onMaximize, onFocus, children }) {
  const manager = useWindowManager();
  const windowObj = manager.getById(windowId);

  // ✅ 初始化本地状态
  const [localPosition, setLocalPosition] = useState(windowObj?.position || { x: 100, y: 100 });
  const [localSize, setLocalSize] = useState(windowObj?.size || { width: 800, height: 600 });
  const [localMinimized, setLocalMinimized] = useState(windowObj?.minimized || false);

  // ✅ Resize 优化（新增，参考拖拽实现）
  const [isResizing, setIsResizing] = useState(false);
  const resizeDataRef = useRef({ ...localPosition, ...localSize });
  const rafIdRef = useRef(null);

  useEffect(() => {
    if (!isResizing) return;

    const handleMouseMove = (e) => {
      const deltaX = e.clientX - resizeStartPos.current.x;
      const deltaY = e.clientY - resizeStartPos.current.y;

      // 计算新尺寸和位置
      const newWidth = ...;
      const newHeight = ...;
      const newX = ...;
      const newY = ...;

      // ✅ 存储到 ref，不触发 state 更新
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

      // ✅ Resize 结束时，更新本地 state
      setLocalPosition({ x: resizeDataRef.current.x, y: resizeDataRef.current.y });
      setLocalSize({
        width: resizeDataRef.current.width,
        height: resizeDataRef.current.height
      });

      // ✅ 同步到 WindowManager（但触发节流）
      syncToManager(resizeDataRef.current);
    };

    document.addEventListener('mousemove', handleMouseMove, { passive: true });
    document.addEventListener('mouseup', handleMouseUp);

    return () => {
      document.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('mouseup', handleMouseUp);
      if (rafIdRef.current !== null) {
        cancelAnimationFrame(rafIdRef.current);
      }
    };
  }, [isResizing]);

  // ✅ 状态同步节流函数
  const syncToManager = useMemo(() => {
    let lastSyncTime = 0;
    const THROTTLE_MS = 500;

    return (data) => {
      const now = Date.now();
      if (now - lastSyncTime > THROTTLE_MS) {
        lastSyncTime = now;

        const windowObj = manager.getById(windowId);
        if (windowObj) {
          windowObj.setPosition(data);
          windowObj.setSize({ width: data.width, height: data.height });
        }
      }
    };
  }, [manager, windowId]);

  return (
    <div
      style={{
        position: 'absolute',
        width: localSize.width,
        height: localSize.height,
        transform: `translate(${localPosition.x}px, ${localPosition.y}px)`,
        display: localMinimized ? 'none' : 'flex',
      }}
    >
      {children}
    </div>
  );
}
```

**关键点**：
- ✅ 位置/大小完全由 WindowShell 自己管理
- ✅ Resize 使用 requestAnimationFrame 优化（与拖拽相同）
- ✅ 状态同步添加 500ms 节流
- ✅ Resize 事件监听器添加 `passive: true`

### 4.3 Desktop 组件调整

**核心改动**：只订阅全局关键状态，不处理位置/大小

```tsx
// Desktop.tsx
function DesktopContent() {
  const { manager, globalState } = useWindowManager();

  // ✅ 只使用全局关键状态
  const { windowList, activeWindowId } = globalState;

  return (
    <div className="desktop-area">
      {windowList.map((win) => (
        <WindowShell
          key={win.id}
          windowId={win.id}
          appId={win.appId}
          isActive={win.id === activeWindowId}
          isMinimized={win.isMinimized}
          mode="standard"
          onClose={() => manager.close(win.id)}
          onMinimize={() => manager.minimize(win.id)}
          onMaximize={() => {/* TODO */}}
          onFocus={() => manager.focus(win.id)}
        >
          <MemoizedAppContent appId={win.appId} windowId={win.id} />
        </WindowShell>
      ))}
    </div>
  );
}

// ✅ 应用内容组件（React.memo 包裹）
const MemoizedAppContent = React.memo(function AppContent({ appId, windowId }) {
  const manager = useWindowManager();
  const win = manager.getById(windowId);

  switch (appId) {
    case 'files': return <FileManager windowId={windowId} preloadData={win?.preloadData} />;
    case 'terminal': return <TerminalApp windowId={windowId} />;
    case 'monitor': return <SystemMonitor windowId={windowId} />;
    case 'settings': return <Settings windowId={windowId} />;
    default: return <div>Unknown app</div>;
  }
}, (prevProps, nextProps) => {
  return prevProps.appId === nextProps.appId && prevProps.windowId === nextProps.windowId;
});
```

**关键点**：
- ✅ Desktop 只订阅全局关键状态
- ✅ 不传递位置/大小 props（由 WindowShell 自己管理）
- ✅ 应用内容使用 React.memo，不受窗口框架变化影响

### 4.4 CSS z-index 简化

**核心改动**：使用固定 z-index 规则，避免动态计算

```css
/* Desktop.css */
.window-shell {
  z-index: 10;
}

.window-shell.active {
  z-index: 90 !important;
}

.window-shell.minimized {
  display: none;
}
```

**关键点**：
- ✅ 删除 JavaScript 动态计算 z-index
- ✅ 使用 CSS 固定规则（性能更好）

---

## 五、数据流与事件处理设计

### 5.1 状态同步流程优化

**优化前**：
```
用户拖动窗口 → WindowShell 更新 → onPositionChange → 
Window.setPosition() → manager.save() → manager.onAny() → 
Desktop forceUpdate() → 全局重渲染
```

**优化后**：
```
用户拖动窗口 → WindowShell 内部 ref 更新 → 
requestAnimationFrame 视觉更新（GPU 加速）→ 
拖拽结束 → setLocalPosition()（WindowShell 内部）→ 
500ms 节流 → syncToManager() → 
manager.save()（后台执行）→ 
❌ 不触发 onAny()（位置变化不广播）
```

### 5.2 事件总线精细化

**核心改动**：事件分类，只广播必要事件

```tsx
// WindowEventBus.ts（新增）
export class WindowEventBus {
  private globalListeners: Map<WindowEventType, Set<Function>> = new Map();
  private criticalEvents = new Set([
    'window:created',
    'window:closed',
    'window:focused',
    'window:minimized',
    'window:restored'
  ]);

  emit(event: WindowEvent) {
    // ✅ 只广播关键事件到全局监听器
    if (this.criticalEvents.has(event.type)) {
      const listeners = this.globalListeners.get(event.type);
      if (listeners) {
        listeners.forEach(handler => handler(event));
      }

      this.onAnyListeners.forEach(handler => handler(event));
    }
  }
}
```

### 5.3 窗口激活逻辑优化

**核心改动**：z-index 由 CSS 控制，不触发重新计算

```tsx
// WindowManager.ts
focus(windowId: string): void {
  const window = this.windows.get(windowId);
  if (!window) return;

  window.activate();
  this.activeWindowId = windowId;

  this.eventBus.emit({
    type: 'window:focused',
    windowId,
    timestamp: Date.now(),
  });
}

// Desktop.tsx
{windowList.map((win) => (
  <WindowShell
    isActive={win.id === activeWindowId}  // ✅ CSS 会自动处理 z-index
  />
))}
```

### 5.4 save() 优化：后台异步执行

**核心改动**：延迟 1 秒后执行，避免频繁写入

```tsx
// WindowManager.ts
private saveTimer: NodeJS.Timeout | null = null;

save(): void {
  if (this.saveTimer) {
    clearTimeout(this.saveTimer);
  }

  this.saveTimer = setTimeout(() => {
    const data = this.windows.getAll().map(w => w.serialize());
    localStorage.setItem('gnome-remote-windows', JSON.stringify(data));
    this.saveTimer = null;
  }, 1000);
}
```

### 5.5 窗口最小化/恢复优化

**核心改动**：最小化状态由 WindowShell 自己管理

```tsx
// WindowShell.tsx
<div
  style={{
    display: localMinimized ? 'none' : 'flex',
  }}
>

const handleRestore = useCallback(() => {
  setLocalMinimized(false);

  const windowObj = manager.getById(windowId);
  if (windowObj) {
    windowObj.setMinimized(false);
  }
}, [manager, windowId]);
```

---

## 六、错误处理机制

### 6.1 状态初始化失败

```tsx
// WindowShell.tsx
export function WindowShell({ windowId, appId, ... }) {
  const manager = useWindowManager();
  const windowObj = manager.getById(windowId);

  if (!windowObj) {
    console.error(`[WindowShell] Window ${windowId} not found`);
    return null;
  }

  const [localPosition, setLocalPosition] = useState(
    windowObj.position || { x: 100, y: 100 }
  );
}
```

### 6.2 状态同步失败

```tsx
const syncToManager = useMemo(() => {
  return (data) => {
    try {
      const windowObj = manager.getById(windowId);
      if (windowObj && data.x >= 0 && data.y >= 0) {
        windowObj.setPosition(data);
        windowObj.setSize({ width: data.width, height: data.height });
      }
    } catch (error) {
      console.error('[WindowShell] Sync failed:', error);
    }
  };
}, [manager, windowId]);
```

### 6.3 localStorage 保存失败

```tsx
// WindowManager.ts
save(): void {
  this.saveTimer = setTimeout(() => {
    try {
      const data = this.windows.getAll().map(w => w.serialize());
      localStorage.setItem('gnome-remote-windows', JSON.stringify(data));
    } catch (error) {
      console.error('[WindowManager] Save failed:', error);
      try {
        localStorage.removeItem('gnome-remote-windows');
        localStorage.setItem('gnome-remote-windows', JSON.stringify(data));
      } catch (retryError) {
        console.error('[WindowManager] Retry failed:', retryError);
      }
    }
  }, 1000);
}
```

### 6.4 窗口边界计算错误

```tsx
const handleMouseMove = (e: MouseEvent) => {
  try {
    const parentEl = windowRef.current?.parentElement;
    const parentWidth = parentEl?.clientWidth || window.innerWidth;
    const parentHeight = parentEl?.clientHeight || window.innerHeight;

    const newWidth = Math.max(minWidth, Math.min(maxWidth, calculatedWidth));
    const newHeight = Math.max(minHeight, Math.min(maxHeight, calculatedHeight));

    resizeDataRef.current = { x: newX, y: newY, width: newWidth, height: newHeight };
  } catch (error) {
    console.error('[WindowShell] Resize error:', error);
  }
};
```

---

## 七、测试设计

### 7.1 性能测试指标

| 测试场景 | 优化前指标 | 优化后目标 | 测试方法 |
|---------|-----------|-----------|---------|
| 窗口拖拽移动 | FPS 40-50，CPU 15% | FPS 55-60，CPU < 10% | Chrome Performance Monitor |
| 窗口 Resize | FPS 20-30，CPU 25% | FPS 55-60，CPU < 10% | Chrome Performance Monitor |
| 窗口激活切换 | 延迟 100-200ms | 延迟 < 50ms | Performance API |
| 应用内容稳定性 | 移动时闪烁 | 完全无闪烁 | React DevTools |
| 多窗口性能 | 5 个窗口时卡顿 | 10+ 窗口流畅 | 压力测试 |

### 7.2 功能测试清单

```markdown
## 窗口操作测试
- [ ] 拖拽窗口移动流畅度
- [ ] 窗口边界限制
- [ ] 拖拽结束后位置同步

## Resize 测试
- [ ] Resize 流畅度
- [ ] 最小窗口尺寸限制
- [ ] Resize 边界检查
- [ ] Resize 结束后尺寸同步

## 窗口激活测试
- [ ] 点击窗口激活
- [ ] 多窗口 z-index 正确
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
- [ ] localStorage 持久化正确

## 错误处理测试
- [ ] 窗口对象不存在时显示错误提示
- [ ] localStorage 保存失败不影响窗口运行
- [ ] 边界计算错误时使用安全默认值
```

### 7.3 自动化测试代码

```tsx
// __tests__/window-performance.test.tsx
describe('Window Performance Tests', () => {
  test('拖拽窗口 FPS > 55', async () => {
    const { container } = render(<Desktop />);
    const window = container.querySelector('.window-shell');

    const dragEvents = generateDragEvents(100, 50, 200, 150);
    const startTime = performance.now();

    dragEvents.forEach((event, i) => {
      fireEvent.mouseMove(window, event);
      if (i % 10 === 0) {
        const fps = 1000 / (performance.now() - startTime) * i;
        expect(fps).toBeGreaterThan(55);
      }
    });
  });

  test('Resize 窗口 FPS > 55', async () => {
    // 类似拖拽测试
  });

  test('应用内容不因窗口移动而重渲染', () => {
    const renderCounts = { fileManager: 0, terminal: 0 };

    const { container } = render(<Desktop />);
    const window = container.querySelector('.window-shell');

    fireEvent.mouseDown(window.querySelector('.window-header-bar'));
    fireEvent.mouseMove(document, { clientX: 200, clientY: 150 });
    fireEvent.mouseUp(document);

    expect(renderCounts.fileManager).toBe(0);
    expect(renderCounts.terminal).toBe(0);
  });
});
```

---

## 八、实施计划

### 8.1 文件修改清单

| 文件 | 修改类型 | 主要改动 |
|------|---------|---------|
| `WindowManagerContext.tsx` | 重构 | 选择性事件监听 + 状态精简 |
| `WindowShell.tsx` | 重构 | 完全独立状态管理 + Resize 优化 |
| `Desktop.tsx` | 重构 | 只订阅全局状态 + 应用内容 React.memo |
| `WindowManager.ts` | 优化 | save() 异步延迟 + focus() 精简 |
| `WindowEventBus.ts` | 新增 | 事件分类广播 |
| `Desktop.css` | 简化 | z-index 固定规则 |

### 8.2 实施步骤

**Phase 1：核心架构调整（3天）**
- Day 1: WindowManagerContext + WindowShell 状态分离
- Day 2: Resize requestAnimationFrame 优化 + 状态同步节流
- Day 3: Desktop 组件调整 + 应用内容 React.memo

**Phase 2：事件处理优化（2天）**
- Day 4: WindowEventBus 精细化 + z-index CSS 规则
- Day 5: save() 异步延迟 + 最小化状态独立管理

**Phase 3：测试与验证（2天）**
- Day 6: 性能测试 + 功能回归测试
- Day 7: 错误处理测试 + 边缘场景验证

**总计：7天**

### 8.3 验收标准

- ✅ Resize FPS > 55，拖拽 FPS > 55
- ✅ 窗口激活延迟 < 50ms
- ✅ 应用内容无闪烁/重渲染
- ✅ 10+ 窗口流畅运行
- ✅ 所有功能测试通过
- ✅ 错误处理机制有效

---

## 九、风险与应对

### 9.1 潜在风险

| 风险 | 影响 | 应对策略 |
|------|------|---------|
| 状态分离导致数据不一致 | 窗口位置/大小同步失败 | 添加节流机制 + 错误处理 |
| React.memo 失效 | 应用内容仍重渲染 | 严格相等性检查 + DevTools 监控 |
| localStorage 异步保存丢失 | 重启后窗口状态丢失 | 添加重试机制 + 错误日志 |
| Resize 优化导致视觉延迟 | 用户感知延迟感 | requestAnimationFrame 60fps 保证流畅 |

### 9.2 回滚策略

- **Git 标签**：优化前打 tag `v1.0-before-optimization`
- **渐进式发布**：先部署到测试环境，验证后发布到生产
- **监控指标**：FPS、CPU 占用、错误日志实时监控
- **紧急回滚**：发现问题立即回滚到优化前版本

---

## 十、附录

### 附录 A：窗口操作卡顿原因分析总结

#### A.1 核心性能瓶颈

**1. 全局重渲染机制过度触发**

位置：[WindowManagerContext.tsx](file:///e:/MyWork/gnome-remote/src/window-system/WindowManagerContext.tsx#L64-L78)

问题：
- `onAny()` 监听所有窗口事件
- 每次任何窗口事件都会触发 Desktop 整个组件树重渲染
- 移动/resize 窗口时频繁触发 → 连续强制重渲染

**2. Resize 操作缺少性能优化**

位置：[WindowShell.tsx](file:///e:/MyWork/gnome-remote/src/components/window-shell/WindowShell.tsx#L222-L294)

对比：
- ✅ 拖拽已优化：使用 ref + requestAnimationFrame
- ❌ Resize 未优化：直接更新 state

**3. 状态同步引发连锁重渲染**

位置：[Desktop.tsx](file:///e:/MyWork/gnome-remote/src/shell/Desktop.tsx#L272-L285)

连锁反应链：
```
用户拖动窗口 → WindowShell 内联 style 更新 → 
拖拽结束时调用 onPositionChange → 
Window 对象状态更新 → manager.save() 触发 → 
manager.onAny() 触发 → Desktop forceUpdate() → 
整个 Desktop 组件树重渲染
```

#### A.2 架构设计问题

**4. CSS 性能配置不当**

位置：[window-shell.css](file:///e:/MyWork/gnome-remote/src/components/window-shell/window-shell.css#L28-L30)

问题：`will-change: transform` 全局启用，增加内存占用

**5. 缺少组件级性能优化**

问题：项目中没有使用 `React.memo`、`PureComponent`、`useMemo`

**6. z-index 动态计算开销**

位置：[Desktop.tsx](file:///e:/MyWork/gnome-remote/src/shell/Desktop.tsx#L250-L264)

问题：每次渲染都要遍历所有窗口计算 z-index

#### A.3 事件处理问题

**7. 被动事件监听器配置不一致**

位置：[WindowShell.tsx](file:///e:/MyWork/gnome-remote/src/components/window-shell/WindowShell.tsx#L178)

问题：Resize 的 mousemove 事件监听器没有 `passive: true`

**8. 状态更新频率控制缺失**

问题：resize 没有节流机制，每帧多次 state 更新

#### A.4 内存与渲染问题

**9. 所有窗口始终渲染（包括最小化的）**

位置：[Desktop.tsx](file:///e:/MyWork/gnome-remote/src/shell/Desktop.tsx#L245-L247)

问题：最小化的窗口仍然在渲染，累积 DOM 开销

**10. CSS 过渡动画冲突**

位置：[WindowShell.tsx](file:///e:/MyWork/gnome-remote/src/components/window-shell/WindowShell.tsx#L323-L325)

问题：resize 时 transform 改变 + CSS transition 启用 → 视觉卡顿

#### A.5 性能问题严重程度排序

| 问题 | 严重度 | 影响 | 优先级 |
|------|--------|------|--------|
| 全局重渲染机制 | 🔴 极高 | 每次窗口事件触发整个 Desktop 重渲染 | P0 |
| Resize 缺少优化 | 🔴 极高 | resize 时每帧 60+ 次 state 更新 | P0 |
| 状态同步连锁反应 | 🟠 高 | 拖拽/resize → save → 全局重渲染 | P1 |
| 缺少组件 memoization | 🟠 高 | 所有应用内容随窗口移动重渲染 | P1 |
| CSS will-change 全局启用 | 🟡 中 | 增加内存占用，无实际收益 | P2 |
| Resize 事件监听器未 passive | 🟡 中 | 阻塞主线程 | P2 |
| z-index 动态计算 | 🟢 低 | 每次渲染遍历窗口 | P3 |
| 最小化窗口仍渲染 | 🟢 低 | 累积 DOM 开销 | P3 |

---

## 十一、参考资料

### 11.1 桌面系统窗口管理

- [Windows Desktop Window Manager (DWM)](https://docs.microsoft.com/en-us/windows/win32/dwm/dwm-overview)
- [macOS Quartz Compositor](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/CocoaPerformance/CocoaPerformance.html)
- [GNOME Mutter Compositor](https://gitlab.gnome.org/GNOME/mutter)

### 11.2 React 性能优化

- [React.memo 官方文档](https://reactjs.org/docs/react-api.html#reactmemo)
- [requestAnimationFrame 性能优化](https://developer.mozilla.org/en-US/docs/Web/API/window/requestAnimationFrame)
- [Passive Event Listeners](https://developer.mozilla.org/en-US/docs/Web/API/EventTarget/addEventListener#passive)

### 11.3 项目规范

- [GNOME 远程控制客户端 — 完整方案](file:///e:/MyWork/gnome-remote/.trae/rules/项目规范.md)
- [项目记忆文档](file:///c:/Users/w8067/.trae-cn/memory/projects/-e-MyWork-gnome-remote/project_memory.md)

---

**文档状态**：Draft → Pending Review → Approved → Implementation

**下一步**：用户审查设计文档 → 调用 writing-plans 创建实施计划