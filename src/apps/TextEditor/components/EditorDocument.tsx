/**
 * EditorDocument — 编辑器窗口中的单个文档（页签）
 *
 * 每个页签一个组件实例，各自持有独立的 useFileManager 文档状态
 * （内容、脏标记、撤销历史、加载/保存状态互不干扰）。
 * 非活动页签保持挂载仅隐藏（display:none），切换回来时
 * 内容、光标与撤销历史原样保留。
 *
 * 状态上报：文档状态变化时通过 onStatus 通知窗口层，
 * 窗口层据此渲染页签（文件名/脏点）、菜单栏可用态与状态栏。
 */

import { forwardRef, memo, useEffect, useImperativeHandle, useRef, useState } from 'react';
import i18n from '../../../i18n';
import { createLogger } from '../../../utils/logger';
import { useFileManager } from '../hooks/useFileManager';
import { TextEditorPane } from './TextEditorPane';
import type { CursorPosition, DocStats, TextEditorPaneHandle } from './TextEditorPane';
import { HexEditorPane } from './HexEditorPane';
import { detectLineEnding, formatFileSize } from '../utils/format';
import '../TextEditor.css';

const log = createLogger('EditorDocument');

/**
 * 文档状态快照（窗口层渲染页签/菜单/状态栏的数据源）
 */
export interface DocStatus {
  /** 页签/标题显示名 */
  label: string;

  /** 文件路径；未保存到服务器前为 null */
  path: string | null;

  /** 文件类型；未打开文件为 null，加载中为 'loading' */
  fileType: 'text' | 'binary' | 'loading' | null;

  /** 修改时间（未打开为 0） */
  mtime: number;

  /** 是否有未保存变更 */
  dirty: boolean;

  isLoading: boolean;
  isSaving: boolean;

  /** 错误信息（窗口层渲染内联横幅） */
  error: string | null;

  cursor: CursorPosition;
  docStats: DocStats | null;

  /** 是否处于十六进制查看模式 */
  hexMode: boolean;

  /** 换行符（仅文本文件） */
  eol: 'LF' | 'CRLF';

  /** 文件大小标签（仅二进制文件，如 "1.5 KB"） */
  sizeLabel: string;
}

export interface EditorDocumentHandle {
  /** 保存到原路径（仅已打开的文本文件） */
  save(serverId: string): Promise<void>;

  /** 另存为指定路径（成功后文档变为已打开文件） */
  saveAs(serverId: string, path: string): Promise<void>;

  undo(): void;
  redo(): void;

  /** 打开查找替换面板（同 Ctrl+F） */
  openSearch(): void;

  /** 切换十六进制查看模式 */
  toggleHexMode(): void;

  clearError(): void;
}

interface EditorDocumentProps {
  /** 页签唯一 ID（状态上报回传用） */
  tabId: string;

  /** 初始打开的远程文件（可选；不传则为未命名新文件） */
  initialPath?: string;

  initialServerId?: string;

  /** 是否隐藏（非活动页签保持挂载） */
  hidden: boolean;

  /** 自动换行（窗口级设置） */
  wrapText: boolean;

  /** 状态上报（父组件需保证引用稳定） */
  onStatus: (tabId: string, status: DocStatus) => void;
}

/** 根据路径取显示名 */
function labelOf(path: string | null, initialPath?: string): string {
  const source = path ?? initialPath;
  if (!source) return i18n.t('textEditor.untitledFile');
  return source.split('/').pop() || source;
}

