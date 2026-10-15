/**
 * 状态栏组件
 *
 * 纯信息展示：光标位置、行数、字符数、语言、编码、换行符、保存状态、修改时间
 * （操作类入口已按频率归位：高频→菜单栏，低频→菜单）
 *
 * 数据源为活动页签的 DocStatus 快照（由 EditorDocument 上报），
 * 未打开任何页签/新文件时展示对应的新建态信息。
 */

import React from 'react';
import { useTranslation } from 'react-i18next';
import '../TextEditor.css'; // 导入样式文件
import type { DocStatus } from './EditorDocument';
import { detectLanguage } from '../utils/language';
import { formatTime } from '../utils/format';

/**
 * StatusBar 组件 Props
 */
interface StatusBarProps {
  /** 活动页签的文档状态（无页签/状态未上报时为 null） */
  status: DocStatus | null;

  /** 活动服务器名（未连接为 null） */
  serverName: string | null;
}

/**
 * 状态栏组件
 *
 * @param props - 组件属性
 *
 * @example
 * ```tsx
 * <StatusBar status={activeStatus} serverName={activeServer?.name ?? null} />
 * ```
 */
export function StatusBar({ status, serverName }: StatusBarProps): React.ReactElement {
  const { t } = useTranslation();
  // 无状态（理论上窗口至少有一个页签，此处兜底）
  if (!status) {
    return (
      <div className="te-status-bar">
        <div className="te-status-group">
          <span className="te-status-item">{t('textEditor.untitledFile')}</span>
          <span className="te-status-item">{t('textEditor.cursorPos', { line: 1, column: 1 })}</span>
        </div>
        <div className="te-status-group" />
      </div>
    );
  }

  const isText = status.fileType === 'text' || status.fileType === null; // 未命名新文件等同文本编辑
  const isBinary = status.fileType === 'binary';
  const untitled = status.path === null;

  // 左组：编辑信息（未命名标识 / 光标 / 选中数 / 行数 / 字符数或文件大小）
  const leftItems: React.ReactNode[] = [];
  if (isBinary) {
    leftItems.push(
      <span key="ro" className="te-status-badge te-status-badge-readonly">{t('textEditor.readonly')}</span>,
    );
  }
  if (untitled && !isBinary) {
    leftItems.push(
      <span key="untitled" className="te-status-item">{t('textEditor.untitledFile')}</span>,
    );
  }
  if (isText) {
    leftItems.push(
      <span key="cursor" className="te-status-item">
        {t('textEditor.cursorPos', { line: status.cursor.line, column: status.cursor.column })}
      </span>,
    );
    if ((status.docStats?.selected ?? 0) > 0) {
      leftItems.push(
        <span key="sel" className="te-status-item">
          {t('textEditor.selectedChars', { count: status.docStats!.selected })}
        </span>,
      );
    }
    leftItems.push(
      <span key="lines" className="te-status-item">
        {t('textEditor.lines', { count: status.docStats?.lines ?? 0 })}
      </span>,
    );
  }
  leftItems.push(
    <span key="size" className="te-status-item">
      {isBinary
        ? status.sizeLabel
        : isText
          ? t('textEditor.chars', { count: status.docStats?.chars ?? 0 })
          : t('textEditor.chars', { count: 0 })}
    </span>,
  );

  // 右组：文件属性（语言 / 编码 / 换行符 / 保存状态 / 修改时间 / 服务器）
  const rightItems: React.ReactNode[] = [];
  if (isText && status.path) {
    rightItems.push(
      <span key="lang" className="te-status-item">
        {detectLanguage(status.path)}
      </span>,
    );
  }
  if (!untitled || isBinary) {
    rightItems.push(
      <span key="enc" className="te-status-item">
        {isBinary ? t('textEditor.binary') : 'UTF-8'}
      </span>,
    );
  }
  if (isText) {
    rightItems.push(
      <span key="eol" className="te-status-item">{status.eol}</span>,
    );
  }
  if (isText) {
    rightItems.push(
      status.dirty
        ? <span key="save" className="te-status-badge te-status-badge-unsaved">{t('textEditor.unsaved')}</span>
        : untitled
          ? null
          : <span key="save" className="te-status-item">{t('textEditor.saved')}</span>,
    );
  }
  if (!untitled && status.mtime > 0) {
    rightItems.push(
      <span key="mtime" className="te-status-item">{formatTime(status.mtime)}</span>,
    );
  }
  if (serverName) {
    rightItems.push(
      <span key="server" className="te-status-item">{serverName}</span>,
    );
  }

  return (
    <div className="te-status-bar">
      {/* 左组：编辑信息 */}
      <div className="te-status-group">
        {leftItems}
      </div>
      {/* 右组：文件属性 */}
      <div className="te-status-group">
        {rightItems}
      </div>
    </div>
  );
}
