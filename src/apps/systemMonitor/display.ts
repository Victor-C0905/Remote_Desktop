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

/* ── 进程列表排序（对齐 FileManager 列头点击排序交互）────── */

/** 进程信息（协议 GetProcesses → ProcessListResponse 条目，snake_case 与协议一致；
 *  自 SystemMonitor.tsx 迁入，排序纯函数与渲染层共用） */
export interface ProcessInfo {
  pid: number;
  name: string;
  user: string;
  cpu_percent: number;
  mem_percent: number;
  mem_bytes: number;
  state: string;
}

export type ProcessSortKey = "pid" | "name" | "user" | "cpu" | "mem" | "state";
export type SortDir = "asc" | "desc";

export interface ProcessSortState {
  key: ProcessSortKey;
  dir: SortDir;
}

/** 默认排序：CPU 降序（最活跃进程置顶，任务管理器惯例） */
export const DEFAULT_PROCESS_SORT: ProcessSortState = { key: "cpu", dir: "desc" };

/** 新列首次点击的初始方向：cpu/mem 为"占用"语义，降序优先（直奔高占用进程）；
 *  pid/name/user/state 升序优先（同 FileManager toggleSort 的新列默认方向） */
const PROCESS_SORT_DEFAULT_DIR: Record<ProcessSortKey, SortDir> = {
  pid: "asc",
  name: "asc",
  user: "asc",
  state: "asc",
  cpu: "desc",
  mem: "desc",
};

/** 点击列头翻转排序：同列翻转方向，异列取该列默认方向
 *  （与 FileManager toggleSort 同构，差异仅在新列默认方向按列语义区分） */
export function toggleProcessSort(prev: ProcessSortState, key: ProcessSortKey): ProcessSortState {
  if (prev.key === key) {
    return { key, dir: prev.dir === "asc" ? "desc" : "asc" };
  }
  return { key, dir: PROCESS_SORT_DEFAULT_DIR[key] };
}

/** 列主排序值：数值列取数、文本列取串（无 null 情形，进程各字段恒有值） */
function processPrimaryValue(key: ProcessSortKey, p: ProcessInfo): number | string {
  switch (key) {
    case "pid": return p.pid;
    case "name": return p.name;
    case "user": return p.user;
    case "cpu": return p.cpu_percent;
    case "mem": return p.mem_percent;
    case "state": return p.state;
  }
}

/** 进程列表排序：主值比较（文本走 localeCompare 归一排序）+
 *  相同主值回退 pid 升序（稳定次级键，不随方向翻转，同 FileManager 惯例）。
 *  返回新数组，不修改入参 */
export function sortProcesses(processes: ProcessInfo[], sort: ProcessSortState): ProcessInfo[] {
  const dir = sort.dir === "asc" ? 1 : -1;
  return [...processes].sort((a, b) => {
    const va = processPrimaryValue(sort.key, a);
    const vb = processPrimaryValue(sort.key, b);
    const r = typeof va === "string" || typeof vb === "string"
      ? String(va).localeCompare(String(vb))
      : va - vb;
    if (r !== 0) return r * dir;
    return a.pid - b.pid;
  });
}
