# 窗口 Aero Snap 设计(对齐 Windows 11)

> spec 日期: 2026-08-19
> 状态: 待实现
> 关联代码: `src/components/window-shell/WindowShell.tsx`、`src/window-system/core/Window.ts`、`src/shell/Desktop.tsx`

## 目标

三条硬性目标(用户提出):

1. **完整 Aero Snap**: 拖拽窗口到屏幕四边/四角时,识别为对应 snap 区,松手后平滑过渡到目标位置/尺寸。覆盖:
   - 顶部 → 最大化
   - 左/右 → 半屏
   - 下 → 下半屏
   - 四角 → 四分之一屏
2. **预览 + 平滑过渡**: 拖拽到边缘附近时显示半透明预览框;松手后窗口以 CSS transition 平滑滑入 snap 位置。
3. **Windows 11 风格"撕下"还原**: 从最大化状态按下标题栏拖下时,立即还原为最大化前尺寸,窗口跟随光标,可继续拖到任意位置;半屏/四分之一 snap 同理。

## 背景

### 现状(已核实代码)

- `WindowShell.tsx` 已有 `isMaximized` / `onMaximize` 支持;双击标题栏、绿点按钮触发最大化,且最大化时禁用拖拽/resize。这是 **Adwaita 风格**,与 Windows 不同。
- `Window.ts` 已有 `_preMaximizeState` 保存最大化前位置/尺寸,`setMaximized(false)` 自动还原。
- `Desktop.tsx` 中 `maxWindowSize = { width: innerWidth, height: innerHeight - 32 }`、`maxWindowPosition = { x: 0, y: 0 }`,工作区为 TopBar 下方。
- `src/window-system/layout/SnapLayout.ts`、`FreeLayout.ts`、`LayoutEngine.ts` 三个模块存在但 **未被任何代码引用**(Grep 已确认),且其 `top` snap 是半高(不是最大化),与 Windows 行为不符。
- 现有拖拽边界约束允许窗口部分移出屏幕但保留 100px 可见;`minY = 0`(不可越过 TopBar)。

### 与 Windows 11 行为对齐的关键差异

| 行为 | 现状 | Windows 11 | 本设计 |
|---|---|---|---|
| 拖到顶边 | 无 | 触发最大化预览 → 松手最大化 | 实现 |
| 拖到左/右/下边 | 无 | 半屏预览 → 松手半屏 | 实现 |
| 拖到四角 | 无 | 1/4 屏预览 → 松手 1/4 屏 | 实现 |
| 最大化状态拖下标题栏 | 禁用拖拽 | 立即还原 + 跟随光标 | 实现 |
| 半屏状态拖出 | N/A | 立即还原 + 跟随光标 | 实现 |
| 拖拽中的视觉反馈 | 无 | 半透明预览矩形 | 实现 |

## 架构

```
┌──────────────────────────────────────────────────┐
│ 渲染层: WindowShell.tsx                          │
│  - mousemove 调 detectSnapZone → setSnapPreview  │
│  - mouseup 调 computeSnapRect → onSnap/onMaximize│
│  - dragstart 检测 isMaximized/snapZone → 立即还原│
│  - <SnapPreview> Portal 到 body                  │
├──────────────────────────────────────────────────┤
│ 几何层: aeroSnap.ts (纯函数, 零依赖)             │
│  - detectSnapZone(cursorX, cursorY, container)   │
│  - computeSnapRect(zone, container)             │
│  - isMaximizeZone(zone)                         │
├──────────────────────────────────────────────────┤
│ 状态层: Window.ts / WindowManager.ts             │
│  - Window._snapZone / _preSnapState             │
│  - WindowManager.snap(id, zone, rect) / unsnap │
└──────────────────────────────────────────────────┘
```

**maximize 与 snap 互斥**: 同一时刻窗口只可能处于"普通 / 最大化 / 半屏-四分之一 snap"三态之一。`top` zone 复用现有 maximize 通道(`_preMaximizeState`),其余 zone 走新 snap 通道(`_preSnapState`)。两个状态字段独立保存,不互相覆盖。

