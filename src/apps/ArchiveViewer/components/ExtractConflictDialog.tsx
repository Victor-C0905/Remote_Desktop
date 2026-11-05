/**
 * 解压目标冲突对话框
 *
 * 解压前检测到目标位置已有同名项时出现，行为对齐 Windows 资源管理器：
 * 覆盖 / 跳过 / 取消，未选择前不执行解压。
 */

import { useTranslation } from 'react-i18next';
import { Dialog } from '../../../components/Dialog/Dialog';

interface ExtractConflictDialogProps {
  /** 检测到的真实冲突条目数（同名文件 / 类型冲突） */
  count: number;
  /** 冲突路径预览（相对目标目录，可能少于 count） */
  paths?: string[];
  /** 目录层级过多，未完整扫描（保守提示） */
  truncated?: boolean;
  onOverwrite: () => void;
  onSkip: () => void;
  onCancel: () => void;
}

export function ExtractConflictDialog({
  count, paths, truncated, onOverwrite, onSkip, onCancel,
}: ExtractConflictDialogProps) {
  const { t } = useTranslation();
  const preview = paths ?? [];
  return (
    <Dialog onClose={onCancel} width={440}>
      <div className="dg-title">{t('archive.conflictTitle')}</div>
      <div className="dg-message">
        {truncated
          ? t('archive.conflictMessageTruncated')
          : t('archive.conflictMessage', { count })}
      </div>
      {preview.length > 0 && (
        <div className="av-conflict-paths">
          {preview.map((path) => (
            <div key={path} className="av-conflict-path">{path}</div>
          ))}
          {count > preview.length && (
            <div className="av-conflict-more">
              {t('archive.conflictMore', { count: count - preview.length })}
            </div>
          )}
        </div>
      )}
      <button className="dg-btn dg-btn-block" onClick={onOverwrite}>
        {t('archive.conflictOverwrite')}
      </button>
      <button className="dg-btn dg-btn-block" onClick={onSkip}>
        {t('archive.conflictSkip')}
      </button>
      <div className="dg-actions">
        <button className="dg-btn" onClick={onCancel}>{t('common.cancel')}</button>
      </div>
    </Dialog>
  );
}
