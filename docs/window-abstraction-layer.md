# 窗口抽象层设计方案

> 版本: v1.0 | 日期: 2026-06-29
>
> 目标：像 Windows 11 一样，彻底分离窗口交互层和应用内容层，让后续每个应用开发都不涉及窗口抽象层的设置。

---

## 一、架构分层

### 1.1 三层职责划分

| 层级 | 职责 | 组件 | 样式范围 |
|---|---|---|---|
| **WindowShell (抽象层)** | 窗口交互 | `WindowShell.tsx` | `window-shell.css` (独立) |
| **AppContent (应用层)** | 业务逻辑 | `FileManager.tsx` 等 | `FileManager.css` 等 (独立) |
| **Desktop (桌面层)** | 窗口管理 | `Desktop.tsx` | `Desktop.css` |

### 1.2 职责边界

**WindowShell 负责：**
- ✅ 窗口位置、尺寸、z-index 管理
- ✅ 窗口拖拽、resize 交互
- ✅ HeaderBar + WindowControls (最小化/最大化/关闭)
- ✅ 窗口动画 (打开/关闭/激活)
- ✅ 窗口边框、阴影、圆角

**AppContent 负责：**
- ✅ 应用布局 (侧边栏 + 主区域 + 状态栏)
- ✅ 业务逻辑 (文件管理/终端/监控/设置)
- ✅ 应用内部滚动
- ✅ 应用组件渲染

**Desktop 负责：**
- ✅ 窗口创建/关闭/聚焦
- ✅ z-index 分配
- ✅ 窗口持久化
- ✅ 应用注册表

---

## 二、组件设计

### 2.1 WindowShell 组件

```tsx
// src/components/window-system/WindowShell.tsx

import { WindowControls } from './WindowControls';
import './window-shell.css';

interface WindowShellProps {
  // 窗口属性 (由 Desktop 提供)
  windowId: string;
  title: string;
  isActive: boolean;
  position: { x: number; y: number };
  size: { width: number; height: number };
  zIndex: number;
  
  // 窗口控制回调 (由 Desktop 提供)
  onClose: () => void;
  onMinimize: () => void;
  onMaximize: () => void;
  onFocus: () => void;
  onPositionChange: (pos: { x: number; y: number }) => void;
  onSizeChange: (size: { width: number; height: number }) => void;
  
  // 应用内容 (由 Desktop 注入)
  children: React.ReactNode;
}

/**
 * WindowShell - 窗口抽象层
 * 
 * 职责：
 * - 窗口容器 (position: absolute, transform: translate())
 * - 窗口交互 (拖拽、resize、聚焦)
 * - HeaderBar + WindowControls
 * - 窗口样式 (边框、阴影、圆角)
 * 
 * 不涉及：
 * - 应用内容布局
 * - 应用滚动
 * - 应用样式
 */
export function WindowShell({
  windowId,
  title,
  isActive,
  position,
  size,
  zIndex,
  onClose,
  onMinimize,
  onMaximize,
  onFocus,
  onPositionChange,
  onSizeChange,
  children,
}: WindowShellProps) {
  const [isDragging, setIsDragging] = useState(false);
  const [isResizing, setIsResizing] = useState(false);
  
  // 拖拽逻辑 (window-shell.tsx 内部实现)
  const handleDragStart = (e: React.MouseEvent) => {
    // ... 拖拽逻辑
  };
  
  // resize 逻辑 (window-shell.tsx 内部实现)
  const handleResizeStart = (e: React.MouseEvent, direction: string) => {
    // ... resize 逻辑
  };
  
  return (
    <div
      className="window-shell"
      data-window-id={windowId}
      style={{
        position: 'absolute',
        width: size.width,
        height: size.height,
        transform: `translate(${position.x}px, ${position.y}px)`,
        zIndex: zIndex,
      }}
      onMouseDown={onFocus}
    >
      {/* HeaderBar - 由 Shell 提供 */}
      <div 
        className="window-header-bar"
        onMouseDown={handleDragStart}
      >
        <div className="window-title">{title}</div>
        <WindowControls
          onClose={onClose}
          onMinimize={onMinimize}
          onMaximize={onMaximize}
        />
      </div>
      
      {/* ContentFrame - 隔离层，建立 flex 约束链 */}
      <div className="window-content-frame">
        {children}
      </div>
      
      {/* Resize Handles - 由 Shell 提供 */}
      {/* ... resize handles */}
    </div>
  );
}
```

