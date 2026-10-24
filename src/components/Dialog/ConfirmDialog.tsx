// src/components/Dialog/ConfirmDialog.tsx
// 确认询问弹窗（基于 Dialog 底座）：图标头部 + 主问句 + 补充说明 + 取消/确认按钮
// 头部与属性对话框 pd-header（icon+name）同构；
// 确认按钮统一为主题 accent 实心（主操作视觉语言），危险语义由红色警示图标传达
import { useTranslation } from "react-i18next";
import { Dialog } from "./Dialog";

interface ConfirmDialogProps {
  title: string;
  /** 主问句（text-primary，如：确定删除 "x"？） */
  message: string;
  /** 主问句中需强调的片段（如文件名），渲染为粗体 */
  highlight?: string;
  /** 补充警示（小字弱化，如：此操作无法撤销。） */
  detail?: string;
  confirmLabel?: string;
  cancelLabel?: string;
  /** 破坏性操作：红色警示图标 + 初始焦点落在"取消"上防回车误触 */
  danger?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

/** 把 message 按 highlight 切分，强调片段渲染为粗体 */
function renderMessage(message: string, highlight?: string): React.ReactNode {
  if (!highlight || !message.includes(highlight)) return message;
  const parts = message.split(highlight);
  return parts.flatMap((part, i) =>
    i === 0 ? [part] : [<strong key={i} className="dg-emphasis">{highlight}</strong>, part]
  );
}

export function ConfirmDialog({
  title, message, highlight, detail,
  confirmLabel, cancelLabel, danger,
  onConfirm, onCancel,
}: ConfirmDialogProps) {
  const { t } = useTranslation();
  // 内置默认值改走词典（调用方显式传入时优先）
  const confirmText = confirmLabel ?? t("common.confirm");
  const cancelText = cancelLabel ?? t("common.cancel");
  return (
    <Dialog onClose={onCancel}>
      {/* 图标头部：danger 红色警示 / 普通询问 accent 圆标 */}
      <div className="dg-header">
        <div className={`dg-header-icon ${danger ? "dg-header-icon-danger" : "dg-header-icon-default"}`}>
          {danger ? "!" : "?"}
        </div>
        <div className="dg-header-title">{title}</div>
      </div>
      <div className={`dg-message${detail ? " dg-message-compact" : ""}`}>
        {renderMessage(message, highlight)}
      </div>
      {detail && <div className="dg-detail">{detail}</div>}
      <div className="dg-actions">
        {/* 幽灵按钮：透明底细边框，hover 底色浮出（项目统一按钮视觉语言） */}
        <button className="dg-btn" onClick={onCancel} autoFocus={danger}>{cancelText}</button>
        {/* 主操作：accent 实心（主题强调色，随主题变换） */}
        <button className="dg-btn-primary" onClick={onConfirm} autoFocus={!danger}>
          {confirmText}
        </button>
      </div>
    </Dialog>
  );
}
