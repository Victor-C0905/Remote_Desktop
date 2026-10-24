// src/components/Dialog/Dialog.tsx
// 应用内模态弹窗统一底座：portal 到 body + 遮罩 + Esc/遮罩点击关闭
// 视觉基准与 FileManager 属性对话框（pd-*）一致，全项目弹窗共用此底座收敛样式
import { useEffect } from "react";
import type { ReactNode } from "react";
import { createPortal } from "react-dom";
import "./Dialog.css";

interface DialogProps {
  /** Esc / 点击遮罩时触发（关闭语义；disableDismiss 时不会触发） */
  onClose: () => void;
  /** 执行中不可关闭（保存/解压进行时防止丢失反馈），与属性对话框 permSaving 同语义 */
  disableDismiss?: boolean;
  /** 卡片宽度（默认 420，与属性对话框一致） */
  width?: number;
  children: ReactNode;
}

export function Dialog({ onClose, disableDismiss, width = 420, children }: DialogProps) {
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
      /* e.target 判断：点遮罩本身才关闭，点卡片内容冒泡上来不关闭 */
      onClick={(e) => { if (!disableDismiss && e.target === e.currentTarget) onClose(); }}
    >
      <div className="dg-card" style={{ width }}>
        {children}
      </div>
    </div>,
    document.body
  );
}
