/**
 * 保存对话框组件
 *
 * 用于文本编辑器的"另存为"功能
 * 当用户保存新文件时，弹出此对话框让用户输入远程路径
 *
 * 设计遵循 GNOME HIG 规范：
 * - 使用 Adwaita 设计系统的 CSS 变量
 * - 半透明遮罩层 + 圆角对话框
 * - 标题 + 内容区 + 操作按钮区
 */

import { useState } from 'react';

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
      console.warn('[SaveDialog] 路径为空，无法保存');
      return;
    }

    // 调用保存回调
    onSave(path.trim());
  };

  /**
   * 处理取消按钮点击
   */
  const handleCancel = () => {
    onCancel();
  };

  /**
   * 处理键盘事件
   *
   * Enter 键保存，Esc 键取消
   */
  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') {
      handleSave();
    } else if (e.key === 'Escape') {
      handleCancel();
    }
  };

  return (
    <div
      style={{
        position: 'fixed',
        top: 0,
        left: 0,
        right: 0,
        bottom: 0,
        background: 'rgba(0, 0, 0, 0.5)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        zIndex: 1000,
      }}
      onClick={handleCancel}
    >
      <div
        style={{
          background: 'var(--ovelis-view-bg)',
          border: '1px solid var(--ovelis-border-color)',
          borderRadius: 'var(--radius-lg)',
          padding: '24px',
          minWidth: '400px',
          maxWidth: '600px',
          boxShadow: 'var(--shadow-popup)',
        }}
        onClick={(e) => e.stopPropagation()}
      >
        {/* 标题 */}
        <div
          style={{
            fontSize: '16px',
            fontWeight: 600,
            color: 'var(--ovelis-text-primary)',
            marginBottom: '16px',
          }}
        >
          保存文件
        </div>

        {/* 内容区 */}
        <div style={{ marginBottom: '16px' }}>
          {/* 标签 */}
          <label
            style={{
              display: 'block',
              fontSize: '13px',
              color: 'var(--ovelis-text-secondary)',
              marginBottom: '6px',
            }}
          >
            远程路径：
          </label>

          {/* 输入框 */}
          <input
            type="text"
            value={path}
            onChange={(e) => setPath(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder="/home/user/untitled.txt"
            autoFocus
            style={{
              width: '100%',
              padding: '8px 12px',
              background: 'var(--ovelis-window-bg)',
              border: '1px solid var(--ovelis-border-color)',
              borderRadius: 'var(--radius-sm)',
              color: 'var(--ovelis-text-primary)',
              fontSize: '14px',
              fontFamily: 'var(--font-mono)',
              outline: 'none',
            }}
            onFocus={(e) => {
              e.target.style.outline = '2px solid var(--ovelis-accent-bg)';
              e.target.style.outlineOffset = '2px';
            }}
            onBlur={(e) => {
              e.target.style.outline = 'none';
            }}
          />
        </div>

        {/* 操作按钮区 */}
        <div
          style={{
            display: 'flex',
            gap: '12px',
            justifyContent: 'flex-end',
          }}
        >
          {/* 取消按钮 */}
          <button
            onClick={handleCancel}
            style={{
              padding: '8px 16px',
              borderRadius: 'var(--radius-sm)',
              fontSize: '13px',
              cursor: 'pointer',
              background: 'var(--ovelis-card-bg)',
              border: '1px solid var(--ovelis-border-color)',
              color: 'var(--ovelis-text-primary)',
              transition: 'background var(--duration-fast) var(--ease-out)',
            }}
            onMouseEnter={(e) => {
              e.currentTarget.style.background = 'var(--ovelis-headerbar-bg)';
            }}
            onMouseLeave={(e) => {
              e.currentTarget.style.background = 'var(--ovelis-card-bg)';
            }}
          >
            取消
          </button>

          {/* 保存按钮 */}
          <button
            onClick={handleSave}
            disabled={!path.trim()}
            style={{
              padding: '8px 16px',
              borderRadius: 'var(--radius-sm)',
              fontSize: '13px',
              cursor: path.trim() ? 'pointer' : 'not-allowed',
              background: path.trim() ? 'var(--ovelis-accent-bg)' : 'var(--ovelis-text-disabled)',
              border: '1px solid var(--ovelis-accent-bg)',
              color: 'var(--ovelis-accent-fg)',
              transition: 'background var(--duration-fast) var(--ease-out)',
              opacity: path.trim() ? 1 : 0.6,
            }}
            onMouseEnter={(e) => {
              if (path.trim()) {
                e.currentTarget.style.background = 'var(--ovelis-accent-hover)';
              }
            }}
            onMouseLeave={(e) => {
              if (path.trim()) {
                e.currentTarget.style.background = 'var(--ovelis-accent-bg)';
              }
            }}
          >
            保存
          </button>
        </div>
      </div>
    </div>
  );
}