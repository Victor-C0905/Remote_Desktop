import { useState, useEffect, useMemo, useRef } from "react";
import { useTranslation } from "react-i18next";
import { useServerManager } from "../context/ServerManager";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import {
  formatBytes,
  swapDisplay,
  toFsRow,
  sortProcesses,
  toggleProcessSort,
  DEFAULT_PROCESS_SORT,
  type ProcessInfo,
  type ProcessSortKey,
  type ProcessSortState,
  type MountInfo,
} from "./systemMonitor/display";
import { formatUnknownError } from "../types/errors";
import { SUBSCRIPTION_CONFIG } from "../config/subscription";
import {
  OFFLINE_METRICS,
  formatBytesSafe,
  formatPercentSafe,
  formatUptimeSafe,
  formatSpeedSafe,
} from "../utils/offlineDefaults";
import { MonitorSkeleton } from "../components/skeleton/MonitorSkeleton";
import { createLogger } from '../utils/logger';
import { StatsPanel } from "../components/StatsPanel";
import { SymbolicIcon } from "../components/symbolic";
import { ConnectionErrorState, ReconnectingState } from "../components/ConnectionState";
import { AppTopbar } from "../components/app-shell/AppTopbar";
import { getServerErrorInfo } from "../stores/serversStore";
import { useOpenApp } from "../window-system/hooks/useOpenApp";
// import { useWindowState } from "../window-system/hooks/useWindowState"; // 未来集成时使用
import "./SystemMonitor.css";

const log = createLogger('SystemMonitor');

interface MetricsSnapshot {
  cpu_percent: number;
  mem_used_bytes: number;
  mem_total_bytes: number;
  swap_used_bytes: number;
  // 交换分区总量（协议 #[serde(default)] 扩展字段；0 = 旧 Agent 或未配置 swap）
  swap_total_bytes: number;
  disks: DiskInfo[];
  network_rx_bytes: number;
  network_tx_bytes: number;
  uptime_secs: number;
}

interface DiskInfo {
  mount_point: string;
  total_bytes: number;
  used_bytes: number;
}

interface HistoryPoint {
  time: number;
  value: number;
}

type TabId = "processes" | "resources" | "filesystems" | "stats";

/** 进程表头列定义（key 同时用作排序键，对齐 FileManager FILE_COLUMNS 模式；
 *  PID/CPU 为通用缩写不进词典，其余走 labelKey） */
const PROCESS_COLUMNS: { key: ProcessSortKey; label: string; labelKey?: string; cellClass: string }[] = [
  { key: "pid", label: "PID", cellClass: "sm-ph-pid" },
  { key: "name", label: "", labelKey: "monitor.colName", cellClass: "sm-ph-name" },
  { key: "user", label: "", labelKey: "monitor.colUser", cellClass: "sm-ph-user" },
  { key: "cpu", label: "CPU", cellClass: "sm-ph-cpu" },
  { key: "mem", label: "", labelKey: "monitor.colMem", cellClass: "sm-ph-mem" },
  { key: "state", label: "", labelKey: "monitor.colState", cellClass: "sm-ph-state" },
];

function formatPercent(value: number): string {
  return value.toFixed(1) + "%";
}

// 进程列表轮询间隔：页签可见 + 能力支持时按需拉取，离开页签即停
const PROCESSES_REFRESH_MS = 3000;

/* ── Mini Chart Component (Canvas-based) ─────────────── */

interface MiniChartProps {
  data: HistoryPoint[];
  color: string;
  height: number;
  max?: number;
}

function MiniChart({ data, color, height, max }: MiniChartProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const width = 200;

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || data.length < 2) return;

    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    ctx.clearRect(0, 0, width, height);

    const maxValue = max || Math.max(...data.map(d => d.value), 100);
    const padding = 2;

    ctx.strokeStyle = color;
    ctx.lineWidth = 1.5;
    ctx.beginPath();

    data.forEach((point, i) => {
      const x = padding + (i / (data.length - 1)) * (width - padding * 2);
      const y = height - padding - (point.value / maxValue) * (height - padding * 2);
      if (i === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    });

    ctx.stroke();

    ctx.fillStyle = color + "40";
    ctx.beginPath();
    data.forEach((point, i) => {
      const x = padding + (i / (data.length - 1)) * (width - padding * 2);
      const y = height - padding - (point.value / maxValue) * (height - padding * 2);
      if (i === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    });
    ctx.lineTo(width - padding, height - padding);
    ctx.lineTo(padding, height - padding);
    ctx.closePath();
    ctx.fill();
  }, [data, color, height, max]);

  return <canvas ref={canvasRef} width={width} height={height} className="mini-chart" />;
}

