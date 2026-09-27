/**
 * 解压对话框（内嵌于 ArchiveViewer）
 *
 * 两选项：独立文件夹（stem/）/ 压缩包所在目录；执行中不可关闭。
 */

interface ExtractDialogProps {
  fileName: string;
  /** 压缩包完整路径（推导父目录与 stem 展示） */
  path: string;
  status: 'ask' | 'running';
  onExtract: (mode: 'folder' | 'here') => void;
  onClose: () => void;
}

export function ExtractDialog({ fileName, path, status, onExtract, onClose }: ExtractDialogProps) {
  const parent = path.slice(0, path.lastIndexOf('/')) || '/';
  const stem = fileName.replace(/\.(zip|tar|tar\.gz|tgz|tar\.bz2|tar\.xz|7z|rar)$/i, '');
  const targetDir = parent === '/' ? `/${stem}` : `${parent}/${stem}`;

  return (
    <div className="av-dialog-backdrop" onClick={() => status !== 'running' && onClose()}>
      <div className="av-dialog" onClick={(e) => e.stopPropagation()}>
        <h3>解压 {fileName}</h3>
        {status === 'ask' ? (
          <>
            <button className="av-btn" onClick={() => onExtract('folder')}>
              解压到独立文件夹（{targetDir}/）
            </button>
            <button className="av-btn" onClick={() => onExtract('here')}>解压到当前位置</button>
            <div className="av-dialog-btns">
              <button className="av-btn" onClick={onClose}>取消</button>
            </div>
          </>
        ) : (
          <span>解压中…</span>
        )}
      </div>
    </div>
  );
}
