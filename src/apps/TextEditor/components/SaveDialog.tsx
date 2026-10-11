/**
 * 保存对话框组件
 *
 * 用于文本编辑器的"另存为"功能
 * 当用户保存新文件时，弹出此对话框让用户输入远程路径
 *
 * 底座使用共享 Dialog 组件（dg-*）：与全项目弹窗统一视觉
 */

import { useState } from 'react';
import { createLogger } from '../../../utils/logger';
import { Dialog } from '../../../components/Dialog/Dialog';

const log = createLogger('SaveDialog');

/**
 * SaveDialog 组件 Props
 */
interface SaveDialogProps {
  /** 是否打开对话框 */
  isOpen: boolean;

  /** 默认路径 */
  defaultPath?: string;

  /** 保存回调 */
  onSave: (path: string) => void;

  /** 取消回调 */
  onCancel: () => void;
}

/**
 * 保存对话框组件
 *
 * @param props - 组件属性
 *
 * @example
 * ```tsx
 * <SaveDialog
 *   isOpen={showSaveDialog}
 *   defaultPath="/home/user/untitled.txt"
 *   onSave={handleSaveAs}
 *   onCancel={handleCancelSave}
 * />
 * ```
 */
export function SaveDialog({ isOpen, defaultPath, onSave, onCancel }: SaveDialogProps) {
  // 使用 useState 管理路径输入
  const [path, setPath] = useState(defaultPath || '/home/user/untitled.txt');

  // 如果对话框未打开，返回 null
  if (!isOpen) return null;

  /**
   * 处理保存按钮点击
   */
  const handleSave = () => {
    // 验证路径不为空
    if (!path.trim()) {
      log.warn('路径为空，无法保存');
      return;
    }

    // 调用保存回调
    onSave(path.trim());
  };

  /**
   * 处理键盘事件
   *
   * Enter 键保存，Esc 键取消（Esc 已由 Dialog 底座统一处理）
   */
  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') {
      handleSave();
    }
  };

  return (
    <Dialog onClose={onCancel}>
      <div className="dg-title">保存文件</div>

      {/* 内容区 */}
      <div>
        <label className="dg-field-label" htmlFor="save-dialog-path">远程路径：</label>
        <input
          id="save-dialog-path"
          type="text"
          className="dg-input"
          value={path}
          onChange={(e) => setPath(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder="/home/user/untitled.txt"
          autoFocus
        />
      </div>

      {/* 操作按钮区 */}
      <div className="dg-actions">
        <button className="dg-btn" onClick={onCancel}>取消</button>
        <button className="dg-btn-primary" onClick={handleSave} disabled={!path.trim()}>
          保存
        </button>
      </div>
    </Dialog>
  );
}