### 2.2 WindowControls 组件

```tsx
// src/components/window-system/WindowControls.tsx

import './window-controls.css';

interface WindowControlsProps {
  onClose: () => void;
  onMinimize: () => void;
  onMaximize: () => void;
}

/**
 * WindowControls - 窗口控制按钮
 * 
 * GNOME 风格：黄绿红圆点 (最小化/最大化/关闭)
 * 完全独立，不受应用样式影响
 */
export function WindowControls({
  onClose,
  onMinimize,
  onMaximize,
}: WindowControlsProps) {
  return (
    <div className="window-controls">
      <button 
        className="window-control-btn minimize"
        onClick={onMinimize}
        title="最小化"
      />
      <button 
        className="window-control-btn maximize"
        onClick={onMaximize}
        title="最大化"
      />
      <button 
        className="window-control-btn close"
        onClick={onClose}
        title="关闭"
      />
    </div>
  );
}
```

### 2.3 应用组件改造

```tsx
// src/apps/FileManager.tsx (改造后)

export function FileManager({ windowId }: { windowId: string }) {
  // ... 业务逻辑
  
  return (
    // ✅ 应用组件只关心布局和内容，不涉及窗口交互
    <div className="fm-app">
      {/* 应用 HeaderBar - 由应用自己提供 */}
      <div className="fm-header-bar">
        {/* ... 应用工具栏 */}
      </div>
      
      {/* 应用内容区 - flex布局，height: 100% */}
      <div className="fm-content">
        {/* 应用侧边栏 - overflow-y: auto */}
        <div className="fm-sidebar">
          {/* ... */}
        </div>
        
        {/* 应用主区域 - overflow-y: auto */}
        <div className="fm-main">
          {/* ... */}
        </div>
      </div>
      
      {/* 应用状态栏 */}
      <div className="fm-status-bar">
        {/* ... */}
      </div>
    </div>
  );
}
```

---

## 三、样式隔离

### 3.1 窗口样式 (window-shell.css)

```css
/* src/components/window-system/window-shell.css */

/* ═════════════════════════════════════════════════════════════
   WindowShell 样式 - 完全独立，不与应用样式混合
   ═════════════════════════════════════════════════════════════ */

/* 窗口容器 */
.window-shell {
  position: absolute;
  background: var(--window-bg);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-popup);
  display: flex;
  flex-direction: column;
  overflow: hidden;  /* ✅ 窗口层管理overflow，防止内容溢出 */
  
  /* GPU 加速 */
  will-change: transform;
  transition: opacity 0.2s ease-out;
}

.window-shell.active {
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.2);
}

.window-shell.dragging {
  opacity: 0.95;
  cursor: move;
  transition: none;
}

/* HeaderBar - 窗口标题栏 */
.window-header-bar {
  height: var(--headerbar-height);
  background: var(--headerbar-bg);
  border-bottom: 1px solid var(--border-color);
  display: flex;
  align-items: center;
  padding: 0 12px;
  gap: 8px;
  flex-shrink: 0;
  cursor: move;
  user-select: none;
}

.window-title {
  font-size: var(--font-small);
  font-weight: 600;
  color: var(--text-primary);
  flex: 1;
  text-align: center;
  white-space: nowrap;
}

/* ContentFrame - 隔离层 */
.window-content-frame {
  flex: 1;
  min-height: 0;  /* ✅ 建立 flex 高度约束链 */
  overflow: hidden;  /* ✅ 防止应用内容溢出窗口边界 */
  
  /* 不设置其他样式，完全交给应用层管理 */
}

/* Resize Handles */
.window-resize-handle {
  /* ... resize handles 样式 */
}
```

