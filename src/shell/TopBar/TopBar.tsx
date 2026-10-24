import { memo, useCallback } from "react";
import { useTranslation } from "react-i18next";
import type { MouseEvent as ReactMouseEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useServerManager, getStatusColor } from "../../context/ServerManager";
import { formatBytesSafe, formatPercentSafe } from "../../utils/offlineDefaults";
import { NotificationBadge } from "../NotificationCenter";
import { SymbolicIcon } from "../../components/symbolic";
import styles from "./TopBar.module.css";

// 完整的 MetricsSnapshot 类型（匹配 Agent）
interface MetricsSnapshot {
  cpu_percent: number;
  mem_used_bytes: number;
  mem_total_bytes: number;
  swap_used_bytes: number;
  disks: Array<{
    mount_point: string;
    total_bytes: number;
    used_bytes: number;
  }>;
  network_rx_bytes: number;
  network_tx_bytes: number;
  uptime_secs: number;
}

interface TopBarProps {
  metrics: MetricsSnapshot | null;
  clock: string;
  unreadNotifications: number;
  criticalNotifications: number;
  onActivitiesClick: () => void;
  onNotificationClick: () => void;
}

export const TopBar = memo(function TopBar({
  metrics,
  clock,
  unreadNotifications,
  criticalNotifications,
  onActivitiesClick,
  onNotificationClick,
}: TopBarProps) {
  const { t } = useTranslation();
  const { activeServer, servers } = useServerManager();
  // 网络断开自动重连期间 activeServerId 已清空（activeServer 为 null），
  // 回退到 reconnecting 状态的服务器，保持状态指示可见
  const statusServer = activeServer || servers.find((s) => s.status === "reconnecting") || null;

  // OS 窗口拖拽：invoke 单命令 start_os_drag，全屏态由 Rust 单点裁决
  // （拖拽允许性与全屏状态同在 fullscreen 模块，无前端状态/事件链时序盲区）。
  const onBarMouseDown = useCallback(async (e: ReactMouseEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    // 子控件（按钮/输入框）不触发拖拽（对齐原 drag-region 的 target 属性语义）
    if ((e.target as HTMLElement).closest("button, a, input, [data-no-drag]")) return;
    await invoke("start_os_drag").catch((err) =>
      console.warn("[TopBar] 拖拽请求失败:", err)
    );
  }, []);

  return (
    <div className={styles.topBar} onMouseDown={onBarMouseDown}>
      <button
        className={styles.activitiesBtn}
        onClick={onActivitiesClick}
      >
        {t('topbar.activities')}
      </button>
      <div className={styles.separator} />
      <div className={styles.connectionIndicator}>
        <div
          className={styles.connectionDot}
          style={{ background: statusServer ? getStatusColor(statusServer.status) : "#9a9996" }}
        />
        <span>
          {statusServer?.status === "connected" ? t('connection.status.connected') :
           statusServer?.status === "connecting" ? t('connection.status.connecting') :
           statusServer?.status === "reconnecting" ? t('connection.status.reconnecting') :
           statusServer?.status === "error" ? t('connection.status.error') : t('connection.status.disconnected')}
        </span>
        {statusServer && (
          <span className={styles.serverName}>
            {statusServer.name || statusServer.host}
          </span>
        )}
      </div>
      <div className={styles.spacer} />
      
      {/* Metrics */}
      <div className={styles.metrics}>
        <span>CPU {metrics ? formatPercentSafe(metrics.cpu_percent) : "—%"}</span>
        <span>MEM {metrics ? formatBytesSafe(metrics.mem_used_bytes) : "—"}</span>
      </div>
      
      <div className={styles.separator} />
      
      {/* Clock */}
      <div className={styles.clock}>{clock}</div>
      
      {/* Notification */}
      <button className={styles.notificationBtn} onClick={onNotificationClick} aria-label={t('topbar.notifications')}>
        <SymbolicIcon name={unreadNotifications > 0 ? "bell" : "bell-outline"} size={14} />
        <NotificationBadge
          count={unreadNotifications}
          criticalCount={criticalNotifications}
        />
      </button>
    </div>
  );
});