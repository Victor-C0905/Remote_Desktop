/**
 * 文件管理器多选纯函数
 *
 * 框选命中 / 范围选择 / Ctrl 切换 / 移动目标构造的共享逻辑，
 * 与 UI 解耦便于单测（fileSelection.test.ts）。路径一律为
 * 完整 POSIX 路径（entryFullPath 产物）。
 */

/** 视口坐标矩形（px） */
export interface Rect {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

/** 两矩形是否相交（边缘相切不算命中，避免贴边框选误选） */
export function rectsIntersect(a: Rect, b: Rect): boolean {
  return a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom;
}

/** 由拖拽起止两点构造规范矩形（支持任意方向拖拽） */
export function normalizeRect(x0: number, y0: number, x1: number, y1: number): Rect {
  return {
    left: Math.min(x0, x1),
    top: Math.min(y0, y1),
    right: Math.max(x0, x1),
    bottom: Math.max(y0, y1),
  };
}

/** 框选命中：返回与 marquee 矩形相交的条目路径（保持条目给定顺序） */
export function marqueeHit(paths: string[], rects: Rect[], marquee: Rect): string[] {
  return paths.filter((_, i) => rectsIntersect(marquee, rects[i]));
}

/** Shift 范围选择：按可见顺序取 anchor..target 闭区间；anchor 缺失/不在列表时回退单选 target */
export function rangeSelection(orderedPaths: string[], anchor: string | null, target: string): string[] {
  const a = anchor ?? target;
  const ai = orderedPaths.indexOf(a);
  const ti = orderedPaths.indexOf(target);
  if (ai === -1 || ti === -1) return [target];
  const [lo, hi] = ai <= ti ? [ai, ti] : [ti, ai];
  return orderedPaths.slice(lo, hi + 1);
}

/** Ctrl 点击切换：已选中则移除，否则追加（保序） */
export function toggleSelection(selected: string[], path: string): string[] {
  return selected.includes(path) ? selected.filter((p) => p !== path) : [...selected, path];
}

/** 取路径末段名称（"/a/b/c.txt" → "c.txt"） */
export function entryName(path: string): string {
  return path.slice(path.lastIndexOf("/") + 1);
}

/** 拼子路径：根目录特殊处理避免 "//name" */
export function childPath(dir: string, name: string): string {
  return dir === "/" ? `/${name}` : `${dir}/${name}`;
}

/**
 * 移动/复制合法性：目标目录不能是源条目自身或其子目录
 * （把 /a 移动进 /a/b 会连同自身被搬运，语义悬空；移动到自身无意义）
 */
export function isSelfOrDescendant(src: string, dstDir: string): boolean {
  return dstDir === src || dstDir.startsWith(`${src}/`);
}

/** 批量移动/复制目标构造：过滤非法（自身/子目录），返回 {src, dst} 列表 */
export function buildMoveTargets(
  srcs: string[],
  dstDir: string,
): Array<{ src: string; dst: string }> {
  return srcs
    .filter((src) => !isSelfOrDescendant(src, dstDir))
    .map((src) => ({ src, dst: childPath(dstDir, entryName(src)) }));
}
