import { memo } from "react";
import { useServerManager, getStatusColor } from "../../context/ServerManager";
import { formatBytesSafe, formatPercentSafe } from "../../utils/offlineDefaults";
import { NotificationBadge } from "../NotificationCenter";
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
  const { activeServer } = useServerManager();

  return (
    <div className={styles.topBar} data-tauri-drag-region>
      <button
        className={styles.activitiesBtn}
        onClick={onActivitiesClick}
      >
        活动
      </button>
      <div className={styles.separator} />
      <div className={styles.connectionIndicator}>
        <div
          className={styles.connectionDot}
          style={{ background: activeServer ? getStatusColor(activeServer.status) : "#9a9996" }}
        />
        <span>
          {activeServer?.status === "connected" ? "已连接" :
           activeServer?.status === "connecting" ? "连接中..." :
           activeServer?.status === "error" ? "连接失败" : "未连接"}
        </span>
        {activeServer && (
          <span className={styles.serverName}>
            {activeServer.name || activeServer.host}
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
      <button className={styles.notificationBtn} onClick={onNotificationClick}>
        🔔
        <NotificationBadge
          unread={unreadNotifications}
          critical={criticalNotifications}
        />
      </button>
    </div>
  );
});