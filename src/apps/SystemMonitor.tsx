import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useServerManager } from "../context/ServerManager";
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

function formatUptime(seconds: number): string {
  const days = Math.floor(seconds / 86400);
  const hours = Math.floor((seconds % 86400) / 3600);
  const mins = Math.floor((seconds % 3600) / 60);
  if (days > 0) return `${days} 天 ${hours}:${mins.toString().padStart(2, "0")}`;
  return `${hours}:${mins.toString().padStart(2, "0")}`;
}

function formatPercent(value: number): string {
  return value.toFixed(1) + "%";
}

function generateDemoMetrics(): MetricsSnapshot {
  return {
    cpu_percent: Math.random() * 30 + 15,
    mem_used_bytes: Math.floor(Math.random() * 2 + 3) * 1024 * 1024 * 1024,
    mem_total_bytes: 8 * 1024 * 1024 * 1024,
    swap_used_bytes: Math.floor(Math.random() * 512) * 1024 * 1024,
    disks: [
      { mount_point: "/", total_bytes: 100 * 1024 * 1024 * 1024, used_bytes: 67 * 1024 * 1024 * 1024 },
      { mount_point: "/var", total_bytes: 200 * 1024 * 1024 * 1024, used_bytes: 64 * 1024 * 1024 * 1024 },
      { mount_point: "/home", total_bytes: 50 * 1024 * 1024 * 1024, used_bytes: 28 * 1024 * 1024 * 1024 },
    ],
    network_rx_bytes: Math.floor(Math.random() * 1.5 + 0.5) * 1024 * 1024,
    network_tx_bytes: Math.floor(Math.random() * 500 + 100) * 1024,
    uptime_secs: Math.floor(Math.random() * 3600 * 24 * 10 + 3600 * 24 * 5),
  };
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

export function SystemMonitor() {
  const { activeServerId } = useServerManager();
  
  const [activeTab, setActiveTab] = useState<TabId>("resources");
  const [metrics, setMetrics] = useState<MetricsSnapshot>(generateDemoMetrics());
  const [cpuHistory, setCpuHistory] = useState<HistoryPoint[]>([]);
  const [memHistory, setMemHistory] = useState<HistoryPoint[]>([]);
  const [processes, setProcesses] = useState<ProcessInfo[]>(generateDemoProcesses());
  const [processSort, setProcessSort] = useState<"cpu" | "mem" | "pid">("cpu");
  const [selectedPid, setSelectedPid] = useState<number | null>(null);
  const [_loading, setLoading] = useState(false);
  const [_error, setError] = useState<string | null>(null);

  const HISTORY_LENGTH = 60;

  const fetchMetrics = useCallback(async () => {
    if (!activeServerId) {
      // 无连接时使用 demo 数据
      const demoMetrics = generateDemoMetrics();
      setMetrics(demoMetrics);
      const now = Date.now();
      setCpuHistory(prev => {
        const next = [...prev, { time: now, value: demoMetrics.cpu_percent }];
        return next.length > HISTORY_LENGTH ? next.slice(-HISTORY_LENGTH) : next;
      });
      setMemHistory(prev => {
        const memPercent = (demoMetrics.mem_used_bytes / demoMetrics.mem_total_bytes) * 100;
        const next = [...prev, { time: now, value: memPercent }];
        return next.length > HISTORY_LENGTH ? next.slice(-HISTORY_LENGTH) : next;
      });
      return;
    }

    setLoading(true);
    try {
      const resp = await invoke<MetricsSnapshot>("remote_get_metrics", {
        serverId: activeServerId,
      });
      setMetrics(resp);
      const now = Date.now();
      setCpuHistory(prev => {
        const next = [...prev, { time: now, value: resp.cpu_percent }];
        return next.length > HISTORY_LENGTH ? next.slice(-HISTORY_LENGTH) : next;
      });
      setMemHistory(prev => {
        const memPercent = (resp.mem_used_bytes / resp.mem_total_bytes) * 100;
        const next = [...prev, { time: now, value: memPercent }];
        return next.length > HISTORY_LENGTH ? next.slice(-HISTORY_LENGTH) : next;
      });
      setError(null);
    } catch (e: any) {
      setError(e.toString());
    } finally {
      setLoading(false);
    }
  }, [activeServerId]);

  useEffect(() => {
    const interval = setInterval(fetchMetrics, 2000);
    return () => clearInterval(interval);
  }, [fetchMetrics]);

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

  const memPercent = (metrics.mem_used_bytes / metrics.mem_total_bytes) * 100;
  const swapPercent = metrics.swap_used_bytes > 0 ? 0 : 0;

  return (
    <div className="sm">
      {/* Header Bar */}
      <div className="sm-headerbar">
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
        <div className="sm-headerbar-spacer" />
        <button className="sm-menu-btn" title="菜单">⋮</button>
      </div>

      {/* Content */}
      <div className="sm-content">
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
                  <span className="sm-pr-cpu">{formatPercent(proc.cpu_percent)}</span>
                  <span className="sm-pr-mem">{formatPercent(proc.mem_percent)}</span>
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
                <div className="sm-res-title">CPU 历史 (60秒)</div>
                <div className="sm-res-chart">
                  <MiniChart data={cpuHistory} color="#3584e4" height={60} max={100} />
                </div>
                <div className="sm-res-stats">
                  <span className="sm-res-value">{formatPercent(metrics.cpu_percent)}</span>
                  <span className="sm-res-label">当前使用率</span>
                </div>
              </div>

              <div className="sm-res-card">
                <div className="sm-res-title">内存历史 (60秒)</div>
                <div className="sm-res-chart">
                  <MiniChart data={memHistory} color="#33d17a" height={60} max={100} />
                </div>
                <div className="sm-res-stats">
                  <span className="sm-res-value">{formatBytes(metrics.mem_used_bytes)}</span>
                  <span className="sm-res-label">/ {formatBytes(metrics.mem_total_bytes)}</span>
                </div>
              </div>
            </div>

            <div className="sm-res-section">
              <div className="sm-res-bar-card">
                <div className="sm-bar-header">
                  <span className="sm-bar-title">内存</span>
                  <span className="sm-bar-value">{formatPercent(memPercent)}</span>
                </div>
                <div className="sm-bar-track">
                  <div className="sm-bar-fill" style={{ width: `${memPercent}%`, background: "#33d17a" }} />
                </div>
                <div className="sm-bar-detail">
                  {formatBytes(metrics.mem_used_bytes)} / {formatBytes(metrics.mem_total_bytes)}
                </div>
              </div>

              <div className="sm-res-bar-card">
                <div className="sm-bar-header">
                  <span className="sm-bar-title">交换</span>
                  <span className="sm-bar-value">{formatPercent(swapPercent)}</span>
                </div>
                <div className="sm-bar-track">
                  <div className="sm-bar-fill" style={{ width: `${swapPercent}%`, background: "#e8a416" }} />
                </div>
                <div className="sm-bar-detail">
                  {formatBytes(metrics.swap_used_bytes)} / —
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
                    <span className="sm-net-value">{formatBytes(metrics.network_rx_bytes)}/s</span>
                  </div>
                  <div className="sm-net-item">
                    <span className="sm-net-icon">↑</span>
                    <span className="sm-net-label">发送</span>
                    <span className="sm-net-value">{formatBytes(metrics.network_tx_bytes)}/s</span>
                  </div>
                </div>
              </div>

              <div className="sm-res-card">
                <div className="sm-res-title">运行时间</div>
                <div className="sm-res-stats sm-res-stats-center">
                  <span className="sm-res-value sm-res-value-large">{formatUptime(metrics.uptime_secs)}</span>
                </div>
              </div>
            </div>

            <div className="sm-res-section sm-res-disks">
              <div className="sm-res-title-section">磁盘用量</div>
              {metrics.disks.map((disk) => {
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
              })}
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
            </div>
          </div>
        )}
      </div>
    </div>
  );
}