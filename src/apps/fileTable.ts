// src/apps/fileTable.ts
// 文件列表表头领域逻辑：排序（点击表头翻转）+ 列筛选（表头下拉）+ 列宽（拖拽调节/持久化）
// 纯函数模块，与 permissions.ts 同模式：不含 React 依赖，单测覆盖（fileTable.test.ts）
import type { FileEntry } from "./FileManager";

/* ── 排序 ─────────────────────────────────────────────────── */

export type SortKey = "name" | "size" | "mtime" | "type" | "perm";
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
    case "type": {
      // 无扩展名视同未知类型 → null：排组内最后（同 mtime 空值惯例，不随方向翻转）
      const k = typeKeyOf(e);
      return k === "__none" ? null : k;
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

/* ── 列筛选（表头下拉菜单，Win11 Explorer 风格）───────────── */

/** 大小区间（Win11 Explorer 分组：空/极小/小/中/大/巨大），区间左开右闭 */
export type SizeBucket = "empty" | "tiny" | "small" | "medium" | "large" | "huge";

/** 修改时间组（Win11 Explorer 分组：今天/昨天/本周/上周/更早），按本地日历日划分 */
export type MtimeBucket = "today" | "yesterday" | "thisWeek" | "lastWeek" | "earlier";

/** 名称组（字母/拼音首字母范围）：A–F / G–L / M–R / S–Z / 其他（数字与符号） */
export type NameBucket = "aToF" | "gToL" | "mToR" | "sToZ" | "other";

/** 列筛选状态：各列为已勾选的桶集合（空数组 = 该列不过滤）；会话内状态不持久化
 *  type 为动态清单（当前目录实际出现的扩展名），键为 string 而非固定枚举 */
export interface ColumnFilterState {
  size: SizeBucket[];
  mtime: MtimeBucket[];
  name: NameBucket[];
  type: string[];
}

/** 默认无筛选（会话内决策：每次打开默认全部显示） */
export const DEFAULT_COLUMN_FILTERS: ColumnFilterState = { size: [], mtime: [], name: [], type: [] };

/** 下拉菜单展示用桶清单（顺序即菜单勾选顺序） */
export const SIZE_BUCKETS: { key: SizeBucket; label: string }[] = [
  { key: "empty", label: "空（0 字节）" },
  { key: "tiny", label: "极小（0 – 16 KB）" },
  { key: "small", label: "小（16 KB – 1 MB）" },
  { key: "medium", label: "中（1 – 128 MB）" },
  { key: "large", label: "大（128 MB – 1 GB）" },
  { key: "huge", label: "巨大（超过 1 GB）" },
];

export const MTIME_BUCKETS: { key: MtimeBucket; label: string }[] = [
  { key: "today", label: "今天" },
  { key: "yesterday", label: "昨天" },
  { key: "thisWeek", label: "本周" },
  { key: "lastWeek", label: "上周" },
  { key: "earlier", label: "更早" },
];

export const NAME_BUCKETS: { key: NameBucket; label: string }[] = [
  { key: "aToF", label: "A – F" },
  { key: "gToL", label: "G – L" },
  { key: "mToR", label: "M – R" },
  { key: "sToZ", label: "S – Z" },
  { key: "other", label: "其他（数字与符号）" },
];

/** 条目所属类型键：目录 → __dir；无扩展名（含隐藏文件点前缀、点结尾）→ __none；
 *  其余取最后一个点后的扩展名并小写归一（同一类型不同大小写同组） */
export function typeKeyOf(e: FileEntry): string {
  if (e.is_dir) return "__dir";
  const dot = e.name.lastIndexOf(".");
  if (dot <= 0 || dot === e.name.length - 1) return "__none";
  return e.name.slice(dot + 1).toLowerCase();
}

/** 类型键的用户可见标签：__dir → 文件夹、__none → 文件、扩展名 → 大写（TXT/MD） */
export function typeKeyLabel(key: string): string {
  if (key === "__dir") return "文件夹";
  if (key === "__none") return "文件";
  return key.toUpperCase();
}

/** 列出当前目录实际出现的类型（类型列下拉菜单的动态桶清单）：
 *  文件夹置顶，其余按扩展名字母序去重——与 Win11 按当前内容动态生成类型组一致 */
export function listFileTypeBuckets(entries: FileEntry[]): { key: string; label: string }[] {
  const seen = new Set<string>();
  let hasDir = false;
  for (const e of entries) {
    const k = typeKeyOf(e);
    if (k === "__dir") hasDir = true;
    else seen.add(k);
  }
  const list = [...seen]
    .sort((a, b) => a.localeCompare(b))
    .map((k) => ({ key: k, label: typeKeyLabel(k) }));
  return hasDir ? [{ key: "__dir", label: "文件夹" }, ...list] : list;
}

const KB = 1024, MB = 1024 * 1024, GB = 1024 * 1024 * 1024;

/** 条目所属大小区间：null 表示不参与大小筛选——目录 size 恒为 0 但不代表
 *  内容大小（协议端 is_dir → 0），对齐 Explorer：文件夹不落任何区间，
 *  大小筛选激活时目录随之隐藏（用户勾区间是为了找文件） */
export function sizeBucketOf(e: FileEntry): SizeBucket | null {
  if (e.is_dir) return null;
  const s = e.size;
  if (s === 0) return "empty";
  if (s <= 16 * KB) return "tiny";
  if (s <= 1 * MB) return "small";
  if (s <= 128 * MB) return "medium";
  if (s <= 1 * GB) return "large";
  return "huge";
}

/** 条目所属修改时间组：null 表示无时间/不可解析，不落任何组（筛选激活时被滤除）
 *  周界按本地日历日划分、周一为一周之始（中国区域惯例，同 Win11 依系统区域）；
 *  未来时间与 now 同日归"今天"（服务器时钟略偏移不致分组漂移）
 *  now 注入而非内部取 Date.now()，保证纯函数可测试 */
export function mtimeBucketOf(e: FileEntry, now: number): MtimeBucket | null {
  if (!e.mtime) return null;
  const t = Date.parse(e.mtime);
  if (Number.isNaN(t)) return null;

  // 本地日历日界（new Date 的日历运算自动处理月末/闰年，不依赖 86400000 毫秒减法）
  const nowDate = new Date(now);
  const y = nowDate.getFullYear(), m = nowDate.getMonth(), d = nowDate.getDate();
  const startOfToday = new Date(y, m, d).getTime();
  const startOfYesterday = new Date(y, m, d - 1).getTime();
  // 周一起点：(getDay() 周日=0 … 周六=6) 转周一基序 → 日历回退
  const mondayOffset = (nowDate.getDay() + 6) % 7;
  const startOfThisWeek = new Date(y, m, d - mondayOffset).getTime();
  const startOfLastWeek = new Date(y, m, d - mondayOffset - 7).getTime();

  if (t >= startOfToday) return "today";
  if (t >= startOfYesterday) return "yesterday";
  if (t >= startOfThisWeek) return "thisWeek";
  if (t >= startOfLastWeek) return "lastWeek";
  return "earlier";
}

/** 条目所属名称组（按首字符的字母/拼音首字母范围）：
 *  ASCII 字母走快路径直接归段；汉字用 zh 拼音 collation 与段下界锚字比较——
 *  锚字取各段最小拼音 ga(嘎)/ma(妈)/sa(洒)，判定"首字拼音 >= 段下界"即归段。
 *  不能直接与拉丁字母比较：ICU zh collation 中单拉丁字母有独立权重区，
 *  排在全部汉字之后（实测"工".localeCompare("g") < 0），锚字才落在拼音序内。
 *  数字/符号（含隐藏文件点前缀、emoji）不参与字母分组，归"其他" */
export function nameBucketOf(e: FileEntry): NameBucket {
  const c = e.name.charAt(0);
  if (!c) return "other";  // 防御：协议层不应出现空名
  if (/[a-z]/i.test(c)) {
    const ch = c.toLowerCase();
    if (ch < "g") return "aToF";
    if (ch < "m") return "gToL";
    if (ch < "s") return "mToR";
    return "sToZ";
  }
  // 非文字（数字/标点/符号/emoji）→ 其他
  if (!/\p{L}/u.test(c)) return "other";
  // 文字（汉字等）：拼音序与段下界锚字比较归段
  if (c.localeCompare("嘎", "zh-Hans-CN") < 0) return "aToF";
  if (c.localeCompare("妈", "zh-Hans-CN") < 0) return "gToL";
  if (c.localeCompare("洒", "zh-Hans-CN") < 0) return "mToR";
  return "sToZ";
}

/** 勾选/取消勾选一个桶：已选则移除（保序），未选则追加；返回新数组不改入参 */
export function toggleBucket<T extends string>(list: readonly T[], bucket: T): T[] {
  return list.includes(bucket)
    ? list.filter((b) => b !== bucket)
    : [...list, bucket];
}

/** 是否有任何列筛选激活（空状态文案、列头漏斗指示共用） */
export function hasActiveColumnFilters(f: ColumnFilterState): boolean {
  return f.size.length > 0 || f.mtime.length > 0 || f.name.length > 0 || f.type.length > 0;
}

/** 列筛选：勾选桶为并集（OR），多列同时激活为 AND；无勾选原样返回（不复制）
 *  目录/文件的粗过滤已由类型列 __dir 组承担（原工具栏分段控件已移除） */
export function filterByColumnFilters(
  entries: FileEntry[],
  filters: ColumnFilterState,
  now: number = Date.now()
): FileEntry[] {
  if (!hasActiveColumnFilters(filters)) return entries;
  let result = entries;
  if (filters.size.length > 0) {
    result = result.filter((e) => {
      const b = sizeBucketOf(e);
      return b !== null && (filters.size as readonly string[]).includes(b);
    });
  }
  if (filters.mtime.length > 0) {
    result = result.filter((e) => {
      const b = mtimeBucketOf(e, now);
      return b !== null && (filters.mtime as readonly string[]).includes(b);
    });
  }
  if (filters.name.length > 0) {
    // 名称组对目录同样生效（首字符分组无 null 情形）
    result = result.filter((e) => (filters.name as readonly string[]).includes(nameBucketOf(e)));
  }
  if (filters.type.length > 0) {
    // 类型组对目录同样生效（__dir 组）；首字符分组无 null 情形
    result = result.filter((e) => (filters.type as readonly string[]).includes(typeKeyOf(e)));
  }
  return result;
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
  type: number;
  perm: number;
}

/** 默认列宽：与 CSS 历史值一致（fm-list-header 的 grid-template-columns） */
export const DEFAULT_COLUMN_WIDTHS: ColumnWidths = { name: null, size: 90, mtime: 140, type: 90, perm: 90 };

/** 各列最小宽度（名称列与 minmax 下限一致，防止拖拽把列压没了）
 *  name=130：图标 18 + 间距 8 + 文本区约 100px（≈10 个中文字符），
 *  配合省略号截断可读性足够，且给其余固定列留出更多空间 */
export const COLUMN_MIN_WIDTHS: Record<SortKey, number> = {
  name: 130,
  size: 56,
  mtime: 90,
  type: 60,
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
  const nameCol = widths.name === null ? "minmax(130px, 2fr)" : `${widths.name}px`;
  // 列序：名称 / 大小 / 修改时间 / 类型 / 权限（类型列插在时间与权限之间）
  return `${nameCol} ${widths.size}px ${widths.mtime}px ${widths.type}px ${widths.perm}px`;
}
