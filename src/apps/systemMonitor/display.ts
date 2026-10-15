// src/apps/systemMonitor/display.ts
// SystemMonitor 展示层纯函数（与渲染解耦便于单测，同 FileManager 筛选规则模式）

/** 通用字节格式化（自 SystemMonitor.tsx 迁入，二者共用） */
export function formatBytes(bytes: number): string {
  if (bytes === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  return (bytes / Math.pow(1024, i)).toFixed(i === 0 ? 0 : 1) + " " + units[i];
}

const P = "\u2014"; // em dash 占位符，语义与 offlineDefaults.PLACEHOLDER 一致

/** remote_get_mounts 返回的挂载点信息（snake_case 与协议一致） */
export interface MountInfo {
  mount_point: string;
  device: string;
  filesystem: string;
  total_bytes: number;
  used_bytes: number;
}

/** 文件系统页签表格行 */
export interface FsRow {
  device: string;
  dir: string;
  type: string;
  total: string;
  used: string;
  avail: string;
}

/** MountInfo → 表格行：设备名缺失回退占位符，可用 = 总量 − 已用 */
export function toFsRow(m: MountInfo): FsRow {
  return {
    device: m.device || P,
    dir: m.mount_point,
    type: m.filesystem,
    total: formatBytes(m.total_bytes),
    used: formatBytes(m.used_bytes),
    avail: formatBytes(m.total_bytes - m.used_bytes),
  };
}

/** Swap 卡片展示三元组 */
export interface SwapDisplay {
  /** 百分比文案 */
  value: string;
  /** 进度条宽度（0-100） */
  widthPercent: number;
  /** 明细行文案 */
  detail: string;
}

/**
 * Swap 展示逻辑：
 * - 离线 → 全占位符
 * - total<=0（旧 Agent 无字段 或 未配置 swap）→ 百分比/进度条占位，used 有值仍展示
 * - 正常 → 真实百分比 + used/total 明细
 */
export function swapDisplay(used: number, total: number, offline: boolean): SwapDisplay {
  if (offline) {
    return { value: `${P}%`, widthPercent: 0, detail: `${P} / ${P}` };
  }
  if (total <= 0) {
    return { value: `${P}%`, widthPercent: 0, detail: `${formatBytes(used)} / ${P}` };
  }
  const percent = (used / total) * 100;
  return {
    value: percent.toFixed(1) + "%",
    widthPercent: Math.min(percent, 100),
    detail: `${formatBytes(used)} / ${formatBytes(total)}`,
  };
}