### 3.2 应用样式 (FileManager.css 改造后)

```css
/* src/apps/FileManager.css */

/* ═════════════════════════════════════════════════════════════
   FileManager 样式 - 完全独立，不受窗口样式影响
   ═════════════════════════════════════════════════════════════ */

/* 应用容器 - height: 100%，填满 ContentFrame */
.fm-app {
  display: flex;
  flex-direction: column;
  height: 100%;  /* ✅ 填满 ContentFrame (已有 min-height: 0 约束) */
  background: var(--window-bg);
}

/* 应用 HeaderBar */
.fm-header-bar {
  height: 48px;
  background: var(--headerbar-bg);
  border-bottom: 1px solid var(--border-color);
  display: flex;
  align-items: center;
  padding: 0 12px;
  gap: 6px;
  flex-shrink: 0;
}

/* 应用内容区 - flex布局 */
.fm-content {
  display: flex;
  flex: 1;
  min-height: 0;  /* ✅ 建立 flex 约束链 */
  overflow: visible;  /* ✅ 应用层不管理overflow，由子元素管理 */
}

/* 应用侧边栏 - 滚动 */
.fm-sidebar {
  width: var(--sidebar-width);
  background: var(--sidebar-bg);
  overflow-y: auto;  /* ✅ 侧边栏独立滚动 */
  flex-shrink: 0;
}

/* 应用主区域 - 滚动 */
.fm-main {
  flex: 1;
  overflow-y: auto;  /* ✅ 主区域独立滚动 */
  overflow-x: hidden;
  padding: 8px;
}

/* 应用状态栏 */
.fm-status-bar {
  height: 28px;
  background: var(--headerbar-bg);
  border-top: 1px solid var(--border-color);
  display: flex;
  align-items: center;
  padding: 0 12px;
  flex-shrink: 0;
}
```

---

## 四、Desktop 改造

```tsx
// src/shell/Desktop.tsx (改造后)

export function Desktop() {
  const manager = useWindowManager();
  
  return (
    <div className="shell">
      {/* Top Bar */}
      {/* ... */}
      
      {/* Desktop Area */}
      <div className="desktop-area">
        {/* ... */}
        
        {/* Application Windows */}
        {windows.map(win => (
          <WindowShell
            key={win.id}
            windowId={win.id}
            title={manager.getApp(win.appId)?.label || 'Unknown'}
            isActive={win.id === activeWindow?.id}
            position={win.position}
            size={win.size}
            zIndex={win.focused ? 90 : 10}
            onClose={() => manager.close(win.id)}
            onMinimize={() => manager.minimize(win.id)}
            onMaximize={() => manager.maximize(win.id)}
            onFocus={() => manager.focus(win.id)}
            onPositionChange={(pos) => manager.setPosition(win.id, pos)}
            onSizeChange={(size) => manager.setSize(win.id, size)}
          >
            {/* 应用内容 - 完全独立 */}
            {renderAppContent(win)}
          </WindowShell>
        ))}
      </div>
      
      {/* Dock */}
      {/* ... */}
    </div>
  );
}
  
  const renderAppContent = (win: Window) => {
    switch (win.appId) {
      case 'filemanager':
        return <FileManager windowId={win.id} />;
      case 'terminal':
        return <TerminalApp windowId={win.id} />;
      case 'monitor':
        return <SystemMonitor windowId={win.id} />;
      case 'settings':
        return <Settings windowId={win.id} />;
      default:
        return <div>Unknown app</div>;
    }
  };
}
```

---

## 五、优势总结

### 5.1 职责分离