## 文件改动

| 文件 | 操作 | 内容 |
|---|---|---|
| `src/components/window-shell/aeroSnap.ts` | 新增 | 纯函数 + 类型 `SnapZone` |
| `src/components/window-shell/WindowShell.tsx` | 修改 | 拖拽中边缘检测、预览状态、mouseup 平滑 snap、dragstart 从已 snap 状态还原 |
| `src/components/window-shell/WindowShell.module.css` | 修改 | 新增 `.snapPreview` 样式 |
| `src/components/window-shell/index.ts` | 修改 | 导出 aeroSnap API |
| `src/window-system/core/Window.ts` | 修改 | 新增 `_snapZone` / `_preSnapState` 及 `setSnap` / `unsetSnap` |
| `src/window-system/core/WindowManager.ts` | 修改 | 新增 `snap` / `unsnap` 方法 + 事件 |
| `src/window-system/types.ts` | 修改 | 新增 `window:snapped` / `window:unsnapped` 事件类型 |
| `src/shell/Desktop.tsx` | 修改 | 把 `preMaximizeState` / `preSnapState` / `snapZone` 作 prop 注入;提供 `onSnap` / `onUnsnap` 回调 |
| `src/styles/variables.css` | 修改 | 新增 `--z-snap-preview: 100;` |
| `src/window-system/layout/SnapLayout.ts` | 删除 | 未引用死代码,被本方案取代 |
| `src/window-system/layout/FreeLayout.ts` | 删除 | 同上 |
| `src/window-system/layout/LayoutEngine.ts` | 删除 | 同上 |

## 详细设计

### 1. `aeroSnap.ts` 几何 API

```ts
export type SnapZone =
  | 'top' | 'left' | 'right' | 'bottom'
  | 'top-left' | 'top-right' | 'bottom-left' | 'bottom-right';

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

// 距屏幕边缘多少 px 触发边吸附(Windows 11 实测约 8px)
export const SNAP_EDGE_THRESHOLD = 8;
// 屏幕四角多大正方形区域触发四分之一吸附
export const SNAP_CORNER_SIZE = 8;

/**
 * 由鼠标在容器(workspace)内的坐标判断 snap 区。
 * 四角优先于边: cursorX≤8 && cursorY≤8 → 'top-left',不再走"左半屏"。
 */
export function detectSnapZone(
  cursorX: number,
  cursorY: number,
  containerSize: { width: number; height: number },
): SnapZone | null {
  const W = containerSize.width;
  const H = containerSize.height;

  // 四角优先
  if (cursorX <= SNAP_CORNER_SIZE && cursorY <= SNAP_CORNER_SIZE) return 'top-left';
  if (cursorX >= W - SNAP_CORNER_SIZE && cursorY <= SNAP_CORNER_SIZE) return 'top-right';
  if (cursorX <= SNAP_CORNER_SIZE && cursorY >= H - SNAP_CORNER_SIZE) return 'bottom-left';
  if (cursorX >= W - SNAP_CORNER_SIZE && cursorY >= H - SNAP_CORNER_SIZE) return 'bottom-right';

  // 四边
  if (cursorY <= SNAP_EDGE_THRESHOLD) return 'top';
  if (cursorX <= SNAP_EDGE_THRESHOLD) return 'left';
  if (cursorX >= W - SNAP_EDGE_THRESHOLD) return 'right';
  if (cursorY >= H - SNAP_EDGE_THRESHOLD) return 'bottom';

  return null;
}

/** 由 snap 区算出最终目标矩形。'top' = 全屏 = maximize。 */
export function computeSnapRect(
  zone: SnapZone,
  containerSize: { width: number; height: number },
): Rect {
  const W = containerSize.width;
  const H = containerSize.height;
  const halfW = W / 2;
  const halfH = H / 2;

  switch (zone) {
    case 'top':         return { x: 0,      y: 0,      width: W,     height: H };
    case 'left':        return { x: 0,      y: 0,      width: halfW, height: H };
    case 'right':       return { x: halfW,  y: 0,      width: halfW, height: H };
    case 'bottom':      return { x: 0,      y: halfH,  width: W,     height: halfH };
    case 'top-left':    return { x: 0,      y: 0,      width: halfW, height: halfH };
    case 'top-right':   return { x: halfW,  y: 0,      width: halfW, height: halfH };
    case 'bottom-left': return { x: 0,      y: halfH,  width: halfW, height: halfH };
    case 'bottom-right':return { x: halfW,  y: halfH,  width: halfW, height: halfH };
  }
}

/** 'top' zone 走 maximize 通道;其余走 snap 通道。 */
export function isMaximizeZone(zone: SnapZone | null): boolean {
  return zone === 'top';
}
```

