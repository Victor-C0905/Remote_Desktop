import { memo, useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { MouseEvent as ReactMouseEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useServerManager, getStatusColor } from "../../context/ServerManager";
import { formatBytesSafe, formatPercentSafe } from "../../utils/offlineDefaults";
import { NotificationBadge } from "../NotificationCenter";
import { ToolbarMenu, type MenuEntry } from "../../components/ToolbarMenu/ToolbarMenu";
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

  // 最大化状态：由 Rust Resized 段探测（app-maximize-changed），驱动
  // 窗口控制菜单「最大化/还原」的图标与文案切换（与最小化闸门同路径）
  const [maximized, setMaximized] = useState(false);
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    listen<boolean>("app-maximize-changed", (e) => setMaximized(e.payload))
      .then((f) => {
        if (disposed) f();
        else unlisten = f;
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  // 全屏状态：toggle_fullscreen 完成时发出（F11 与网页全屏联动同源），
  // 驱动菜单项「进入全屏/退出全屏」的图标与文案切换
  const [fullscreen, setFullscreen] = useState(false);
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    listen<boolean>("fullscreen://changed", (e) => setFullscreen(e.payload))
      .then((f) => {
        if (disposed) f();
        else unlisten = f;
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  // 窗口控制菜单：无边框模式下系统标题栏不再存在，最小化/最大化/关闭
  // 经独立命令作用于主窗口；全屏复用既有 toggle_fullscreen（F11 同源）
  const windowMenu: MenuEntry[] = [
    {
      label: fullscreen ? t("topbar.exitFullscreen") : t("topbar.enterFullscreen"),
      shortcut: "F11",
      icon: <SymbolicIcon name={fullscreen ? "view-restore" : "view-fullscreen"} size={16} />,
      onClick: () => void invoke("toggle_fullscreen").catch(() => {}),
    },
    {
      label: t("topbar.minimize"),
      icon: <SymbolicIcon name="window-minimize" size={16} />,
      onClick: () => void invoke("window_minimize").catch(() => {}),
    },
    {
      label: maximized ? t("topbar.restore") : t("topbar.maximize"),
      icon: <SymbolicIcon name={maximized ? "window-restore" : "window-maximize"} size={16} />,
      onClick: () => void invoke("window_toggle_maximize").catch(() => {}),
    },
    { separator: true },
    {
      label: t("topbar.close"),
      icon: <SymbolicIcon name="window-close" size={16} />,
      danger: true,
      onClick: () => void invoke("window_close").catch(() => {}),
    },
  ];

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
        {/* 服务器名槽位常驻：连接瞬间才插入该字段会让按钮宽度跳变、
            挤压顶栏布局；未连接时以占位符占位（宽度由 CSS 固定） */}
        <span className={styles.serverName}>
          {statusServer ? statusServer.name || statusServer.host : "—"}
        </span>
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

      {/* 窗口控制菜单（无边框模式：最小化/最大化/关闭 + 全屏，⌄ 触发按钮
          为 SVG mask 几何居中，与通知铃同基线；按钮自身不触发拖拽——
          onBarMouseDown 已排除 button 元素） */}
      <ToolbarMenu title={t('topbar.windowControls')} icon="⌄" items={windowMenu} />
    </div>
  );
});