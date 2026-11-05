/**
 * 解压对话框（内嵌于 ArchiveViewer）
 *
 * 对齐 7-Zip / WinRAR 的提取面板：
 * - 目标目录可编辑，默认压缩包所在目录
 * - 可直接浏览远程目录（保留表单状态）
 * - 可选“解压到独立子文件夹”
 * - 执行中不可关闭
 */

import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Dialog } from '../../../components/Dialog/Dialog';
import { RemoteDirectoryPicker } from '../../../components/RemoteDirectoryPicker/RemoteDirectoryPicker';
import { joinRemotePath, normalizeRemotePath } from '../../../components/RemoteDirectoryPicker/path';
import { archiveParentPath, archiveStemName } from '../extractTarget';

interface ExtractDialogProps {
  fileName: string;
  /** 压缩包完整路径（推导默认目标目录与子文件夹名） */
  path: string;
  serverId: string;
  status: 'ask' | 'running';
  /** 选中项数量；为空表示按目录/整个压缩包解压 */
  selectionCount?: number;
  /** 当前浏览的包内目录（非空时解压范围 = 该目录子树） */
  currentDir?: string;
  onExtract: (targetDir: string) => void;
  onClose: () => void;
}

export function ExtractDialog({
  fileName, path, serverId, status, selectionCount, currentDir, onExtract, onClose,
}: ExtractDialogProps) {
  const { t } = useTranslation();
  const parent = archiveParentPath(path);
  const stem = archiveStemName(fileName);
  const [baseDir, setBaseDir] = useState(parent);
  const [subfolder, setSubfolder] = useState(false);
  const [browsing, setBrowsing] = useState(false);

  const currentDirName = currentDir?.split('/').filter(Boolean).pop() ?? currentDir ?? '';
  const title = selectionCount !== undefined
    ? t('archive.dialogSelectedTitle', { count: selectionCount })
    : currentDir
      ? t('archive.dialogCurrentDirTitle', { name: currentDirName })
      : t('archive.dialogTitle', { name: fileName });
  // 解压当前目录时包内路径已带目录前缀，再套独立文件夹会出现 A/quirel-web 双层嵌套
  const subfolderAvailable = !currentDir;

  if (browsing) {
    return (
      <RemoteDirectoryPicker
        serverId={serverId}
        initialPath={baseDir}
        onSelect={(dir) => { setBaseDir(dir); setBrowsing(false); }}
        onCancel={() => setBrowsing(false)}
      />
    );
  }

  if (status === 'running') {
    return (
      <Dialog onClose={onClose} disableDismiss width={380}>
        <div className="dg-title">{title}</div>
        <div className="dg-message">{t('archive.extracting')}</div>
      </Dialog>
    );
  }

  const finalTarget = subfolder && subfolderAvailable ? joinRemotePath(baseDir, stem) : baseDir;
  const normalized = normalizeRemotePath(finalTarget);
  const invalidTarget = !finalTarget.trim() || normalized === '/';

  return (
    <Dialog onClose={onClose} width={460}>
      <div className="dg-title">{title}</div>

      <div>
        <label className="dg-field-label" htmlFor="extract-target">{t('archive.targetDirectory')}</label>
        <div className="rdp-save-dir">
          <input
            id="extract-target"
            type="text"
            className="dg-input"
            value={baseDir}
            onChange={(e) => setBaseDir(e.target.value)}
            spellCheck={false}
            autoFocus
          />
          <button className="dg-btn" onClick={() => setBrowsing(true)} disabled={!serverId}>
            {t('archive.browseTarget')}
          </button>
        </div>
        <div className="av-field-hint">{t('archive.targetAutoCreate')}</div>
      </div>

      <label
        className={`av-subfolder${subfolderAvailable ? '' : ' av-subfolder-disabled'}`}
        title={subfolderAvailable ? undefined : t('archive.subfolderUnavailable')}
      >
        <input
          type="checkbox"
          checked={subfolder && subfolderAvailable}
          disabled={!subfolderAvailable}
          onChange={(e) => setSubfolder(e.target.checked)}
        />
        <span>{t('archive.extractToFolder', { dir: stem })}</span>
      </label>
      {!subfolderAvailable && (
        <div className="av-subfolder-hint">{t('archive.subfolderUnavailable')}</div>
      )}

      {invalidTarget && <div className="av-extract-error">{t('archive.invalidTarget')}</div>}

      <div className="dg-actions">
        <button className="dg-btn" onClick={onClose}>{t('common.cancel')}</button>
        <button
          className="dg-btn-primary"
          onClick={() => onExtract(normalized)}
          disabled={invalidTarget || !serverId}
        >
          {t('archive.extractAction')}
        </button>
      </div>
    </Dialog>
  );
}
