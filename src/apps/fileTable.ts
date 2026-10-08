// src/apps/fileTable.ts
// 文件列表表头领域逻辑：排序（点击表头翻转）+ 列宽（拖拽调节/持久化）
// 纯函数模块，与 permissions.ts 同模式：不含 React 依赖，单测覆盖（fileTable.test.ts）
import type { FileEntry } from "./FileManager";

/* ── 排序 ─────────────────────────────────────────────────── */

export type SortKey = "name" | "size" | "mtime" | "perm";
export type SortDir = "asc" | "desc";

export interface SortState {
  key: SortKey;
  dir: SortDir;
}

/** 默认排序：名称升序（与历史行为一致：目录优先 + 名称 localeCompare） */
export const DEFAULT_SORT: SortState = { key: "name", dir: "asc" };

/** 提取条目的主排序值：null 表示"无此值"（目录的 size、缺失/不可解析的时间等）
 *  null 值不参与数值序，统一排在组内末尾并按名称聚拢（参考 Explorer/Nautilus：
 *  文件夹不按文件系统 size 排序——该值无意义；未知日期不混入正常时间序列） */
function primaryValue(key: SortKey, e: FileEntry): number | string | null {
  switch (key) {
    case "name":
      return e.name;
    case "size":
      // 目录 size 恒为 0（协议端 is_dir → 0），不代表内容大小，不参与数值排序
      return e.is_dir ? null : e.size;
    case "mtime": {
      // 数值时间比较：兼容 ISO 8601 的 Z / +08:00 偏移 / 小数秒等格式变体，
      // 不依赖字符串字典序（chrono to_rfc3339 的偏移与毫秒长度可变）
      if (!e.mtime) return null;
      const t = Date.parse(e.mtime);
      return Number.isNaN(t) ? null : t;
    }
    case "perm":
      return e.permissions || null;
  }
}

/** 组内比较：主值数值/字典比较 + 无效值排后 + 相同主值回退名称（恒升序）
 *  次级键不随方向翻转，与 Explorer/Nautilus 的次级排序键行为一致 */
function compareBy(key: SortKey, dir: SortDir, a: FileEntry, b: FileEntry): number {
  const va = primaryValue(key, a);
  const vb = primaryValue(key, b);

  // 无效值：统一排最后，相互之间按名称聚拢（不论升降序）
  if (va === null && vb === null) return a.name.localeCompare(b.name);
  if (va === null) return 1;
  if (vb === null) return -1;

  // 主值相同 → 回退名称比较（tie-break，大项目通用惯例）
  if (va === vb) return a.name.localeCompare(b.name);

  const r = va < vb ? -1 : 1;
  return dir === "asc" ? r : -r;
}

/** 排序文件列表：目录始终排在文件前（桌面文件管理器惯例），组内按 key+dir
 *  返回新数组，不修改入参（便于点击表头时对现有 entries 原地重排） */
export function sortFileEntries(entries: FileEntry[], sort: SortState): FileEntry[] {
  const dirs = entries.filter((e) => e.is_dir);
  const files = entries.filter((e) => !e.is_dir);
  const cmp = (a: FileEntry, b: FileEntry) => compareBy(sort.key, sort.dir, a, b);
  // sort 作用于副本，避免 filter 结果与原数组共享引用后被原地重排
  return [...dirs].sort(cmp).concat([...files].sort(cmp));
}

/* ── 过滤（目录/文件，工具栏分段控件）───────────────────── */

export type FilterMode = "all" | "dirs" | "files";

/** 默认显示全部（会话内状态不持久化，每次打开默认全部显示） */
export const DEFAULT_FILTER: FilterMode = "all";

/** 过滤文件列表：dirs=仅目录，files=仅文件；all 原样返回（不复制数组） */
export function filterFileEntries(entries: FileEntry[], mode: FilterMode): FileEntry[] {
  if (mode === "dirs") return entries.filter((e) => e.is_dir);
  if (mode === "files") return entries.filter((e) => !e.is_dir);
  return entries;
}

/* ── 列宽 ─────────────────────────────────────────────────── */

/**
 * 列宽状态（持久化于 settingsStore，经 Tauri Store 写盘）
 * name 为 null 表示名称列保持弹性占满剩余空间（默认态 minmax(180px, 2fr)）；
 * 拖拽名称列右边界后转为固定 px
 */
export interface ColumnWidths {
  name: number | null;
  size: number;
  mtime: number;
  perm: number;
}

/** 默认列宽：与 CSS 历史值一致（fm-list-header 的 grid-template-columns） */
export const DEFAULT_COLUMN_WIDTHS: ColumnWidths = { name: null, size: 90, mtime: 140, perm: 90 };

/** 各列最小宽度（名称列与 minmax 下限一致，防止拖拽把列压没了） */
export const COLUMN_MIN_WIDTHS: Record<SortKey, number> = {
  name: 180,
  size: 56,
  mtime: 90,
  perm: 70,
};

/** 夹取列宽：不低于列最小值，不高于 maxWidth（容器可用空间，防溢出被裁剪）
 *  上限本身小于最小值时保底最小值，不产生矛盾值 */
export function clampColumnWidth(key: SortKey, width: number, maxWidth?: number): number {
  const min = COLUMN_MIN_WIDTHS[key];
  let w = Math.round(width);
  if (w < min) w = min;
  if (maxWidth !== undefined && w > maxWidth) {
    w = Math.max(min, Math.round(maxWidth));
  }
  return w;
}

/** 生成 grid-template-columns：表头与行共用，经 CSS 变量 --fm-cols 下发一次 */
export function buildGridTemplate(widths: ColumnWidths): string {
  const nameCol = widths.name === null ? "minmax(180px, 2fr)" : `${widths.name}px`;
  return `${nameCol} ${widths.size}px ${widths.mtime}px ${widths.perm}px`;
}
