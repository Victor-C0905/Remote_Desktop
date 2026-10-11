/**
 * 文本编辑器主组件
 *
 * 多页签文档窗口（Chrome 式）：
 * - 一个窗口承载多个文档页签，每个页签一个 EditorDocument 实例
 *   （内容、脏标记、撤销历史互相独立，非活动页签保持挂载）
 * - [+] 在窗口内原地新增页签（不再打开新窗口）
 * - 菜单栏与页签同行（菜单在左、页签居右），低频操作收进菜单
 * - 页签 × 关闭：脏页签先确认，最后一个页签关闭时关闭窗口
 *
 * 整体架构：
 * ┌──────────────────────────────────────────┐
 * │ [文件 编辑 视图] ┊ [📄 a.ts ● ×][📄 b] [+] │ ← 单行标签条
 * │ [server › path › to › file.ts]            │ ← 面包屑（仅已打开文件）
 * │ 错误横幅（内联，仅出错时）                  │
 * │ 活动文档（CodeMirror / Hex）               │
 * │ StatusBar（纯信息）                        │
 * └──────────────────────────────────────────┘
 */

import { Fragment, useCallback, useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { createLogger } from '../../utils/logger';
import { AppLayout } from '../../components/app-shell/AppLayout';
import { MenuBar } from '../../components/ToolbarMenu/ToolbarMenu';
import { ConfirmDialog } from '../../components/Dialog/ConfirmDialog';
import { useServerManager } from '../../context/ServerManager';
import { useWindowManager } from '../../window-system/WindowManagerContext';
import { EditorDocument } from './components/EditorDocument';
import type { DocStatus, EditorDocumentHandle } from './components/EditorDocument';
import { StatusBar } from './components/StatusBar';
import { SaveDialog } from './components/SaveDialog';
import { getFileIconForName } from '../fileIcons';
import './TextEditor.css';

const log = createLogger('TextEditor');

/** 页签元数据（文档状态由 EditorDocument 异步上报） */
interface EditorTab {
  id: string;

  /** 初始打开的远程文件（可选；不传则为未命名新文件） */
  initialPath?: string;
  initialServerId?: string;
}

let tabSeq = 0;
function nextTabId(): string {
  tabSeq += 1;
  return `tab-${tabSeq}`;
}

function pathTail(path: string): string {
  return path.split('/').pop() || path;
}

/**
 * TextEditor 组件 Props
 */
interface TextEditorProps {
  /** 窗口 ID（用于窗口管理） */
  windowId: string;

  /** 预加载的数据（可选；作为首个页签打开） */
  preloadData?: {
    /** 文件路径 */
    path: string;
    /** 服务器 ID */
    serverId: string;
  };
}

/**
 * 文本编辑器主组件（多页签文档窗口）
 *
 * @param props - 组件属性
 *
 * @example
 * ```tsx
 * // 空窗口（一个未命名页签）
 * <TextEditor windowId="editor-1" />
 *
 * // 以预加载文件作为首个页签
 * <TextEditor
 *   windowId="editor-2"
 *   preloadData={{ path: '/home/user/file.txt', serverId: 'server-1' }}
 * />
 * ```
 */
export function TextEditor({ windowId, preloadData }: TextEditorProps) {
  const { activeServerId, activeServer } = useServerManager();
  const { manager } = useWindowManager();

  // 页签与活动页签（首渲染时按 preloadData 初始化一次）
  const initialTabs = useRef<EditorTab[] | null>(null);
  if (initialTabs.current === null) {
    initialTabs.current = preloadData?.path
      ? [{ id: nextTabId(), initialPath: preloadData.path, initialServerId: preloadData.serverId }]
      : [{ id: nextTabId() }];
    log.info('窗口初始化页签:', initialTabs.current);
  }
  const [tabs, setTabs] = useState<EditorTab[]>(initialTabs.current);
  const [activeTabId, setActiveTabId] = useState<string>(initialTabs.current[0].id);

  // 各页签文档状态（EditorDocument 上报）
  const [tabStatuses, setTabStatuses] = useState<Record<string, DocStatus>>({});

  // 自动换行（窗口级设置）
  const [wrapText, setWrapText] = useState(false);

  // 另存为对话框（绑定活动页签）
  const [showSaveDialog, setShowSaveDialog] = useState(false);

  // 待确认关闭的脏页签
  const [confirmCloseTabId, setConfirmCloseTabId] = useState<string | null>(null);

  // 各页签文档句柄（菜单栏操作入口）
  const docRefs = useRef<Map<string, EditorDocumentHandle>>(new Map());

  // 标签条挂载点：窗口标题栏槽位（Chrome 形态——标题栏即标签条）。
  // WindowShell 的 HeaderBar 先于应用内容渲染，挂载时槽位必已存在；
  // 找不到时（异常形态）回退到应用内工具栏。
  const [titlebarSlot, setTitlebarSlot] = useState<HTMLElement | null>(null);
  useEffect(() => {
    setTitlebarSlot(document.querySelector<HTMLElement>(`[data-titlebar-slot="${windowId}"]`));
  }, [windowId]);

  const activeTab = tabs.find((t) => t.id === activeTabId) ?? tabs[0];
  const activeStatus: DocStatus | null = tabStatuses[activeTab.id] ?? null;

  /** 文档状态上报（引用稳定，避免 EditorDocument memo 失效） */
  const handleStatus = useCallback((tabId: string, status: DocStatus) => {
    setTabStatuses((prev) => ({ ...prev, [tabId]: status }));
  }, []);

  /** 原地新增页签（未命名新文件） */
  const addTab = useCallback(() => {
    const id = nextTabId();
    setTabs((prev) => [...prev, { id }]);
    setActiveTabId(id);
  }, []);

  /** 移除页签；最后一个页签关闭时关闭窗口 */
  const removeTab = useCallback((tabId: string) => {
    setTabs((prev) => {
      const idx = prev.findIndex((t) => t.id === tabId);
      if (idx === -1) return prev;
      const next = prev.filter((t) => t.id !== tabId);
      if (next.length === 0) {
        manager.close(windowId);
        return prev;
      }
      // 若关闭的是活动页签，激活相邻页签（优先左侧）
      setActiveTabId((cur) => {
        if (cur !== tabId) return cur;
        return next[Math.max(0, idx - 1)].id;
      });
      return next;
    });
  }, [manager, windowId]);

  /** 页签 ×：脏页签先确认，干净页签直接关 */
  const closeTab = useCallback((tabId: string) => {
    if (tabStatuses[tabId]?.dirty) {
      setConfirmCloseTabId(tabId);
      return;
    }
    removeTab(tabId);
  }, [tabStatuses, removeTab]);

  /**
   * Ctrl+S 快捷键保存（保存活动页签）
   */
  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key === 's') {
        event.preventDefault();
        if (activeServerId) {
          if (activeStatus?.path) {
            docRefs.current.get(activeTab.id)?.save(activeServerId);
          } else {
            // 未命名文件：走另存为流程
            setShowSaveDialog(true);
          }
        }
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [activeServerId, activeStatus, activeTab, handleStatus]);

  /** 阻止双击冒泡到标题栏（避免在页签上双击触发最大化） */
  const stopDoubleClick = (e: React.MouseEvent) => e.stopPropagation();

  // ── 渲染：标题栏页签条（仅页签 + 新建；空白区域可拖拽窗口）──

  const renderTabs = () => {
    return (
      <div className="te-tabstrip">
        {/* 文档页签（Chrome 式：活动页签底部与内容区融合） */}
        {tabs.map((t) => {
          const st = tabStatuses[t.id];
          const label = st?.label ?? (t.initialPath ? pathTail(t.initialPath) : '未命名文件');
          const isActive = t.id === activeTab.id;
          return (
            <div
              key={t.id}
              className={`te-tab${isActive ? ' active' : ''}`}
              onClick={() => setActiveTabId(t.id)}
              onDoubleClick={stopDoubleClick}
              title={st?.path ?? label}
            >
              <span className="te-tab-icon">
                {st?.path ? getFileIconForName(label) : t.initialPath ? getFileIconForName(label) : '📝'}
              </span>
              <span className="te-tab-name">{label}</span>
              {st?.dirty && <span className="te-tab-dot" title="未保存" />}
              <span
                className="te-tab-close"
                title="关闭页签"
                onClick={(e) => {
                  e.stopPropagation();
                  closeTab(t.id);
                }}
              >
                ×
              </span>
            </div>
          );
        })}

        {/* 原地新增页签 */}
        <button
          type="button"
          className="te-tab-new"
          onClick={addTab}
          title="新建页签"
        >
          +
        </button>
      </div>
    );
  };

  // ── 渲染：应用内顶栏（菜单栏 + 路径面包屑，单行） ─────────
  // 菜单栏不进标题栏（与页签分层），与路径上下文同行节省纵向空间

  const renderTopbar = () => {
    // 保存可用性：有变更、非保存中；未打开文件需有内容（dirty 已覆盖）
    const canSave = !!activeStatus
      && !activeStatus.isSaving
      && activeStatus.dirty
      && (activeStatus.path ? activeStatus.fileType === 'text' : true);

    return (
      <div className="te-topbar">
        <div className="te-menubar">
          <MenuBar
            menus={[
              {
                label: '文件',
                items: [
                  {
                    label: '保存',
                    shortcut: 'Ctrl+S',
                    disabled: !canSave,
                    onClick: () => {
                      if (activeServerId && activeStatus?.path) {
                        docRefs.current.get(activeTab.id)?.save(activeServerId);
                      } else {
                        // 未命名文件：走另存为流程
                        setShowSaveDialog(true);
                      }
                    },
                  },
                  {
                    label: '另存为…',
                    disabled: !activeStatus?.path || activeStatus.isSaving,
                    onClick: () => setShowSaveDialog(true),
                  },
                  { separator: true },
                  { label: '关闭窗口', onClick: () => manager.close(windowId) },
                ],
              },
              {
                label: '编辑',
                items: [
                  { label: '撤销', shortcut: 'Ctrl+Z', onClick: () => docRefs.current.get(activeTab.id)?.undo() },
                  { label: '重做', shortcut: 'Ctrl+Y', onClick: () => docRefs.current.get(activeTab.id)?.redo() },
                  { separator: true },
                  { label: '查找 / 替换', shortcut: 'Ctrl+F', onClick: () => docRefs.current.get(activeTab.id)?.openSearch() },
                ],
              },
              {
                label: '视图',
                items: [
                  {
                    label: '十六进制模式',
                    checked: activeStatus?.hexMode ?? false,
                    keepOpen: true,
                    onClick: () => docRefs.current.get(activeTab.id)?.toggleHexMode(),
                  },
                  {
                    label: '自动换行',
                    checked: wrapText,
                    keepOpen: true,
                    onClick: () => setWrapText((w) => !w),
                  },
                ],
              },
            ]}
          />
        </div>

        {/* 菜单栏与路径的分隔竖线 */}
        <div className="te-strip-divider" />

        {/* 路径面包屑（仅已打开文件时显示分段） */}
        {activeStatus?.path && (
          <div className="te-breadcrumb" title={activeStatus.path}>
            {activeServer && (
              <>
                <span className="te-crumb te-crumb-server">{activeServer.name}</span>
                <span className="te-crumb-sep">›</span>
              </>
            )}
            {activeStatus.path.split('/').filter(Boolean).map((seg, i, arr) => (
              <Fragment key={i}>
                {i > 0 && <span className="te-crumb-sep">›</span>}
                <span
                  className={`te-crumb${i === arr.length - 1 ? ' te-crumb-current' : ''}`}
                >
                  {seg}
                </span>
              </Fragment>
            ))}
          </div>
        )}
      </div>
    );
  };

  // ── 渲染：内联错误横幅（活动页签出错时） ──────────────────

  const renderErrorBanner = () => {
    if (!activeStatus?.error) return null;
    return (
      <div className="te-error-banner" role="alert">
        <span className="te-error-banner-icon">⚠️</span>
        <span className="te-error-banner-text">{activeStatus.error}</span>
        <button
          className="te-error-banner-close"
          onClick={() => docRefs.current.get(activeTab.id)?.clearError()}
          title="关闭"
        >
          ✕
        </button>
      </div>
    );
  };

  // ── 待关闭脏页签确认框 ────────────────────────────────────

  const renderCloseConfirm = () => {
    if (!confirmCloseTabId) return null;
    const st = tabStatuses[confirmCloseTabId];
    return (
      <ConfirmDialog
        title="关闭页签"
        message="确定关闭该页签？"
        highlight={st?.label}
        detail="该文档有未保存的更改，关闭后将丢失。"
        confirmLabel="放弃更改并关闭"
        danger
        onConfirm={() => {
          const id = confirmCloseTabId;
          setConfirmCloseTabId(null);
          removeTab(id);
        }}
        onCancel={() => setConfirmCloseTabId(null)}
      />
    );
  };

  return (
    <div className="te-app">
      {/* 页签 portal 进窗口标题栏（Chrome 形态）；槽位缺失时回退应用内 */}
      {titlebarSlot
        ? createPortal(renderTabs(), titlebarSlot)
        : renderTabs()}
      <div className="te-app-content">
        <AppLayout
          toolbar={renderTopbar()}
        >
          <div className="te-content-wrapper">
            {renderErrorBanner()}
            {/* 文档层：所有页签保持挂载，仅活动页签可见 */}
            <div className="te-docs">
              {tabs.map((t) => (
                <EditorDocument
                  key={t.id}
                  ref={(handle) => {
                    if (handle) {
                      docRefs.current.set(t.id, handle);
                    } else {
                      docRefs.current.delete(t.id);
                    }
                  }}
                  tabId={t.id}
                  initialPath={t.initialPath}
                  initialServerId={t.initialServerId}
                  hidden={t.id !== activeTab.id}
                  wrapText={wrapText}
                  onStatus={handleStatus}
                />
              ))}
            </div>
          </div>
        </AppLayout>
      </div>

      <StatusBar status={activeStatus} serverName={activeServer?.name ?? null} />

      {/* 另存为对话框（绑定活动页签） */}
      <SaveDialog
        isOpen={showSaveDialog}
        defaultPath="/home/user/untitled.txt"
        onSave={async (path) => {
          if (activeServerId) {
            await docRefs.current.get(activeTab.id)?.saveAs(activeServerId, path);
          }
          setShowSaveDialog(false);
        }}
        onCancel={() => setShowSaveDialog(false)}
      />

      {/* 脏页签关闭确认 */}
      {renderCloseConfirm()}
    </div>
  );
}
