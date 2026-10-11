/**
 * 文本编辑器主组件
 *
 * 这是文件编辑器的核心组件，负责：
 * - 整合所有子组件（编辑面板、状态栏、模式切换等）
 * - 管理文件状态（通过 useFileManager Hook）
 * - 管理编辑模式（text/hex）
 * - 管理未保存变更状态
 * - 预加载文件（通过 preloadData）
 * - 窗口关闭时释放资源
 *
 * 整体架构：
 * ┌─────────────────────────────────────┐
 * │ AppLayout                            │
 * │ ┌─────────────────────────────────┐ │
 * │ │ Toolbar（文件名 + 保存 + 模式） │ │
 * │ └─────────────────────────────────┘ │
 * │ ┌─────────────────────────────────┐ │
 * │ │ 错误横幅（内联，仅出错时）       │ │
 * │ ├─────────────────────────────────┤ │
 * │ │ Content（CodeMirror 编辑面板）  │ │
 * │ └─────────────────────────────────┘ │
 * │ ┌─────────────────────────────────┐ │
 * │ │ StatusBar（状态栏）             │ │
 * │ └─────────────────────────────────┘ │
 * └─────────────────────────────────────┘
 */

import { useEffect, useState, useRef, useCallback } from 'react';
import { createLogger } from '../../utils/logger';
import { AppLayout } from '../../components/app-shell/AppLayout';
import { ToolbarMenu } from '../../components/ToolbarMenu/ToolbarMenu';
import type { ToolbarMenuItem, ToolbarMenuSeparator } from '../../components/ToolbarMenu/ToolbarMenu';
import { useServerManager } from '../../context/ServerManager';
import { useWindowManager } from '../../window-system/WindowManagerContext';
import { useFileManager } from './hooks/useFileManager';
import { TextEditorPane } from './components/TextEditorPane';
import type { CursorPosition, DocStats, TextEditorPaneHandle } from './components/TextEditorPane';
import { HexEditorPane } from './components/HexEditorPane';
import { StatusBar } from './components/StatusBar';
import { SaveDialog } from './components/SaveDialog';
import { getFileIconForName } from '../fileIcons';
import type { EditorMode as EditorModeType } from './types/editor';
import './TextEditor.css';

const log = createLogger('TextEditor');

/**
 * TextEditor 组件 Props
 */
interface TextEditorProps {
  /** 窗口 ID（用于窗口管理） */
  windowId: string;

  /** 预加载的数据（可选） */
  preloadData?: {
    /** 文件路径 */
    path: string;
    /** 服务器 ID */
    serverId: string;
  };
}

/**
 * 文本编辑器主组件
 *
 * 提供完整的文件编辑功能，包括：
 * - 文件打开、保存、关闭
 * - 文本编辑（CodeMirror 6：行号/语法高亮/查找替换）和十六进制查看
 * - 状态显示（行数、字符数、保存状态等）
 * - 预加载文件支持
 *
 * @param props - 组件属性
 * @param props.windowId - 窗口 ID
 * @param props.preloadData - 预加载的数据（可选）
 *
 * @example
 * ```tsx
 * // 基本用法
 * <TextEditor windowId="editor-1" />
 *
 * // 预加载文件
 * <TextEditor
 *   windowId="editor-2"
 *   preloadData={{ path: '/home/user/file.txt', serverId: 'server-1' }}
 * />
 * ```
 */