/* ── Main Component ───────────────────────────────────── */

export function SystemMonitor({ windowId: _windowId }: { windowId: string }) {
  const { t } = useTranslation();
  // 窗口系统集成（未来可能需要使用 windowState）
  // const windowState = useWindowState(windowId);
  const { activeServerId, activeServer, servers, connectServer } = useServerManager();
  const openApp = useOpenApp();

  // 断连故障服务器（activeServerId 已清空，按状态识别；reconnecting 优先于 error）
  const troubledServer =
    servers.find((s) => s.status === "reconnecting") ||
    servers.find((s) => s.status === "error") ||
    null;

  const [activeTab, setActiveTab] = useState<TabId>("resources");
  // 永远不为 null：离线时 = OFFLINE_METRICS，在线时 = 真实数据
  const [metrics, setMetrics] = useState<MetricsSnapshot | typeof OFFLINE_METRICS>(OFFLINE_METRICS);
  const [processes, setProcesses] = useState<ProcessInfo[]>([]);
  // 能力门控：null=查询中，false=旧 Agent（绝不盲发——盲发会断流）
  const [procSupported, setProcSupported] = useState<boolean | null>(null);
  const [procError, setProcError] = useState<string | null>(null);
  // 文件系统页签：remote_get_mounts 真实挂载点（设备名/文件系统类型来自 sysinfo，
  // 是 2026-06 就存在的老命令，早于能力系统，所有存量 Agent 均支持，无需门控）
  const [mounts, setMounts] = useState<MountInfo[]>([]);
  // 进程排序：列头点击切换（对齐 FileManager，状态机见 display.ts）
  const [processSort, setProcessSort] = useState<ProcessSortState>(DEFAULT_PROCESS_SORT);
  // 是否已收到过真实在线数据（用于区分「从未连接」和「断连后」）
  const [hasReceivedData, setHasReceivedData] = useState(false);

  const HISTORY_LENGTH = SUBSCRIPTION_CONFIG.HISTORY_LENGTH; // 使用配置的历史长度

  // 使用 useRef 来存储历史数据，避免每次创建新数组
  const cpuHistoryRef = useRef<HistoryPoint[]>([]);
  const memHistoryRef = useRef<HistoryPoint[]>([]);
  const historyIndexRef = useRef<number>(0);

  // 使用 useMemo 来创建固定大小的数组
  const [cpuHistory, setCpuHistory] = useState<HistoryPoint[]>([]);
  const [memHistory, setMemHistory] = useState<HistoryPoint[]>([]);

  // 网络速率计算：存储上一次的网络字节数和时间戳
  const lastNetworkRxRef = useRef<number>(0);
  const lastNetworkTxRef = useRef<number>(0);
  const lastNetworkTimeRef = useRef<number>(0);
  const [networkRxSpeed, setNetworkRxSpeed] = useState<number>(0); // bytes/s
  const [networkTxSpeed, setNetworkTxSpeed] = useState<number>(0); // bytes/s

  // ── 连接状态变化：断连时立即切到离线占位符 ─────────
  useEffect(() => {
    if (!activeServerId) {
      // 断连/无连接 → 立即切换为离线占位符（同步，UI 即时响应）
      setMetrics(OFFLINE_METRICS);
      setProcesses([]);
      setProcSupported(null);
      setProcError(null);
      setMounts([]);
      setHasReceivedData(false);
      // 清空历史数据（重连后重新积累）
      cpuHistoryRef.current = [];
      memHistoryRef.current = [];
      setCpuHistory([]);
      setMemHistory([]);
      // 重置网络速率计算
      lastNetworkRxRef.current = 0;
      lastNetworkTxRef.current = 0;
      lastNetworkTimeRef.current = 0;
      setNetworkRxSpeed(0);
      setNetworkTxSpeed(0);
    }
  }, [activeServerId]);

  // 监听系统指标事件（由 ServerManager 自动订阅）
  useEffect(() => {
    if (!activeServerId) return;

    const setupListener = async () => {
      const unlisten = await listen<{ server_id: string; event_type: string; data: MetricsSnapshot }>(
        'subscription_event',
        (event) => {
          if (event.payload.server_id === activeServerId && event.payload.event_type === 'metrics') {
            const newMetrics = event.payload.data;
            setMetrics(newMetrics);
            // 标记已收到在线数据（用于 UI 区分「从未连接」和「断连后」）
            if (!hasReceivedData) {
              setHasReceivedData(true);
            }
            const now = Date.now();

            // 使用环形缓冲区更新历史数据
            const cpuPoint = { time: now, value: newMetrics.cpu_percent };
            const _memPercent = (newMetrics.mem_used_bytes / newMetrics.mem_total_bytes) * 100;
            const memPoint = { time: now, value: _memPercent };

            // 更新环形缓冲区
            if (cpuHistoryRef.current.length < HISTORY_LENGTH) {
              cpuHistoryRef.current.push(cpuPoint);
              memHistoryRef.current.push(memPoint);
            } else {
              cpuHistoryRef.current[historyIndexRef.current] = cpuPoint;
              memHistoryRef.current[historyIndexRef.current] = memPoint;
              historyIndexRef.current = (historyIndexRef.current + 1) % HISTORY_LENGTH;
            }

            // 触发重新渲染（复制数组）
            setCpuHistory([...cpuHistoryRef.current]);
            setMemHistory([...memHistoryRef.current]);

            // 计算网络速率（bytes/s）
            const timeDiff = (now - lastNetworkTimeRef.current) / 1000; // 秒
            if (timeDiff > 0 && lastNetworkTimeRef.current > 0) {
              const rxDiff = newMetrics.network_rx_bytes - lastNetworkRxRef.current;
              const txDiff = newMetrics.network_tx_bytes - lastNetworkTxRef.current;

              // 避免负数（可能是网络接口重启或计数器重置）
              const rxSpeed = rxDiff > 0 ? rxDiff / timeDiff : 0;
              const txSpeed = txDiff > 0 ? txDiff / timeDiff : 0;

              log.debug('网络速率计算:', {
                rxDiff,
                txDiff,
                timeDiff,
                rxSpeed,
                txSpeed,
              });

              setNetworkRxSpeed(rxSpeed);
              setNetworkTxSpeed(txSpeed);
            }

            // 更新上一次的值
            lastNetworkRxRef.current = newMetrics.network_rx_bytes;
            lastNetworkTxRef.current = newMetrics.network_tx_bytes;
            lastNetworkTimeRef.current = now;
          }
        }
      );
      return unlisten;
    };

    let unlistenFn: (() => void) | undefined;
    setupListener().then((fn) => {
      unlistenFn = fn;
    }).catch((e) => log.error('事件监听设置失败:', e));

    return () => {
      if (unlistenFn) {
        unlistenFn();
      }
    };
  }, [activeServerId, HISTORY_LENGTH]);

  // 移除原有的轮询逻辑（fetchMetrics 和 setInterval）

  // 能力查询：决定进程页签是真实数据还是「不支持」空态
  useEffect(() => {
    if (!activeServerId) {
      setProcSupported(null);
      return;
    }
    let cancelled = false;
    setProcSupported(null);
    invoke<string[]>("get_agent_capabilities", { serverId: activeServerId })
      .then((caps) => {
        if (!cancelled) setProcSupported(caps.includes("processes"));
      })
      .catch(() => {
        // 查询失败按不支持处理（空态），绝不发送 GetProcesses
        if (!cancelled) setProcSupported(false);
      });
    return () => {
      cancelled = true;
    };
  }, [activeServerId]);

  // 进程拉取：页签可见 + 能力支持时 3s 轮询；切页签/断连/卸载即停
  useEffect(() => {
    if (activeTab !== "processes" || !activeServerId || procSupported !== true) return;
    let cancelled = false;
    const fetchOnce = () => {
      invoke<ProcessInfo[]>("get_processes", { serverId: activeServerId })
        .then((list) => {
          if (!cancelled) {
            setProcesses(list);
            setProcError(null);
          }
        })
        .catch((e) => {
          if (!cancelled) setProcError(formatUnknownError(e));
        });
    };
    fetchOnce();
    const timer = setInterval(fetchOnce, PROCESSES_REFRESH_MS);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [activeTab, activeServerId, procSupported]);

  // 文件系统页签：切到时拉取一次真实挂载点（挂载信息变化低频，无需定时刷新）
  useEffect(() => {
    if (activeTab !== "filesystems" || !activeServerId) return;
    let cancelled = false;
    invoke<MountInfo[]>("remote_get_mounts", { serverId: activeServerId })
      .then((m) => {
        if (!cancelled) setMounts(m);
      })
      .catch((e) => log.error("获取挂载点失败:", e));
    return () => {
      cancelled = true;
    };
  }, [activeTab, activeServerId]);

  // 排序纯函数在 display.ts（单测覆盖）；渲染层只做数据编排
  const sortedProcesses = useMemo(
    () => sortProcesses(processes, processSort),
    [processes, processSort]
  );

  // ── 离线判断 ──
  const isOffline = '_offline' in metrics && (metrics as typeof OFFLINE_METRICS)._offline === true;

  // ── 三层状态门控 ─────────────────────────────────────
  // 状态机: 骨架屏(加载中) → 真实数据(在线) → 离线占位符(断连/无连接)
  //   - 无 activeServerId        → 离线占位符（—）
  //   - 有 activeServerId 但未收到数据 → 骨架屏（shimmer）
  //   - 有 activeServerId 且已收到数据 → 真实内容
  const showSkeleton = !!activeServerId && !hasReceivedData;
  const showOffline = !activeServerId;

  // 资源页签 Swap 卡片展示三元组（渲染前统一计算，避免渲染处三次重复调用）
  const swap = swapDisplay(metrics.swap_used_bytes, metrics.swap_total_bytes, isOffline);

  return (
    <div className="sm">
      {/* 应用内顶栏 — 对齐 TextEditor（共享 AppTopbar）：页签直接置左
          （原「视图/排序」菜单已移除——视图与右侧页签重复，排序改为
          进程列表列头点击排序，对齐 FileManager 交互）；固定在内容区
          上方，不随内容滚动 */}
      <AppTopbar>
        {/* 视图页签 */}
        <div className="sm-tabs">
          <button
            className={`sm-tab${activeTab === "processes" ? " active" : ""}`}
            onClick={() => setActiveTab("processes")}
          >
            {t('monitor.tabProcesses')}
          </button>
          <button
            className={`sm-tab${activeTab === "resources" ? " active" : ""}`}
            onClick={() => setActiveTab("resources")}
          >
            {t('monitor.tabResources')}
          </button>
          <button
            className={`sm-tab${activeTab === "filesystems" ? " active" : ""}`}
            onClick={() => setActiveTab("filesystems")}
          >
            {t('monitor.tabFilesystems')}
          </button>
          <button
            className={`sm-tab${activeTab === "stats" ? " active" : ""}`}
            onClick={() => setActiveTab("stats")}
          >
            {t('monitor.tabStats')}
          </button>
        </div>
      </AppTopbar>

      {/* Content — 三层状态门控 */}
      <div className="sm-content">
        {showOffline ? (
          troubledServer ? (
            /* 连接中断：自动重连轻量态 / 最终失败错误态 */
            <div className="sm-offline-state">
              <MonitorSkeleton />
              <div className="sm-offline-overlay">
                {troubledServer.status === "reconnecting" ? (
                  <ReconnectingState serverName={troubledServer.name || troubledServer.host} />
                ) : (
                  <ConnectionErrorState
                    {...getServerErrorInfo(troubledServer)}
                    onRetry={() => connectServer(troubledServer.id)}
                    onOpenSettings={() => openApp("settings")}
                  />
                )}
              </div>
            </div>
          ) : (
          /* 层 3: 离线占位符（从未连接/用户主动断开） */
          <div className="sm-offline-state">
            <MonitorSkeleton />
            <div className="sm-offline-overlay">
              <div className="offline-badge">
                <SymbolicIcon name="network-offline" size={16} className="offline-badge-icon" />
                {t('monitor.notConnected')}
              </div>
              <div className="offline-hint">{t('monitor.offlineHint')}</div>
            </div>
          </div>
          )
        ) : showSkeleton ? (
          /* 层 1: 骨架屏（有连接但等待首条数据） */
          <MonitorSkeleton />
        ) : (
          /* 层 2: 真实数据内容（已收到在线数据） */
          <>
            {activeTab === "processes" && (
          <div className="sm-processes">
            {/* 列头点击排序（对齐 FileManager）：同列翻转方向，异列取默认方向 */}
            <div className="sm-process-header">
              {PROCESS_COLUMNS.map((col) => (
                <span
                  key={col.key}
                  className={`${col.cellClass}${processSort.key === col.key ? " sorted" : ""}`}
                  onClick={() => setProcessSort((prev) => toggleProcessSort(prev, col.key))}
                >
                  {col.labelKey ? t(col.labelKey) : col.label}
                  {processSort.key === col.key && (
                    <SymbolicIcon
                      name={processSort.dir === "asc" ? "chevron-up" : "chevron-down"}
                      size={12}
                      className="sm-sort-indicator"
                    />
                  )}
                </span>
              ))}
            </div>
            <div className="sm-process-list">
              {procSupported === false ? (
                <div style={{ padding: '20px 0', color: 'var(--quirel-text-disabled)', textAlign: 'center' }}>
                  {t('monitor.processUnsupported')}
                </div>
              ) : procError ? (
                <div style={{ padding: '20px 0', color: 'var(--quirel-text-error, var(--quirel-text-secondary))', textAlign: 'center' }}>
                  {procError}
                </div>
              ) : (
                sortedProcesses.map((proc) => (
                  <div key={proc.pid} className="sm-process-row">
                    <span className="sm-pr-pid">{proc.pid}</span>
                    <span className="sm-pr-name">{proc.name}</span>
                    <span className="sm-pr-user">{proc.user}</span>
                    <span className="sm-pr-cpu">{formatPercentSafe(proc.cpu_percent, isOffline)}</span>
                    <span className="sm-pr-mem">{formatPercentSafe(proc.mem_percent, isOffline)}</span>
                    <span className="sm-pr-state">{proc.state}</span>
                  </div>
                ))
              )}
            </div>
            <div className="sm-process-footer">
              <span>{t('monitor.processCount', { count: processes.length })}</span>
            </div>
          </div>
        )}

        {activeTab === "resources" && (
          <div className="sm-resources">
            {/* CPU & Memory Section */}
            <div className="sm-res-section">
              <div className="sm-res-card">
                <div className="sm-res-title">{t('monitor.cpuHistory')}</div>
                <div className="sm-res-chart">
                  <MiniChart data={cpuHistory} color="#3584e4" height={60} max={100} />
                </div>
                <div className="sm-res-stats">
                  <span className="sm-res-value">{formatPercentSafe(metrics.cpu_percent, isOffline)}</span>
                  <span className="sm-res-label">{t('monitor.currentUsage')}</span>
                </div>
              </div>

              <div className="sm-res-card">
                <div className="sm-res-title">{t('monitor.memHistory')}</div>
                <div className="sm-res-chart">
                  <MiniChart data={memHistory} color="#33d17a" height={60} max={100} />
                </div>
                <div className="sm-res-stats">
                  <span className="sm-res-value">{formatBytesSafe(metrics.mem_used_bytes, isOffline)}</span>
                  <span className="sm-res-label">/ {formatBytesSafe(metrics.mem_total_bytes, isOffline)}</span>
                </div>
              </div>
            </div>

            <div className="sm-res-section">
              <div className="sm-res-bar-card">
                <div className="sm-bar-header">
                  <span className="sm-bar-title">{t('monitor.mem')}</span>
                  <span className="sm-bar-value">{formatPercentSafe(
                    metrics.mem_total_bytes > 0 ? (metrics.mem_used_bytes / metrics.mem_total_bytes) * 100 : 0,
                    isOffline
                  )}</span>
                </div>
                <div className="sm-bar-track">
                  <div
                    className="sm-bar-fill"
                    style={{
                      transform: `scaleX(${isOffline ? 0 : (metrics.mem_used_bytes / Math.max(metrics.mem_total_bytes, 1))})`,
                      background: "#33d17a",
                      opacity: isOffline ? 0.3 : 1,
                    }}
                  />
                </div>
                <div className="sm-bar-detail">
                  {formatBytesSafe(metrics.mem_used_bytes, isOffline)} / {formatBytesSafe(metrics.mem_total_bytes, isOffline)}
                </div>
              </div>

              <div className="sm-res-bar-card">
                <div className="sm-bar-header">
                  <span className="sm-bar-title">{t('monitor.swap')}</span>
                  <span className="sm-bar-value">{swap.value}</span>
                </div>
                <div className="sm-bar-track">
                  <div className="sm-bar-fill" style={{
                    transform: `scaleX(${swap.widthPercent / 100})`,
                    background: "#e8a416",
                    opacity: isOffline ? 0.3 : 1,
                  }} />
                </div>
                <div className="sm-bar-detail">
                  {swap.detail}
                </div>
              </div>
            </div>

            <div className="sm-res-section">
              <div className="sm-res-card sm-res-card-wide">
                <div className="sm-res-title">{t('monitor.network')}</div>
                <div className="sm-network-stats">
                  <div className="sm-net-item">
                    <span className="sm-net-icon">↓</span>
                    <span className="sm-net-label">{t('monitor.rx')}</span>
                    <span className="sm-net-value">{formatSpeedSafe(networkRxSpeed, isOffline)}</span>
                  </div>
                  <div className="sm-net-item">
                    <span className="sm-net-icon">↑</span>
                    <span className="sm-net-label">{t('monitor.tx')}</span>
                    <span className="sm-net-value">{formatSpeedSafe(networkTxSpeed, isOffline)}</span>
                  </div>
                </div>
              </div>

              <div className="sm-res-card">
                <div className="sm-res-title">{t('monitor.uptime')}</div>
                <div className="sm-res-stats sm-res-stats-center">
                  <span className="sm-res-value sm-res-value-large">{formatUptimeSafe(metrics.uptime_secs, isOffline)}</span>
                </div>
              </div>
            </div>

            <div className="sm-res-section sm-res-disks">
              <div className="sm-res-title-section">{t('monitor.diskUsage')}</div>
              {isOffline ? (
                <div style={{ padding: '16px 0', color: 'var(--quirel-text-disabled)', textAlign: 'center' }}>
                  {t('monitor.noData')}
                </div>
              ) : metrics.disks.length === 0 ? (
                <div style={{ padding: '16px 0', color: 'var(--quirel-text-secondary)', textAlign: 'center' }}>
                  {t('monitor.noDisks')}
                </div>
              ) : (
                metrics.disks.map((disk) => {
                  const percent = (disk.used_bytes / disk.total_bytes) * 100;
                  return (
                    <div key={disk.mount_point} className="sm-disk-item">
                      <div className="sm-bar-header">
                        <span className="sm-bar-title">{disk.mount_point}</span>
                        <span className="sm-bar-value">{formatPercent(percent)}</span>
                      </div>
                      <div className="sm-bar-track">
                        <div
                          className="sm-bar-fill"
                          style={{
                            transform: `scaleX(${percent / 100})`,
                            background: percent > 80 ? "#e01b24" : percent > 60 ? "#e8a416" : "#3584e4",
                          }}
                        />
                      </div>
                      <div className="sm-bar-detail">
                        {formatBytes(disk.used_bytes)} / {formatBytes(disk.total_bytes)}
                      </div>
                    </div>
                  );
                })
              )}
            </div>
          </div>
        )}

        {activeTab === "filesystems" && (
          <div className="sm-filesystems">
            <div className="sm-fs-header">
              <span className="sm-fs-device">{t('monitor.colDevice')}</span>
              <span className="sm-fs-dir">{t('monitor.colDir')}</span>
              <span className="sm-fs-type">{t('monitor.colType')}</span>
              <span className="sm-fs-total">{t('monitor.colTotal')}</span>
              <span className="sm-fs-used">{t('monitor.colUsed')}</span>
              <span className="sm-fs-avail">{t('monitor.colAvail')}</span>
            </div>
            <div className="sm-fs-list">
              {mounts.length === 0 ? (
                <div className="sm-fs-row" style={{ justifyContent: 'center', color: 'var(--quirel-text-disabled)', padding: '20px 0' }}>
                  {t('monitor.noDisks')}
                </div>
              ) : (
                mounts.map((m) => {
                  const row = toFsRow(m);
                  return (
                    <div key={`${row.device}-${row.dir}`} className="sm-fs-row">
                      <span className="sm-fs-device">{row.device}</span>
                      <span className="sm-fs-dir">{row.dir}</span>
                      <span className="sm-fs-type">{row.type}</span>
                      <span className="sm-fs-total">{row.total}</span>
                      <span className="sm-fs-used">{row.used}</span>
                      <span className="sm-fs-avail">{row.avail}</span>
                    </div>
                  );
                })
              )}
            </div>
          </div>
        )}

        {activeTab === "stats" && (
          <div className="sm-stats">
            <StatsPanel
              serverId={activeServerId || ''}
              isRoot={activeServer?.auth?.username === 'root'}
            />
          </div>
        )}
          </>
        )}
      </div>
    </div>
  );
}