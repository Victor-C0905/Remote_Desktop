// src/apps/fileMutations.ts
// 文件管理器视图增量更新：服务器确认写操作后，按路径补丁维护
// 浏览列表（entries）与搜索结果（searchResults），避免整目录 read_dir 刷新。
//
// 约定：
// - 浏览列表只保存 currentPath 直属条目（条目自身 path 字段一律清除）；
// - 搜索结果条目必须带完整 path（remote_search 的命中结构）；
// - 补丁只做结构增删改，排序由调用方按当前表头统一重排。

import type { FileEntry } from "./fileTypes";

/** 单个已确认的文件系统变更 */
export type FileViewChange =
  | { kind: "upsert"; path: string; entry: FileEntry }
  | { kind: "remove"; path: string }
  | { kind: "rename"; from: string; to: string; entry?: FileEntry };

/** 父目录：'/a/b' → '/a'；'/a' → '/'；'/' → '/' */
export function parentDirOf(path: string): string {
  const idx = path.lastIndexOf("/");
  if (idx <= 0) return "/";
  return path.slice(0, idx);
}

/** 末段名称：'/a/b' → 'b'；'/' → '' */
export function baseNameOf(path: string): string {
  return path.slice(path.lastIndexOf("/") + 1);
}

/** 上传临时文件（quireld_<uuid>.tmp）：内部实现细节，不属于用户可见内容。
 *  新 Agent 的目录读取与变更推送已过滤；这里兜底旧 Agent / 历史事件，
 *  避免上传期间或完成瞬间出现随机名临时行。 */
export function isTransferTempName(name: string): boolean {
  return name.startsWith("quireld_") && name.endsWith(".tmp");
}

/** 按 key 去重（保留最后一次出现，输出保持保留项的原相对顺序）。
 *  增量补丁可能重复投递（如 rename 同时以 Both/To 事件到达），刷新能修好
 *  说明是内存态重复——这里保证补丁本身不可能产生重复行。 */
function dedupeTail<T>(items: T[], keyOf: (item: T) => string): T[] {
  const seen = new Set<string>();
  const out: T[] = [];
  for (let i = items.length - 1; i >= 0; i -= 1) {
    const key = keyOf(items[i]);
    if (seen.has(key)) continue;
    seen.add(key);
    out.push(items[i]);
  }
  out.reverse();
  return out;
}

/** 目录直属条目：去掉 path 字段，避免浏览缓存混入绝对路径 */
function asBrowseEntry(entry: FileEntry, name: string): FileEntry {
  return { ...entry, name, path: undefined };
}

/** 应用补丁到浏览列表（currentPath 直属条目） */
export function applyBrowseChanges(
  entries: FileEntry[],
  currentPath: string,
  changes: FileViewChange[]
): FileEntry[] {
  let result = entries;

  for (const change of changes) {
    if (change.kind === "remove") {
      if (parentDirOf(change.path) !== currentPath) continue;
      const name = baseNameOf(change.path);
      result = result.filter((e) => e.name !== name);
      continue;
    }

    if (change.kind === "upsert") {
      if (parentDirOf(change.path) !== currentPath) continue;
      const name = baseNameOf(change.path);
      // 上传临时文件不进入用户视图（旧 Agent 的增量兜底）
      if (isTransferTempName(name)) continue;
      const next = asBrowseEntry(change.entry, name);
      const idx = result.findIndex((e) => e.name === name);
      result = idx >= 0
        ? result.map((e, i) => (i === idx ? next : e))
        : [...result, next];
      continue;
    }

    // rename：同目录改名保持原条目元数据；跨目录离开则移除，跨目录进入用回传条目
    const fromParent = parentDirOf(change.from);
    const toParent = parentDirOf(change.to);
    const fromName = baseNameOf(change.from);
    const toName = baseNameOf(change.to);
    if (isTransferTempName(toName)) continue;

    if (fromParent === currentPath) {
      const idx = result.findIndex((e) => e.name === fromName);
      if (toParent === currentPath) {
        const next = change.entry
          ? asBrowseEntry(change.entry, toName)
          : idx >= 0
            ? { ...result[idx], name: toName }
            : undefined;
        if (next) {
          // 幂等：目标名已存在（同一 rename 重复投递/临时文件占位）时替换而非追加，
          // 否则每次重复补丁都会多一行（刷新才能恢复）
          const existingTo = result.findIndex((e) => e.name === toName);
          if (idx >= 0) {
            result = result.map((e, i) => (i === idx ? next : e));
          } else if (existingTo >= 0) {
            result = result.map((e, i) => (i === existingTo ? next : e));
          } else {
            result = [...result, next];
          }
        }
      } else if (idx >= 0) {
        result = result.filter((_, i) => i !== idx);
      }
    } else if (toParent === currentPath && change.entry) {
      const existingTo = result.findIndex((e) => e.name === toName);
      const next = asBrowseEntry(change.entry, toName);
      result = existingTo >= 0
        ? result.map((e, i) => (i === existingTo ? next : e))
        : [...result, next];
    }
  }

  return dedupeTail(result, (e) => e.name);
}

