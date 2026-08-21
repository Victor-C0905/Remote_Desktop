/**
 * 编辑模式切换组件
 *
 * 提供文本模式和十六进制模式之间的切换功能
 * 遵循 Adwaita 设计规范的按钮样式
 */

import React from 'react';
import '../TextEditor.css'; // 导入样式文件
import type { EditorMode } from '../types/editor';

/**
 * EditorMode 组件 Props
 */
interface EditorModeProps {
  /** 当前编辑模式 */
  mode: EditorMode;

  /** 模式变更回调函数 */
  onChange: (mode: EditorMode) => void;
}

/**
 * 编辑模式切换组件
 *
 * 显示两个按钮："文本" 和 "十六进制"
 * 当前激活的模式会显示 accent 颜色背景
 *
 * @param props - 组件属性
 * @param props.mode - 当前编辑模式 ('text' | 'hex')
 * @param props.onChange - 模式变更回调函数
 *
 * @example
 * ```tsx
 * <EditorMode
 *   mode="text"
 *   onChange={(newMode) => setEditorMode(newMode)}
 * />
 * ```
 */
export function EditorMode({ mode, onChange }: EditorModeProps): React.ReactElement {
  return (
    <div className="te-editor-mode">
      {/* 文本模式按钮 */}
      <button
        className={`te-mode-btn ${mode === 'text' ? 'active' : ''}`}
        onClick={() => onChange('text')}
        aria-pressed={mode === 'text'}
        type="button"
      >
        文本
      </button>

      {/* 十六进制模式按钮 */}
      <button
        className={`te-mode-btn ${mode === 'hex' ? 'active' : ''}`}
        onClick={() => onChange('hex')}
        aria-pressed={mode === 'hex'}
        type="button"
      >
        十六进制
      </button>
    </div>
  );
}