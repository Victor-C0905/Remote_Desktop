/**
 * 离线默认值与占位符格式化工具
 *
 * 业内标准：断连/无数据时使用明确的占位符（-- / — / 提示文字），
 * 而非 0 值或空白，确保 UI 始终可读且语义清晰。
 */

/* ── 占位符常量 ──────────────────────────────────────── */

/** 数值型指标的通用占位符 */
export const PLACEHOLDER = "\u2014"; // em dash: —

/** 格式化后的占位符字符串集 */
export const PLACEHOLDERS = {
  /** CPU / 内存等百分比 */
  percent: `${PLACEHOLDER}%`,
  /** 字节数（内存/磁盘/网络） */
  bytes: `${PLACEHOLDER}`,
  /** 速率 */
  speed: `${PLACEHOLDER}/s`,
  /** 时间 */
  time: `${PLACEHOLDER}:${PLACEHOLDER}:${PLACEHOLDER}`,
  /** 通用数值 */
  value: `${PLACEHOLDER}`,
} as const;

/**
 * SystemMonitor 离线默认指标
 * 所有字段均为合法值，组件可直接渲染不会崩溃。
 * 数值字段用 0，展示层通过 isOffline 标志决定显示 -- 还是真实值。
 */
export interface OfflineMetricsSnapshot {
  cpu_percent: number;
  mem_used_bytes: number;
  mem_total_bytes: number;
  swap_used_bytes: number;
  swap_total_bytes: number;
  disks: Array<{ mount_point: string; total_bytes: number; used_bytes: number }>;
  network_rx_bytes: number;
  network_tx_bytes: number;
  uptime_secs: number;
  /** 是否处于离线状态（用于控制显示 -- 还是数字） */
  _offline: true;
}

export const OFFLINE_METRICS: OfflineMetricsSnapshot = {
  cpu_percent: 0,
  mem_used_bytes: 0,
  mem_total_bytes: 0,
  swap_used_bytes: 0,
  swap_total_bytes: 0,
  disks: [],
  network_rx_bytes: 0,
  network_tx_bytes: 0,
  uptime_secs: 0,
  _offline: true,
};

/* ── 安全格式化函数（兼容离线状态） ──────────────────── */

/**
 * 格式化字节数：离线时返回 "--"
 * @param bytes 字节数
 * @param offline 是否离线
 */
export function formatBytesSafe(bytes: number, offline = false): string {
  if (offline || bytes === 0) return PLACEHOLDER;
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  return (bytes / Math.pow(1024, i)).toFixed(i === 0 ? 0 : 1) + " " + units[i];
}

/**
 * 格式化百分比：离线时返回 "--%"
 * @param value 百分比值 (0-100)
 * @param offline 是否离线
 */
export function formatPercentSafe(value: number, offline = false): string {
  if (offline) return PLACEHOLDERS.percent;
  return value.toFixed(1) + "%";
}

/**
 * 格式化运行时间：离线时返回 "--:--"
 * @param seconds 秒数
 * @param offline 是否离线
 */
export function formatUptimeSafe(seconds: number, offline = false): string {
  if (offline || seconds <= 0) return PLACEHOLDERS.time;
  const days = Math.floor(seconds / 86400);
  const hours = Math.floor((seconds % 86400) / 3600);
  const mins = Math.floor((seconds % 3600) / 60);
  if (days > 0) return `${days}d ${hours}:${mins.toString().padStart(2, "0")}`;
  return `${hours}:${mins.toString().padStart(2, "0")}`;
}

/**
 * 格式化网络速率：离线时返回 "-- B/s"
 * @param bytesPerSec 每秒字节数
 * @param offline 是否离线
 */
export function formatSpeedSafe(bytesPerSec: number, offline = false): string {
  if (offline || bytesPerSec === 0) return PLACEHOLDERS.speed;
  return formatBytesSafe(bytesPerSec);
}
