// src/components/ConnectionState.tsx
// 连接状态覆盖层（全应用统一）：
// - ConnectionErrorState：status=error 时的完整错误态（图标/标题/消息/建议/对比提示/重试）
// - ReconnectingState：status=reconnecting 时的轻量自动重连指示

import { SymbolicIcon } from "./symbolic";
import "./ConnectionState.css";

export interface ConnectionErrorStateProps {
  title: string;
  message: string;
  /** 行动建议（ERROR_MAP.action） */
  action?: string;
  /** 其他服务器连接正常时的对比提示（多行） */
  hint?: string;
  /** 错误细节 */
  detail?: string;
  onRetry: () => void;
  onOpenSettings?: () => void;
}

export function ConnectionErrorState({
  title, message, action, hint, detail, onRetry, onOpenSettings,
}: ConnectionErrorStateProps) {
  return (
    <div className="cs-error">
      <SymbolicIcon name="network-offline" size={48} className="cs-error-icon" />
      <div className="cs-error-title">{title}</div>
      <div className="cs-error-message">
        {message}{detail ? `（${detail}）` : ""}
      </div>
      {action && <div className="cs-error-action">{action}</div>}
      {hint && (
        <div className="cs-error-hint">
          <SymbolicIcon name="dialog-information" size={12} />
          <span>{hint}</span>
        </div>
      )}
      <div className="cs-error-buttons">
        <button className="cs-btn-primary" onClick={onRetry}>
          <SymbolicIcon name="view-refresh" size={14} />
          重试
        </button>
        {onOpenSettings && (
          <button className="cs-btn-ghost" onClick={onOpenSettings}>查看服务器</button>
        )}
      </div>
    </div>
  );
}

export interface ReconnectingStateProps {
  serverName?: string;
}

export function ReconnectingState({ serverName }: ReconnectingStateProps) {
  return (
    <div className="cs-reconnecting">
      <SymbolicIcon name="view-refresh" size={16} className="cs-reconnecting-icon" />
      <span>{serverName ? `正在重新连接到 ${serverName}…` : "正在重新连接…"}</span>
    </div>
  );
}