/** 搜索结果条目路径（正常由 Rust 回填完整路径；缺失时按 root 拼装兜底） */
function searchEntryPath(entry: FileEntry, root: string): string {
  if (entry.path) return entry.path;
  return root === "/" ? `/${entry.name}` : `${root}/${entry.name}`;
}

/**
 * 新增命中是否满足搜索条件（与 Rust remote_search 语义对齐）：
 * 名称前缀匹配（大小写不敏感）+ 父目录在 root 内且下钻层数不超过 depth
 * （depth=0 仅 root 一层；depth=N 扫描到 root 下 N 层目录）
 */
function isSearchCandidate(
  path: string,
  root: string,
  query: string,
  depth: number
): boolean {
  const needle = query.trim().toLowerCase();
  if (!needle) return false;
  if (!baseNameOf(path).toLowerCase().startsWith(needle)) return false;

  const parent = parentDirOf(path);
  if (parent === root) return true;

  const prefix = root === "/" ? "/" : `${root}/`;
  if (!parent.startsWith(prefix)) return false;
  if (depth <= 0) return false;

  const rel = parent.slice(prefix.length);
  return rel.split("/").filter(Boolean).length <= depth;
}

/** 应用补丁到搜索结果（root/query/depth 决定新增命中是否可见） */
export function applySearchChanges(
  results: FileEntry[],
  root: string,
  query: string,
  depth: number,
  changes: FileViewChange[]
): FileEntry[] {
  let out = results;

  for (const change of changes) {
    if (change.kind === "remove") {
      // 删除目录时其下命中一并消失（命中路径为绝对路径，可直接按前缀判定）
      const prefix = change.path === "/" ? "/" : `${change.path}/`;
      out = out.filter((e) => {
        const p = searchEntryPath(e, root);
        return p !== change.path && !p.startsWith(prefix);
      });
      continue;
    }

    if (change.kind === "upsert") {
      if (isTransferTempName(baseNameOf(change.path))) continue;
      const target = { ...change.entry, name: baseNameOf(change.path), path: change.path };
      const idx = out.findIndex((e) => searchEntryPath(e, root) === change.path);
      if (idx >= 0) {
        out = out.map((e, i) => (i === idx ? target : e));
      } else if (isSearchCandidate(change.path, root, query, depth)) {
        out = [...out, target];
      }
      continue;
    }

    // rename：目录改名的后代命中同步改写路径前缀
    const fromPrefix = change.from === "/" ? "/" : `${change.from}/`;
    out = out.map((e) => {
      const p = searchEntryPath(e, root);
      if (!p.startsWith(fromPrefix)) return e;
      return { ...e, path: `${change.to}${p.slice(change.from.length)}` };
    });

    // 该条目本身按新路径重建；新名称不再匹配前缀则自然消失
    const fromPath = change.from;
    const idx = out.findIndex((e) => searchEntryPath(e, root) === fromPath);
    const source = idx >= 0 ? out[idx] : change.entry;
    const toName = baseNameOf(change.to);
    out = idx >= 0 ? out.filter((_, i) => i !== idx) : out;

    if (source && !isTransferTempName(toName) && isSearchCandidate(change.to, root, query, depth)) {
      const target = { ...source, ...(change.entry ?? {}), name: toName, path: change.to };
      // 幂等：目标路径已存在时替换，避免重复 rename 补丁追加重复命中
      const existingTo = out.findIndex((e) => searchEntryPath(e, root) === change.to);
      out = existingTo >= 0
        ? out.map((e, i) => (i === existingTo ? target : e))
        : [...out, target];
    }
  }

  return dedupeTail(out, (e) => searchEntryPath(e, root));
}
