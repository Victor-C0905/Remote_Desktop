/**
 * useArchiveListing：压缩包列表加载 hook
 *
 * 职责：列表命令 → 解析 → 树 + 统计；暴露 loading/error/refresh。
 * 不含提取/解压动作（见 useEntryActions）。
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { invoke } from '@tauri-apps/api/core';
import { createLogger } from '../../../utils/logger';
import { formatUnknownError } from '../../../types/errors';
import { describeExecFailure, describeInvokeError } from '../errors';
import type { ArchiveKind } from '../commands';
import { parseListing } from '../utils/format';
import { buildTree, type ArchiveTreeNode } from '../tree';

const log = createLogger('ArchiveViewer');

/** remote_archive_list 返回结构（Agent 固定命令输出） */
interface ExecResult { stdout: string; stderr: string; exitCode: number; }

export interface ArchiveListingState {
  tree: ArchiveTreeNode[];
  loading: boolean;
  error: { msg: string; detail?: string } | null;
  /** 文件数 + 总大小（由树派生） */
  stats: { files: number; total: number };
  refresh: () => Promise<void>;
}

export function useArchiveListing(
  path: string | undefined,
  serverId: string | undefined,
  kind: ArchiveKind | null,
  tool?: string,
  formatId?: string,
  enabled = true,
): ArchiveListingState {
  const [tree, setTree] = useState<ArchiveTreeNode[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<{ msg: string; detail?: string } | null>(null);
  const { t } = useTranslation();
  const refreshEpochRef = useRef(0);

  const refresh = useCallback(async () => {
    // 竞态守卫（审计 L6）：path/serverId 快速变化时旧列表响应不得覆盖新状态
    const epoch = ++refreshEpochRef.current;
    // 能力探测中或格式不可用时，不发起必然失败的列表命令
    if (!enabled) {
      setTree([]);
      setLoading(false);
      setError(null);
      return;
    }
    if (!path || !serverId || !kind) {
      setError({ msg: t(kind ? 'archive.errNoTarget' : 'archive.errUnsupported') });
      setLoading(false);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      let r: ExecResult;
      try {
        // 列表 argv 由 Agent 按 formatId 生成，客户端不再传命令/参数
        r = await invoke<ExecResult>('remote_archive_list', {
          serverId, archivePath: path, formatId: formatId ?? kind,
        });
      } catch (err) {
        // 连接失败/Agent 拒绝与“输出解析失败”不是一回事，不能统一报解析错误
        log.error('加载压缩包列表失败:', err);
        if (epoch !== refreshEpochRef.current) return;
        setError({ msg: describeInvokeError(err) });
        return;
      }
      if (r.exitCode !== 0) {
        log.error('列表命令失败:', r.exitCode, r.stderr || r.stdout);
        if (epoch !== refreshEpochRef.current) return;
        setError({ msg: describeExecFailure(r, 'list') });
        return;
      }
      // zip 由 7z 兜底列表时输出是 -slt 格式，解析器必须跟随实际工具
      try {
        const parseKind = kind === 'zip' && (tool === '7z' || tool === '7zz') ? '7z' : kind;
        const nextTree = buildTree(parseListing(parseKind, r.stdout));
        if (epoch !== refreshEpochRef.current) return; // 过期响应，丢弃
        setTree(nextTree);
      } catch (err) {
        log.error('解析列表输出失败:', err);
        if (epoch !== refreshEpochRef.current) return;
        setError({ msg: t('archive.errParse'), detail: formatUnknownError(err) });
      }
    } finally {
      if (epoch === refreshEpochRef.current) {
        setLoading(false);
      }
    }
  }, [path, serverId, kind, tool, formatId, enabled, t]);

  useEffect(() => { refresh(); }, [refresh]);

  // 统计：文件数 + 总大小（由树派生，树不变则 memo 命中）
  const stats = useMemo(() => {
    let files = 0, total = 0;
    const walk = (nodes: ArchiveTreeNode[]) => {
      for (const n of nodes) {
        if (n.isDir) walk(n.children); else { files++; total += n.size; }
      }
    };
    walk(tree);
    return { files, total };
  }, [tree]);

  return { tree, loading, error, stats, refresh };
}
