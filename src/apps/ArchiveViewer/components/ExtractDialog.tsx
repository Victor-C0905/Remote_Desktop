/**
 * 解压对话框（内嵌于 ArchiveViewer）
 *
 * 两选项：独立文件夹（stem/）/ 压缩包所在目录；执行中不可关闭。
 * 底座使用共享 Dialog 组件（dg-*）：与全项目弹窗统一视觉
 */

import { useTranslation } from 'react-i18next';
import { Dialog } from '../../../components/Dialog/Dialog';

interface ExtractDialogProps {
  fileName: string;
  /** 压缩包完整路径（推导父目录与 stem 展示） */
  path: string;
  status: 'ask' | 'running';
  onExtract: (mode: 'folder' | 'here') => void;
  onClose: () => void;
}

export function ExtractDialog({ fileName, path, status, onExtract, onClose }: ExtractDialogProps) {
  const { t } = useTranslation();
  const parent = path.slice(0, path.lastIndexOf('/')) || '/';
  const stem = fileName.replace(/\.(zip|tar|tar\.gz|tgz|tar\.bz2|tar\.xz|7z|rar)$/i, '');
  const targetDir = parent === '/' ? `/${stem}` : `${parent}/${stem}`;

  return (
    <Dialog onClose={onClose} disableDismiss={status === 'running'} width={360}>
      <div className="dg-title">{t('archive.dialogTitle', { name: fileName })}</div>
      {status === 'ask' ? (
        <>
          <div className="dg-message">{t('archive.chooseTarget')}</div>
          <button className="dg-btn dg-btn-block" onClick={() => onExtract('folder')}>
            {t('archive.extractToFolder', { dir: targetDir })}
          </button>
          <button className="dg-btn dg-btn-block" onClick={() => onExtract('here')}>{t('archive.extractHere')}</button>
          <div className="dg-actions">
            <button className="dg-btn" onClick={onClose}>{t('common.cancel')}</button>
          </div>
        </>
      ) : (
        <div className="dg-message">{t('archive.extracting')}</div>
      )}
    </Dialog>
  );
}
