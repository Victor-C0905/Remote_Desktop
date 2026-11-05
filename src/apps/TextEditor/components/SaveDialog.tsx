/**
 * 保存对话框组件
 *
 * 用于文本编辑器的“另存为”与未命名文件保存：
 * - 文件名输入
 * - 目录通过通用 RemoteDirectoryPicker 选择
 * - 最终拼接为远程绝对路径
 */

import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Dialog } from '../../../components/Dialog/Dialog';
import { RemoteDirectoryPicker } from '../../../components/RemoteDirectoryPicker/RemoteDirectoryPicker';
import { joinRemotePath, normalizeRemotePath } from '../../../components/RemoteDirectoryPicker/path';

interface SaveDialogProps {
  isOpen: boolean;
  /** 当前服务器；为空时只能手动取消 */
  serverId: string;
  defaultDirectory?: string;
  defaultFileName?: string;
  onSave: (path: string) => void;
  onCancel: () => void;
}

export function SaveDialog({
  isOpen, serverId, defaultDirectory = '/home/user', defaultFileName = 'untitled.txt',
  onSave, onCancel,
}: SaveDialogProps) {
  const { t } = useTranslation();
  const [directory, setDirectory] = useState(() => normalizeRemotePath(defaultDirectory));
  const [fileName, setFileName] = useState(defaultFileName);
  const [browsing, setBrowsing] = useState(false);

  // 每次重新打开时恢复默认目录/文件名
  useEffect(() => {
    if (!isOpen) return;
    setDirectory(normalizeRemotePath(defaultDirectory));
    setFileName(defaultFileName);
    setBrowsing(false);
  }, [isOpen, defaultDirectory, defaultFileName]);

  if (!isOpen) return null;

  if (browsing) {
    return (
      <RemoteDirectoryPicker
        serverId={serverId}
        initialPath={directory}
        onSelect={(p) => { setDirectory(p); setBrowsing(false); }}
        onCancel={() => setBrowsing(false)}
      />
    );
  }

  const name = fileName.trim();
  const invalidName = !name || name.includes('/') || name.includes('\0');
  const fullPath = invalidName ? '' : joinRemotePath(directory, name);

  const handleSave = () => {
    if (!invalidName) onSave(fullPath);
  };

  return (
    <Dialog onClose={onCancel}>
      <div className="dg-title">{t('textEditor.saveTitle')}</div>

      <div>
        <label className="dg-field-label" htmlFor="save-dialog-name">{t('textEditor.fileName')}</label>
        <input
          id="save-dialog-name"
          type="text"
          className="dg-input"
          value={fileName}
          onChange={(e) => setFileName(e.target.value)}
          onKeyDown={(e) => { if (e.key === 'Enter') handleSave(); }}
          autoFocus
        />
      </div>

      <div>
        <label className="dg-field-label" htmlFor="save-dialog-dir">{t('textEditor.saveDirectory')}</label>
        <div className="rdp-save-dir">
          <input id="save-dialog-dir" type="text" className="dg-input" value={directory} readOnly />
          <button className="dg-btn" onClick={() => setBrowsing(true)} disabled={!serverId}>
            {t('textEditor.browseDirectory')}
          </button>
        </div>
      </div>

      <div className="dg-actions">
        <button className="dg-btn" onClick={onCancel}>{t('common.cancel')}</button>
        <button className="dg-btn-primary" onClick={handleSave} disabled={invalidName || !serverId}>
          {t('textEditor.menuSave')}
        </button>
      </div>
    </Dialog>
  );
}
