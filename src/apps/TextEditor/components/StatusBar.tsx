/**
 * 状态栏组件
 *
 * 显示文件的状态信息，包括光标位置、行数、字符数、编码、保存状态和修改时间
 * 遵循 Adwaita 设计规范的状态栏样式
 *
 * 布局：左右分组（左侧=编辑信息，右侧=文件属性），组间竖线分隔（CSS 实现，非文本 |）
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
 * 根据扩展名推断编辑语言（状态栏显示用）
 *
 * @param path - 文件路径
 * @returns 语言显示名（例如：TypeScript、Markdown）
 */
function detectLanguage(path: string): string {
  const ext = path.split('.').pop()?.toLowerCase() || '';
  const langMap: Record<string, string> = {
    ts: 'TypeScript', tsx: 'TypeScript React',
    js: 'JavaScript', jsx: 'JavaScript React', mjs: 'JavaScript',
    py: 'Python', rs: 'Rust', go: 'Go', java: 'Java', kt: 'Kotlin',
    c: 'C', h: 'C Header', cpp: 'C++', cc: 'C++', hpp: 'C++',
    cs: 'C#', rb: 'Ruby', php: 'PHP', swift: 'Swift',
    css: 'CSS', scss: 'SCSS', less: 'Less',
    html: 'HTML', htm: 'HTML', xml: 'XML', json: 'JSON', jsonc: 'JSON',
    md: 'Markdown', markdown: 'Markdown', rst: 'reStructuredText',
    sh: 'Shell', bash: 'Shell', zsh: 'Shell', fish: 'Shell',
    yml: 'YAML', yaml: 'YAML', toml: 'TOML', ini: 'INI', conf: 'Config',
    sql: 'SQL', lua: 'Lua', vim: 'Vim Script',
    dockerfile: 'Dockerfile', makefile: 'Makefile',
    txt: '纯文本', log: '日志',
  };
  return langMap[ext] || (ext ? ext.toUpperCase() : '纯文本');
}

/**
 * 检测换行符类型（LF / CRLF）
 *
 * @param content - 文本内容
 * @returns 换行符标识
 */
function detectLineEnding(content: string): 'LF' | 'CRLF' {
  return content.includes('\r\n') ? 'CRLF' : 'LF';
}

/**
 * 状态栏组件
 *
 * 显示文件信息：
 * - 未打开文件：显示"未命名文件"、光标位置、字符数
 * - 已打开文件：左组（光标位置、行数、字符数）+ 右组（语言、编码、换行符、保存状态、修改时间）
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
        {/* 左组：光标位置 + 字符数 */}
        <div className="te-status-group">
          <span className="te-status-item">未命名文件</span>
          <span className="te-status-item">
            行 {cursorPosition.line}, 列 {cursorPosition.column}
          </span>
          <span className="te-status-item">
            {newFileCharCount || 0} 个字符
          </span>
        </div>
        {/* 右组：新建状态提示 */}
        <div className="te-status-group">
          {hasUnsavedChanges && (
            <span className="te-status-badge te-status-badge-unsaved">未保存</span>
          )}
        </div>
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
      {/* 左组：编辑信息（光标位置 / 行数 / 字符数或大小） */}
      <div className="te-status-group">
        {/* 只读标识（二进制文件） */}
        {fileState.fileType === 'binary' && (
          <span className="te-status-badge te-status-badge-readonly">
            只读
          </span>
        )}

        {/* 光标位置（仅文本文件显示） */}
        {fileState.fileType === 'text' && (
          <span className="te-status-item">
            行 {cursorPosition.line}, 列 {cursorPosition.column}
          </span>
        )}

        {/* 行数（仅文本文件显示） */}
        {fileState.fileType === 'text' && (
          <span className="te-status-item">
            {lineCount} 行
          </span>
        )}

        {/* 字符数或文件大小 */}
        <span className="te-status-item">
          {fileState.fileType === 'text' ? `${charCount} 个字符` : fileSize}
        </span>
      </div>

      {/* 右组：文件属性（语言 / 编码 / 换行符 / 保存状态 / 修改时间） */}
      <div className="te-status-group">
        {/* 语言（从扩展名推断，仅文本文件显示） */}
        {fileState.fileType === 'text' && (
          <span className="te-status-item">
            {detectLanguage(fileState.path)}
          </span>
        )}

        {/* 文件编码 */}
        <span className="te-status-item">
          {encoding}
        </span>

        {/* 换行符（仅文本文件显示） */}
        {fileState.fileType === 'text' && (
          <span className="te-status-item">
            {detectLineEnding(fileState.content)}
          </span>
        )}

        {/* 保存状态徽标（仅文本文件显示）：未保存=橙色徽标，已保存=普通灰字 */}
        {fileState.fileType === 'text' && (
          hasUnsavedChanges ? (
            <span className="te-status-badge te-status-badge-unsaved">未保存</span>
          ) : (
            <span className="te-status-item">已保存</span>
          )
        )}

        {/* 修改时间 */}
        <span className="te-status-item">
          {formatTime(fileState.mtime)}
        </span>
      </div>
    </div>
  );
}
