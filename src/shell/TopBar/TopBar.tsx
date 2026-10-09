import { memo } from "react";
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
  const { activeServer, servers } = useServerManager();
  // 网络断开自动重连期间 activeServerId 已清空（activeServer 为 null），
  // 回退到 reconnecting 状态的服务器，保持状态指示可见
  const statusServer = activeServer || servers.find((s) => s.status === "reconnecting") || null;

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
          style={{ background: statusServer ? getStatusColor(statusServer.status) : "#9a9996" }}
        />
        <span>
          {statusServer?.status === "connected" ? "已连接" :
           statusServer?.status === "connecting" ? "连接中..." :
           statusServer?.status === "reconnecting" ? "重连中..." :
           statusServer?.status === "error" ? "连接失败" : "未连接"}
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
      <button className={styles.notificationBtn} onClick={onNotificationClick} aria-label="通知">
        <SymbolicIcon name={unreadNotifications > 0 ? "bell" : "bell-outline"} size={14} />
        <NotificationBadge
          count={unreadNotifications}
          criticalCount={criticalNotifications}
        />
      </button>
    </div>
  );
});