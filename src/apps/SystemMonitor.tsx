import { useState, useEffect, useCallback, useRef } from "react";
import { useServerManager } from "../context/ServerManager";
import { listen } from "@tauri-apps/api/event";
import { SUBSCRIPTION_CONFIG } from "../config/subscription";
import {
  OFFLINE_METRICS,
  formatBytesSafe,
  formatPercentSafe,
  formatUptimeSafe,
  formatSpeedSafe,
} from "../utils/offlineDefaults";
import { MonitorSkeleton } from "../components/skeleton/MonitorSkeleton";
// import { useWindowState } from "../window-system/hooks/useWindowState"; // 未来集成时使用
import "./SystemMonitor.css";

interface MetricsSnapshot {
  cpu_percent: number;
  mem_used_bytes: number;
  mem_total_bytes: number;
  swap_used_bytes: number;
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

interface ProcessInfo {
  pid: number;
  name: string;
  user: string;
  cpu_percent: number;
  mem_percent: number;
  mem_bytes: number;
  state: string;
}

interface HistoryPoint {
  time: number;
  value: number;
}

type TabId = "processes" | "resources" | "filesystems";

function formatBytes(bytes: number): string {
  if (bytes === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  return (bytes / Math.pow(1024, i)).toFixed(i === 0 ? 0 : 1) + " " + units[i];
}

function formatPercent(value: number): string {
  return value.toFixed(1) + "%";
}

function generateDemoProcesses(): ProcessInfo[] {
  const processes: ProcessInfo[] = [
    { pid: 1, name: "systemd", user: "root", cpu_percent: 0.1, mem_percent: 0.5, mem_bytes: 4096000, state: "S" },
    { pid: 1234, name: "gnome-shell", user: "user", cpu_percent: 3.2, mem_percent: 4.5, mem_bytes: 368640000, state: "S" },
    { pid: 2345, name: "firefox", user: "user", cpu_percent: 8.5, mem_percent: 12.3, mem_bytes: 1000000000, state: "S" },
    { pid: 3456, name: "code", user: "user", cpu_percent: 5.1, mem_percent: 8.2, mem_bytes: 670000000, state: "S" },
    { pid: 4567, name: "node", user: "user", cpu_percent: 2.3, mem_percent: 3.1, mem_bytes: 250000000, state: "S" },
    { pid: 5678, name: "rustc", user: "user", cpu_percent: 15.2, mem_percent: 6.5, mem_bytes: 520000000, state: "R" },
    { pid: 6789, name: "docker", user: "root", cpu_percent: 1.2, mem_percent: 2.1, mem_bytes: 170000000, state: "S" },
    { pid: 7890, name: "nginx", user: "root", cpu_percent: 0.3, mem_percent: 0.8, mem_bytes: 65000000, state: "S" },
    { pid: 8901, name: "postgres", user: "postgres", cpu_percent: 0.5, mem_percent: 1.5, mem_bytes: 120000000, state: "S" },
    { pid: 9012, name: "redis-server", user: "redis", cpu_percent: 0.2, mem_percent: 0.3, mem_bytes: 24000000, state: "S" },
  ];
  return processes.sort((a, b) => b.cpu_percent - a.cpu_percent);
}

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
  // 窗口系统集成（未来可能需要使用 windowState）
  // const windowState = useWindowState(windowId);
  const { activeServerId } = useServerManager();

  const [activeTab, setActiveTab] = useState<TabId>("resources");
  // 永远不为 null：离线时 = OFFLINE_METRICS，在线时 = 真实数据
  const [metrics, setMetrics] = useState<MetricsSnapshot | typeof OFFLINE_METRICS>(OFFLINE_METRICS);
  const [processes, setProcesses] = useState<ProcessInfo[]>([]);
  const [processSort, setProcessSort] = useState<"cpu" | "mem" | "pid">("cpu");
  const [selectedPid, setSelectedPid] = useState<number | null>(null);
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

              console.log('网络速率计算:', {
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
    });

    return () => {
      if (unlistenFn) {
        unlistenFn();
      }
    };
  }, [activeServerId, HISTORY_LENGTH]);

  // 移除原有的轮询逻辑（fetchMetrics 和 setInterval）

  useEffect(() => {
    if (activeTab === "processes") {
      // 进程列表暂时使用 demo 数据（Agent 未实现进程列表）
      setProcesses(generateDemoProcesses());
    }
  }, [activeTab]);

  const sortedProcesses = useCallback(() => {
    const sorted = [...processes];
    switch (processSort) {
      case "cpu": return sorted.sort((a, b) => b.cpu_percent - a.cpu_percent);
      case "mem": return sorted.sort((a, b) => b.mem_percent - a.mem_percent);
      case "pid": return sorted.sort((a, b) => a.pid - b.pid);
    }
    return sorted;
  }, [processes, processSort]);

  // ── 离线判断 ──
  const isOffline = '_offline' in metrics && (metrics as typeof OFFLINE_METRICS)._offline === true;

  // ── 三层状态门控 ─────────────────────────────────────
  // 状态机: 骨架屏(加载中) → 真实数据(在线) → 离线占位符(断连/无连接)
  //   - 无 activeServerId        → 离线占位符（—）
  //   - 有 activeServerId 但未收到数据 → 骨架屏（shimmer）
  //   - 有 activeServerId 且已收到数据 → 真实内容
  const showSkeleton = !!activeServerId && !hasReceivedData;
  const showOffline = !activeServerId;

  return (
    <div className="sm">
      {/* Content — 三层状态门控 */}
      <div className="sm-content">
        {showOffline ? (
          /* 层 3: 离线占位符（无连接/断连后） */
          <div className="sm-offline-state">
            <MonitorSkeleton />
            <div className="sm-offline-overlay">
              <div className="offline-badge">📡 未连接</div>
              <div className="offline-hint">连接到远程服务器以查看系统监控数据</div>
            </div>
          </div>
        ) : showSkeleton ? (
          /* 层 1: 骨架屏（有连接但等待首条数据） */
          <MonitorSkeleton />
        ) : (
          /* 层 2: 真实数据内容（已收到在线数据） */
          <>
            {/* Tab Toolbar - 原HeaderBar的Tab功能 */}
            <div className="sm-toolbar">
              <div className="sm-tabs">
                <button
                  className={`sm-tab${activeTab === "processes" ? " active" : ""}`}
                  onClick={() => setActiveTab("processes")}
                >
                  进程
                </button>
                <button
                  className={`sm-tab${activeTab === "resources" ? " active" : ""}`}
                  onClick={() => setActiveTab("resources")}
                >
                  资源
                </button>
                <button
                  className={`sm-tab${activeTab === "filesystems" ? " active" : ""}`}
                  onClick={() => setActiveTab("filesystems")}
                >
                  文件系统
                </button>
              </div>
              <button className="sm-menu-btn" title="菜单">⋮</button>
            </div>

            {activeTab === "processes" && (
          <div className="sm-processes">
            <div className="sm-process-header">
              <span className="sm-ph-pid" onClick={() => setProcessSort("pid")}>
                PID {processSort === "pid" && "▼"}
              </span>
              <span className="sm-ph-name">进程名称</span>
              <span className="sm-ph-user">用户</span>
              <span className="sm-ph-cpu" onClick={() => setProcessSort("cpu")}>
                CPU {processSort === "cpu" && "▼"}
              </span>
              <span className="sm-ph-mem" onClick={() => setProcessSort("mem")}>
                内存 {processSort === "mem" && "▼"}
              </span>
              <span className="sm-ph-state">状态</span>
            </div>
            <div className="sm-process-list">
              {sortedProcesses().map((proc) => (
                <div
                  key={proc.pid}
                  className={`sm-process-row${selectedPid === proc.pid ? " selected" : ""}`}
                  onClick={() => setSelectedPid(proc.pid)}
                >
                  <span className="sm-pr-pid">{proc.pid}</span>
                  <span className="sm-pr-name">{proc.name}</span>
                  <span className="sm-pr-user">{proc.user}</span>
                  <span className="sm-pr-cpu">{formatPercentSafe(proc.cpu_percent, isOffline)}</span>
                  <span className="sm-pr-mem">{formatPercentSafe(proc.mem_percent, isOffline)}</span>
                  <span className="sm-pr-state">{proc.state}</span>
                </div>
              ))}
            </div>
            <div className="sm-process-footer">
              <span>共 {processes.length} 个进程</span>
            </div>
          </div>
        )}

        {activeTab === "resources" && (
          <div className="sm-resources">
            {/* CPU & Memory Section */}
            <div className="sm-res-section">
              <div className="sm-res-card">
                <div className="sm-res-title">CPU 历史 (最近120秒)</div>
                <div className="sm-res-chart">
                  <MiniChart data={cpuHistory} color="#3584e4" height={60} max={100} />
                </div>
                <div className="sm-res-stats">
                  <span className="sm-res-value">{formatPercentSafe(metrics.cpu_percent, isOffline)}</span>
                  <span className="sm-res-label">当前使用率</span>
                </div>
              </div>

              <div className="sm-res-card">
                <div className="sm-res-title">内存历史 (最近120秒)</div>
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
                  <span className="sm-bar-title">内存</span>
                  <span className="sm-bar-value">{formatPercentSafe(
                    metrics.mem_total_bytes > 0 ? (metrics.mem_used_bytes / metrics.mem_total_bytes) * 100 : 0,
                    isOffline
                  )}</span>
                </div>
                <div className="sm-bar-track">
                  <div
                    className="sm-bar-fill"
                    style={{
                      width: isOffline ? '0%' : `${(metrics.mem_used_bytes / Math.max(metrics.mem_total_bytes, 1)) * 100}%`,
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
                  <span className="sm-bar-title">交换</span>
                  <span className="sm-bar-value">{formatPercentSafe(0, isOffline)}</span>
                </div>
                <div className="sm-bar-track">
                  <div className="sm-bar-fill" style={{ width: '0%', background: "#e8a416", opacity: isOffline ? 0.3 : 1 }} />
                </div>
                <div className="sm-bar-detail">
                  {formatBytesSafe(metrics.swap_used_bytes, isOffline)} / —
                </div>
              </div>
            </div>

            <div className="sm-res-section">
              <div className="sm-res-card sm-res-card-wide">
                <div className="sm-res-title">网络</div>
                <div className="sm-network-stats">
                  <div className="sm-net-item">
                    <span className="sm-net-icon">↓</span>
                    <span className="sm-net-label">接收</span>
                    <span className="sm-net-value">{formatSpeedSafe(networkRxSpeed, isOffline)}</span>
                  </div>
                  <div className="sm-net-item">
                    <span className="sm-net-icon">↑</span>
                    <span className="sm-net-label">发送</span>
                    <span className="sm-net-value">{formatSpeedSafe(networkTxSpeed, isOffline)}</span>
                  </div>
                </div>
              </div>

              <div className="sm-res-card">
                <div className="sm-res-title">运行时间</div>
                <div className="sm-res-stats sm-res-stats-center">
                  <span className="sm-res-value sm-res-value-large">{formatUptimeSafe(metrics.uptime_secs, isOffline)}</span>
                </div>
              </div>
            </div>

            <div className="sm-res-section sm-res-disks">
              <div className="sm-res-title-section">磁盘用量</div>
              {isOffline ? (
                <div style={{ padding: '16px 0', color: 'var(--ovelis-text-disabled)', textAlign: 'center' }}>
                  无数据（未连接）
                </div>
              ) : metrics.disks.length === 0 ? (
                <div style={{ padding: '16px 0', color: 'var(--ovelis-text-secondary)', textAlign: 'center' }}>
                  暂无磁盘信息
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
                            width: `${percent}%`,
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
              <span className="sm-fs-device">设备</span>
              <span className="sm-fs-dir">目录</span>
              <span className="sm-fs-type">类型</span>
              <span className="sm-fs-total">总大小</span>
              <span className="sm-fs-used">已用</span>
              <span className="sm-fs-avail">可用</span>
            </div>
            <div className="sm-fs-list">
              {isOffline ? (
                <div className="sm-fs-row" style={{ justifyContent: 'center', color: 'var(--ovelis-text-disabled)', padding: '20px 0' }}>
                  无数据（未连接）
                </div>
              ) : (
              <>
              {metrics.disks.map((disk) => (
                <div key={disk.mount_point} className="sm-fs-row">
                  <span className="sm-fs-device">/dev/sda{metrics.disks.indexOf(disk) + 1}</span>
                  <span className="sm-fs-dir">{disk.mount_point}</span>
                  <span className="sm-fs-type">ext4</span>
                  <span className="sm-fs-total">{formatBytes(disk.total_bytes)}</span>
                  <span className="sm-fs-used">{formatBytes(disk.used_bytes)}</span>
                  <span className="sm-fs-avail">{formatBytes(disk.total_bytes - disk.used_bytes)}</span>
                </div>
              ))}
              <div className="sm-fs-row">
                <span className="sm-fs-device">tmpfs</span>
                <span className="sm-fs-dir">/tmp</span>
                <span className="sm-fs-type">tmpfs</span>
                <span className="sm-fs-total">4 GB</span>
                <span className="sm-fs-used">128 MB</span>
                <span className="sm-fs-avail">3.9 GB</span>
              </div>
              </>
              )}
            </div>
          </div>
        )}
          </>
        )}
      </div>
    </div>
  );
}