| 改进前 | 改进后 |
|---|---|
| `DraggableWindow` 混合窗口交互和应用内容 | `WindowShell` 只负责窗口交互 |
| 应用组件涉及窗口样式 (`.fm` 受 `.app-window` 影响) | 应用组件完全独立，不涉及窗口样式 |
| CSS层级混乱 (`.app-window-content` 影响应用滚动) | CSS层级清晰，样式隔离 |

### 5.2 开发便利

**新应用开发流程：**
1. ✅ 创建应用组件 (`MyApp.tsx`)
2. ✅ 创建应用样式 (`MyApp.css`)
3. ✅ 注册到 `WindowRegistry`
4. ✅ 完成，不需要处理窗口逻辑

**不需要关心：**
- ❌ 窗口拖拽、resize 逻辑
- ❌ 窗口 z-index 管理
- ❌ 窗口动画
- ❌ HeaderBar + WindowControls
- ❌ 窗口边框、阴影、圆角

### 5.3 样式隔离

**窗口样式：**
- ✅ `window-shell.css` - 窗口容器、HeaderBar、WindowControls
- ✅ 不影响应用样式

**应用样式：**
- ✅ `FileManager.css` - 应用布局、侧边栏、主区域、状态栏
- ✅ 不受窗口样式影响
- ✅ 应用滚动独立管理

---

## 六、实施路线

### 阶段 1：创建 WindowShell (1天)
- 创建 `WindowShell.tsx` 和 `window-shell.css`
- 创建 `WindowControls.tsx` 和 `window-controls.css`
- 实现拖拽、resize、聚焦逻辑

### 阶段 2：改造 Desktop (半天)
- 移除 `DraggableWindow.tsx`
- Desktop 使用 `WindowShell` 渲染窗口
- 应用组件作为 children 注入

### 阶段 3：改造应用组件 (1天)
- FileManager: 移除窗口样式依赖，改为 `fm-app`
- Settings: 移除窗口样式依赖，改为 `st-app`
- Terminal: 移除窗口样式依赖，改为 `terminal-app`
- Monitor: 移除窗口样式依赖，改为 `monitor-app`

### 阶段 4：样式隔离测试 (半天)
- 测试窗口交互 (拖拽、resize、聚焦)
- 测试应用滚动 (侧边栏、主区域)
- 测试终端占满窗口
- 测试样式隔离 (窗口样式不影响应用)

---

## 七、技术栈对比

| | Windows 11 | 本方案 |
|---|---|---|
| 窗口容器 | `WindowShell` (DWM) | `WindowShell.tsx` |
| 窗口控制 | `WindowControls` (标题栏按钮) | `WindowControls.tsx` |
| 内容区域 | `ContentFrame` (独立) | `ContentFrame` (flex容器) |
| 应用组件 | 应用程序 (独立渲染) | `FileManager.tsx` 等 (独立渲染) |
| 样式隔离 | Win32k.sys 分层 | CSS 文件分离 |
| 职责分离 | 窗口管理器 vs 应用程序 | WindowShell vs AppContent |

---

## 八、后续扩展

### 8.1 多窗口模式

**标准模式：**
- WindowShell 提供 HeaderBar + WindowControls
- 应用组件渲染内容

**无框模式：**
- WindowShell 只提供窗口容器和 resize handles
- 应用组件自己实现 HeaderBar + WindowControls (如 Terminal)

### 8.2 主题系统

**窗口主题：**
- `window-shell.css` 使用 CSS 变量
- 自动跟随 GNOME 主题

**应用主题：**
- 应用 CSS 独立管理主题
- 不受窗口主题影响

---

## 九、总结

**核心原则：**
1. ✅ **职责分离** - 窗口交互 vs 应用内容
2. ✅ **样式隔离** - 窗口样式 vs 应用样式
3. ✅ **开发便利** - 新应用不涉及窗口逻辑
4. ✅ **架构清晰** - 三层架构，边界明确

**改进效果：**
- ✅ 文件管理器/设置可以正常滚动
- ✅ 终端占满整个窗口
- ✅ 窗口交互逻辑独立，应用开发更简单
- ✅ 样式隔离，不会互相影响