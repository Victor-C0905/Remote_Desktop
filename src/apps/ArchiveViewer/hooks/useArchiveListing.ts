/**
 * useArchiveListing：压缩包列表加载 hook
 *
 * 职责：列表命令 → 解析 → 树 + 统计；暴露 loading/error/refresh。
 * 不含提取/解压动作（见 useEntryActions）。
 */

import { useCallback, useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { createLogger } from '../../../utils/logger';
import { buildListCommand, type ArchiveKind } from '../commands';
import { parseListing } from '../utils/format';
import { buildTree, type ArchiveTreeNode } from '../tree';

const log = createLogger('ArchiveViewer');

/** remote_execute_command 返回结构（与 FileManager 一致） */
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
): ArchiveListingState {
  const [tree, setTree] = useState<ArchiveTreeNode[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<{ msg: string; detail?: string } | null>(null);

  const refresh = useCallback(async () => {
    if (!path || !serverId || !kind) {
      setError({ msg: kind ? '未指定压缩包或未连接服务器' : `不支持的压缩格式` });
      setLoading(false);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const cmd = buildListCommand(path, kind);
      const r = await invoke<ExecResult>('remote_execute_command', {
        serverId, command: cmd.command, args: cmd.args,
        workingDirectory: '/', timeoutSecs: 30,
      });
      if (r.exitCode !== 0) {
        setError({ msg: `列表命令失败（exit ${r.exitCode}）`, detail: r.stderr || r.stdout });
        return;
      }
      setTree(buildTree(parseListing(kind, r.stdout)));
    } catch (err) {
      log.error('加载压缩包列表失败:', err);
      setError({ msg: '无法解析列表输出', detail: String(err) });
    } finally {
      setLoading(false);
    }
  }, [path, serverId, kind]);

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