### 2. `Window.ts` 状态模型

复用现有 maximize 模式,新增独立的 snap 状态字段。**maximize 与 snap 互斥**,由 `WindowShell.dragstart` 与 `mouseup` 保证原子切换(下文 § 4 详述)。

```ts
// 新增字段(runtime state, 不持久化)
private _snapZone: SnapZone | null = null;
private _preSnapState: { position: { x: number; y: number }; size: { width: number; height: number } } | null = null;

get snapZone(): SnapZone | null { return this._snapZone; }
get preSnapState(): { position: { x: number; y: number }; size: { width: number; height: number } } | null {
  return this._preSnapState;
}

/**
 * 进入半屏/四分之一屏 snap。
 * 首次 snap 时保存当前位置/尺寸;已 snap 状态再换 zone 不覆盖 preSnapState。
 */
setSnap(zone: SnapZone, rect: Rect): void {
  if (this._snapZone === null) {
    this._preSnapState = {
      position: { ...this._position },
      size: { ...this._size },
    };
  }
  this._snapZone = zone;
  this._position = { x: rect.x, y: rect.y };
  this._size = { width: rect.width, height: rect.height };
  this._updatedAt = Date.now();
}

/** 退出 snap,恢复保存的位置/尺寸。 */
unsetSnap(): void {
  if (this._preSnapState) {
    this._position = { ...this._preSnapState.position };
    this._size = { ...this._preSnapState.size };
    this._preSnapState = null;
  }
  this._snapZone = null;
  this._updatedAt = Date.now();
}
```

**deserialize**: `_snapZone = null`、`_preSnapState = null`(同 maximize,运行时状态不持久化)。

### 3. `WindowManager.ts` 方法

```ts
/** 进入半屏/四分之一 snap(非 maximize)。 */
snap(windowId: string, zone: SnapZone, rect: Rect): void {
  const window = this.windows.get(windowId);
  if (!window) return;
  window.setSnap(zone, rect);
  this.eventBus.emit({
    type: 'window:snapped',
    windowId,
    timestamp: Date.now(),
  });
}

/** 退出 snap。 */
unsnap(windowId: string): void {
  const window = this.windows.get(windowId);
  if (!window) return;
  window.unsetSnap();
  this.eventBus.emit({
    type: 'window:unsnapped',
    windowId,
    timestamp: Date.now(),
  });
}
```

`WindowEventType` 新增 `'window:snapped'` / `'window:unsnapped'`。

### 4. `WindowShell.tsx` 拖拽核心改动(最复杂)

#### 4.1 新增 props

```ts
// 由 Desktop 提供(从 Window 实例读取并注入)
preMaximizeState?: { position: { x: number; y: number }; size: { width: number; height: number } } | null;
preSnapState?:     { position: { x: number; y: number }; size: { width: number; height: number } } | null;
snapZone?: SnapZone | null;
onSnap?:   (zone: SnapZone, rect: Rect) => void;
onUnsnap?: () => void;
// onMaximize 已存在,内部调 manager.maximize / unmaximize
```

