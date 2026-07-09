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
 * │ │ Content（编辑面板）             │ │
 * │ └─────────────────────────────────┘ │
 * │ ┌─────────────────────────────────┐ │
 * │ │ StatusBar（状态栏）             │ │
 * │ └─────────────────────────────────┘ │
 * └─────────────────────────────────────┘
 */

import { useEffect, useState } from 'react';
import { AppLayout } from '../../components/app-shell/AppLayout';
import { useServerManager } from '../../context/ServerManager';
import { useFileManager } from './hooks/useFileManager';
import { TextEditorPane } from './components/TextEditorPane';
import { HexEditorPane } from './components/HexEditorPane';
import { EditorMode } from './components/EditorMode';
import { StatusBar } from './components/StatusBar';
import { SaveDialog } from './components/SaveDialog';
import type { EditorMode as EditorModeType } from './types/editor';
import './TextEditor.css';

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
 * - 文本编辑和十六进制查看
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
  // 获取服务器管理器（用于获取活跃服务器 ID）
  const { activeServerId } = useServerManager();

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

  // 保存对话框状态
  const [showSaveDialog, setShowSaveDialog] = useState(false);

  // 注意：handleOpenFile 函数已移除，因为当前版本未使用
  // 未来版本可以添加打开文件的功能（例如通过菜单或快捷键）

  /**
   * 处理保存文件
   *
   * 如果是新文件（fileState == null），弹出对话框让用户输入路径
   * 如果是已打开文件（fileState != null），直接保存
   */
  const handleSave = async () => {
    // 检查是否有活跃服务器
    if (!activeServerId) {
      console.error('[TextEditor] 未连接到服务器，无法保存文件');
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
  };

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
   * 处理编辑模式切换
   *
   * @param mode - 新的编辑模式
   */
  const handleModeChange = (mode: EditorModeType) => {
    setEditorMode(mode);
  };

  /**
   * 预加载文件
   *
   * 如果提供了 preloadData，在组件挂载时自动打开文件
   */
  useEffect(() => {
    if (preloadData && preloadData.path && preloadData.serverId) {
      console.log('[TextEditor] 预加载文件:', preloadData.path);
      openFile(preloadData.serverId, preloadData.path).catch((err) => {
        console.error('[TextEditor] 预加载文件失败:', err);
      });
    }
  }, [preloadData, openFile]);

  /**
   * 窗口关闭时释放资源
   *
   * 清理文件状态，释放内存
   */
  useEffect(() => {
    return () => {
      console.log('[TextEditor] 窗口关闭，清理资源:', windowId);
      closeFile();
    };
  }, [windowId, closeFile]);

  /**
   * 渲染工具栏
   *
   * 只包括保存按钮和编辑模式切换，文件信息由状态栏展示
   */
  const renderToolbar = () => {
    // 判断是否可以保存
    // 新文件（fileState == null）：只要有内容就可以保存
    // 已打开文件（fileState != null）：文本文件且有未保存变更时可以保存
    const canSave = !isSaving && (
      // 新文件：有内容就可以保存
      (!fileState && localContent.length > 0) ||
      // 已打开文件：文本文件且有未保存变更
      (fileState && fileState.fileType === 'text' && hasUnsavedChanges())
    );

    return (
      <div className="te-toolbar">
        {/* 保存按钮 */}
        <button
          className="te-toolbar-btn te-save-btn"
          onClick={handleSave}
          disabled={!canSave}
          title="保存文件 (Ctrl+S)"
        >
          {isSaving ? '保存中...' : '保存'}
        </button>

        {/* 编辑模式切换 */}
        <EditorMode mode={editorMode} onChange={handleModeChange} />
      </div>
    );
  };

  /**
   * 渲染内容区域
   *
   * 根据状态渲染不同的内容：
   * - 加载中：显示加载提示
   * - 已打开文件：根据编辑模式渲染编辑面板
   * - 未打开文件：显示空状态提示
   */
  const renderContent = () => {
    // 错误提示
    if (error) {
      return (
        <div className="te-error">
          <div className="te-error-content">
            <span className="te-error-icon">⚠️</span>
            <span className="te-error-text">{error}</span>
            <button className="te-error-close" onClick={clearError}>
              ✕
            </button>
          </div>
        </div>
      );
    }

    // 加载中
    if (isLoading) {
      return (
        <div className="te-loading">
          <span className="te-loading-text">正在加载文件...</span>
        </div>
      );
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
              console.warn('[TextEditor] 十六进制编辑器当前不支持编辑');
            }}
          />
        );
      } else {
        // 文本模式
        if (fileState.fileType === 'text') {
          return (
            <TextEditorPane
              content={fileState.content}
              onChange={handleContentChange}
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
          return (
            <div className="te-loading">
              <span className="te-loading-text">正在加载文件...</span>
            </div>
          );
        }
      }
    }

    // 未打开文件（新文件） - 像记事本一样默认可编辑
    return (
      <TextEditorPane
        content={localContent}
        onChange={handleContentChange}
      />
    );
  };

  /**
   * 渲染状态栏
   *
   * 显示文件的行数、字符数、编码、保存状态等
   */
  const renderStatusBar = () => {
    return (
      <StatusBar
        fileState={fileState}
        hasUnsavedChanges={hasUnsavedChanges()}
      />
    );
  };

  return (
    <div className="te-app">
      <div className="te-app-content">
        <AppLayout toolbar={renderToolbar()}>
          {renderContent()}
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