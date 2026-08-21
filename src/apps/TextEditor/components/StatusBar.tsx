/**
 * 状态栏组件
 *
 * 显示文件的状态信息，包括光标位置、行数、字符数、编码、保存状态和修改时间
 * 遵循 Adwaita 设计规范的状态栏样式
 */

import React from 'react';
import '../TextEditor.css'; // 导入样式文件
import type { FileState } from '../types/editor';
import type { CursorPosition } from './TextEditorPane';

/**
 * StatusBar 组件 Props
 */
interface StatusBarProps {
  /** 当前文件状态（未打开文件时为 null） */
  fileState: FileState | null;

  /** 是否有未保存的变更 */
  hasUnsavedChanges: boolean;

  /** 光标位置（行号、列号） */
  cursorPosition: CursorPosition;

  /** 新文件的字符数（可选，仅用于未打开文件时） */
  newFileCharCount?: number;
}

/**
 * 格式化文件大小
 *
 * @param bytes - 字节数
 * @returns 格式化后的文件大小字符串（例如：1.5 KB）
 */
function formatFileSize(bytes: number): string {
  if (bytes === 0) return '0 B';

  const units = ['B', 'KB', 'MB', 'GB'];
  const k = 1024;
  const i = Math.floor(Math.log(bytes) / Math.log(k));

  return `${(bytes / Math.pow(k, i)).toFixed(1)} ${units[i]}`;
}

/**
 * 格式化修改时间
 *
 * @param timestamp - Unix 时间戳（毫秒）
 * @returns 格式化后的时间字符串（例如：2024-01-15 14:30）
 */
function formatTime(timestamp: number): string {
  const date = new Date(timestamp);
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, '0');
  const day = String(date.getDate()).padStart(2, '0');
  const hours = String(date.getHours()).padStart(2, '0');
  const minutes = String(date.getMinutes()).padStart(2, '0');

  return `${year}-${month}-${day} ${hours}:${minutes}`;
}

/**
 * 计算文本文件的行数
 *
 * @param content - 文本内容
 * @returns 行数
 */
function countLines(content: string): number {
  if (!content) return 0;
  return content.split('\n').length;
}

/**
 * 状态栏组件
 *
 * 显示文件信息：
 * - 未打开文件：显示"未命名文件"、光标位置
 * - 已打开文件：显示光标位置、行数、字符数、编码、保存状态、修改时间
 *
 * @param props - 组件属性
 * @param props.fileState - 当前文件状态
 * @param props.hasUnsavedChanges - 是否有未保存的变更
 * @param props.cursorPosition - 光标位置（行号、列号）
 *
 * @example
 * ```tsx
 * <StatusBar
 *   fileState={currentFile}
 *   hasUnsavedChanges={hasUnsavedChanges}
 *   cursorPosition={{ line: 1, column: 1 }}
 * />
 * ```
 */
export function StatusBar({ fileState, hasUnsavedChanges, cursorPosition, newFileCharCount }: StatusBarProps): React.ReactElement {
  // 未打开文件时显示"未命名文件"
  if (!fileState) {
    return (
      <div className="te-status-bar">
        <span className="te-status-item">未命名文件</span>
        {/* 分隔符 */}
        <span className="te-status-separator">|</span>
        {/* 光标位置（新文件也可以显示） */}
        <span className="te-status-item">
          行 {cursorPosition.line}, 列 {cursorPosition.column}
        </span>
        {/* 分隔符 */}
        <span className="te-status-separator">|</span>
        {/* 字符个数（新文件） */}
        <span className="te-status-item">
          {newFileCharCount || 0}个字符
        </span>
        {/* 分隔符 */}
        <span className="te-status-separator">|</span>
        <span className="te-status-item">
          {hasUnsavedChanges ? '未保存' : ''}
        </span>
      </div>
    );
  }

  // 根据文件类型计算状态信息
  const lineCount = fileState.fileType === 'text' ? countLines(fileState.content) : 0;
  const charCount = fileState.fileType === 'text' ? fileState.content.length : 0;
  const fileSize = fileState.fileType === 'binary' ? formatFileSize(fileState.binaryData.length) : formatFileSize(charCount);
  const encoding = fileState.fileType === 'text' ? 'UTF-8' : '二进制';

  return (
    <div className="te-status-bar">
      {/* 只读标识（二进制文件） */}
      {fileState.fileType === 'binary' && (
        <span className="te-status-item te-status-readonly">
          只读
        </span>
      )}

      {/* 光标位置（仅文本文件显示） */}
      {fileState.fileType === 'text' && (
        <span className="te-status-item">
          行 {cursorPosition.line}, 列 {cursorPosition.column}
        </span>
      )}

      {/* 分隔符 */}
      <span className="te-status-separator">|</span>

      {/* 行数（仅文本文件显示） */}
      {fileState.fileType === 'text' && (
        <span className="te-status-item">
          {lineCount} 行
        </span>
      )}

      {/* 字符数或文件大小 */}
      <span className="te-status-item">
        {fileState.fileType === 'text' ? `${charCount}个字符` : fileSize}
      </span>

      {/* 分隔符 */}
      <span className="te-status-separator">|</span>

      {/* 文件编码 */}
      <span className="te-status-item">
        {encoding}
      </span>

      {/* 分隔符 */}
      <span className="te-status-separator">|</span>

      {/* 保存状态（仅文本文件显示） */}
      {fileState.fileType === 'text' && (
        <span className="te-status-item">
          {hasUnsavedChanges ? '未保存' : '已保存'}
        </span>
      )}

      {/* 分隔符 */}
      <span className="te-status-separator">|</span>

      {/* 修改时间 */}
      <span className="te-status-item">
        {formatTime(fileState.mtime)}
      </span>
    </div>
  );
}