> **为什么 WindowShell 需要 `preMaximizeState` / `preSnapState` 作 prop?**
> WindowShell 不 import `Window` 类(保持边界),但 dragstart 时需要同步读取还原尺寸来避免 React 异步渲染导致的首帧跳变。Desktop 从 `win.preMaximizeState` / `win.preSnapState` 读取并作为 prop 注入,数据流单向。

#### 4.2 新增内部状态

```ts
const [snapPreview, setSnapPreview] = useState<Rect | null>(null);  // 当前预览矩形
const snapZoneRef  = useRef<SnapZone | null>(null);                  // mouseup 时读取
const containerRef = useRef<{ width: number; height: number }>({ width: 0, height: 0 }); // workspace 尺寸
```

#### 4.3 mousemove 中检测 zone(在现有 `boundedX/Y` 计算之后插入)

```ts
const handleMouseMove = (e: MouseEvent) => {
  // ... 现有 boundedX/Y 计算 ...

  // Aero Snap 边缘检测
  const parentEl = windowRef.current?.parentElement;
  if (parentEl) {
    containerRef.current = { width: parentEl.clientWidth, height: parentEl.clientHeight };
    // 鼠标在容器内的坐标 = clientX/Y - 容器相对视口偏移
    const rect = parentEl.getBoundingClientRect();
    const cursorX = e.clientX - rect.left;
    const cursorY = e.clientY - rect.top;
    const zone = detectSnapZone(cursorX, cursorY, containerRef.current);
    snapZoneRef.current = zone;
    const newPreview = zone ? computeSnapRect(zone, containerRef.current) : null;
    // 仅在变化时 setState,避免每帧重渲染
    setSnapPreview(prev => {
      if (prev === null && newPreview === null) return prev;
      if (prev && newPreview && prev.x === newPreview.x && prev.y === newPreview.y
          && prev.width === newPreview.width && prev.height === newPreview.height) return prev;
      return newPreview;
    });
  }
};
```

#### 4.4 mouseup 应用 snap(在现有 `setPosition` 之前插入)

```ts
const handleMouseUp = () => {
  // 取消 rafId(现有代码)

  if (snapZoneRef.current) {
    const zone = snapZoneRef.current;
    const rect = computeSnapRect(zone, containerRef.current);

    // 1. 隐藏预览
    setSnapPreview(null);
    snapZoneRef.current = null;

    // 2. 同步 ref 到目标 rect(避免 transition 完成前 ref 残留旧值)
    windowStateRef.current = {
      x: rect.x, y: rect.y,
      width: rect.width, height: rect.height,
    };

    // 3. 走对应通道
    if (isMaximizeZone(zone)) {
      onMaximize();  // manager.maximize,Window 内部走 _preMaximizeState
    } else {
      onSnap?.(zone, rect);
    }

    // 4. 恢复 transition,让浏览器跑平滑动画
    if (windowRef.current) {
      windowRef.current.style.transition =
        'transform 300ms cubic-bezier(0.25, 0, 0, 1), ' +
        'width 300ms cubic-bezier(0.25, 0, 0, 1), ' +
        'height 300ms cubic-bezier(0.25, 0, 0, 1)';
    }

    setIsDragging(false);
    return;
  }

  // 普通拖拽结束逻辑(现有代码,不动)
  // ...
};
```

#### 4.5 dragstart 从已 snap 状态还原(Windows 11 "撕下"行为)

现有代码:`if (isMaximized) return;` —— 直接禁用拖拽。**改为**按下即还原:

