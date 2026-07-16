// src/components/window-shell/WindowShell.tsx
// 窗口抽象层 - 职责：窗口交互（拖拽、resize、聚焦），不涉及应用内容

import { useState, useRef, useEffect, useCallback } from 'react';
import { WindowControls } from './WindowControls';
import styles from './WindowShell.module.css';

export interface WindowShellProps {
  // 窗口属性 (由 Desktop 提供)
  windowId: string;
  title: string;
  isActive: boolean;
  position: { x: number; y: number };
  size: { width: number; height: number };
  // ✅ 删除 zIndex prop，改用 CSS 固定规则
  isMinimized?: boolean;
  isMaximized?: boolean; // ✅ 新增：最大化状态
  mode?: 'standard' | 'frameless'; // 窗口模式：standard(标准) | frameless(无框)

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
 * 支持两种模式：
 * - standard: 标准模式，提供 HeaderBar + WindowControls
 * - frameless: 无框模式，不提供 HeaderBar，应用自己实现控制按钮
 *
 * 职责：
 * - 窗口容器 (position: absolute, transform: translate())
 * - 窗口交互 (拖拽、resize、聚焦)
 * - HeaderBar + WindowControls（standard模式）
 * - 窗口样式 (边框、阴影、圆角)
 * - 窗口动画 (打开/关闭/激活)
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
  // ✅ 删除 zIndex prop，改用 CSS 固定规则
  isMinimized = false,
  isMaximized = false, // ✅ 新增：最大化状态
  mode = 'standard', // 默认标准模式
  onClose,
  onMinimize,
  onMaximize,
  onFocus,
  onPositionChange,
  onSizeChange,
  children,
}: WindowShellProps) {
  // ── 统一窗口状态管理（原子性修复）──────────────────
  // ✅ 核心：使用单一ref管理窗口完整状态，确保拖拽和resize共享同一状态源
  const windowStateRef = useRef({
    x: position.x,
    y: position.y,
    width: size.width,
    height: size.height
  });

  // ── 拖拽状态 ────────────────────────────────────────
  const [isDragging, setIsDragging] = useState(false);
  const dragStartPos = useRef({ x: 0, y: 0 });  // 鼠标相对偏移
  const rafIdRef = useRef<number | null>(null);

  // ── Resize 状态 ──────────────────────────────────────
  const [isResizing, setIsResizing] = useState(false);
  const [resizeDirection, setResizeDirection] = useState<string | null>(null);
  const resizeStartPos = useRef({
    x: 0,
    y: 0,
    width: size.width,
    height: size.height,
    posX: position.x,
    posY: position.y,
  });

  // ── 窗口打开动画 ────────────────────────────────────
  const [isOpening, setIsOpening] = useState(true);

  // ── 原子性状态管理 ────────────────────────────────────
  // ✅ 核心原则：windowStateRef是操作的唯一状态源，不允许被props重置
  // ✅ 只在组件挂载时初始化，操作过程中实时更新，操作结束后提交到Desktop
  // ✅ 删除双向同步逻辑，保证原子性

  useEffect(() => {
    const timer = setTimeout(() => setIsOpening(false), 200);
    return () => clearTimeout(timer);
  }, []);

  // ── 拖拽开始 ────────────────────────────────────────
  // 只在 standard 模式下通过 HeaderBar 拖拽
  // frameless 模式下应用自己实现拖拽（通过回调API）
  // ✅ 最大化状态下禁用拖拽
  const handleDragStart = useCallback(
    (e: React.MouseEvent) => {
      // frameless 模式下，WindowShell 不处理拖拽
      if (mode === 'frameless') return;

      // ✅ 最大化状态下禁用拖拽
      if (isMaximized) return;

      e.preventDefault();
      e.stopPropagation();

      // 触发窗口置顶（GNOME 标准：点击窗口 → raise + focus）
      onFocus();

      setIsDragging(true);
      // ✅ 原子性：windowStateRef是唯一状态源，不会被props重置
      const currentState = windowStateRef.current;
      dragStartPos.current = {
        x: e.clientX - currentState.x,
        y: e.clientY - currentState.y,
      };
    },
    [onFocus, mode, isMaximized] // ✅ 移除 position 依赖，改用 ref
  );

  // ── 双击标题栏最大化（GNOME 标准）────────────────────
  const handleHeaderBarDoubleClick = useCallback(
    (e: React.MouseEvent) => {
      // frameless 模式下，不处理双击
      if (mode === 'frameless') return;

      // ✅ GNOME 标准：双击 HeaderBar 触发最大化/取消最大化
      e.preventDefault();
      e.stopPropagation();
      onMaximize();
    },
    [mode, onMaximize]
  );

  // ── 拖拽移动（使用 requestAnimationFrame 优化性能）────
  useEffect(() => {
    if (!isDragging) return;

    const handleMouseMove = (e: MouseEvent) => {
      const newX = e.clientX - dragStartPos.current.x;
      const newY = e.clientY - dragStartPos.current.y;

      // 获取父容器尺寸
      const parentEl = windowRef.current?.parentElement;
      const parentWidth = parentEl?.clientWidth || window.innerWidth;
      const parentHeight = parentEl?.clientHeight || window.innerHeight;

      // ── 边界约束逻辑（符合标准窗口设计）──────────────
      // 1. 顶部边界：工作区内 y >= 0（TopBar 已由 shell flex 布局分离）
      const minY = 0;

      // 2. 左、右、下边界：可以穿越，但保留最小可见区域
      // 防止窗口完全隐藏在边缘，导致无法选中
      const MIN_VISIBLE = 100; // 最小可见区域（像素）

      // 左边界：窗口可以向左移出屏幕，但至少保留 MIN_VISIBLE 在可视区域
      const minX = -(size.width - MIN_VISIBLE);

      // 右边界：窗口可以向右移出屏幕，但至少保留 MIN_VISIBLE 在可视区域
      const maxX = parentWidth - MIN_VISIBLE;

      // 下边界：窗口可以向下移出屏幕，但至少保留 MIN_VISIBLE 在可视区域
      const maxY = parentHeight - MIN_VISIBLE;

      const boundedX = Math.max(minX, Math.min(newX, maxX));
      const boundedY = Math.max(minY, Math.min(newY, maxY));

      // ✅ 统一状态管理：更新windowStateRef，不触发 React 重渲染
      windowStateRef.current.x = boundedX;
      windowStateRef.current.y = boundedY;

      // 取消之前的动画帧
      if (rafIdRef.current !== null) {
        cancelAnimationFrame(rafIdRef.current);
      }

      // 使用 requestAnimationFrame 更新视觉位置
      rafIdRef.current = requestAnimationFrame(() => {
        if (windowRef.current) {
          // ✅ 保留 scale，避免大小突变（拖拽不应改变 scale）
          windowRef.current.style.transform = `translate(${boundedX}px, ${boundedY}px) scale(${isOpening ? 0.96 : 1})`;
          // ✅ 禁用 transition，避免动画冲突
          windowRef.current.style.transition = 'none';
        }
      });
    };

    const handleMouseUp = () => {
      // 取消动画帧
      if (rafIdRef.current !== null) {
        cancelAnimationFrame(rafIdRef.current);
        rafIdRef.current = null;
      }

      // ✅ 恢复 transition
      if (windowRef.current) {
        windowRef.current.style.transition = isOpening
          ? 'opacity 200ms cubic-bezier(0.25, 0, 0, 1), transform 200ms cubic-bezier(0.25, 0, 0, 1)'
          : 'opacity 0.2s ease-out';
      }

      // 拖拽结束时，更新 state（触发一次重渲染）
      setIsDragging(false);

      // ✅ 统一状态管理：同步位置到 Desktop
      if (onPositionChange) {
        onPositionChange({ x: windowStateRef.current.x, y: windowStateRef.current.y });
      }
    };

    // 使用 passive 事件监听器，提升性能
    document.addEventListener('mousemove', handleMouseMove, { passive: true });
    document.addEventListener('mouseup', handleMouseUp);

    return () => {
      document.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('mouseup', handleMouseUp);

      // 清理动画帧
      if (rafIdRef.current !== null) {
        cancelAnimationFrame(rafIdRef.current);
        rafIdRef.current = null;
      }
    };
  }, [isDragging, size, onPositionChange, isOpening]); // ✅ 移除 position：避免resize改变position时频繁重新绑定事件监听器

  // ── Resize 开始 ──────────────────────────────────────
  // ✅ 最大化状态下禁用 resize
  const handleResizeStart = useCallback(
    (e: React.MouseEvent, direction: string) => {
      // ✅ 最大化状态下禁用 resize
      if (isMaximized) return;

      e.preventDefault();
      e.stopPropagation();

      // ✅ 激活窗口（点击 resize handles 也应该激活）
      onFocus();

      setIsResizing(true);
      setResizeDirection(direction);

      // ✅ 原子性：windowStateRef是唯一状态源，不会被props重置
      const currentState = windowStateRef.current;
      resizeStartPos.current = {
        x: e.clientX,
        y: e.clientY,
        width: currentState.width,
        height: currentState.height,
        posX: currentState.x,
        posY: currentState.y,
      };
    },
    [onFocus, isMaximized] // ✅ 移除 size/position 依赖，改用 ref
  );

  // ── Resize 移动 ──────────────────────────────────────
  useEffect(() => {
    if (!isResizing || !resizeDirection) return;

    const handleMouseMove = (e: MouseEvent) => {
      const deltaX = e.clientX - resizeStartPos.current.x;
      const deltaY = e.clientY - resizeStartPos.current.y;

      let newWidth = resizeStartPos.current.width;
      let newHeight = resizeStartPos.current.height;
      let newX = resizeStartPos.current.posX;
      let newY = resizeStartPos.current.posY;

      // 获取父容器尺寸
      const parentEl = windowRef.current?.parentElement;
      const parentWidth = parentEl?.clientWidth || window.innerWidth;
      const parentHeight = parentEl?.clientHeight || window.innerHeight;

      // 最小窗口尺寸
      const minWidth = 400;
      const minHeight = 300;

      // ── 边界约束逻辑（符合标准窗口设计）──────────────
      // 1. 顶部边界：工作区内 y >= 0（TopBar 已由 shell flex 布局分离）
      const minY = 0;

      // 2. 左、右、下边界：可以穿越，但保留最小可见区域
      const MIN_VISIBLE = 100; // 最小可见区域（像素）

      // 根据调整方向计算新尺寸和位置
      // 右侧拖拽：无最大宽度限制，仅保留最小宽度
      if (resizeDirection.includes('e')) {
        newWidth = Math.max(minWidth, resizeStartPos.current.width + deltaX);
      }
      if (resizeDirection.includes('w')) {
        // 左边界：可以向左扩展，但至少保留 MIN_VISIBLE 在可视区域
        const widthDelta = Math.min(deltaX, resizeStartPos.current.width - minWidth);
        newWidth = resizeStartPos.current.width - widthDelta;
        const newXCandidate = resizeStartPos.current.posX + widthDelta;
        // 左边界最小位置：-(width - MIN_VISIBLE)
        newX = Math.max(-(newWidth - MIN_VISIBLE), newXCandidate);
      }
      // 下方拖拽：无最大高度限制，仅保留最小高度
      if (resizeDirection.includes('s')) {
        newHeight = Math.max(minHeight, resizeStartPos.current.height + deltaY);
      }
      if (resizeDirection.includes('n')) {
        // 上边界：无法超越 TopBar
        const heightDelta = Math.min(deltaY, resizeStartPos.current.height - minHeight);
        newHeight = resizeStartPos.current.height - heightDelta;
        const newYCandidate = resizeStartPos.current.posY + heightDelta;
        newY = Math.max(minY, newYCandidate);
      }

      // ✅ 统一状态管理：更新windowStateRef，不触发 React 重渲染
      windowStateRef.current = { x: newX, y: newY, width: newWidth, height: newHeight };

      // ✅ 使用 requestAnimationFrame 更新 DOM（实时视觉反馈）
      if (rafIdRef.current !== null) {
        cancelAnimationFrame(rafIdRef.current);
      }

      rafIdRef.current = requestAnimationFrame(() => {
        if (windowRef.current) {
          windowRef.current.style.width = `${newWidth}px`;
          windowRef.current.style.height = `${newHeight}px`;
          // ✅ 保留 scale，避免大小突变（resize 不应改变 scale）
          windowRef.current.style.transform = `translate(${newX}px, ${newY}px) scale(${isOpening ? 0.96 : 1})`;
          // ✅ 禁用 transition，避免动画冲突（类似拖拽）
          windowRef.current.style.transition = 'none';
        }
      });
    };

    const handleMouseUp = () => {
      // ✅ 清除 requestAnimationFrame
      if (rafIdRef.current !== null) {
        cancelAnimationFrame(rafIdRef.current);
        rafIdRef.current = null;
      }

      setIsResizing(false);
      setResizeDirection(null);

      // ✅ 恢复 transition
      if (windowRef.current) {
        windowRef.current.style.transition = isOpening
          ? 'opacity 200ms cubic-bezier(0.25, 0, 0, 1), transform 200ms cubic-bezier(0.25, 0, 0, 1)'
          : 'opacity 0.2s ease-out';
      }

      // ✅ 统一状态管理：同步位置和大小到 Desktop（使用 windowStateRef 的最终值）
      const finalPosition = { x: windowStateRef.current.x, y: windowStateRef.current.y };
      const finalSize = { width: windowStateRef.current.width, height: windowStateRef.current.height };

      if (onPositionChange) {
        onPositionChange(finalPosition);
      }
      if (onSizeChange) {
        onSizeChange(finalSize);
      }
    };

    // ✅ 添加 passive: true，避免阻塞主线程
    document.addEventListener('mousemove', handleMouseMove, { passive: true });
    document.addEventListener('mouseup', handleMouseUp);

    return () => {
      document.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('mouseup', handleMouseUp);
      // ✅ 清除 requestAnimationFrame
      if (rafIdRef.current !== null) {
        cancelAnimationFrame(rafIdRef.current);
        rafIdRef.current = null;
      }
    };
  }, [isResizing, resizeDirection, onPositionChange, onSizeChange, isOpening]); // ✅ 添加 isOpening

  const windowRef = useRef<HTMLDivElement>(null);

  // ── 窗口聚焦（点击窗口任意区域）──────────────────────
  const handleWindowMouseDown = (e: React.MouseEvent) => {
    e.stopPropagation();
    onFocus();
  };

  return (
    <div
      ref={windowRef}
      className={`${styles.windowShell}${isActive ? ` ${styles.windowShellActive}` : ` ${styles.windowShellNotActive}`}${isDragging ? ` ${styles.windowShellDragging}` : ` ${styles.windowShellNotDragging}`}${isMaximized ? ` ${styles.windowShellMaximized}` : ''}`}
      data-window-id={windowId}
      data-resizing={isResizing ? 'true' : 'false'} // ✅ 新增：用于 CSS 禁用动画
      style={{
        left: 0,
        top: 0,
        width: size.width,
        height: size.height,
        display: isMinimized ? 'none' : 'flex',
        transform: `translate(${position.x}px, ${position.y}px) scale(${isOpening ? 0.96 : 1})`,
        willChange: isDragging ? 'transform' : 'auto',
        opacity: isOpening ? 0 : 1,
        transition: isOpening
          ? 'opacity 200ms cubic-bezier(0.25, 0, 0, 1), transform 200ms cubic-bezier(0.25, 0, 0, 1)'
          : (isDragging || isResizing)
            ? 'opacity 0.2s ease-out'
            : 'opacity 0.2s ease-out, transform 300ms cubic-bezier(0.25, 0, 0, 1), width 300ms cubic-bezier(0.25, 0, 0, 1), height 300ms cubic-bezier(0.25, 0, 0, 1)',
      }}
      onMouseDown={handleWindowMouseDown}
    >
      {/* ── HeaderBar - 窗口标题栏（由 Shell 提供）────────── */}
      {/* standard 模式：显示标准 HeaderBar + WindowControls */}
      {/* frameless 模式：不显示 HeaderBar，应用自己实现控制按钮 */}
      {mode === 'standard' && (
        <div
          className={styles.windowHeaderBar}
          onMouseDown={handleDragStart}
          onDoubleClick={handleHeaderBarDoubleClick}
        >
          <div className={styles.windowTitle}>{title}</div>
          <WindowControls onClose={onClose} onMinimize={onMinimize} onMaximize={onMaximize} />
        </div>
      )}

      {/* ── ContentFrame - 隔离层，建立 flex 约束链 ──────── */}
      <div className={styles.windowContentFrame}>{children}</div>

      {/* ── Resize Handles（由 Shell 提供）───────────────── */}
      {/* ✅ 最大化时隐藏 resize handles */}
      {!isMaximized && (
        <>
          {/* 右边 */}
          <div
            style={{
              position: 'absolute',
              right: 0,
              top: 0,
              bottom: 0,
              width: 8,
              cursor: 'e-resize',
            }}
            onMouseDown={(e) => handleResizeStart(e, 'e')}
          />
          {/* 下边 */}
          <div
            style={{
              position: 'absolute',
              bottom: 0,
              left: 0,
              right: 0,
              height: 8,
              cursor: 's-resize',
            }}
            onMouseDown={(e) => handleResizeStart(e, 's')}
          />
          {/* 右下角 */}
          <div
            style={{
              position: 'absolute',
              right: 0,
              bottom: 0,
              width: 16,
              height: 16,
              cursor: 'se-resize',
            }}
            onMouseDown={(e) => handleResizeStart(e, 'se')}
          />
          {/* 左边 */}
          <div
            style={{
              position: 'absolute',
              left: 0,
              top: 0,
              bottom: 0,
              width: 8,
              cursor: 'w-resize',
            }}
            onMouseDown={(e) => handleResizeStart(e, 'w')}
          />
          {/* 上边 */}
          <div
            style={{
              position: 'absolute',
              top: 0,
              left: 0,
              right: 0,
              height: 8,
              cursor: 'n-resize',
              // ✅ 删除 zIndex，继承窗口的 z-index
            }}
            onMouseDown={(e) => handleResizeStart(e, 'n')}
          />
          {/* 左上角 */}
          <div
            style={{
              position: 'absolute',
              left: 0,
              top: 0,
              width: 16,
              height: 16,
              cursor: 'nw-resize',
              // ✅ 删除 zIndex，继承窗口的 z-index
            }}
            onMouseDown={(e) => handleResizeStart(e, 'nw')}
          />
          {/* 右上角 */}
          <div
            style={{
              position: 'absolute',
              right: 0,
              top: 0,
              width: 16,
              height: 16,
              cursor: 'ne-resize',
              // ✅ 删除 zIndex，继承窗口的 z-index
            }}
            onMouseDown={(e) => handleResizeStart(e, 'ne')}
          />
          {/* 左下角 */}
          <div
            style={{
              position: 'absolute',
              left: 0,
              bottom: 0,
              width: 16,
              height: 16,
              cursor: 'sw-resize',
            }}
            onMouseDown={(e) => handleResizeStart(e, 'sw')}
          />
        </>
      )}
    </div>
  );
}