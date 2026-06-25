import { useState, useRef, useEffect, useCallback } from "react";

interface DraggableWindowProps {
  title: string;
  isActive: boolean;
  onClose: () => void;
  onMinimize: () => void;
  onFocus: () => void;
  children: React.ReactNode;
  initialPosition?: { x: number; y: number };
  initialSize?: { width: number; height: number };
  minWidth?: number;
  minHeight?: number;
  zIndex?: number; // 窗口层级，由父组件根据激活顺序动态分配
}

/**
 * 可拖拽的窗口组件
 * 支持在父容器内拖动和调整大小
 */
export function DraggableWindow({
  title,
  isActive,
  onClose,
  onMinimize,
  onFocus,
  children,
  initialPosition = { x: 50, y: 50 },
  initialSize = { width: 800, height: 600 },
  minWidth = 400,
  minHeight = 300,
  zIndex = 10, // 默认 z-index，低于 top-bar(100) 和 dock(50)
}: DraggableWindowProps) {
  const [position, setPosition] = useState(initialPosition);
  const [size, setSize] = useState(initialSize);
  const [isDragging, setIsDragging] = useState(false);
  const [isResizing, setIsResizing] = useState(false);
  const [resizeDirection, setResizeDirection] = useState<string | null>(null);
  const [isOpening, setIsOpening] = useState(true); // 窗口打开动画状态

  const windowRef = useRef<HTMLDivElement>(null);
  const dragStartPos = useRef({ x: 0, y: 0 });
  const resizeStartPos = useRef({ x: 0, y: 0, width: 0, height: 0, posX: 0, posY: 0 }); // 添加 posX 和 posY
  
  // 使用 ref 存储拖拽时的临时位置，避免频繁触发 React 重渲染
  const dragPositionRef = useRef({ x: 0, y: 0 });
  // requestAnimationFrame ID，用于取消动画帧
  const rafIdRef = useRef<number | null>(null);

  // 窗口打开动画：组件挂载后 200ms 结束动画
  useEffect(() => {
    const timer = setTimeout(() => {
      setIsOpening(false);
    }, 200);
    return () => clearTimeout(timer);
  }, []);

  // 拖拽开始
  const handleDragStart = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation(); // 阻止事件传播，防止触发外部窗口移动
    setIsDragging(true);

    // 点击 titlebar 时，触发窗口置顶（GNOME 标准：点击窗口 → raise + focus）
    onFocus();

    // 记录拖拽起始位置
    dragStartPos.current = {
      x: e.clientX - position.x,
      y: e.clientY - position.y,
    };

    // 初始化临时位置
    dragPositionRef.current = position;
  }, [position, onFocus]);

  // 拖拽移动 - 使用 requestAnimationFrame 优化性能
  useEffect(() => {
    if (!isDragging) return;

    const handleMouseMove = (e: MouseEvent) => {
      const newX = e.clientX - dragStartPos.current.x;
      const newY = e.clientY - dragStartPos.current.y;

      // 获取父容器尺寸，限制窗口在父容器内
      const parentEl = windowRef.current?.parentElement;
      const parentWidth = parentEl?.clientWidth || window.innerWidth;
      const parentHeight = parentEl?.clientHeight || window.innerHeight;

      // 窗口必须完全在父容器内可见
      const maxX = parentWidth - size.width;
      const maxY = parentHeight - size.height;
      
      // 最小边界：窗口不能移出父容器
      const minX = 0;
      const minY = 0;

      const boundedX = Math.max(minX, Math.min(newX, maxX));
      const boundedY = Math.max(minY, Math.min(newY, maxY));

      // 存储到 ref，不触发 React 重渲染
      dragPositionRef.current = { x: boundedX, y: boundedY };

      // 取消之前的动画帧（如果有）
      if (rafIdRef.current !== null) {
        cancelAnimationFrame(rafIdRef.current);
      }

      // 使用 requestAnimationFrame 更新视觉位置
      rafIdRef.current = requestAnimationFrame(() => {
        if (windowRef.current) {
          // 使用 transform 代替 left/top，性能更好（只触发重绘，不触发重排）
          windowRef.current.style.transform = `translate(${boundedX}px, ${boundedY}px)`;
        }
      });
    };

    const handleMouseUp = () => {
      // 取消动画帧
      if (rafIdRef.current !== null) {
        cancelAnimationFrame(rafIdRef.current);
        rafIdRef.current = null;
      }

      // 拖拽结束时，更新 state（触发一次重渲染）
      setPosition(dragPositionRef.current);
      setIsDragging(false);
    };

    // 使用 passive 事件监听器，提升性能
    document.addEventListener("mousemove", handleMouseMove, { passive: true });
    document.addEventListener("mouseup", handleMouseUp);

    return () => {
      document.removeEventListener("mousemove", handleMouseMove);
      document.removeEventListener("mouseup", handleMouseUp);
      
      // 清理动画帧
      if (rafIdRef.current !== null) {
        cancelAnimationFrame(rafIdRef.current);
        rafIdRef.current = null;
      }
    };
  }, [isDragging, size]);

  // 调整大小开始
  const handleResizeStart = useCallback((e: React.MouseEvent, direction: string) => {
    e.preventDefault();
    e.stopPropagation();
    setIsResizing(true);
    setResizeDirection(direction);
    resizeStartPos.current = {
      x: e.clientX,
      y: e.clientY,
      width: size.width,
      height: size.height,
      posX: position.x, // 保存窗口位置
      posY: position.y,
    };
  }, [size, position]);

  // 调整大小移动
  useEffect(() => {
    if (!isResizing || !resizeDirection) return;

    const handleMouseMove = (e: MouseEvent) => {
      const deltaX = e.clientX - resizeStartPos.current.x;
      const deltaY = e.clientY - resizeStartPos.current.y;

      let newWidth = resizeStartPos.current.width;
      let newHeight = resizeStartPos.current.height;
      let newX = resizeStartPos.current.posX;
      let newY = resizeStartPos.current.posY;

      // 获取父容器尺寸，限制窗口在父容器内
      const parentEl = windowRef.current?.parentElement;
      const parentWidth = parentEl?.clientWidth || window.innerWidth;
      const parentHeight = parentEl?.clientHeight || window.innerHeight;

      // 根据调整方向计算新尺寸和位置
      if (resizeDirection.includes("e")) {
        const maxWidth = parentWidth - newX;
        newWidth = Math.max(minWidth, Math.min(maxWidth, resizeStartPos.current.width + deltaX));
      }
      if (resizeDirection.includes("w")) {
        const widthDelta = Math.min(deltaX, resizeStartPos.current.width - minWidth);
        newWidth = resizeStartPos.current.width - widthDelta;
        const newXCandidate = resizeStartPos.current.posX + widthDelta;
        newX = Math.max(0, newXCandidate);
      }
      if (resizeDirection.includes("s")) {
        const maxHeight = parentHeight - newY;
        newHeight = Math.max(minHeight, Math.min(maxHeight, resizeStartPos.current.height + deltaY));
      }
      if (resizeDirection.includes("n")) {
        const heightDelta = Math.min(deltaY, resizeStartPos.current.height - minHeight);
        newHeight = resizeStartPos.current.height - heightDelta;
        const newYCandidate = resizeStartPos.current.posY + heightDelta;
        newY = Math.max(0, newYCandidate);
      }

      setSize({ width: newWidth, height: newHeight });
      setPosition({ x: newX, y: newY });
    };

    const handleMouseUp = () => {
      setIsResizing(false);
      setResizeDirection(null);
    };

    document.addEventListener("mousemove", handleMouseMove);
    document.addEventListener("mouseup", handleMouseUp);

    return () => {
      document.removeEventListener("mousemove", handleMouseMove);
      document.removeEventListener("mouseup", handleMouseUp);
    };
  }, [isResizing, resizeDirection, position, minWidth, minHeight]);

  return (
    <div
      ref={windowRef}
      className={`app-window${isActive ? " active" : ""}${isDragging ? " dragging" : ""}`}
      style={{
        position: "absolute",
        left: 0,
        top: 0,
        width: size.width,
        height: size.height,
        zIndex: zIndex, // 使用动态 z-index，确保活动窗口在最上层
        cursor: isDragging ? "move" : "default",
        // 使用 transform 代替 left/top，性能更好（GPU 加速）
        // 同时包含 translate 和 scale，避免 CSS 动画冲突
        transform: `translate(${position.x}px, ${position.y}px) scale(${isOpening ? 0.96 : 1})`,
        // 添加 will-change 提示浏览器优化
        willChange: isDragging ? "transform" : "auto",
        // 窗口打开动画：opacity 和 scale 的平滑过渡
        opacity: isOpening ? 0 : 1,
        transition: isOpening 
          ? "opacity 200ms cubic-bezier(0.25, 0, 0, 1), transform 200ms cubic-bezier(0.25, 0, 0, 1)"
          : "opacity 0.2s ease-out",
      }}
      onMouseDown={(e) => {
        e.stopPropagation(); // 阻止事件传播，防止触发外部窗口移动
        onFocus();
      }}
    >
      {/* HeaderBar */}
      <div
        className="app-window-titlebar"
        onMouseDown={handleDragStart}
        style={{
          cursor: isDragging ? "move" : "move",
          userSelect: "none",
        }}
      >
        <div className="awt-spacer" />
        <span className="awt-title">{title}</span>
        <div className="awt-btns">
          {/* GNOME 标准：黄绿红顺序（最小化、全屏、关闭） */}
          <button
            className="awt-btn minimize"
            onMouseDown={(e) => e.stopPropagation()}
            onClick={(e) => {
              e.stopPropagation();
              onMinimize();
            }}
            title="最小化"
          />
          <button
            className="awt-btn maximize"
            onMouseDown={(e) => e.stopPropagation()}
            onClick={(e) => {
              e.stopPropagation();
              // TODO: 全屏功能
            }}
            title="全屏"
          />
          <button
            className="awt-btn close"
            onMouseDown={(e) => e.stopPropagation()}
            onClick={(e) => {
              e.stopPropagation();
              onClose();
            }}
            title="关闭"
          />
        </div>
      </div>

      {/* Content */}
      <div
        className="app-window-content"
        onMouseDown={() => {
          // GNOME 标准：点击内容区域 → 激活窗口
          // 使用 requestAnimationFrame 延迟状态更新，确保事件处理完成
          requestAnimationFrame(() => {
            onFocus();
          });
        }}
      >
        {children}
      </div>

      {/* Resize Handles */}
      {/* 右边 */}
      <div
        style={{
          position: "absolute",
          right: 0,
          top: 0,
          bottom: 0,
          width: 8,
          cursor: "e-resize",
        }}
        onMouseDown={(e) => handleResizeStart(e, "e")}
      />
      {/* 下边 */}
      <div
        style={{
          position: "absolute",
          bottom: 0,
          left: 0,
          right: 0,
          height: 8,
          cursor: "s-resize",
        }}
        onMouseDown={(e) => handleResizeStart(e, "s")}
      />
      {/* 右下角 */}
      <div
        style={{
          position: "absolute",
          right: 0,
          bottom: 0,
          width: 16,
          height: 16,
          cursor: "se-resize",
        }}
        onMouseDown={(e) => handleResizeStart(e, "se")}
      />
      {/* 左边 */}
      <div
        style={{
          position: "absolute",
          left: 0,
          top: 0,
          bottom: 0,
          width: 8,
          cursor: "w-resize",
        }}
        onMouseDown={(e) => handleResizeStart(e, "w")}
      />
      {/* 上边 */}
      <div
        style={{
          position: "absolute",
          top: 0, // 在 HeaderBar 上方，避免被 HeaderBar 的 onMouseDown 覆盖
          left: 0,
          right: 0,
          height: 8,
          cursor: "n-resize",
          zIndex: 1, // 确保 resize handle 在 HeaderBar 上方
        }}
        onMouseDown={(e) => handleResizeStart(e, "n")}
      />
      {/* 左上角 */}
      <div
        style={{
          position: "absolute",
          left: 0,
          top: 0, // 在 HeaderBar 上方，避免被 HeaderBar 的 onMouseDown 覆盖
          width: 16,
          height: 16,
          cursor: "nw-resize",
          zIndex: 1, // 确保 resize handle 在 HeaderBar 上方
        }}
        onMouseDown={(e) => handleResizeStart(e, "nw")}
      />
      {/* 右上角 */}
      <div
        style={{
          position: "absolute",
          right: 0,
          top: 0, // 在 HeaderBar 上方，避免被 HeaderBar 的 onMouseDown 覆盖
          width: 16,
          height: 16,
          cursor: "ne-resize",
          zIndex: 1, // 确保 resize handle 在 HeaderBar 上方
        }}
        onMouseDown={(e) => handleResizeStart(e, "ne")}
      />
      {/* 左下角 */}
      <div
        style={{
          position: "absolute",
          left: 0,
          bottom: 0,
          width: 16,
          height: 16,
          cursor: "sw-resize",
        }}
        onMouseDown={(e) => handleResizeStart(e, "sw")}
      />
    </div>
  );
}