```ts
const handleDragStart = useCallback((e: React.MouseEvent) => {
  if (mode === 'frameless') return;

  e.preventDefault();
  e.stopPropagation();
  onFocus();

  // ── 情况 A:从最大化拖下 ──────────────────────
  if (isMaximized && preMaximizeState) {
    const ratioX = (e.clientX - position.x) / size.width;  // 光标在最大化标题栏上的比例
    const restored = preMaximizeState;
    const restoredX = e.clientX - ratioX * restored.size.width;
    const restoredY = 0;

    // 同步 ref,避免 React 异步渲染导致首帧跳变
    windowStateRef.current = {
      x: restoredX, y: restoredY,
      width: restored.size.width, height: restored.size.height,
    };
    dragStartPos.current = { x: e.clientX - restoredX, y: e.clientY - restoredY };

    // 直接刷 DOM,立刻看见窗口缩小
    if (windowRef.current) {
      windowRef.current.style.width = `${restored.size.width}px`;
      windowRef.current.style.height = `${restored.size.height}px`;
      windowRef.current.style.transform = `translate(${restoredX}px, ${restoredY}px) scale(1)`;
      windowRef.current.style.transition = 'none';  // 撕下瞬间不要过渡
    }

    // 调 onMaximize:Desktop 的 onMaximize 是 toggle,
    //   - isMaximized=true 时 → manager.unmaximize() → Window.setMaximized(false) 同步还原 _position/_size 并清空 _preMaximizeState
    //   - isMaximized=false 时 → manager.maximize() (本场景不会走到)
    // 当前 isMaximized=true,因此触发 unmaximize
    onMaximize();
    // ⚠️ 关键顺序:onMaximize 内部会把 Window._position/_size 还原为 preMaximizeState 的值,
    //    我们必须 *之后* 再调 onPositionChange/onSizeChange 覆盖,确保 cursor 跟随位置生效
    onPositionChange?.({ x: restoredX, y: restoredY });
    onSizeChange?.(restored.size);

    setIsDragging(true);
    return;
  }

  // ── 情况 B:从半屏/四分之一 snap 拖出(逻辑对称)──
  if (snapZone && preSnapState) {
    // 已 snap 状态下 position/size 是 snap 后的 rect
    const ratioX = (e.clientX - position.x) / size.width;
    const restored = preSnapState;
    const restoredX = e.clientX - ratioX * restored.size.width;
    const restoredY = 0;

    windowStateRef.current = {
      x: restoredX, y: restoredY,
      width: restored.size.width, height: restored.size.height,
    };
    dragStartPos.current = { x: e.clientX - restoredX, y: e.clientY - restoredY };

    if (windowRef.current) {
      windowRef.current.style.width = `${restored.size.width}px`;
      windowRef.current.style.height = `${restored.size.height}px`;
      windowRef.current.style.transform = `translate(${restoredX}px, ${restoredY}px) scale(1)`;
      windowRef.current.style.transition = 'none';
    }

    // 调 onUnsnap:manager.unsnap → Window.unsetSnap 同步还原 _position/_size 为 preSnapState 并清空 _preSnapState
    onUnsnap?.();
    // ⚠️ 关键顺序:同 case A,onUnsnap 内部会还原 _position/_size,必须 *之后* 调 onPositionChange/onSizeChange 覆盖
    onPositionChange?.({ x: restoredX, y: restoredY });
    onSizeChange?.(restored.size);

    setIsDragging(true);
    return;
  }

  // ── 情况 C:普通拖拽(现有逻辑不变)────────────
  setIsDragging(true);
  const currentState = windowStateRef.current;
  dragStartPos.current = {
    x: e.clientX - currentState.x,
    y: e.clientY - currentState.y,
  };
}, [mode, isMaximized, snapZone, preMaximizeState, preSnapState, position, size, onFocus, onMaximize, onUnsnap, onPositionChange, onSizeChange]);
```

**关键点**:
- 还原比例公式:`restoredX = cursorX - (cursorX - maximizedX) / maximizedWidth * restoredWidth`,保证光标在标题栏上的相对位置不变(Windows 11 行为)。
- `windowStateRef` 同步刷 DOM 是为了避免 React 异步渲染导致 mousemove 第一帧用旧尺寸算位置。
- `transition: 'none'` 在撕下瞬间禁用动画,否则会看到 300ms 的"缩小动画"挂在光标上,跟手差。
- 还原后立即调 `onMaximize` / `onUnsnap` 让 manager 把状态翻为 false,后续拖拽正常进行。

