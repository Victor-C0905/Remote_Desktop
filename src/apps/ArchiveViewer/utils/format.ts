/**
 * ArchiveViewer 工具函数
 *
 * parseListing：kind → 解析器分发；fmtSize：人类可读大小。
 */

import type { ArchiveKind } from '../commands';
import {
  parseUnzipListing, parseTarListing, parse7zListing, parseUnrarListing, type ArchiveEntry,
} from '../parsers';

/** 按 kind 选择解析器（解析失败抛错由调用方呈现） */
export function parseListing(kind: ArchiveKind, out: string): ArchiveEntry[] {
  switch (kind) {
    case 'zip': return parseUnzipListing(out);
    case 'tar': return parseTarListing(out);
    case '7z':  return parse7zListing(out);
    case 'rar': return parseUnrarListing(out);
  }
}

/** 人类可读大小（0/未知 → "-"） */
export function fmtSize(n: number): string {
  if (n <= 0) return '-';
  const units = ['B', 'KB', 'MB', 'GB'];
  let v = n, i = 0;
  while (v >= 1024 && i < units.length - 1) { v /= 1024; i++; }
  return `${v >= 100 ? Math.round(v) : v.toFixed(1)} ${units[i]}`;
}
