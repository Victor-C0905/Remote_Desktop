// src/components/Dialog/Dialog.tsx
// 应用内模态弹窗统一底座：portal 到 body + 遮罩 + Esc/遮罩点击关闭
// 视觉基准与 FileManager 属性对话框（pd-*）一致，全项目弹窗共用此底座收敛样式
import { useEffect, useRef, useState } from "react";
import type { ReactNode, PointerEvent as ReactPointerEvent } from "react";
import { createPortal } from "react-dom";
import "./Dialog.css";

interface DialogProps {
  /** Esc / 点击遮罩时触发（关闭语义；disableDismiss 时不会触发） */
  onClose: () => void;
  /** 执行中不可关闭（保存/解压进行时防止丢失反馈），与属性对话框 permSaving 同语义 */
  disableDismiss?: boolean;
  /** 卡片宽度（默认 420，与属性对话框一致） */
  width?: number;
  /**
   * 可拖动：按住卡片空白处/标题拖动（表单控件、按钮等交互元素不触发），
   * 用于需要临时让出背后内容的浮层（如目录选择器）
   */
  draggable?: boolean;
  /**
   * 点击遮罩是否关闭（默认 true）。
   * 置 false 时等同 Windows 模态对话框：未关闭前宿主应用不可操作，
   * 点遮罩不关闭（Esc 仍可取消）
   */
  dismissOnOverlay?: boolean;
  children: ReactNode;
}

/** 拖动起点与可移动范围（保证卡片不拖出视口） */
interface DragOrigin {
  pointerX: number;
  pointerY: number;
  offsetX: number;
  offsetY: number;
  maxX: number;
  maxY: number;
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

/** 当前打开的模态弹窗数量（宿主应用的快捷键据此让位） */
let openDialogCount = 0;

/** 是否有模态弹窗打开：应用级快捷键应先检查此标记，保证弹窗期间宿主不可用 */
export function isDialogOpen(): boolean {
  return openDialogCount > 0;
}

export function Dialog({
  onClose, disableDismiss, width = 420, draggable, dismissOnOverlay = true, children,
}: DialogProps) {
  const [offset, setOffset] = useState({ x: 0, y: 0 });
  const [dragging, setDragging] = useState(false);
  const dragOrigin = useRef<DragOrigin | null>(null);
  const cardRef = useRef<HTMLDivElement>(null);

  // 弹窗期间宿主应用视为不可用：登记打开数量（快捷键拦截依据）+ 焦点锁定在弹窗内
  useEffect(() => {
    openDialogCount += 1;
    const card = cardRef.current;
    const focusables = () => card
      ? Array.from(card.querySelectorAll<HTMLElement>(
        'button:not(:disabled), [href], input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex="-1"])',
      ))
      : [];
    // 已有 autoFocus 元素时不抢焦点（ConfirmDialog 危险操作默认聚焦取消按钮）
    if (card && !card.contains(document.activeElement)) {
      const firstInput = card.querySelector<HTMLElement>(
        'input:not(:disabled), textarea:not(:disabled), select:not(:disabled)',
      );
      (firstInput ?? focusables()[0])?.focus();
    }
    const trapTab = (e: KeyboardEvent) => {
      if (e.key !== 'Tab' || !card) return;
      const items = focusables();
      if (items.length === 0) {
        e.preventDefault();
        return;
      }
      const first = items[0];
      const last = items[items.length - 1];
      const active = document.activeElement as HTMLElement | null;
      if (!active || !card.contains(active)) {
        e.preventDefault();
        (e.shiftKey ? last : first).focus();
        return;
      }
      if (e.shiftKey && active === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && active === last) {
        e.preventDefault();
        first.focus();
      }
    };
    document.addEventListener('keydown', trapTab, true);
    return () => {
      openDialogCount -= 1;
      document.removeEventListener('keydown', trapTab, true);
    };
  }, []);

  /** 交互控件上不启动拖动，避免与输入/按钮点击冲突 */
  const handlePointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (!draggable) return;
    const target = e.target as HTMLElement;
    if (target.closest('input, textarea, select, button, a, [role="button"], [contenteditable="true"]')) return;
    e.preventDefault();
    const rect = e.currentTarget.getBoundingClientRect();
    dragOrigin.current = {
      pointerX: e.clientX,
      pointerY: e.clientY,
      offsetX: offset.x,
      offsetY: offset.y,
      // 卡片居中于视口：最大平移量 = (视口尺寸 - 卡片尺寸) / 2
      maxX: Math.max(0, (window.innerWidth - rect.width) / 2),
      maxY: Math.max(0, (window.innerHeight - rect.height) / 2),
    };
    e.currentTarget.setPointerCapture(e.pointerId);
    setDragging(true);
  };

  const handlePointerMove = (e: ReactPointerEvent<HTMLDivElement>) => {
    const origin = dragOrigin.current;
    if (!origin) return;
    setOffset({
      x: clamp(origin.offsetX + e.clientX - origin.pointerX, -origin.maxX, origin.maxX),
      y: clamp(origin.offsetY + e.clientY - origin.pointerY, -origin.maxY, origin.maxY),
    });
  };

  const endDrag = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (!dragOrigin.current) return;
    dragOrigin.current = null;
    setDragging(false);
    if (e.currentTarget.hasPointerCapture(e.pointerId)) {
      e.currentTarget.releasePointerCapture(e.pointerId);
    }
  };

  // Esc 关闭（与 pd-overlay 点遮罩关闭同一交互模式）
  useEffect(() => {
    if (disableDismiss) return;
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, disableDismiss]);

  // data-browser-overlay：浏览器 webview 子窗口让位标记（child HWND 无法被
  // DOM z-index 遮挡），BrowserApp 检测到即隐藏 webview 让位
  return createPortal(
    <div
      className="dg-overlay"
      data-browser-overlay
      /* e.target 判断：点遮罩本身才关闭，点卡片内容冒泡上来不关闭；
         dismissOnOverlay=false 时遮罩只拦截宿主交互，不关闭弹窗 */
      onClick={(e) => {
        if (dismissOnOverlay && !disableDismiss && e.target === e.currentTarget) onClose();
      }}
    >
      <div
        ref={cardRef}
        className={`dg-card${draggable ? " dg-card-draggable" : ""}${dragging ? " dg-card-dragging" : ""}`}
        style={draggable ? { width, transform: `translate(${offset.x}px, ${offset.y}px)` } : { width }}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
      >
        {children}
      </div>
    </div>,
    document.body
  );
}