export const EditorDocument = memo(
  forwardRef<EditorDocumentHandle, EditorDocumentProps>(function EditorDocument(
    { tabId, initialPath, initialServerId, hidden, wrapText, onStatus },
    ref,
  ) {
    const {
      fileState,
      isLoading,
      isSaving,
      error,
      localContent,
      openFile,
      saveFile,
      saveAs,
      closeFile,
      updateContent,
      hasUnsavedChanges,
      clearError,
    } = useFileManager();

    const [hexMode, setHexMode] = useState(false);
    const [cursor, setCursor] = useState<CursorPosition>({ line: 1, column: 1 });
    const [docStats, setDocStats] = useState<DocStats | null>(null);
    const paneRef = useRef<TextEditorPaneHandle>(null);
    const autoHexSwitched = useRef(false);

    // 打开初始文件（仅挂载时一次；initialPath 是页签的固有属性）
    // ⚠️ openedRef 守卫：React 对被拖动页签的子树会强制重跑 effect（deps 未变，
    // 见 Terminal.tsx 会话保活注释）。openFile 若重跑会从服务器重载内容，
    // 未保存修改随之丢失——因此真重跑必须跳过
    const openedRef = useRef(false);
    useEffect(() => {
      if (openedRef.current) return;
      openedRef.current = true;
      if (initialPath && initialServerId) {
        log.info('页签打开文件:', initialPath);
        openFile(initialServerId, initialPath).catch((err) => {
          log.error('页签打开文件失败:', err);
        });
      }
      // eslint-disable-next-line react-hooks/exhaustive-deps -- initialPath 为页签固有属性，仅挂载执行一次
    }, []);

    // 卸载时清理文档状态。⚠️ cleanup 不立即 closeFile（会清空文档内容，
    // 未命名文件的输入随之丢失）：只调度延迟清理；假重跑（拖拽重排）的
    // setup 会取消它实现保活，真实卸载时无人取消，照常清理
    const closeTimerRef = useRef<number | null>(null);
    useEffect(() => {
      // 取消上一次 cleanup 调度的延迟 closeFile（假重跑保活路径）
      if (closeTimerRef.current !== null) {
        clearTimeout(closeTimerRef.current);
        closeTimerRef.current = null;
      }
      return () => {
        closeTimerRef.current = window.setTimeout(() => {
          closeTimerRef.current = null;
          closeFile();
        }, 0);
      };
      // eslint-disable-next-line react-hooks/exhaustive-deps -- closeFile 为稳定引用
    }, []);

    // 二进制文件自动切换到十六进制模式（仅一次，不劫持用户手动切换）
    useEffect(() => {
      if (fileState?.fileType === 'binary' && !autoHexSwitched.current) {
        autoHexSwitched.current = true;
        setHexMode(true);
      }
    }, [fileState]);

    // 状态上报：文档任一相关状态变化时聚合上报
    // （hasUnsavedChanges 的身份随其内部依赖变化，作为依赖即可覆盖脏标记变化）
    const eol = fileState?.fileType === 'text'
      ? detectLineEnding(fileState.content)
      : 'LF';
    const sizeLabel = fileState?.fileType === 'binary'
      ? formatFileSize(fileState.binaryData.length)
      : '';

    useEffect(() => {
      onStatus(tabId, {
        label: labelOf(fileState?.path ?? null, initialPath),
        path: fileState?.path ?? null,
        fileType: fileState?.fileType ?? null,
        mtime: fileState?.mtime ?? 0,
        dirty: hasUnsavedChanges(),
        isLoading,
        isSaving,
        error,
        cursor,
        docStats,
        hexMode,
        eol,
        sizeLabel,
      });
    }, [
      onStatus, tabId, initialPath, fileState, hasUnsavedChanges,
      isLoading, isSaving, error, cursor, docStats, hexMode, eol,
    ]);

    // 命令句柄：菜单栏直连当前文档（fm 经 ref 取最新值）
    const fmRef = useRef({ saveFile, saveAs, clearError });
    fmRef.current = { saveFile, saveAs, clearError };

    useImperativeHandle(ref, () => ({
      save: (serverId) => fmRef.current.saveFile(serverId),
      saveAs: (serverId, path) => fmRef.current.saveAs(serverId, path),
      undo: () => paneRef.current?.undo(),
      redo: () => paneRef.current?.redo(),
      openSearch: () => paneRef.current?.openSearch(),
      toggleHexMode: () => setHexMode((m) => !m),
      clearError: () => fmRef.current.clearError(),
    }), []);

    const renderBody = () => {
      // 加载中：居中 spinner + 文件名
      if (isLoading) {
        const name = fileState?.path.split('/').pop() || initialPath?.split('/').pop() || '';
        return (
          <div className="te-loading">
            <div className="te-spinner" aria-hidden="true" />
            <span className="te-loading-text">{i18n.t('textEditor.loading', { name: name ? ` ${name}` : '' })}</span>
          </div>
        );
      }

      // 已打开文件
      if (fileState) {
        if (hexMode) {
          return (
            <HexEditorPane
              data={fileState.fileType === 'binary' ? fileState.binaryData : undefined}
              onChange={() => {
                // 当前版本十六进制编辑器为只读
                log.warn('十六进制编辑器当前不支持编辑');
              }}
            />
          );
        }
        if (fileState.fileType === 'text') {
          return (
            <TextEditorPane
              ref={paneRef}
              content={fileState.content}
              onChange={updateContent}
              onCursorChange={setCursor}
              onStatsChange={setDocStats}
              filePath={fileState.path}
              wrapText={wrapText}
            />
          );
        }
        if (fileState.fileType === 'binary') {
          // 二进制文件在文本模式下显示提示（正常情况已自动切到 hex）
          return (
            <div className="te-binary-warning">
              <span className="te-binary-warning-text">
                此文件是二进制文件，请在十六进制模式下查看
              </span>
            </div>
          );
        }
        // fileType === 'loading' 兜底
        return (
          <div className="te-loading">
            <div className="te-spinner" aria-hidden="true" />
          </div>
        );
      }

      // 未打开文件（新文件）— 像记事本一样默认可编辑
      return (
        <TextEditorPane
          ref={paneRef}
          content={localContent}
          onChange={updateContent}
          onCursorChange={setCursor}
          onStatsChange={setDocStats}
          wrapText={wrapText}
          placeholder={i18n.t('textEditor.startTyping')}
        />
      );
    };

    return (
      <div
        className={`te-doc${hidden ? ' te-doc-hidden' : ''}`}
        aria-hidden={hidden}
      >
        {renderBody()}
      </div>
    );
  }),
);
