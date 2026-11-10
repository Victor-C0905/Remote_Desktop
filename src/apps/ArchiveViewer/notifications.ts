/**
 * 解压失败 → 通知中心
 *
 * 工具栏提示只在压缩包窗口可见；解压失败（尤其大包长任务）需要进入通知中心，
 * 避免用户切走窗口后错过。成功/取消等普通结果仍只保留工具栏提示。
 */

import i18n from '../../i18n';
import { useServersStore } from '../../stores/serversStore';
import { pushServerNotification } from '../../utils/notify';

export type ArchiveFailureKind = 'extractOne' | 'extractAll';

export interface ArchiveFailureNotice {
  kind: ArchiveFailureKind;
  serverId: string;
  archiveName: string;
  message: string;
  /** 全部解压时的目标目录（单条提取不传） */
  targetDir?: string;
}

/** 推送一条解压失败通知（标题/正文均为用户可读文案） */
export function notifyArchiveFailure(notice: ArchiveFailureNotice): void {
  const { kind, serverId, archiveName, message, targetDir } = notice;
  const title = i18n.t(
    kind === 'extractOne' ? 'archive.notify.extractOneTitle' : 'archive.notify.extractAllTitle',
  );
  const lines = [message];
  if (archiveName) lines.push(archiveName);
  if (targetDir) lines.push(targetDir);

  const server = useServersStore.getState().servers.find((s) => s.id === serverId);
  pushServerNotification(serverId, {
    title,
    body: lines.join('\n'),
    action: i18n.t('archive.notify.action'),
    urgency: 'normal',
    source: server?.name || server?.host || archiveName || i18n.t('archive.notify.source'),
  });
}