export function TextEditor({ windowId, preloadData }: TextEditorProps) {
  // 获取服务器管理器（用于获取活跃服务器信息：保存目标 + 面包屑前缀显示）
  const { activeServerId, activeServer } = useServerManager();

  // 窗口管理器（标签栏：× 关闭本窗口，+ 新建编辑器窗口）
  const { manager } = useWindowManager();

  // 文件管理 Hook（管理文件状态和操作）
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

  // 编辑模式状态（text/hex）
  const [editorMode, setEditorMode] = useState<EditorModeType>('text');

  // 光标位置状态（行号、列号）
  const [cursorPosition, setCursorPosition] = useState<CursorPosition>({ line: 1, column: 1 });

  // 文档统计（行数/字符数/选中数，由编辑面板上报）
  const [docStats, setDocStats] = useState<DocStats | null>(null);

  // 自动换行开关
  const [wrapText, setWrapText] = useState(false);

  // 保存对话框状态
  const [showSaveDialog, setShowSaveDialog] = useState(false);

  // 防止重复打开同一个文件（关键修复）
  const lastOpenedPath = useRef<string | null>(null);

  // 二进制文件自动切换 hex 的标记（只在加载完成时执行一次，不劫持用户手动切换）
  const autoHexSwitched = useRef(false);

  // 编辑面板命令句柄（撤销/重做/查找按钮直连 CodeMirror 命令）
  const paneRef = useRef<TextEditorPaneHandle>(null);

  /**
   * 预加载文件（仅执行一次）
   *
   * 当窗口创建时，如果提供了 preloadData，自动打开文件
   * 使用 ref 防止重复打开同一个文件
   */
  useEffect(() => {
    if (preloadData && preloadData.path && preloadData.serverId) {
      // 检查是否已经打开过这个文件
      if (lastOpenedPath.current === preloadData.path) {
        log.debug('文件已打开，跳过重复打开:', preloadData.path);
        return;
      }

      log.info('预加载文件:', preloadData.path);
      lastOpenedPath.current = preloadData.path; // 记录已打开的文件

      openFile(preloadData.serverId, preloadData.path).catch((err) => {
        log.error('预加载文件失败:', err);
        lastOpenedPath.current = null; // 失败时重置，允许重试
      });
    }
  }, [preloadData, openFile]);

  /**
   * 二进制文件自动切换到十六进制模式
   *
   * 仅在首次检测到二进制文件时切换一次；用户随后手动切回文本模式不被劫持
   */
  useEffect(() => {
    if (fileState?.fileType === 'binary' && !autoHexSwitched.current) {
      autoHexSwitched.current = true;
      setEditorMode('hex');
    }
  }, [fileState]);

  /**
   * 处理保存文件
   *
   * 如果是新文件（fileState == null），弹出对话框让用户输入路径
   * 如果是已打开文件（fileState != null），直接保存
   * 使用 useCallback 包装，避免不必要的重新创建
   */
  const handleSave = useCallback(async () => {
    // 检查是否有活跃服务器
    if (!activeServerId) {
      log.error('未连接到服务器，无法保存文件');
      return;
    }

    // 如果是新文件（未打开文件）
    if (!fileState) {
      // 打开保存对话框
      setShowSaveDialog(true);
      return;
    }

    // 如果是已打开文件，直接保存
    await saveFile(activeServerId);
  }, [activeServerId, fileState, saveFile]); // 依赖项

  /**
   * 处理另存为
   *
   * @param path - 保存路径
   */
  const handleSaveAs = async (path: string) => {
    if (!activeServerId) return;

    // 保存到指定路径
    await saveAs(activeServerId, path);

    // 关闭对话框
    setShowSaveDialog(false);
  };

  /**
   * 处理取消保存
   */
  const handleCancelSave = () => {
    setShowSaveDialog(false);
  };

  /**
   * 处理内容变更
   *
   * @param newContent - 新的文件内容
   */
  const handleContentChange = (newContent: string) => {
    // 更新本地文件内容
    updateContent(newContent);
  };

  /**
   * 窗口关闭时释放资源
   *
   * 清理文件状态，释放内存
   */
  useEffect(() => {
    return () => {
      log.info('窗口关闭，清理资源:', windowId);
      closeFile();
    };
  }, [windowId, closeFile]);

  /**
   * Ctrl+S 快捷键保存
   *
   * 监听键盘事件，当按下 Ctrl+S 时触发保存操作
   */
  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      // 检查是否按下 Ctrl+S（或 Cmd+S 在 Mac 上）
      if ((event.ctrlKey || event.metaKey) && event.key === 's') {
        event.preventDefault(); // 阻止浏览器默认的保存行为
        handleSave();
      }
    };

    // 添加键盘事件监听
    window.addEventListener('keydown', handleKeyDown);

    // 清理监听器
    return () => {
      window.removeEventListener('keydown', handleKeyDown);
    };
  }, [handleSave]); // 依赖 handleSave 函数

  /**
   * 当前文件显示名（标签栏/标题）
   */
  const fileName = fileState ? fileState.path.split('/').pop() || fileState.path : '未命名文件';

  /**
   * 渲染动作栏（第一行）
   *
   * 对齐 VS Code 极简动作栏：高频编辑动作，仅此而已。
   * - 左组：撤销/重做（编辑核心动作，GNOME Text Editor 同款位置）
   * - 右组：保存（accent 主操作）、查找面板、更多操作菜单
   *   低频操作（另存为/十六进制/自动换行）全部收进 ⋮ 菜单
   */
  const renderActionBar = () => {
    // 判断是否可以保存
    // 新文件（fileState == null）：只要有内容就可以保存
    // 已打开文件（fileState != null）：文本文件且有未保存变更时可以保存
    const canSave = !isSaving && (
      // 新文件：有内容就可以保存
      (!fileState && localContent.length > 0) ||
      // 已打开文件：文本文件且有未保存变更
      (fileState && fileState.fileType === 'text' && hasUnsavedChanges())
    );

    const menuItems: Array<ToolbarMenuItem | ToolbarMenuSeparator> = [
      {
        label: '另存为…',
        disabled: !fileState || isSaving,
        onClick: () => setShowSaveDialog(true),
      },
      { separator: true },
      {
        label: '十六进制模式',
        checked: editorMode === 'hex',
        keepOpen: true,
        onClick: () => setEditorMode((m) => (m === 'hex' ? 'text' : 'hex')),
      },
      {
        label: '自动换行',
        checked: wrapText,
        keepOpen: true,
        onClick: () => setWrapText((w) => !w),
      },
    ];

    return (
      <div className="te-actionbar">
        {/* 左组：编辑历史 */}
        <div className="te-actionbar-left">
          <button
            type="button"
            className="te-icon-btn"
            onClick={() => paneRef.current?.undo()}
            title="撤销 (Ctrl+Z)"
          >
            ↩️
          </button>
          <button
            type="button"
            className="te-icon-btn"
            onClick={() => paneRef.current?.redo()}
            title="重做 (Ctrl+Y)"
          >
            ↪️
          </button>
        </div>

        {/* 右组：保存 / 查找 / 更多操作 */}
        <div className="te-actionbar-right">
          <button
            type="button"
            className="te-icon-btn te-icon-btn-accent"
            onClick={handleSave}
            disabled={!canSave}
            title={isSaving ? '保存中...' : '保存文件 (Ctrl+S)'}
          >
            💾
          </button>
          <button
            type="button"
            className="te-icon-btn"
            onClick={() => paneRef.current?.openSearch()}
            title="查找 / 替换 (Ctrl+F)"
          >
            🔍
          </button>
          <ToolbarMenu title="更多操作" items={menuItems} />
        </div>
      </div>
    );
  };

  /**
   * 渲染标签栏（第二行）
   *
   * 对齐现代编辑器的文档标签：文件图标 + 文件名 + 未保存圆点 + 关闭；
   * 右侧 [+] 新建编辑器窗口。单标签即当前窗口文档，为多标签迭代预留结构。
   */
  const renderTabBar = () => {
    return (
      <div className="te-tabbar">
        <div
          className="te-tab active"
          title={fileState ? fileState.path : '未命名文件'}
        >
          <span className="te-tab-icon">{fileState ? getFileIconForName(fileName) : '📝'}</span>
          <span className="te-tab-name">{fileName}</span>
          {hasUnsavedChanges() && <span className="te-tab-dot" title="未保存" />}
          <span
            className="te-tab-close"
            title="关闭窗口"
            onClick={(e) => {
              e.stopPropagation();
              manager.close(windowId);
            }}
          >
            ×
          </span>
        </div>
        <button
          type="button"
          className="te-tab-new"
          onClick={() => manager.create('editor', {})}
          title="新建编辑器窗口"
        >
          +
        </button>
      </div>
    );
  };

  /**
   * 渲染面包屑（第三行，仅已打开文件时）
   *
   * 对齐 VS Code 面包屑：服务器名 › 逐级目录 › 文件名；
   * 过长时左侧收缩省略（direction: rtl 技巧，与 FileManager 拖放层一致），
   * 悬浮显示完整路径。
   */
  const renderBreadcrumb = () => {
    if (!fileState) return null;
    const segments = fileState.path.split('/').filter(Boolean);
    const text = `${activeServer ? `${activeServer.name} › ` : ''}${segments.join(' › ')}`;
    return (
      <div className="te-breadcrumb" title={fileState.path}>
        {text}
      </div>
    );
  };

  /**
   * 渲染内联错误横幅
   *
   * 错误不再替换整个内容区（对齐大型编辑器惯例）：
   * 横幅出现在编辑区上方，文件内容/编辑能力保持可用
   */
  const renderErrorBanner = () => {
    if (!error) return null;
    return (
      <div className="te-error-banner" role="alert">
        <span className="te-error-banner-icon">⚠️</span>
        <span className="te-error-banner-text">{error}</span>
        <button className="te-error-banner-close" onClick={clearError} title="关闭">
          ✕
        </button>
      </div>
    );
  };

  /**
   * 渲染加载态
   *
   * 居中 spinner + 文件名（复用 FileManager 的视觉语言）
   */
  const renderLoading = () => {
    const fileName = fileState?.path.split('/').pop() || preloadData?.path.split('/').pop() || '';
    return (
      <div className="te-loading">
        <div className="te-spinner" aria-hidden="true" />
        <span className="te-loading-text">正在加载{fileName ? ` ${fileName}` : '文件'}...</span>
      </div>
    );
  };

  /**
   * 渲染编辑主体
   *
   * 根据状态渲染不同的内容：
   * - 加载中：居中 spinner
   * - 已打开文件：根据编辑模式渲染编辑面板
   * - 未打开文件：像记事本一样默认可编辑的新文件
   */
  const renderBody = () => {
    // 加载中
    if (isLoading) {
      return renderLoading();
    }

    // 已打开文件
    if (fileState) {
      // 根据编辑模式渲染不同的编辑面板
      if (editorMode === 'hex') {
        // 十六进制模式
        return (
          <HexEditorPane
            data={fileState.fileType === 'binary' ? fileState.binaryData : undefined}
            onChange={() => {
              // 当前版本十六进制编辑器为只读
              log.warn('十六进制编辑器当前不支持编辑');
            }}
          />
        );
      } else {
        // 文本模式
        if (fileState.fileType === 'text') {
          return (
            <TextEditorPane
              ref={paneRef}
              content={fileState.content}
              onChange={handleContentChange}
              onCursorChange={setCursorPosition}
              onStatsChange={setDocStats}
              filePath={fileState.path}
              wrapText={wrapText}
            />
          );
        } else if (fileState.fileType === 'binary') {
          // 二进制文件在文本模式下显示提示
          return (
            <div className="te-binary-warning">
              <span className="te-binary-warning-text">
                此文件是二进制文件，请在十六进制模式下查看
              </span>
            </div>
          );
        } else {
          // 加载中状态（不应该出现在这里，但作为兜底处理）
          return renderLoading();
        }
      }
    }

    // 未打开文件（新文件） - 像记事本一样默认可编辑
    return (
      <TextEditorPane
        ref={paneRef}
        content={localContent}
        onChange={handleContentChange}
        onCursorChange={setCursorPosition}
        onStatsChange={setDocStats}
        wrapText={wrapText}
        placeholder="开始输入…"
      />
    );
  };

  /**
   * 渲染状态栏
   *
   * 纯信息展示：光标位置、行数、字符数、语言、编码、换行符、保存状态、修改时间
   * （操作类入口已按频率归位：高频→动作栏，低频→⋮ 菜单）
   */
  const renderStatusBar = () => {
    return (
      <StatusBar
        fileState={fileState}
        hasUnsavedChanges={hasUnsavedChanges()}
        cursorPosition={cursorPosition}
        newFileCharCount={docStats?.chars ?? localContent.length}
        docStats={docStats}
      />
    );
  };

  return (
    <div className="te-app">
      <div className="te-app-content">
        <AppLayout
          toolbar={
            <>
              {renderActionBar()}
              {renderTabBar()}
              {renderBreadcrumb()}
            </>
          }
        >
          <div className="te-content-wrapper">
            {renderErrorBanner()}
            {renderBody()}
          </div>
        </AppLayout>
      </div>
      {renderStatusBar()}

      {/* 保存对话框 */}
      <SaveDialog
        isOpen={showSaveDialog}
        defaultPath="/home/user/untitled.txt"
        onSave={handleSaveAs}
        onCancel={handleCancelSave}
      />
    </div>
  );
}