#### 4.6 mousemove 中"已 snap 状态被拖出"的处理

由于 § 4.5 在 dragstart 时已经同步 unsnap + 还原尺寸,后续 mousemove 走普通拖拽逻辑即可,**不需要额外处理**。这是关键设计决策:把"撕下"完全放在 dragstart,而非 mousemove。

#### 4.7 SnapPreview 组件

新增内部小组件,用 React Portal 渲染到 `document.body`,避免被任何父级 `transform` / `overflow: hidden` 影响:

```tsx
import { createPortal } from 'react-dom';

function SnapPreview({ rect }: { rect: Rect | null }) {
  if (!rect) return null;
  return createPortal(
    <div
      className={styles.snapPreview}
      style={{
        left: rect.x,
        top: rect.y,
        width: rect.width,
        height: rect.height,
      }}
    />,
    document.body,
  );
}
```

注意:Portal 到 body 后,`left/top` 是相对视口的坐标,需要把容器(workspace)相对视口的偏移加上:

```ts
// 在 setSnapPreview 前转换坐标
const parentRect = parentEl.getBoundingClientRect();
const viewportRect = {
  x: rect.x + parentRect.left,
  y: rect.y + parentRect.top,
  width: rect.width,
  height: rect.height,
};
```

### 5. `Desktop.tsx` 接线

```tsx
const DesktopWindow = memo(function DesktopWindow({ windowId, appId }) {
  const { manager } = useWindowManager();
  const win = manager.getById(windowId);
  const app = manager.getApp(appId);
  const windowState = useWindowState(windowId);
  if (!win || !app || !windowState) return null;

  const maxWindowSize = { width: window.innerWidth, height: window.innerHeight - 32 };
  const maxWindowPosition = { x: 0, y: 0 };

  return (
    <WindowShell
      // ... 现有 props ...
      snapZone={win.snapZone}
      preMaximizeState={win.preMaximizeState ?? null}
      preSnapState={win.preSnapState ?? null}
      onSnap={(zone, rect) => manager.snap(windowId, zone, rect)}
      onUnsnap={() => manager.unsnap(windowId)}
      // onMaximize 已存在,逻辑不变
    >
      <MemoizedAppContent app={app} windowId={windowId} preloadData={win.preloadData} />
    </WindowShell>
  );
});
```

### 6. CSS 样式

`WindowShell.module.css` 新增:

```css
.snapPreview {
  position: absolute;
  pointer-events: none;            /* 不挡鼠标 */
  background: rgba(0, 120, 212, 0.25);
  border: 1px solid rgba(0, 120, 212, 0.8);
  border-radius: 4px;
  z-index: var(--z-snap-preview);
  transition: all 120ms ease-out;  /* 预览框自身切换 zone 时也有平滑感 */
  box-shadow: 0 0 0 2px rgba(255, 255, 255, 0.4) inset;
}
```

`variables.css` 新增:

```css
--z-snap-preview: 100;  /* 高于 --z-window-active(30)*/
```

### 7. 删除死代码

`src/window-system/layout/SnapLayout.ts` / `FreeLayout.ts` / `LayoutEngine.ts` 三个文件 Grep 确认无引用(除相互 import),全部删除。

## 验证标准

### 单元测试(vitest)

