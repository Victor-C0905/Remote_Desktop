/**
 * 传输任务按服务器分组的纯函数
 *
 * 设计：传输列表全局唯一（跨服务器可见），按 session_id（即 server_id）分组；
 * 当前活跃服务器的组在前且默认展开，其他服务器折叠为摘要行。
 * 边界原则：服务器之间的边界画在"执行权"上，不画在"可见性"上。
 */

import type { TransferTask } from '../../hooks/useTransferProgress';
// ServerConfig 定义于 src/types/server.ts（serversStore 仅重导出，此处直接引用源头）
import type { ServerConfig } from '../../types/server';

/** 分组结果 */
export interface TransferGroup {
  serverId: string;
  serverName: string;
  /** 是否为当前活跃服务器的组 */
  isCurrent: boolean;
  tasks: TransferTask[];
  /** 折态摘要，如 "2 个已中断 · 1 个已完成" */
  summary: string;
}

/** 状态中文标签（用于折叠组的摘要行） */
const STATUS_LABELS: Record<TransferTask['status'], string> = {
  active: '进行中',
  pending: '排队中',
  paused: '已暂停',
  completed: '已完成',
  error: '失败',
  cancelled: '已取消',
  interrupted: '已中断',
};

/**
 * 按 session_id（= server_id）分组
 * - 当前活跃服务器的组排在最前
 * - 其余组按组内最近任务的 start_time 倒序
 * - 服务器已被删除时兜底显示「未知服务器」
 */
export function groupTransfersByServer(
  transfers: TransferTask[],
  servers: Pick<ServerConfig, 'id' | 'name' | 'host'>[],
  activeServerId: string | null
): TransferGroup[] {
  // 按 session_id 分组（保持插入顺序）
  const byServer = new Map<string, TransferTask[]>();
  for (const t of transfers) {
    const list = byServer.get(t.session_id) ?? [];
    list.push(t);
    byServer.set(t.session_id, list);
  }

  const groups: TransferGroup[] = [];
  for (const [serverId, tasks] of byServer) {
    const server = servers.find((s) => s.id === serverId);
    groups.push({
      serverId,
      serverName: server?.name || server?.host || '未知服务器',
      isCurrent: serverId === activeServerId,
      tasks,
      summary: buildGroupSummary(tasks),
    });
  }

  groups.sort((a, b) => {
    // 当前服务器组置顶
    if (a.isCurrent !== b.isCurrent) return a.isCurrent ? -1 : 1;
    // 其余按组内最近任务时间倒序（新的在前）
    const aLatest = Math.max(...a.tasks.map((t) => t.start_time));
    const bLatest = Math.max(...b.tasks.map((t) => t.start_time));
    return bLatest - aLatest;
  });

  return groups;
}

/** 聚合组内各状态计数，生成摘要文案 */
export function buildGroupSummary(tasks: TransferTask[]): string {
  const counts = new Map<TransferTask['status'], number>();
  for (const t of tasks) {
    counts.set(t.status, (counts.get(t.status) ?? 0) + 1);
  }
  return [...counts.entries()]
    .map(([status, n]) => `${n} 个${STATUS_LABELS[status]}`)
    .join(' · ');
}
