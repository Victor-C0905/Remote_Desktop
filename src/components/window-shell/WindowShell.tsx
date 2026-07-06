// src/components/window-shell/WindowShell.tsx
// 窗口抽象层 - 职责：窗口交互（拖拽、resize、聚焦），不涉及应用内容

import { useState, useRef, useEffect, useCallback } from 'react';
import { WindowControls } from './WindowControls';
import './window-shell.css';

export interface WindowShellProps {
  // 窗口属性 (由 Desktop 提供)
  windowId: string;
  title: string;
  isActive: boolean;
  position: { x: number; y: number };
  size: { width: number; height: number };
  // ✅ 删除 zIndex prop，改用 CSS 固定规则
  isMinimized?: boolean;
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
  mode = 'standard', // 默认标准模式
  onClose,
  onMinimize,
  onMaximize,
  onFocus,
  onPositionChange,
  onSizeChange,
  children,
}: WindowShellProps) {
  // ── 拖拽状态 ────────────────────────────────────────
  const [isDragging, setIsDragging] = useState(false);
  const dragStartPos = useRef({ x: 0, y: 0 });
  const dragPositionRef = useRef({ x: position.x, y: position.y });
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
  const resizeDataRef = useRef({
    x: position.x,
    y: position.y,
    width: size.width,
    height: size.height,
  });

  // ── 窗口打开动画 ────────────────────────────────────
  const [isOpening, setIsOpening] = useState(true);
  useEffect(() => {
    const timer = setTimeout(() => setIsOpening(false), 200);
    return () => clearTimeout(timer);
  }, []);

  // ✅ 同步 position props 到 ref（只在非拖拽时）
  useEffect(() => {
    if (!isDragging) {
      dragPositionRef.current = { x: position.x, y: position.y };
    }
  }, [position, isDragging]);

  // ✅ 同步 position/size props 到 ref（只在非 resize 时）
  useEffect(() => {
    if (!isResizing) {
      resizeDataRef.current = {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height
      };
    }
  }, [position, size, isResizing]);

  // ── 拖拽开始 ────────────────────────────────────────
  // 只在 standard 模式下通过 HeaderBar 拖拽
  // frameless 模式下应用自己实现拖拽（通过回调API）
  const handleDragStart = useCallback(
    (e: React.MouseEvent) => {
      // frameless 模式下，WindowShell 不处理拖拽
      if (mode === 'frameless') return;

      e.preventDefault();
      e.stopPropagation();

      // 触发窗口置顶（GNOME 标准：点击窗口 → raise + focus）
      onFocus();

      setIsDragging(true);
      dragStartPos.current = {
        x: e.clientX - position.x,
        y: e.clientY - position.y,
      };
      dragPositionRef.current = { x: position.x, y: position.y };
    },
    [position, onFocus, mode]
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
      // 1. 顶部边界：无法超越 TopBar（y >= 0）
      const minY = 0; // TopBar 高度已由 Desktop 处理，Desktop 区域 y=0 即 TopBar 下方

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

      // 存储到 ref，不触发 React 重渲染
      dragPositionRef.current = { x: boundedX, y: boundedY };

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

      // 同步位置到 Desktop
      if (onPositionChange) {
        onPositionChange(dragPositionRef.current);
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
  }, [isDragging, size, onPositionChange, isOpening]); // ✅ 添加 isOpening

  // ── Resize 开始 ──────────────────────────────────────
  const handleResizeStart = useCallback(
    (e: React.MouseEvent, direction: string) => {
      e.preventDefault();
      e.stopPropagation();

      // ✅ 激活窗口（点击 resize handles 也应该激活）
      onFocus();

      setIsResizing(true);
      setResizeDirection(direction);

      resizeStartPos.current = {
        x: e.clientX,
        y: e.clientY,
        width: size.width,
        height: size.height,
        posX: position.x,
        posY: position.y,
      };

      resizeDataRef.current = {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
      };
    },
    [size, position, onFocus] // ✅ 添加 onFocus 到依赖项
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
      // 1. 顶部边界：无法超越 TopBar（y >= 0）
      const minY = 0; // TopBar 高度已由 Desktop 处理，Desktop 区域 y=0 即 TopBar 下方

      // 2. 左、右、下边界：可以穿越，但保留最小可见区域
      const MIN_VISIBLE = 100; // 最小可见区域（像素）

      // 根据调整方向计算新尺寸和位置
      if (resizeDirection.includes('e')) {
        // 右边界：可以向右扩展，但至少保留 MIN_VISIBLE 在可视区域
        const maxWidth = parentWidth - newX + (newX < 0 ? -newX : 0);
        newWidth = Math.max(minWidth, Math.min(maxWidth, resizeStartPos.current.width + deltaX));
        // 确保右边界不超出允许范围
        if (newX + newWidth > parentWidth - MIN_VISIBLE) {
          newWidth = parentWidth - MIN_VISIBLE - newX;
        }
      }
      if (resizeDirection.includes('w')) {
        // 左边界：可以向左扩展，但至少保留 MIN_VISIBLE 在可视区域
        const widthDelta = Math.min(deltaX, resizeStartPos.current.width - minWidth);
        newWidth = resizeStartPos.current.width - widthDelta;
        const newXCandidate = resizeStartPos.current.posX + widthDelta;
        // 左边界最小位置：-(width - MIN_VISIBLE)
        newX = Math.max(-(newWidth - MIN_VISIBLE), newXCandidate);
      }
      if (resizeDirection.includes('s')) {
        // 下边界：可以向下扩展，但至少保留 MIN_VISIBLE 在可视区域
        const maxHeight = parentHeight - newY + (newY < 0 ? -newY : 0);
        newHeight = Math.max(minHeight, Math.min(maxHeight, resizeStartPos.current.height + deltaY));
        // 确保下边界不超出允许范围
        if (newY + newHeight > parentHeight - MIN_VISIBLE) {
          newHeight = parentHeight - MIN_VISIBLE - newY;
        }
      }
      if (resizeDirection.includes('n')) {
        // 上边界：无法超越 TopBar
        const heightDelta = Math.min(deltaY, resizeStartPos.current.height - minHeight);
        newHeight = resizeStartPos.current.height - heightDelta;
        const newYCandidate = resizeStartPos.current.posY + heightDelta;
        newY = Math.max(minY, newYCandidate);
      }

      resizeDataRef.current = { x: newX, y: newY, width: newWidth, height: newHeight };

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

      // 同步位置和大小到 Desktop（使用 ref 中的最终值）
      if (onPositionChange) {
        onPositionChange({
          x: resizeDataRef.current.x,
          y: resizeDataRef.current.y,
        });
      }
      if (onSizeChange) {
        onSizeChange({
          width: resizeDataRef.current.width,
          height: resizeDataRef.current.height,
        });
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
      className={`window-shell${isActive ? ' active' : ''}${isDragging ? ' dragging' : ''}`}
      data-window-id={windowId}
      style={{
        position: 'absolute',
        left: 0,
        top: 0,
        width: size.width,
        height: size.height,
        // ✅ 删除 inline zIndex，改用 CSS 固定规则
        display: isMinimized ? 'none' : 'flex',
        flexDirection: 'column',
        cursor: isDragging ? 'move' : 'default',
        // 使用 transform 代替 left/top，性能更好（GPU 加速）
        transform: `translate(${position.x}px, ${position.y}px) scale(${isOpening ? 0.96 : 1})`,
        willChange: isDragging ? 'transform' : 'auto',
        opacity: isOpening ? 0 : 1,
        transition: isOpening
          ? 'opacity 200ms cubic-bezier(0.25, 0, 0, 1), transform 200ms cubic-bezier(0.25, 0, 0, 1)'
          : 'opacity 0.2s ease-out',
      }}
      onMouseDown={handleWindowMouseDown}
    >
      {/* ── HeaderBar - 窗口标题栏（由 Shell 提供）────────── */}
      {/* standard 模式：显示标准 HeaderBar + WindowControls */}
      {/* frameless 模式：不显示 HeaderBar，应用自己实现控制按钮 */}
      {mode === 'standard' && (
        <div
          className="window-header-bar"
          onMouseDown={handleDragStart}
          style={{
            cursor: isDragging ? 'move' : 'move',
            userSelect: 'none',
          }}
        >
          <div className="window-title">{title}</div>
          <WindowControls onClose={onClose} onMinimize={onMinimize} onMaximize={onMaximize} />
        </div>
      )}

      {/* ── ContentFrame - 隔离层，建立 flex 约束链 ──────── */}
      <div className="window-content-frame">{children}</div>

      {/* ── Resize Handles（由 Shell 提供）───────────────── */}
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
    </div>
  );
}