1. **`aeroSnap.test.ts`**:
   - `detectSnapZone` 各 zone 边界值:cursorX=0/Y=0 → `top-left`;cursorX=W-1/Y=0 → `top-right`;cursorX=0/Y=H-1 → `bottom-left`;cursorX=W-1/Y=H-1 → `bottom-right`
   - corner 优先:cursorX=4/Y=4 → `top-left`(不是 `top` 也不是 `left`)
   - 边:cursorX=100/Y=2 → `top`;cursorX=2/Y=100 → `left`;cursorX=W-2/Y=100 → `right`;cursorX=100/Y=H-2 → `bottom`
   - 远离边缘:cursorX=500/Y=500 → `null`
   - `computeSnapRect` 各 zone 矩形数值正确(用 1920x1080 容器作 fixture)
   - `isMaximizeZone('top') === true`,其余 zone === false,null === false

2. **`Window.test.ts`(新增)**:
   - `setSnap('left', rect)` 后 `snapZone === 'left'`,position/size 等于 rect
   - `preSnapState` 保存了 snap 前的 position/size
   - 已 snap 状态再 `setSnap('right', rect2)`:preSnapState **不**被覆盖(仍是首次 snap 前的状态)
   - `unsetSnap()` 后 position/size 还原到 preSnapState,snapZone === null,preSnapState === null
   - `setMaximized(true)` 后 `snapZone` 仍为 null(maximize 不污染 snap 状态)

3. **`WindowManager.test.ts`(扩展)**:
   - `snap(id, 'left', rect)` 触发 `window:snapped` 事件
   - `unsnap(id)` 触发 `window:unsnapped` 事件
   - `maximize` 后再 `snap`:走 snap 通道,maximize 状态保留(由 setMaximized 内部管理)
   - 事件序列正确

### 手测清单

1. **基础 snap**:
   - 拖正常窗口到顶 → 预览出现 → 松手 → 300ms 平滑最大化
   - 拖到左 → 半屏;拖到右 → 半屏;拖到下 → 下半屏
   - 拖到四角 → 1/4 屏
   - 预览框颜色/位置/尺寸正确,不闪烁
2. **撕下还原**:
   - 最大化窗口按下标题栏拖下 → 立即缩小到原尺寸,光标保持在标题栏同比例位置,可继续拖
   - 半屏窗口按下标题栏拖出 → 立即还原,同上
   - 撕下瞬间无 300ms 缩小动画(`transition: none`),跟手
3. **跨 snap**:
   - 左半屏 → 不松手直接拖到顶 → 应能直接走 top/maximize 路径
   - 最大化 → 拖下到左半屏 → 应能直接走 left/snap 路径
   - preSnapState / preMaximizeState 不被错误覆盖
4. **不回归**:
   - 双击标题栏 → 仍触发最大化/还原
   - 绿点按钮 → 仍触发最大化/还原
   - 最大化/snap 状态下 resize handle 仍隐藏
   - 最大化/snap 状态下再拖拽应先还原(走 § 4.5)
   - 普通拖拽边界约束(100px 可见、minY=0)不回归
5. **z-index**:
   - 预览框始终在最上层,不被其他窗口遮挡
   - 预览框不挡鼠标事件(`pointer-events: none`)

## 未涵盖 / 未来工作

- **多显示器/多 workspace**:本设计假定单 workspace;多屏需扩展 `detectSnapZone` 接收每个屏的 rect
- **键盘快捷键**(Win+方向键):不在本次范围
- **Snap Layouts**(Windows 11 的多窗口布局选择器):不在本次范围,本次只做单窗口 snap
- **持久化**:snap 状态不持久化(localStorage 恢复后窗口都是普通态),与 maximize 一致

## 风险与回滚

- **风险 1**: dragstart 同步刷 DOM + 调 onMaximize 的时序:若 React 批量更新导致 position/size prop 在下一帧才更新,可能出现 1 帧的"撕下未跟随"。**缓解**: windowStateRef + 直接刷 DOM 已经覆盖首帧,后续 mousemove 用 ref 算位置,不依赖 prop。
- **风险 2**: Portal 到 body 后坐标系变化(从 workspace 相对变成视口相对)。**缓解**: § 4.7 已说明坐标转换。
- **回滚**: 所有改动可单 PR 回滚;删除的 3 个 layout 文件可从 git 恢复。
