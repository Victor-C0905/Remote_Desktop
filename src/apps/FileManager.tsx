import React, { useState, useEffect, useLayoutEffect, useCallback, useMemo, useRef } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open, save, ask } from "@tauri-apps/plugin-dialog";
import { useServerManager } from "../context/ServerManager";
import i18n from "../i18n";
import { useVirtualWindow } from "./useVirtual";
import { PLACEHOLDER } from "../utils/offlineDefaults";
import { useWindowManager, useWindowState } from "../window-system/WindowManagerContext";
import { AppLayout } from "../components/app-shell";
import { TransferStatusBar } from "../components/TransferStatusBar";
import { createLogger } from '../utils/logger';
import { openRemoteFile } from './openRemoteFile';
import { FOLDER_ICON, getFileIconForName } from './fileIcons';
import { AppIcon, type AppIconName } from '../components/AppIcon';
import { parsePermissions, matrixToOctal, octalToPermString } from './permissions';
import type { PermMatrix } from './permissions';
import { sortFileEntries, DEFAULT_SORT, DEFAULT_COLUMN_WIDTHS, clampColumnWidth, buildGridTemplate, filterByColumnFilters, toggleBucket, DEFAULT_COLUMN_FILTERS, SIZE_BUCKETS, MTIME_BUCKETS, NAME_BUCKETS, typeKeyOf, typeKeyLabel, listFileTypeBuckets, generateUniqueName } from './fileTable';
import type { SortKey, SortState, ColumnWidths, ColumnFilterState, SizeBucket, MtimeBucket, NameBucket } from './fileTable';
import { SymbolicIcon } from "../components/symbolic";
import { ToolbarMenu, type MenuEntry } from "../components/ToolbarMenu/ToolbarMenu";
import { ContextMenu, type ContextMenuEntry } from "../components/ContextMenu/ContextMenu";
import type { FileEntry } from './fileTypes';
import { rangeSelection, toggleSelection, entryName } from './fileSelection';
import { useFileTransfer } from './useFileTransfer';
import { applyBrowseChanges, applySearchChanges, isTransferTempName, type FileViewChange } from './fileMutations';
import { useFileChangesSubscription } from '../hooks/useSubscription';
import { useMarquee, type EntryBox } from './useMarquee';
import { ConfirmDialog } from "../components/Dialog/ConfirmDialog";
import { Dialog } from "../components/Dialog/Dialog";
import { useTranslation } from "react-i18next";
import { pushServerNotification } from '../utils/notify';
import { useSettingsStore } from '../stores/settingsStore';
import { formatUnknownError, isPermissionDenied } from '../types/errors';
import "./FileManager.css";

const log = createLogger('FileManager');

/** 搜索下钻层数档位（0 = 仅当前目录一层，不在选项内单列；Rust 侧 clamp_search_depth 在 8 截断） */
const SEARCH_DEPTH_OPTIONS = [1, 2, 3, 5, 8];

/** 列表行间距（px）：必须与 .fm-list-content 的 row-gap 一致。
 *  虚拟窗口按「行高 + 间距」计算占位高度，漏算会让滚动高度累计变短，
 *  目录尾部的文件永远滚不到（审计 M25 回归） */
const LIST_ROW_GAP = 2;

interface ReadDirResponse {
  path: string;
  entries: FileEntry[];
}

/** remote_search 响应（Rust RemoteSearchResponse：FileEntry 展平 + path） */
interface SearchResponse {
  root: string;
  results: FileEntry[];
  truncated: boolean;
}

// 挂载点信息
interface MountInfo {
  mount_point: string;
  device: string;
  filesystem: string;
  total_bytes: number;
  used_bytes: number;
}

// 侧边栏项（labelKey: 预置书签词典 key，渲染处 t()；label: 动态标签如挂载点）
interface SidebarItem {
  icon: import('../components/AppIcon').AppIconName;
  labelKey?: string;
  label?: string;
  path: string;
  type: "bookmark" | "mount" | "network";
}

// 侧边栏部分（titleKey: 词典 key，渲染处 t()）
interface SidebarSection {
  titleKey?: string;
  title?: string;
  items: SidebarItem[];
}

/* ── Icon Map（已提取至 ./fileIcons，与 TextEditor 共用） ── */

function getFileIcon(entry: FileEntry): AppIconName {
  if (entry.is_dir) return FOLDER_ICON;
  return getFileIconForName(entry.name);
}

function formatSize(bytes: number): string {
  if (bytes === 0) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  return (bytes / Math.pow(1024, i)).toFixed(i === 0 ? 0 : 1) + " " + units[i];
}

function formatDate(iso: string): string {
  if (!iso) return "—";
  const d = new Date(iso);
  // locale 随当前语言（硬编码 zh-CN 对英文用户产出中文格式，审计 L8）
  return d.toLocaleDateString(i18n.language, {
    year: "numeric", month: "2-digit", day: "2-digit",
    hour: "2-digit", minute: "2-digit",
  });
}

/* ── 权限转换：已抽取至 ./permissions.ts（纯函数 + 单测覆盖） ── */

/* ── Component ─────────────────────────────────────────── */

type ViewMode = "list" | "grid";

/** 列表视图表头列定义（key 同时用作排序键与列宽键；perm 为最后一列不提供拖拽手柄） */
const FILE_COLUMNS: { key: SortKey; labelKey: string }[] = [
  { key: "name", labelKey: "fileManager.colName" },
  { key: "size", labelKey: "fileManager.colSize" },
  { key: "mtime", labelKey: "fileManager.colMtime" },
  { key: "type", labelKey: "fileManager.colType" },
  { key: "perm", labelKey: "fileManager.colPerm" },
];

/** FileManager 应用组件
 *  集成窗口系统：
 *  - 使用 useWindowState(windowId) 获取窗口状态
 *  - 从窗口状态的 preloadData 获取初始数据
 *  - 单实例应用（allowMultipleInstances: false）
 */
interface FileManagerProps {
  windowId: string;
  preloadData?: any;  // 由 Desktop 通过窗口状态注入
}

export function FileManager({ windowId, preloadData }: FileManagerProps) {
  const { t } = useTranslation();
  // ── 窗口系统集成 ─────────────────────────────────────
  // 获取窗口管理器
  const { manager } = useWindowManager();

  // 窗口状态：拖拽上传仅在活动（非最小化）窗口响应——
  // 最小化窗口 display:none 但组件仍挂载，不加以限定会导致拖放误触发隐藏窗口的上传
  const windowState = useWindowState(windowId);
  const isWindowActive = !!windowState?.isActive && !windowState?.minimized;

  // 从 props 获取预加载数据
  const initialData = preloadData;

  const { activeServerId, servers, connectServer } = useServerManager();
  // 连接态（目录监听在断线期间不订阅；重连后 connected 变化触发重新订阅）
  const serverConnected = servers.find((s) => s.id === activeServerId)?.status === "connected";

  // ── 离线状态判断 ─────────────────────────────────────
  // 无活跃连接 = 离线模式（显示服务器列表登录界面，而非空白或本地文件）
  // 注：连接失败已通过通知中心 + Settings 错误行呈现，此处不展示错误态
  const isOffline = !activeServerId;

  // 路径系统：始终使用 Linux 远程路径格式（远程 Linux 服务器控制工具）
  const [currentPath, setCurrentPath] = useState(() => "/");
  /** 视图目录的即时引用：增量补丁必须按「此刻」视图目录过滤，
   *  闭包捕获的 currentPath 在逐项 await 期间会过期（审计 M21） */
  const currentPathRef = useRef("/");
  // 目录实时监听是否生效（file_changes 订阅成功）：生效时最终条目由增量推送，
  // 上传完成事件不必再做 remote_file_info（省每文件一次 RPC）
  const fsWatchActiveRef = useRef(false);
  /** 最近一次权限失败的目标目录（供地址栏导航区分错误类型，审计 L12） */
  const permissionDeniedRef = useRef<string | null>(null);
  const [entries, setEntries] = useState<FileEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // 权限不足目录：进入目录但内容不可见（空列表+轻微提示）
  // 存储无权限目录路径，null 表示当前目录可正常访问
  const [permissionDenied, setPermissionDenied] = useState<string | null>(null);
  // ── 多选（路径制：排序/筛选变化后选中随条目而非位置）──────
  // selected：选中完整路径（保点击顺序）；anchorPath：Shift 范围锚点；
  // focusIdx：键盘焦点位（visibleEntries 下标，-1 = 无）
  const [selected, setSelected] = useState<string[]>([]);
  const [anchorPath, setAnchorPath] = useState<string | null>(null);
  const [focusIdx, setFocusIdx] = useState(-1);

  /** 清空选区（目录切换/搜索切换/断连时调用；setState 稳定，无需 memo） */
  const clearSelection = () => {
    setSelected([]);
    setAnchorPath(null);
    setFocusIdx(-1);
  };
  const [username, setUsername] = useState<string | null>(null);
  // 用户真实家目录（root 为 /root，普通用户从 /etc/passwd 读取，不再硬编码 /home/${username}）
  const [homeDir, setHomeDir] = useState<string | null>(null);
  const [viewMode, setViewMode] = useState<ViewMode>("list");

  // ── 文件搜索（地址栏右侧常驻小搜索框，前缀匹配；下钻层数经 ⌄ 菜单可调）──
  // 匹配在 Rust 侧执行（remote_search：depth=0 单次 ReadDir 仅当前目录，
  // depth>0 受限下钻；输入 a 只命中 a***），前端只负责防抖/过期丢弃/展示；
  // 声明在 visibleEntries 之前（其 useMemo 以搜索结果为数据源）。
  // 无独立「搜索模式」开关：查询词生效中即搜索视图（activeSearch），
  // 无词时回落当前目录内容——不隐藏正在浏览的文件
  const [searchInput, setSearchInput] = useState("");       // 输入框即时值
  const [searchQuery, setSearchQuery] = useState("");       // 防抖后的生效查询词
  const [searchResults, setSearchResults] = useState<FileEntry[] | null>(null); // null = 尚无结果
  const [searching, setSearching] = useState(false);
  const [searchTruncated, setSearchTruncated] = useState(false); // Rust 触达上限，结果可能不全
  const [searchError, setSearchError] = useState<string | null>(null);
  const searchEpochRef = useRef(0);   // 丢弃过期响应（连打/换目录）
  const dirEpochRef = useRef(0);      // 丢弃过期的目录列表响应（快速连续导航，与搜索同模式）
  const [searchNonce, setSearchNonce] = useState(0); // 删除/重命名等操作后重跑搜索的触发器
  const searchInputRef = useRef<HTMLInputElement>(null); // Ctrl+F 聚焦入口
  /** 搜索下钻层数（持久化到设置存储；0 = 仅当前目录一层） */
  const fileManagerSearchDepth = useSettingsStore(s => s.fileManagerSearchDepth);
  const setFileManagerSearchDepth = useSettingsStore(s => s.setFileManagerSearchDepth);
  // 搜索条件 ref：applyViewChanges 会被长流程（删除/移动/解压通知）异步调用，
  // 闭包捕获的查询词可能已过期——补丁必须用「此刻」的搜索语义
  const searchQueryRef = useRef(searchQuery);
  const searchDepthRef = useRef(fileManagerSearchDepth);
  useEffect(() => { searchQueryRef.current = searchQuery; }, [searchQuery]);
  useEffect(() => { searchDepthRef.current = fileManagerSearchDepth; }, [fileManagerSearchDepth]);
  /** 搜索视图判定：查询词生效中（替代旧 searchMode 布尔，对齐 Windows 行为） */
  const activeSearch = searchQuery.length > 0;
  // 搜索范围档位（⌄ 菜单）：0 = 仅当前目录；触发按钮 title 与输入框 placeholder
  // 反映当前档位，占位文本零布局成本地告知范围
  const searchScopeLabel = fileManagerSearchDepth > 0
    ? t("fileManager.searchScopeDepth", { n: fileManagerSearchDepth })
    : t("fileManager.searchScopeCurrent");
  const searchScopeItems: MenuEntry[] = useMemo(() => [
    {
      label: t("fileManager.searchScopeCurrent"),
      checked: fileManagerSearchDepth === 0,
      onClick: () => setFileManagerSearchDepth(0),
    },
    ...SEARCH_DEPTH_OPTIONS.map((n) => ({
      label: t("fileManager.searchScopeDepth", { n }),
      checked: fileManagerSearchDepth === n,
      onClick: () => setFileManagerSearchDepth(n),
    })),
  ], [t, fileManagerSearchDepth, setFileManagerSearchDepth]);

  // ── 列筛选（表头下拉菜单，Win11 Explorer 风格）────────────
  // 会话内状态不持久化：每次打开默认无筛选；名称/大小/时间/类型列可勾选组，
  // 勾选为并集（OR），多列 AND 叠加（目录/文件粗过滤由类型列 __dir 组承担，
  // 原工具栏「全部/文件夹/文件」分段控件已移除）
  const [columnFilters, setColumnFilters] = useState<ColumnFilterState>(DEFAULT_COLUMN_FILTERS);
  // 打开中的表头下拉菜单：锚点列 + 视口坐标（fixed 定位，portal 渲染）
  const [headerMenu, setHeaderMenu] = useState<{ key: SortKey; x: number; y: number } | null>(null);

  // 显示列表 = 已排序的数据源过「列筛选」；数据源：搜索词生效中为命中结果
  // （remote_search 返回后已按当前表头排序），否则为当前目录 entries——
  // 无词时目录内容原样显示（对齐 Windows：搜索框不隐藏正在浏览的文件）
  // （渲染与键盘导航均以此为准，切换过滤/排序不重新加载目录/不重跑搜索）
  const visibleEntries = useMemo(
    () => filterByColumnFilters(activeSearch ? (searchResults ?? []) : entries, columnFilters),
    [activeSearch, searchResults, entries, columnFilters]
  );

  // ── 文件搜索行为 ─────────────────────────────────────
  // 输入防抖 300ms 才成为生效查询词；清空输入即回到目录浏览
  useEffect(() => {
    const timer = setTimeout(() => setSearchQuery(searchInput.trim()), 300);
    return () => clearTimeout(timer);
  }, [searchInput]);

  // 生效查询词变化/目录切换/下钻层数变化/操作后重跑：调 remote_search（epoch 丢弃过期响应）
  useEffect(() => {
    if (!searchQuery || !activeServerId) {
      setSearchResults(null);
      setSearchTruncated(false);
      setSearchError(null);
      return;
    }
    const epoch = ++searchEpochRef.current;
    setSearching(true);
    clearSelection(); // 数据源即将切换，清除悬空选中
    invoke<SearchResponse>("remote_search", {
      serverId: activeServerId,
      root: currentPath,
      query: searchQuery,
      depth: fileManagerSearchDepth,
    })
      .then((resp) => {
        if (epoch !== searchEpochRef.current) return; // 过期响应丢弃
        setSearchResults(sortFileEntries(
          resp.results.filter((e) => !isTransferTempName(e.name)),
          sortStateRef.current
        ));
        setSearchTruncated(resp.truncated);
        setSearchError(null);
      })
      .catch((e) => {
        if (epoch !== searchEpochRef.current) return;
        setSearchResults([]);
        setSearchTruncated(false);
        setSearchError(formatUnknownError(e));
        log.error("搜索失败:", e);
      })
      .finally(() => {
        if (epoch === searchEpochRef.current) setSearching(false);
      });
  }, [searchQuery, activeServerId, currentPath, searchNonce, fileManagerSearchDepth]);

  /** 清空搜索：回到当前目录浏览；在途响应经 epoch 作废 */
  const exitSearch = useCallback(() => {
    searchEpochRef.current++;
    setSearchInput("");
    setSearchQuery("");
    setSearchResults(null);
    setSearching(false);
    setSearchTruncated(false);
    setSearchError(null);
    clearSelection();
  }, []);

  // Ctrl+F 聚焦搜索框（Windows Explorer 惯例）；离线无目标目录不启用
  useEffect(() => {
    if (isOffline) return;
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "f") {
        // 仅响应活动窗口：多文件管理器窗口共存时共用 document，
        // 否则后台窗口的搜索框会被一并抢焦点（实测同类问题）
        if (manager.getActive()?.id !== windowId) return;
        e.preventDefault();
        searchInputRef.current?.focus();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [isOffline, manager, windowId]);

  // ── 表头排序（点击翻转）────────────────────────────────
  // 会话内状态：目录加载与点击表头共用；目录优先规则见 fileTable.sortFileEntries
  const [sortState, setSortState] = useState<SortState>(DEFAULT_SORT);
  // loadDir 需要读最新排序但不能把 sortState 加进其 useCallback 依赖——
  // 否则初始加载 useEffect（依赖 loadDir）会在每次排序翻转时重新加载家目录
  const sortStateRef = useRef(sortState);
  useEffect(() => { sortStateRef.current = sortState; }, [sortState]);

  // ── 列宽（拖拽调节 + 持久化）────────────────────────────
  // 持久值来自设置存储（zustand persist → Tauri Store，Rust 侧写盘）；
  // 拖拽会话中用本地临时值即时预览，mouseup 才提交持久层（避免拖动过程频繁写盘）
  const storedColumns = useSettingsStore(s => s.fileManagerColumns);
  // 旧持久化数据缺 type 字段（类型列为后加列）：合并默认值兜底，避免类型列宽 undefined
  const persistedColumns: ColumnWidths = { ...DEFAULT_COLUMN_WIDTHS, ...(storedColumns ?? {}) };
  const setFileManagerColumns = useSettingsStore(s => s.setFileManagerColumns);
  const [resizing, setResizing] = useState<{ key: SortKey; startX: number; startWidth: number } | null>(null);
  // 悬停的表头列：整列（表头+数据行）淡聚焦染色，让列宽可视化（col-hover-* 容器类 + CSS 选择器）
  const [hoveredCol, setHoveredCol] = useState<SortKey | null>(null);
  const [dragWidth, setDragWidth] = useState<number | null>(null);
  // mouseup 提交时读最新拖拽宽度（handleUp 闭包内同步可达）
  const dragWidthRef = useRef<number | null>(null);
  // 列表容器（fm-list）：用于计算列宽上限的可用空间
  const listAreaRef = useRef<HTMLDivElement>(null);
  // 实际生效列宽：拖拽中的列取临时值，其余取持久值
  const columnWidths: ColumnWidths = resizing && dragWidth !== null
    ? { ...persistedColumns, [resizing.key]: dragWidth }
    : persistedColumns;
  const [history, setHistory] = useState<string[]>([currentPath]);
  const [historyIdx, setHistoryIdx] = useState(0);
  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    targets: FileEntry[] | null;  // 选中条目数组（右键菜单作用对象）；null = 空白区域菜单
  } | null>(null);
  const [propertiesEntry, setPropertiesEntry] = useState<FileEntry | null>(null);
  // 属性对话框可编辑状态（打开时从 entry 初始化；应用时与初始值比对决定是否发请求）
  const [permMatrix, setPermMatrix] = useState<PermMatrix>(parsePermissions(undefined));
  const [permOwner, setPermOwner] = useState("");
  const [permGroup, setPermGroup] = useState("");
  const [permRecursive, setPermRecursive] = useState(false);  // 目录：应用到子文件和文件夹
  const [permSaving, setPermSaving] = useState(false);        // 请求进行中（禁用按钮/遮罩）
  const [permError, setPermError] = useState<string | null>(null);  // 内联错误（不关闭对话框）
  const [permTab, setPermTab] = useState<'general' | 'permissions'>('general');  // 属性分页：常规（只读）/ 权限（可编辑）
  const [editingEntry, setEditingEntry] = useState<FileEntry | null>(null);  // 正在编辑的文件
  const [editingName, setEditingName] = useState<string>("");  // 编辑中的新名称
  // ✅ 移除 mainRef，因为fm-main容器已被AppLayout替代
  const clickTimerRef = useRef<number | null>(null);  // 单击延迟定时器（防止双击误触发）
  // 卸载清理：窗口关闭瞬间挂起的 200ms 定时器会触发 setEditingEntry
  // （React 18 下无害 no-op 但属泄漏路径，审计 L7）
  useEffect(() => {
    return () => {
      if (clickTimerRef.current) {
        clearTimeout(clickTimerRef.current);
        clickTimerRef.current = null;
      }
    };
  }, []);

  // 侧边栏状态
  const [sidebarSections, setSidebarSections] = useState<SidebarSection[]>([]);
  const [mounts, setMounts] = useState<MountInfo[]>([]);

  // 路径输入框状态
  const [pathInput, setPathInput] = useState(currentPath);
  const [isEditingPath, setIsEditingPath] = useState(false);

  // 路径建议列表状态
  const [suggestions, setSuggestions] = useState<string[]>([]);
  const [showSuggestions, setShowSuggestions] = useState(false);
  const [selectedSuggestionIdx, setSelectedSuggestionIdx] = useState<number | null>(null);

  // 路径不存在时的错误弹窗
  const [pathErrorDialog, setPathErrorDialog] = useState<string | null>(null);

  // 文件重命名弹窗状态
  const [renameDialog, setRenameDialog] = useState<{
    localPath: string;
    originalName: string;
    newName: string;
    maxBytes: number;
  } | null>(null);

  // 拖拽上传：文件拖入窗口时显示覆盖层提示（Tauri webview 级拖放事件驱动）
  const [dragActive, setDragActive] = useState(false);

  // 重命名输入框 ref
  const renameInputRef = useRef<HTMLInputElement>(null);

  // 自动选中文件名部分（不包含后缀）
  // 类似 Windows 文件重命名逻辑
  useEffect(() => {
    if (renameDialog && renameInputRef.current) {
      // 等待输入框渲染完成
      const timer = setTimeout(() => {
        if (renameInputRef.current) {
          const fileName = renameDialog.newName;

          // 查找最后一个点号（文件扩展名分隔符）
          const lastDotIndex = fileName.lastIndexOf('.');

          if (lastDotIndex > 0) {
            // 选中文件名部分（不包含后缀）
            // 例如：文件名.txt -> 选中 "文件名"
            renameInputRef.current.setSelectionRange(0, lastDotIndex);
          } else {
            // 没有扩展名，全选
            renameInputRef.current.select();
          }
        }
      }, 0);

      return () => clearTimeout(timer);
    }
  }, [renameDialog?.localPath]); // 只在弹窗首次打开时执行（依赖文件路径而不是整个对象）

  // 内联编辑（新建默认名 / F2 重命名共用）初始选中：文件名含扩展名只选基名
  // （Windows F2 惯例），否则全选。列表/网格共用一个 ref（同一时刻仅渲染其一）；
  // 读 input.value 而非 editingName：只在进入编辑时执行一次，不随输入重跑
  const inlineEditRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (!editingEntry) return;
    const timer = setTimeout(() => {
      const input = inlineEditRef.current;
      if (!input) return;
      const lastDotIndex = input.value.lastIndexOf(".");
      if (lastDotIndex > 0) {
        input.setSelectionRange(0, lastDotIndex);
      } else {
        input.select();
      }
    }, 0);
    return () => clearTimeout(timer);
  }, [editingEntry]);

  // 内联编辑宽度跟随文本：ch 单位以 "0" 字形宽为基准，中日韩全角字符约为其 2 倍宽，
  // 按 length 计 ch 宽对中文必然过窄（截断/滚动）。改用 canvas measureText 按输入框
  // 实际字体测量，加 padding/border/光标余量后定 px 宽；layout 阶段生效，绘制无闪烁。
  // 列表/网格两处输入框字号不同（--font-body / --font-small），从实际元素读计算样式
  const inlineMeasureCtxRef = useRef<CanvasRenderingContext2D | null>(null);
  useLayoutEffect(() => {
    const input = inlineEditRef.current;
    if (!editingEntry || !input) return;
    if (!inlineMeasureCtxRef.current) {
      inlineMeasureCtxRef.current = document.createElement("canvas").getContext("2d");
    }
    const ctx = inlineMeasureCtxRef.current;
    if (!ctx) return;
    const cs = getComputedStyle(input);
    ctx.font = `${cs.fontStyle} ${cs.fontWeight} ${cs.fontSize} ${cs.fontFamily}`;
    const textWidth = ctx.measureText(editingName).width;
    const extra =
      parseFloat(cs.paddingLeft) + parseFloat(cs.paddingRight) +
      parseFloat(cs.borderLeftWidth) + parseFloat(cs.borderRightWidth) + 4; // 4px：光标与末字符间隙
    input.style.width = `${Math.max(Math.ceil(textWidth + extra), 40)}px`;
  }, [editingName, editingEntry]);

  // 全局点击监听：点击外部时让输入框失焦输入框 ref（用于检测点击位置）
  const pathInputRef = useRef<HTMLInputElement>(null);

  // 全局点击监听：点击外部时让输入框失焦
  // 失焦功能：始终监听，不依赖 showSuggestions（保持功能原子性）
  useEffect(() => {
    const handleGlobalClick = (e: MouseEvent) => {
      const target = e.target as HTMLElement;

      // 点击不在输入框和建议列表内时，让输入框失焦
      if (
        pathInputRef.current &&
        !pathInputRef.current.contains(target) &&
        !target.closest(".fm-suggestions")
      ) {
        // 触发 blur，让 handlePathInputBlur 统一处理所有失焦逻辑
        pathInputRef.current.blur();
      }
    };

    // 使用 window + capture 确保能捕获到所有点击（包括标题栏）
    window.addEventListener("mousedown", handleGlobalClick, { capture: true });
    return () => window.removeEventListener("mousedown", handleGlobalClick, { capture: true });
  }, []); // ❌ 不依赖 showSuggestions，让失焦功能始终工作

  const loadDir = useCallback(async (path: string, opts?: { keepSelection?: boolean }): Promise<boolean> => {
    // 竞态守卫：快速连续导航时，先发后至的旧响应不得覆盖新目录视图
    // （与搜索的 searchEpochRef 同模式；过期响应静默丢弃并返回 false，
    // 调用方不据此推导航历史）
    const epoch = ++dirEpochRef.current;

    setLoading(true);
    setError(null);
    setPermissionDenied(null);
    permissionDeniedRef.current = null;
    // 外部刷新（目录监听/兜底重载）保留选区：仅剔除已不存在的条目；
    // 导航（进入/返回/面包屑）仍清空
    if (!opts?.keepSelection) clearSelection();
    setContextMenu(null);

    // 调试：打印当前状态
    log.debug("loadDir 调用:", {
      path,
      activeServerId,
      isRemote: !!activeServerId,
    });

    try {
      let resp: ReadDirResponse | null = null;

      if (activeServerId) {
        // 远程模式：通过 QUIC 从 Agent 获取
        log.debug("远程模式，调用 remote_read_dir");
        resp = await invoke<ReadDirResponse>("remote_read_dir", {
          serverId: activeServerId,
          path,
        });
      } else {
        // 离线模式：不加载本地数据
        resp = null;
      }

      if (epoch !== dirEpochRef.current) return false; // 过期响应，丢弃

      if (resp && resp.entries) {
        // 按当前表头排序（默认：目录优先 + 名称升序，与历史行为一致）
        // 上传临时文件（quireld_*.tmp）不是用户内容：旧 Agent 的列表也可能包含，统一过滤
        const rows = resp.entries.filter((e) => !isTransferTempName(e.name));
        setEntries(sortFileEntries(rows, sortStateRef.current));
        // 保留选区：只保留仍存在的路径（目录内容变化后自动剔除已删除项）
        if (opts?.keepSelection) {
          const valid = new Set(
            rows.map((e) => (path === "/" ? `/${e.name}` : `${path}/${e.name}`))
          );
          setSelected((prev) => prev.filter((p) => valid.has(p)));
          setAnchorPath((prev) => (prev && valid.has(prev) ? prev : null));
        }
      } else {
        setEntries(sortFileEntries(getDemoEntries(path), sortStateRef.current));
      }

      // 成功后才更新当前路径（确保路径确实存在）
      setCurrentPath(path);
      currentPathRef.current = path;
      return true;
    } catch (e: unknown) {
      if (epoch !== dirEpochRef.current) return false; // 过期失败的请求，不污染新视图

      log.error('加载目录失败:', e);
      const errMsg = formatUnknownError(e);
      // 权限不足：当作空目录处理（进入目录但内容不可见）
      // 其他错误（路径不存在等）：显示错误提示，不更新路径
      if (isPermissionDenied(e)) {
        setEntries([]);
        setPermissionDenied(path);
        permissionDeniedRef.current = path;
        setCurrentPath(path);
        currentPathRef.current = path;
      } else {
        setError(errMsg);
        setEntries([]);
      }
      return false;
    } finally {
      // 仅最新一次加载有权收 loading 态（过期请求的 finally 会错杀在途新请求）
      if (epoch === dirEpochRef.current) {
        setLoading(false);
      }
    }
  }, [activeServerId]);

  /* ── 表头排序：点击翻转 ────────────────────────────────── */

  /** 点击表头：同列翻转升/降序，异列切换到该列升序；立即对现有列表重排
   *  （entries 即显示顺序，键盘导航 selectedIdx 语义不受影响） */
  const toggleSort = useCallback((key: SortKey) => {
    const prev = sortStateRef.current;
    const next: SortState = prev.key === key
      ? { key, dir: prev.dir === "asc" ? "desc" : "asc" }
      : { key, dir: "asc" };
    setSortState(next);
    sortStateRef.current = next;  // 同步 ref，防连点时读到旧值
    setEntries(prevEntries => sortFileEntries(prevEntries, next));
    setSearchResults(prev => (prev ? sortFileEntries(prev, next) : prev)); // 搜索视图同步重排
  }, []);

  /* ── 表头下拉菜单（列筛选，Win11 Explorer 风格）─────────── */

  /** 打开/切换关闭表头下拉菜单：锚定按钮下方、左缘对齐列头
   *  （右溢出时向左收进视口）；再点同列按钮 = 关闭。
   *  仅大小/修改时间列有筛选区（名称/权限列不渲染下拉按钮） */
  const openHeaderMenu = useCallback((e: React.MouseEvent, key: SortKey) => {
    e.stopPropagation();  // 不触发表头点击排序
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const MENU_WIDTH = 210;
    setHeaderMenu(prev => {
      if (prev?.key === key) return null;
      const x = Math.max(8, Math.min(rect.left, window.innerWidth - MENU_WIDTH - 8));
      return { key, x, y: Math.round(rect.bottom) + 2 };
    });
  }, []);

  /** 勾选/取消一个筛选桶：菜单保持打开（可连续多选，Win11 同款）
   *  name/size/mtime/type 四列可筛选（perm 无筛选区）；type 桶为动态 string 键 */
  const toggleColumnFilter = useCallback((key: SortKey, bucket: SizeBucket | MtimeBucket | NameBucket | string) => {
    if (key !== "size" && key !== "mtime" && key !== "name" && key !== "type") return;
    setColumnFilters(prev => key === "size"
      ? { ...prev, size: toggleBucket(prev.size, bucket as SizeBucket) }
      : key === "mtime"
        ? { ...prev, mtime: toggleBucket(prev.mtime, bucket as MtimeBucket) }
        : key === "name"
          ? { ...prev, name: toggleBucket(prev.name, bucket as NameBucket) }
          : { ...prev, type: toggleBucket(prev.type, bucket) });
    // 筛选改变可见列表：路径制选中天然跨筛选保留（被过滤掉的选中项不参与
    // 右键批量操作，选中交集见 selectedEntries），不再强制清空
  }, []);

  // 点击菜单外关闭（与右键菜单同一模式：window 级 click 兜底）
  useEffect(() => {
    if (!headerMenu) return;
    const close = () => setHeaderMenu(null);
    window.addEventListener("click", close);
    return () => window.removeEventListener("click", close);
  }, [headerMenu]);

  /* ── 列宽拖拽：mousedown 预览、mouseup 提交持久层 ────────── */

  /** 开始拖拽列宽：记录起点与该列当前渲染宽度（名称列自此从弹性转固定） */
  const beginColumnResize = useCallback((e: React.MouseEvent, key: SortKey) => {
    e.preventDefault();    // 阻止拖拽选中文本
    e.stopPropagation();  // 不触发表头排序
    const colEl = (e.currentTarget as HTMLElement).parentElement;
    const headerEl = colEl?.parentElement;
    if (!colEl || !headerEl) return;
    // 起始宽度取 grid 轨道宽（computed used value），不能用列头盒 getBoundingClientRect——
    // 首列盒为铺满表头左缘有 margin-left: -12px 外扩，盒宽 = 轨道 + 12px，用它当起点
    // 会让名称列每次 mousedown 都膨胀 12px（点击边界尚未拖动列就右移）。
    const headerGrid = getComputedStyle(headerEl).gridTemplateColumns;
    const colIndex = FILE_COLUMNS.findIndex(c => c.key === key);
    const trackPx = parseFloat(headerGrid.split(" ")[colIndex]);
    const startWidth = Number.isFinite(trackPx)
      ? trackPx
      : colEl.getBoundingClientRect().width;  // 解析失败兜底（如浏览器返回 used 值异常）
    dragWidthRef.current = startWidth;
    setDragWidth(startWidth);
    setResizing({ key, startX: e.clientX, startWidth });
  }, []);

  // 拖拽会话：window 级 mousemove/mouseup（鼠标移出表头仍持续跟踪）
  useEffect(() => {
    if (!resizing) return;
    const { key, startX, startWidth } = resizing;

    // 列宽不设上限（对齐 Explorer/Nautilus）：列总和超出容器后由 .fm-list 整体
    // 横向滚动兜底（表头 sticky 顶部、与行同滚）。此前「上限=容器剩余空间」的约束
    // 在列总和占满容器后会互相锁死所有列（无空白可吃，任何列都拖不大）。

    const handleMove = (e: MouseEvent) => {
      const w = clampColumnWidth(key, startWidth + (e.clientX - startX));
      dragWidthRef.current = w;
      setDragWidth(w);
    };
    const handleUp = () => {
      const w = dragWidthRef.current;
      if (w !== null) {
        // 拖拽结束提交一次持久层（getState 读最新值，避免闭包旧值覆盖并发修改）
        const current = useSettingsStore.getState().fileManagerColumns ?? DEFAULT_COLUMN_WIDTHS;
        setFileManagerColumns({ ...current, [key]: w });
      }
      dragWidthRef.current = null;
      setDragWidth(null);
      setResizing(null);
      // 拖拽期间 leave 被抑制（保持整列高亮），结束时统一清理
      setHoveredCol(null);
      // 吞掉拖拽结束时浏览器合成的 click：mousedown 发生在手柄、mouseup 落在
      // 表头上时，click 会派发到二者的公共祖先（列头 span），误触发排序翻转；
      // 捕获阶段一次性拦截——合成 click 在 mouseup 后立即派发，不会误伤用户的下一次真实点击
      const swallow = (ev: MouseEvent) => {
        ev.stopPropagation();
        ev.preventDefault();
      };
      document.addEventListener("click", swallow, { capture: true, once: true });
    };

    window.addEventListener("mousemove", handleMove);
    window.addEventListener("mouseup", handleUp);
    // 全局列宽光标 + 禁止文本选中（拖拽期间）
    const prevCursor = document.body.style.cursor;
    const prevUserSelect = document.body.style.userSelect;
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
    return () => {
      window.removeEventListener("mousemove", handleMove);
      window.removeEventListener("mouseup", handleUp);
      document.body.style.cursor = prevCursor;
      document.body.style.userSelect = prevUserSelect;
    };
    // resizing 即拖拽会话对象，会话期间 key/startX/startWidth 不变
  }, [resizing, setFileManagerColumns]);

  // ── Path Suggestions ───────────────────────────────────
  const suggestEpochRef = useRef(0);
  const fetchSuggestions = useCallback(async (path: string) => {
    if (!activeServerId || path === "") {
      suggestEpochRef.current++; // 清空调用也失效在途响应
      setSuggestions([]);
      setShowSuggestions(false);
      return;
    }
    // 竞态守卫（审计 L3）：连续输入时旧 remote_read_dir 响应不得覆盖新的
    const epoch = ++suggestEpochRef.current;

    try {
      // 解析路径：
      // - 路径以 "/" 结尾：显示该目录下的所有子目录（如 "/home/vic/" → 显示 "/home/vic" 的子目录）
      // - 路径不以 "/" 结尾：显示父目录下匹配部分名称的子目录（如 "/home/us" → 显示 "/home" 下以 "us" 开头的目录）
      let parentPath: string;
      let partialName: string;

      if (path.endsWith("/")) {
        // 以 "/" 结尾：parentPath 是去掉末尾 "/" 的路径本身，partialName 为空
        parentPath = path.slice(0, -1) || "/";
        partialName = "";
      } else {
        // 不以 "/" 结尾：按原逻辑处理
        const parts = path.split("/").filter(p => p !== "");
        parentPath = parts.length === 0 ? "/" : "/" + parts.slice(0, -1).join("/");
        partialName = parts.length === 0 ? "" : parts[parts.length - 1];
      }

      log.debug("fetchSuggestions: 输入路径:", path, "父目录:", parentPath, "部分名称:", partialName);

      // 调用 remote_read_dir 获取父目录的文件列表
      const resp = await invoke<ReadDirResponse>("remote_read_dir", {
        serverId: activeServerId,
        path: parentPath,
      });

      // 过滤出匹配部分名称的目录
      const suggestions: string[] = [];
      for (const entry of resp.entries) {
        // 只建议目录（不包括文件）
        if (!entry.is_dir) continue;

        // 匹配部分名称（如果部分名称为空，建议所有目录）
        if (partialName === "" || entry.name.startsWith(partialName)) {
          // 构建完整路径
          const fullPath = parentPath === "/" ? `/${entry.name}` : `${parentPath}/${entry.name}`;
          suggestions.push(fullPath);

          // 最多 10 个建议
          if (suggestions.length >= 10) break;
        }
      }

      log.debug("fetchSuggestions: 建议列表:", suggestions);

      if (epoch !== suggestEpochRef.current) return; // 过期响应，丢弃
      setSuggestions(suggestions);
      setShowSuggestions(suggestions.length > 0);
      setSelectedSuggestionIdx(null);
    } catch (err) {
      log.error("获取建议失败:", err);
      if (epoch !== suggestEpochRef.current) return;
      setSuggestions([]);
      setShowSuggestions(false);
    }
  }, [activeServerId]);

  // ── 父级数据注入：优先使用预加载数据 ─────────────────
  // 当 Desktop 通过 usePreloader 预加载完成后注入 initialData 时，
  // 直接初始化所有状态，跳过后续的首次加载 useEffect
  useEffect(() => {
    if (initialData) {
      log.debug("使用父级注入的初始数据");
      setUsername(initialData.username);
      setHomeDir(initialData.homeDir);
      setCurrentPath(initialData.currentPath);
      setEntries(initialData.entries);
      setMounts(initialData.mounts);
      setSidebarSections(initialData.sidebarSections);
      setHistory([initialData.currentPath]);
      setPathInput(initialData.currentPath);
      setLoading(false);
    }
  }, [initialData]);

  // 获取远程服务器用户名和真实家目录
  useEffect(() => {
    if (initialData) return; // 有注入数据时跳过，已通过 initialData 初始化
    log.debug("获取用户名 useEffect, activeServerId:", activeServerId);
    if (activeServerId) {
      // 连接建立后，获取用户名和真实家目录
      log.debug("调用 remote_get_current_user, serverId:", activeServerId);
      invoke<{ username: string; home_dir: string }>("remote_get_current_user", { serverId: activeServerId })
        .then((user) => {
          log.info("获取用户信息成功:", user);
          setUsername(user.username);
          // 旧版 Agent 不返回 home_dir（空串）：按 Linux 惯例回退推导初始目录
          const fallback = user.username === "root" ? "/root" : `/home/${user.username}`;
          setHomeDir(user.home_dir || fallback);
        })
        .catch(err => {
          log.error("获取用户信息失败:", err);
          setUsername("user");  // 默认值
          setHomeDir(null);
        });
    } else {
      // 离线模式：清空用户名
      setUsername(null);
      setHomeDir(null);
    }
  }, [activeServerId, initialData]);

  // Initial load
  useEffect(() => {
    if (initialData) return; // 有注入数据时跳过，已通过 initialData 初始化
    log.debug("初始加载 useEffect, activeServerId:", activeServerId, "username:", username, "homeDir:", homeDir);
    if (activeServerId && homeDir) {
      // 远程模式：使用服务端返回的真实家目录加载初始目录
      log.debug("远程模式，加载路径:", homeDir);
      loadDir(homeDir);
    } else if (!activeServerId) {
      // 离线模式：不加载（显示离线占位符）
      log.debug("离线模式，不加载");
    } else {
      log.debug("等待用户信息...");
    }
  }, [activeServerId, homeDir, loadDir, initialData]);

  // 获取挂载点列表
  useEffect(() => {
    if (initialData) return; // 有注入数据时跳过，已通过 initialData 初始化
    log.debug("获取挂载点 useEffect, activeServerId:", activeServerId);
    if (activeServerId) {
      // 远程模式：获取挂载点列表
      log.debug("调用 remote_get_mounts, serverId:", activeServerId);
      invoke<MountInfo[]>("remote_get_mounts", { serverId: activeServerId })
        .then((mounts) => {
          log.debug("获取挂载点成功:", mounts);
          setMounts(mounts);
        })
        .catch(err => {
          log.error("获取挂载点失败:", err);
          setMounts([]);  // 默认空列表
        });
    } else {
      // 离线模式：清空挂载点
      setMounts([]);
    }
  }, [activeServerId, initialData]);

  // 生成侧边栏
  useEffect(() => {
    if (initialData) return; // 有注入数据时跳过，已通过 initialData 初始化
    log.debug("生成侧边栏 useEffect, activeServerId:", activeServerId, "username:", username, "homeDir:", homeDir, "mounts:", mounts.length);
    if (activeServerId && username && homeDir) {
      // 远程模式：使用服务端返回的真实家目录生成侧边栏
      const sections: SidebarSection[] = [
        {
          title: t("fileManager.location"),
          items: [
            { icon: "home", label: t("fileManager.home"), path: homeDir, type: "bookmark" },
            { icon: "documents", label: t("fileManager.documents"), path: `${homeDir}/Documents`, type: "bookmark" },
            { icon: "download", label: t("fileManager.downloads"), path: `${homeDir}/Downloads`, type: "bookmark" },
            { icon: "pictures", label: t("fileManager.pictures"), path: `${homeDir}/Pictures`, type: "bookmark" },
            { icon: "music", label: t("fileManager.music"), path: `${homeDir}/Music`, type: "bookmark" },
            { icon: "videos", label: t("fileManager.videos"), path: `${homeDir}/Videos`, type: "bookmark" },
          ]
        },
        {
          title: t("fileManager.devices"),
          items: mounts.map(m => ({
            icon: "disk",
            label: m.mount_point,
            path: m.mount_point,
            type: "mount"
          }))
        },
        {
          title: t("fileManager.otherLocations"),
          items: [
            { icon: "network", label: t("fileManager.network"), path: "/network", type: "network" }
          ]
        }
      ];
      log.debug("远程侧边栏生成完成:", sections);
      setSidebarSections(sections);
    } else {
      // 离线模式：使用默认 Linux 侧边栏（保持 UI 结构一致）
      const sections: SidebarSection[] = [
        {
          title: t("fileManager.location"),
          items: [
            { icon: "home", label: t("fileManager.home"), path: "/home", type: "bookmark" },
            { icon: "documents", label: t("fileManager.documents"), path: "/home/Documents", type: "bookmark" },
            { icon: "download", label: t("fileManager.downloads"), path: "/home/Downloads", type: "bookmark" },
            { icon: "pictures", label: t("fileManager.pictures"), path: "/home/Pictures", type: "bookmark" },
            { icon: "music", label: t("fileManager.music"), path: "/home/Music", type: "bookmark" },
            { icon: "videos", label: t("fileManager.videos"), path: "/home/Videos", type: "bookmark" },
          ]
        },
        {
          title: t("fileManager.otherLocations"),
          items: [
            { icon: "network", label: t("fileManager.network"), path: "/network", type: "network" }
          ]
        },
      ];
      setSidebarSections(sections);
    }
  }, [activeServerId, username, homeDir, mounts, initialData, t]);

  // ── 断连即时响应 ─────────────────────────────────────
  // 当 activeServerId 从有值变为 null（断连），立即清空数据并显示离线占位符
  useEffect(() => {
    if (!activeServerId) {
      // 断连：立即重置为离线状态（不清空侧边栏，保留 UI 结构）
      setEntries([]);
      setError(null);
      setPermissionDenied(null);
      setLoading(false);
      clearSelection();
      setContextMenu(null);
      log.debug("检测到断连，切换到离线模式");
    }
  }, [activeServerId]);

  // ── 连接即时响应：清空旧本地数据 ─────────────────────
  // 当 activeServerId 从 null 变为有值（从离线/本地→远程连接），
  // 立即清空残留的本地模式数据，避免显示 C:\ 路径或旧文件列表
  // 后续的 username/mounts/sidebar/loadDir useEffect 会自动填充远程数据
  const prevServerIdRef = useRef<string | null>(null);
  useEffect(() => {
    const wasOffline = !prevServerIdRef.current;
    const nowConnected = !!activeServerId;
    if (wasOffline && nowConnected) {
      // 清空所有本地模式的残留数据
      setEntries([]);
      setError(null);
      setPermissionDenied(null);
      setLoading(false);
      clearSelection();
      setContextMenu(null);
      setUsername(null);  // 清空，让 username useEffect 重新获取
      setMounts([]);
      setSidebarSections([]);
      // 重置路径为 Linux 根目录（避免 C:\ 残留，真实家目录由后续 homeDir useEffect 加载）
      setCurrentPath("/");
      setHistory(["/"]);
      setHistoryIdx(0);
      setPathInput("/");
      log.debug("检测到新连接，已清空本地残留数据");
    }
    prevServerIdRef.current = activeServerId;
  }, [activeServerId]);

  // 同步路径输入框
  useEffect(() => {
    if (!isEditingPath) {
      setPathInput(currentPath);
    }
  }, [currentPath, isEditingPath]);

  // 防抖：输入停止 300ms 后获取建议或导航
  useEffect(() => {
    if (!isEditingPath || pathInput === currentPath) {
      return;
    }

    const timer = setTimeout(() => {
      // 获取路径建议
      fetchSuggestions(pathInput);
    }, 300);

    return () => clearTimeout(timer);
  }, [pathInput, isEditingPath, currentPath, fetchSuggestions]);

  // 处理路径输入
  const handlePathInputChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const value = e.target.value;
    setPathInput(value);

    // 输入以 "/" 结尾时立即展开建议（跳过防抖）
    if (value.endsWith("/") && activeServerId) {
      fetchSuggestions(value);
    }
  };

  // 打开终端并传递当前路径作为工作目录
  const openTerminalAtCurrentPath = useCallback(() => {
    manager.create('terminal', {
      preloadData: {
        workingDirectory: currentPath
      }
    });
    // 恢复地址栏显示为当前路径
    setPathInput(currentPath);
    pathInputRef.current?.blur();
  }, [manager, currentPath]);

  const handlePathInputKeyDown = async (e: React.KeyboardEvent<HTMLInputElement>) => {
    // 如果建议列表显示且有选中项，按 Enter 选择建议项
    if (e.key === "Enter" && showSuggestions && selectedSuggestionIdx !== null) {
      e.preventDefault();
      handleSelectSuggestion(suggestions[selectedSuggestionIdx]);
    } else if (e.key === "Enter") {
      e.preventDefault();
      setShowSuggestions(false);

      // 检测 shell 命令：打开当前目录的终端
      if (pathInput.trim() === 'shell') {
        openTerminalAtCurrentPath();
        return;
      }

      // 注意：不在此处设置 setIsEditingPath(false)
      // 过早设为 false 会导致 useEffect 在异步导航期间覆盖用户输入，造成删除异常
      // 统一在导航完成后通过 blur() 触发 handlePathInputBlur 清理

      // 格式化路径
      const cleanPath = normalizePath(pathInput);

      // 直接 loadDir，失败按错误类型分流提示——此前先 remote_read_dir
      // 探测再 navigateTo→loadDir，同一目录读两次（审计 L12）。
      // loadDir 已区分权限/不存在并暴露 ok 返回值
      const ok = await navigateTo(cleanPath);
      pathInputRef.current?.blur();
      if (!ok && !permissionDeniedRef.current) {
        // loadDir 失败已 setError/权限提示；路径不存在类错误补弹窗引导
        // （navigateTo 内部 loadDir 的错误路径会 setEntries([]) + setError）
        setPathErrorDialog(t("fileManager.pathNotFound", { path: cleanPath }));
      }
    } else if (e.key === "Escape") {
      e.preventDefault();
      setShowSuggestions(false);
      setIsEditingPath(false);
      setPathInput(currentPath);
    } else if (e.key === "Tab" && suggestions.length > 0) {
      e.preventDefault();
      handleSelectSuggestion(suggestions[0]);
    } else if (e.key === "ArrowDown" && showSuggestions) {
      e.preventDefault();
      setSelectedSuggestionIdx(prev =>
        prev === null ? 0 : Math.min(prev + 1, suggestions.length - 1)
      );
    } else if (e.key === "ArrowUp" && showSuggestions) {
      e.preventDefault();
      setSelectedSuggestionIdx(prev =>
        prev === null ? suggestions.length - 1 : Math.max(prev - 1, 0)
      );
    }
  };

  const handlePathInputFocus = (e: React.FocusEvent<HTMLInputElement>) => {
    setIsEditingPath(true);
    // 自动全选，方便用户输入新地址
    e.target.select();
  };

  // 解析路径为分段（用于面包屑导航）
  const parsePathSegments = useCallback((path: string): Array<{ name: string; path: string }> => {
    const segments: Array<{ name: string; path: string }> = [];
    if (!path || path === "/") {
      return [{ name: "🏠", path: "/" }];  // 根目录显示图标
    }

    // 分割路径
    const parts = path.split("/").filter(p => p !== "");

    // 第一个分段是根目录 "/"（作为按钮，显示图标）
    segments.push({ name: "🏠", path: "/" });

    // 添加每个路径分段
    let accumulatedPath = "";
    parts.forEach((part) => {
      accumulatedPath += "/" + part;
      segments.push({ name: part, path: accumulatedPath });
    });

    return segments;
  }, []);

  // 路径格式化函数：确保路径格式正确
  const normalizePath = useCallback((path: string): string => {
    // 移除多余的斜杠
    let cleanPath = path.replace(/\/+/g, "/");
    // 移除末尾的斜杠（除非是根目录）
    if (cleanPath.length > 1 && cleanPath.endsWith("/")) {
      cleanPath = cleanPath.slice(0, -1);
    }
    // 确保路径以斜杠开头（除非是空路径）
    if (cleanPath.length > 0 && !cleanPath.startsWith("/")) {
      cleanPath = "/" + cleanPath;
    }
    // 空路径默认为根目录
    if (cleanPath === "") {
      cleanPath = "/";
    }
    return cleanPath;
  }, []);

  const handlePathInputBlur = () => {
    setIsEditingPath(false);
    setShowSuggestions(false);  // 失焦时收起建议列表
    // 恢复为当前路径（currentPath 总是正确格式）
    setPathInput(currentPath);
  };

  // ── Navigation ──────────────────────────────────────
  const navigateTo = useCallback(async (path: string): Promise<boolean> => {
    // 格式化路径
    const cleanPath = normalizePath(path);

    log.debug("navigateTo: 输入路径:", path, "清理后:", cleanPath);

    const success = await loadDir(cleanPath);
    if (success) {
      // 成功后才更新导航历史
      const newHistory = history.slice(0, historyIdx + 1);
      newHistory.push(cleanPath);
      setHistory(newHistory);
      setHistoryIdx(newHistory.length - 1);
    }
    return success;
  }, [history, historyIdx, loadDir, normalizePath]);

  // 跳转到指定路径（面包屑导航使用）
  const handleBreadcrumbClick = useCallback((path: string) => {
    navigateTo(path);
  }, [navigateTo]);

  const handleSelectSuggestion = useCallback((suggestion: string) => {
    setPathInput(suggestion);
    setShowSuggestions(false);
    // 不在此处 setIsEditingPath(false)，与 Enter 保持一致：通过 blur() 统一清理
    navigateTo(suggestion).then(() => {
      // 导航完成后主动失焦，由 handlePathInputBlur 统一处理
      pathInputRef.current?.blur();
    });
  }, [navigateTo]);

  const goBack = useCallback(() => {
    if (historyIdx > 0) {
      exitSearch(); // 任何导航都离开搜索视图（Nautilus 语义：搜索绑定当前目录）
      setHistoryIdx(historyIdx - 1);
      loadDir(history[historyIdx - 1]);
    }
  }, [history, historyIdx, loadDir, exitSearch]);

  const goUp = useCallback(() => {
    const parent = currentPath.split("/").slice(0, -1).join("/") || "/";
    navigateTo(parent);
  }, [currentPath, navigateTo]);

  const goForward = useCallback(() => {
    if (historyIdx < history.length - 1) {
      exitSearch(); // 同 goBack：导航离开搜索视图
      setHistoryIdx(historyIdx + 1);
      loadDir(history[historyIdx + 1]);
    }
  }, [history, historyIdx, loadDir, exitSearch]);

  // ── 搜索命中路径工具 ──────────────────────────────────
  /** 条目完整路径：搜索命中自带 path（嵌套目录定位依据）；普通浏览按当前目录拼装 */
  const entryFullPath = useCallback((entry: FileEntry): string => {
    if (entry.path) return entry.path;
    return currentPath === "/" ? `/${entry.name}` : `${currentPath}/${entry.name}`;
  }, [currentPath]);

  /** 可见条目完整路径（visibleEntries 同序）：范围选择/框选/全选的顺序基准 */
  const visiblePaths = useMemo(
    () => visibleEntries.map((e) => entryFullPath(e)),
    [visibleEntries, entryFullPath]
  );

  // 选中集合（渲染热路径判定 O(1)）：大目录下每行 .includes 是 O(n·m)
  // （审计 M25）
  const selectedSet = useMemo(() => new Set(selected), [selected]);

  // 滚动容器 refs（虚拟窗口化的滚动源，审计 M25）
  // ⚠️ 列表的纵向滚动已上提到外层 .fm-list（CSS overflow: auto + sticky 表头），
  // .fm-list-content 自身不滚动——虚拟窗口必须监听 .fm-list，否则 scrollTop
  // 永远为 0，只会渲染第一屏，滚过之后整段目录是空白（实测「文件显示不全」）。
  // listContentRef 仅用于行高实测与框选几何（其 rect 随外层滚动一起移动）。
  const listContentRef = useRef<HTMLDivElement>(null);
  const gridRef = useRef<HTMLDivElement>(null);
  // 行高实测（首行 offsetHeight；未测得前用经验默认）
  const [listRowH, setListRowH] = useState(32);
  const [gridItemH, setGridItemH] = useState(64);
  // 行步进 = 行高 + 行间距（占位/窗口计算统一口径）
  const listRowStride = listRowH + LIST_ROW_GAP;

  // ── 列表视图窗口化（审计 M25）──────────────────────────
  const listWin = useVirtualWindow({
    scrollRef: listAreaRef,
    itemCount: visibleEntries.length,
    itemHeight: listRowStride,
    enabled: viewMode === "list",
  });

  // 切目录/切换搜索/切回列表：滚动回顶部。
  // 修复滚动源后 .fm-list 会真正滚动，不重置会把上一个目录的滚动位置带进
  // 新目录——首次进入时看起来像「目录开头少了一段文件」。
  useEffect(() => {
    if (viewMode !== "list") return;
    const el = listAreaRef.current;
    if (el) el.scrollTop = 0;
  }, [currentPath, searchQuery, viewMode]);

  // ── 网格视图窗口化（审计 M25）：按「行」开窗（行 = cols 个卡片）────
  // 列数与轨道宽取自浏览器解析的 grid-template-columns（auto-fill 精确值）
  const [gridCols, setGridCols] = useState(0);
  const [gridTrackW, setGridTrackW] = useState<number[]>([]);
  useEffect(() => {
    if (viewMode !== "grid") return;
    const cont = gridRef.current;
    if (!cont) return;
    const tracks = getComputedStyle(cont).gridTemplateColumns.split(" ").filter(Boolean);
    const widths = tracks.map((s) => parseFloat(s) || 0);
    if (widths.length > 0 && widths.length !== gridCols) {
      setGridCols(widths.length);
      setGridTrackW(widths);
    }
  }, [viewMode, gridCols]); // 依赖 viewport 变化由浏览器样式重算触发，此处宽松即可

  const gridRows = gridCols > 0 ? Math.ceil(visibleEntries.length / gridCols) : 0;
  const gridWin = useVirtualWindow({
    scrollRef: gridRef,
    itemCount: gridRows,
    itemHeight: gridItemH + 8, // 行高含 gap（8px）
    enabled: viewMode === "grid" && gridCols > 0 && gridItemH > 0,
  });

  // 行高实测：首行 offsetHeight（行高由字体/内距决定，写死会漂）
  useEffect(() => {
    if (viewMode !== "list") return;
    const el = listContentRef.current?.querySelector<HTMLElement>(".fm-list-row");
    if (el && el.offsetHeight > 0 && el.offsetHeight !== listRowH) {
      setListRowH(el.offsetHeight);
    }
  }, [viewMode, listRowH, visibleEntries.length]);
  useEffect(() => {
    if (viewMode !== "grid") return;
    const el = gridRef.current?.querySelector<HTMLElement>(".fm-grid-item");
    if (el && el.offsetHeight > 0 && el.offsetHeight !== gridItemH) {
      setGridItemH(el.offsetHeight);
    }
  }, [viewMode, gridItemH, visibleEntries.length]);

  // 展开窗口：内联重命名的行强制挂载（否则滚出视口时编辑输入框卸载、
  // 重命名丢失）
  const editingIdx = editingEntry
    ? visibleEntries.findIndex((e) => e.name === editingEntry.name && e.path === editingEntry.path)
    : -1;
  let winStart = listWin.start;
  let winEnd = listWin.end;
  if (viewMode === "list" && editingIdx >= 0) {
    if (editingIdx < winStart) winStart = editingIdx;
    if (editingIdx >= winEnd) winEnd = editingIdx + 1;
  }
  const padTop = winStart * listRowStride;
  const padBottom = Math.max(0, visibleEntries.length - winEnd) * listRowStride;
  const windowedEntries =
    viewMode === "list" ? visibleEntries.slice(winStart, winEnd) : visibleEntries;

  // 网格窗口（条目区间 = 行区间 × cols，含编辑行扩展）
  let gStartRow = gridWin.start;
  let gEndRow = gridWin.end;
  if (viewMode === "grid" && editingIdx >= 0 && gridCols > 0) {
    const er = Math.floor(editingIdx / gridCols);
    if (er < gStartRow) gStartRow = er;
    if (er >= gEndRow) gEndRow = er + 1;
  }
  const gStart = gStartRow * gridCols;
  const gEnd = Math.min(visibleEntries.length, gEndRow * gridCols);
  const GAP = 8;
  const gPadTop = gStartRow > 0 ? gStartRow * (gridItemH + GAP) - GAP : 0;
  const gPadBottom =
    gEndRow < gridRows ? (gridRows - gEndRow) * (gridItemH + GAP) - GAP : 0;
  const windowedGridEntries =
    viewMode === "grid" ? visibleEntries.slice(gStart, gEnd) : visibleEntries;

  /** 选中条目对象（可见交集）：右键批量菜单的操作对象——被筛选掉的选中项不参与 */
  const selectedEntries = useMemo(
    () => visibleEntries.filter((e) => selectedSet.has(entryFullPath(e))),
    [visibleEntries, selectedSet, entryFullPath]
  );

  /** 搜索命中的相对目录展示：path 去掉 root 前缀与末段名称（root 直下命中不显示） */
  const hitDirLabel = useCallback((p: string): string => {
    let rel = p;
    if (currentPath !== "/" && p.startsWith(`${currentPath}/`)) rel = p.slice(currentPath.length + 1);
    else if (p.startsWith("/")) rel = p.slice(1);
    const idx = rel.lastIndexOf("/");
    return idx === -1 ? "" : rel.slice(0, idx);
  }, [currentPath]);

  /** 变更操作后刷新：底层目录始终重载（退出搜索即见新数据）；
   *  搜索词生效中同时重跑搜索（nonce 触发 effect）。
   *  仅在服务器未回传权威条目（旧版 Agent）/目录事件 resync 时作为兜底使用。
   *
   *  目录取「此刻」视图（currentPathRef）：长流程（批量移动/上传完成/解压）
   *  完成时用户可能已切到别的目录，重载旧路径会把用户强行拉回（loadDir 会
   *  setCurrentPath），必须按当前目录刷新（审计 M21 同源问题）。 */
  const refreshAfterMutation = useCallback(() => {
    void loadDir(currentPathRef.current, { keepSelection: true });
    setSearchNonce((n) => n + 1);
  }, [loadDir]);

  // currentPathRef 兜底同步：覆盖 loadDir 之外的 setCurrentPath 调用点
  // （初始化 preloadData、回到根目录等）
  useEffect(() => {
    currentPathRef.current = currentPath;
  }, [currentPath]);

  /** 服务器确认后的增量视图更新：浏览列表与搜索结果同时打补丁，
   *  再按当前表头重排；被移除路径同步从选区/锚点清理。
   *
   *  目录以「此刻」视图为准（currentPathRef）：本回调被 doDelete 等长
   *  流程捕获后，逐项 await 期间用户可能已切换目录——若按闭包捕获的
   *  发起时目录过滤，补丁会命中新目录视图的同名条目（误删/误增）。
   *  视图已切换时补丁因 parentDirOf 不匹配被自然跳过，等价于丢弃
   *  （审计 M21） */
  const applyViewChanges = useCallback((changes: FileViewChange[]) => {
    const viewPath = currentPathRef.current;
    // 搜索条件用 ref：长流程回调的闭包可能停留在旧查询词上
    const query = searchQueryRef.current;
    const depth = searchDepthRef.current;
    setEntries((prev) => sortFileEntries(applyBrowseChanges(prev, viewPath, changes), sortStateRef.current));
    setSearchResults((prev) => (prev
      ? sortFileEntries(applySearchChanges(prev, viewPath, query, depth, changes), sortStateRef.current)
      : prev));

    // 选区/锚点同步：目录改名时其后代路径改写前缀；目录删除时其后代选中一并移除
    const removed = changes.filter((c): c is Extract<FileViewChange, { kind: "remove" }> => c.kind === "remove").map((c) => c.path);
    const renames = changes.filter((c): c is Extract<FileViewChange, { kind: "rename" }> => c.kind === "rename");
    if (removed.length > 0 || renames.length > 0) {
      const remap = (p: string): string => {
        for (const r of renames) {
          const prefix = r.from === "/" ? "/" : `${r.from}/`;
          if (p.startsWith(prefix)) return `${r.to}${p.slice(r.from.length)}`;
        }
        return p;
      };
      const gone = (p: string): boolean => removed.some((path) => {
        if (p === path) return true;
        const prefix = path === "/" ? "/" : `${path}/`;
        return p.startsWith(prefix);
      });
      setSelected((prev) => prev.map(remap).filter((p) => !gone(p)));
      setAnchorPath((prev) => (prev ? (gone(remap(prev)) ? null : remap(prev)) : prev));
    }
  }, []);

  // ── 目录实时变更（agent inotify 文件级增量）────────────────────
  // 事件携带 upserts/removes/renames：直接打补丁（浏览 + 搜索同时更新），
  // 不发全量请求；只有 resync（事件溢出/单批超限/stat 失败）才整目录重读。
  // 弱网/大目录下这是避免卡顿的关键：单条变更只传约 200B。
  const handleRemoteFsChange = useCallback((data: unknown) => {
    const d = data as {
      dir?: string;
      resync?: boolean;
      upserts?: FileEntry[];
      removes?: string[];
      renames?: { from: string; to: string; entry?: FileEntry }[];
    } | null;
    if (!d || typeof d.dir !== "string") return;
    // 事件可能晚于导航到达：只更新“仍停留”的目录
    if (d.dir !== currentPathRef.current) return;
    // 内联重命名期间跳过（整行替换会让输入框卸载）；结束后下一次事件/TTL 补齐
    if (editingEntry) return;

    const join = (name: string) => (d.dir === "/" ? `/${name}` : `${d.dir}/${name}`);

    if (d.resync) {
      refreshAfterMutation();
      return;
    }

    const changes: FileViewChange[] = [];
    for (const entry of d.upserts ?? []) {
      if (!entry?.name || isTransferTempName(entry.name)) continue;
      changes.push({ kind: "upsert", path: join(entry.name), entry });
    }
    for (const name of d.removes ?? []) {
      if (name) changes.push({ kind: "remove", path: join(name) });
    }
    for (const r of d.renames ?? []) {
      if (!r?.from || !r?.to) continue;
      if (isTransferTempName(r.to)) continue;
      // 上传临时文件改名成正式文件：临时文件从未展示，等价于“新文件出现”
      if (isTransferTempName(r.from)) {
        if (r.entry) changes.push({ kind: "upsert", path: join(r.to), entry: r.entry });
        continue;
      }
      changes.push({ kind: "rename", from: join(r.from), to: join(r.to), entry: r.entry });
    }
    if (changes.length > 0) applyViewChanges(changes);
  }, [editingEntry, applyViewChanges, refreshAfterMutation]);

  // 订阅当前目录（非递归）；断线时传 null 退订，重连后 serverConnected 变化会重新订阅
  useFileChangesSubscription(
    serverConnected ? activeServerId : null,
    currentPath,
    false,
    handleRemoteFsChange,
    (active) => { fsWatchActiveRef.current = active; }
  );

  // Double-click entry
  // 双击/上下文菜单打开条目
  // 文件：经 FileOpener 网关探测格式后路由到对应应用（图片/PDF/文本/十六进制）
  const handleOpen = useCallback(async (entry: FileEntry) => {
    const fullPath = entryFullPath(entry);

    // 目录：保持原有导航逻辑（搜索词生效中先清空，回到常规浏览）
    if (entry.is_dir) {
      if (activeSearch) exitSearch();
      navigateTo(fullPath);
      return;
    }

    // 未连接服务器：回退旧行为（直接开编辑器，由编辑器报错）
    if (!activeServerId) {
      manager.create('editor', { preloadData: { path: fullPath, serverId: undefined } });
      return;
    }

    // 共享打开流程（openRemoteFile）：探测格式 → 决策 → 大文件确认 → 路由到对应应用窗口
    // 探测失败时函数内部回退编辑器（旧 Agent 兼容）；entry.size 供回退前的大文件确认
    const outcome = await openRemoteFile(activeServerId, fullPath, manager, entry.size);
    if (outcome.kind === 'error') {
      log.error('打开失败:', outcome.message);
      pushServerNotification(activeServerId, { title: t("common.error"), body: t("fileManager.openFailed", { message: outcome.message }), source: t("fileManager.source"), urgency: "normal" });
    }
  }, [entryFullPath, activeSearch, exitSearch, navigateTo, manager, activeServerId]);

  // ── Properties Dialog ────────────────────────────────
  const showProperties = useCallback((entry: FileEntry) => {
    setPropertiesEntry(entry);
    // 初始化可编辑状态：权限矩阵/所有者/组；每次打开重置到「常规」页；recursive 重置为不勾选
    setPermMatrix(parsePermissions(entry.permissions));
    setPermOwner(entry.owner ?? "");
    setPermGroup(entry.group ?? "");
    setPermRecursive(false);
    setPermSaving(false);
    setPermError(null);
    setPermTab('general');
  }, []);

  // 属性对话框 dirty 判定：权限矩阵或所有者/组相对初始值有变化
  // （applyProperties 与「应用」按钮禁用态共用，保证判定一致）
  const permDirty = useMemo(() => {
    if (!propertiesEntry) return { modeChanged: false, ownerChanged: false };
    const initialMode = matrixToOctal(parsePermissions(propertiesEntry.permissions));
    const newMode = matrixToOctal(permMatrix);
    const ownerChanged = permOwner !== (propertiesEntry.owner ?? "") || permGroup !== (propertiesEntry.group ?? "");
    return { modeChanged: newMode !== initialMode, ownerChanged };
  }, [propertiesEntry, permMatrix, permOwner, permGroup]);
  const permHasChanges = permDirty.modeChanged || permDirty.ownerChanged;

  // ── Properties Dialog：应用更改（chmod / chown）───────
  // 与初始值比对：权限变化 → remote_chmod；所有者/组变化 → remote_chown；都变则先后调用
  // 成功 → 关闭对话框 + 刷新目录；失败 → 对话框内联红色错误（不关闭）
  const applyProperties = useCallback(async () => {
    if (!propertiesEntry || permSaving) return;
    if (!activeServerId) {
      setPermError(t("fileManager.notConnected"));
      return;
    }
    const { modeChanged, ownerChanged } = permDirty;
    if (!modeChanged && !ownerChanged) return;  // 无更改（按钮已禁用，双保险）
    // 完整路径：搜索命中自带 path，普通浏览按当前目录拼装
    const fullPath = entryFullPath(propertiesEntry);
    setPermSaving(true);
    setPermError(null);
    try {
      if (modeChanged) {
        const ok = await invoke<boolean>('remote_chmod', {
          serverId: activeServerId,
          path: fullPath,
          mode: matrixToOctal(permMatrix),
          recursive: permRecursive,
        });
        if (!ok) { setPermError(t("fileManager.permChangeFailed")); return; }
      }
      if (ownerChanged) {
        const ok = await invoke<boolean>('remote_chown', {
          serverId: activeServerId,
          path: fullPath,
          owner: permOwner,
          group: permGroup,
          recursive: permRecursive,
        });
        if (!ok) { setPermError(t("fileManager.ownerChangeFailed")); return; }
      }
      // 成功：关闭对话框 + 用本次请求值对当前视图做增量补丁（服务器已确认成功）
      const patched: FileEntry = {
        ...propertiesEntry,
        ...(modeChanged ? { permissions: octalToPermString(matrixToOctal(permMatrix)) } : {}),
        ...(ownerChanged ? { owner: permOwner, group: permGroup } : {}),
      };
      setPropertiesEntry(null);
      applyViewChanges([{ kind: "upsert", path: fullPath, entry: patched }]);
    } catch (err) {
      log.error("应用属性失败:", err);
      setPermError(formatUnknownError(err));
    } finally {
      setPermSaving(false);
    }
  }, [propertiesEntry, activeServerId, entryFullPath, permMatrix, permOwner, permGroup, permRecursive, permSaving, permDirty, applyViewChanges, t]);

  // ── Script Run（右键菜单入口）：打开终端并注入命令 ────
  // 方案 1 设计：双击 .sh 一律编辑器查看；运行是显式意图（右键菜单选择），
  // 不再弹预览确认——PTY 输出可见、Ctrl+C 可中断、sudo 密码提示是天然确认闸
  // sudo 运行时密码由 PTY 交互输入（NOPASSWD 环境则直接执行）
  const runScriptInTerminal = useCallback((entry: FileEntry, sudo: boolean) => {
    if (!activeServerId) return;
    const fullPath = entryFullPath(entry);
    const cmd = `${sudo ? 'sudo ' : ''}bash ${fullPath}`;
    const dir = fullPath.slice(0, fullPath.lastIndexOf('/')) || '/';
    manager.create('terminal', {
      serverId: activeServerId,
      preloadData: { workingDirectory: dir, initialCommand: cmd },
    });
  }, [activeServerId, entryFullPath, manager]);

  // ── File Operations ───────────────────────────────────

  const handleRename = useCallback((entry: FileEntry) => {
    // Adwaita-style inline editing：直接在文件名上编辑
    setEditingEntry(entry);
    setEditingName(entry.name);
    setContextMenu(null);  // 关闭右键菜单
  }, []);

  // 完成重命名（inline editing）
  const finishRename = useCallback(async () => {
    if (!editingEntry) return;

    const trimmedName = editingName.trim();
    if (trimmedName === "" || trimmedName === editingEntry.name) {
      // 取消编辑
      setEditingEntry(null);
      setEditingName("");
      return;
    }

    // 构建路径（Linux 格式）：搜索命中自带 path，普通浏览按当前目录拼装
    const oldPath = entryFullPath(editingEntry);
    const parentDir = oldPath.slice(0, oldPath.lastIndexOf("/")) || "/";
    const newPath = parentDir === "/" ? `/${trimmedName}` : `${parentDir}/${trimmedName}`;

    // 重名预检：同父目录已有同名条目（排除自身）→ 提示并保留编辑态（改名或 Esc）
    const listing = editingEntry.path === undefined ? entries : (searchResults ?? []);
    const conflict = listing.some((e) =>
      e.name === trimmedName
      && !(e.name === editingEntry.name && e.path === editingEntry.path)
      && (e.path ? e.path.slice(0, e.path.lastIndexOf("/")) || "/" : currentPath) === parentDir
    );
    if (conflict) {
      pushServerNotification(activeServerId, { title: t("common.error"), body: t("fileManager.nameExists", { name: trimmedName }), source: t("fileManager.source"), urgency: "normal" });
      return;
    }

    log.debug("rename:", oldPath, "->", newPath);

    try {
      let renamed: FileEntry | null = null;
      if (activeServerId) {
        renamed = await invoke<FileEntry | null>("remote_rename", {
          serverId: activeServerId,
          oldPath,
          newPath
        });
      } else {
        pushServerNotification(activeServerId, { title: t("common.error"), body: t("fileManager.notConnected"), source: t("fileManager.source"), urgency: "normal" });
        setEditingEntry(null);
        setEditingName("");
        return;
      }

      if (renamed) {
        // 服务器确认：增量改名（浏览列表与搜索结果同步），无需整目录刷新
        applyViewChanges([{ kind: "rename", from: oldPath, to: newPath, entry: renamed }]);
        // 浏览模式保持选中新路径（对齐 Windows「重命名后条目保持选中」）；
        // 搜索模式结果重排后选中语义复杂，保持不选中
        if (editingEntry.path === undefined) setSelected([newPath]);
      } else if (editingEntry.path === undefined) {
        // 旧版 Agent 未回传条目：回退一次权威目录加载，再选中新路径
        const ok = await loadDir(parentDir);
        if (ok) setSelected([newPath]);
      } else {
        // 旧版 Agent + 搜索模式：回退重跑搜索 + 浏览缓存刷新
        refreshAfterMutation();
      }
    } catch (err) {
      log.error("rename 失败:", err);
      pushServerNotification(activeServerId, { title: t("common.error"), body: t("fileManager.renameFailed", { message: formatUnknownError(err) }), source: t("fileManager.source"), urgency: "normal" });
    }

    // 清除编辑状态
    setEditingEntry(null);
    setEditingName("");
  }, [editingEntry, editingName, entryFullPath, activeServerId, refreshAfterMutation, applyViewChanges, entries, searchResults, currentPath, loadDir, t]);

  // 取消重命名（inline editing）
  const cancelRename = useCallback(() => {
    setEditingEntry(null);
    setEditingName("");
  }, []);

  // 处理编辑输入框的键盘事件
  const handleEditKeyDown = useCallback((e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter") {
      e.preventDefault();
      e.stopPropagation();  // 阻止事件冒泡，防止触发父元素的键盘事件
      finishRename();
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();  // 阻止事件冒泡
      cancelRename();
    }
  }, [finishRename, cancelRename]);

  // ── 删除确认（应用内 ConfirmDialog，替代原生 confirm）────
  // UI 入口（右键菜单/Del 键）先 setDeleteTargets 打开确认弹窗，确认后执行；
  // 危险操作：红色警示图标 + 初始焦点在取消上（防回车误触）
  const [deleteTargets, setDeleteTargets] = useState<FileEntry[]>([]);

  // 执行删除（仅由 ConfirmDialog 确认回调调用）：批量顺序执行，逐项收集失败
  const doDelete = useCallback(async (targets: FileEntry[]) => {
    if (!activeServerId) {
      pushServerNotification(activeServerId, { title: t("common.error"), body: t("fileManager.notConnected"), source: t("fileManager.source"), urgency: "normal" });
      return;
    }

    const failed: string[] = [];
    const removed: string[] = [];
    for (const entry of targets) {
      // 构建路径（Linux 格式）：搜索命中自带 path，普通浏览按当前目录拼装
      const path = entryFullPath(entry);
      log.debug("delete:", path);
      try {
        await invoke("remote_delete", { serverId: activeServerId, path });
        removed.push(path);
      } catch (err) {
        log.error("delete 失败:", err);
        failed.push(entry.name);
      }
    }

    if (failed.length > 0) {
      pushServerNotification(activeServerId, {
        title: t("common.error"),
        body: t("fileManager.deleteFailedBatch", { n: failed.length, names: failed.slice(0, 3).join("、") }),
        source: t("fileManager.source"),
        urgency: "normal",
      });
    }
    // 服务器确认成功的条目按路径增量移除（部分失败只移除成功项），不做整目录刷新
    if (removed.length > 0) {
      applyViewChanges(removed.map((path) => ({ kind: "remove" as const, path })));
    }
  }, [entryFullPath, activeServerId, applyViewChanges, t]);

  // 刷新当前目录
  const handleRefresh = useCallback(() => {
    setContextMenu(null);
    refreshAfterMutation();
  }, [refreshAfterMutation]);

  // ── 新建（Windows 语义：立即创建默认名空条目 → 内联命名）──
  // 默认名重名时按 Windows 规则递增（新建文件夹 (2)…）；创建成功进入内联编辑态
  // （名称预选中，未改名不发 rename 请求）；Esc 取消命名但条目保留（Windows 同款）
  const createEntry = useCallback(async (kind: "folder" | "file") => {
    setContextMenu(null);
    if (!activeServerId) {
      pushServerNotification(activeServerId, { title: t("common.error"), body: t("fileManager.notConnected"), source: t("fileManager.source"), urgency: "normal" });
      return;
    }
    if (activeSearch) exitSearch();  // 搜索结果视图无处落新条目：回浏览视图在当前目录创建

    const isDir = kind === "folder";
    const baseName = isDir ? t("fileManager.newFolder") : t("fileManager.defaultNewFileName");
    const name = generateUniqueName(baseName, isDir, entries);
    const newPath = entryFullPath({ name, is_dir: isDir, size: 0, mtime: "", permissions: "" });

    log.debug("create entry:", newPath);

    try {
      // 服务器返回权威条目：直接插入视图并进入内联命名，不做整目录刷新
      const created: FileEntry | null = isDir
        ? await invoke<FileEntry | null>("remote_mkdir", { serverId: activeServerId, path: newPath })
        : (await invoke<{ entry: FileEntry | null }>("remote_write_file", {
            serverId: activeServerId,
            path: newPath,
            content: ""
          })).entry;

      if (created) {
        applyViewChanges([{ kind: "upsert", path: newPath, entry: created }]);
        setEditingEntry(created);
      } else {
        // 旧版 Agent 未回传条目：回退一次权威目录加载，成功才进编辑态
        const ok = await loadDir(currentPath);
        if (!ok) return;
        // 合成条目仅作内联编辑的 name+path 匹配键（渲染取真实条目）
        setEditingEntry({ name, is_dir: isDir, size: 0, mtime: "", permissions: "" });
      }
      setEditingName(name);
      setSelected([newPath]);
    } catch (err) {
      log.error("create entry 失败:", err);
      const body = isDir
        ? t("fileManager.createFolderFailed", { message: formatUnknownError(err) })
        : t("fileManager.createFileFailed", { message: formatUnknownError(err) });
      pushServerNotification(activeServerId, { title: t("common.error"), body, source: t("fileManager.source"), urgency: "normal" });
    }
  }, [activeServerId, activeSearch, exitSearch, entries, entryFullPath, currentPath, loadDir, applyViewChanges, t]);

  // ── 文件传输功能 ──────────────────────────────────────────

  /** Linux 文件名最大 255 字节；临时文件会追加 ".{random}.tmp" 后缀（约 13 字节），实际限制 242 */
  const MAX_NAME_BYTES = 242;

  /**
   * 以指定文件名上传单个文件到当前目录（文件选择与拖拽共用路径）
   *
   * 上传前检查远程文件是否存在，存在则询问用户是否覆盖
   */
  const uploadFileWithName = useCallback(async (localPath: string, fileName: string) => {
    if (!activeServerId) {
      pushServerNotification(activeServerId, { title: t("common.error"), body: t("fileManager.notConnected"), source: t("fileManager.source"), urgency: "normal" });
      return;
    }

    const remotePath = currentPath === "/"
      ? `/${fileName}`
      : `${currentPath}/${fileName}`;

    // 同名文件夹无法用文件覆盖（Linux 中 rename 文件到已存在目录必然 EISDIR）：
    // 直接拦截，避免传完整个文件才在落盘阶段失败。
    // 搜索结果可能来自其它目录，不能作为本目录的同名判断，此时交给 Agent 兜底拒绝。
    if (!activeSearch && entries.some((e) => e.name === fileName && e.is_dir)) {
      pushServerNotification(activeServerId, {
        title: t("common.error"),
        body: t("fileManager.uploadNameIsFolder", { name: fileName }),
        source: t("fileManager.source"),
        urgency: "normal",
      });
      return;
    }

    log.debug(`检查文件是否存在: ${remotePath}`);

    // 检查远程文件是否存在
    try {
      const fileInfo = await invoke<{ exists: boolean; size?: number; mtime?: number } | null>(
        "check_file_exists",
        { serverId: activeServerId, path: remotePath }
      );

      // 如果文件存在，询问用户是否覆盖
      if (fileInfo && fileInfo.exists) {
        const size = fileInfo.size ? formatSize(fileInfo.size) : t("fileManager.unknown");
        const mtime = fileInfo.mtime
          ? new Date(fileInfo.mtime * 1000).toLocaleString(i18n.language)
          : t("fileManager.unknown");

        const confirmed = await ask(
          t("fileManager.overwriteMessage", { name: fileName, size, mtime }),
          {
            title: t("fileManager.overwriteTitle"),
            kind: 'warning',
            okLabel: t("fileManager.yes"),
            cancelLabel: t("fileManager.no"),
          }
        );

        // 用户选择"否"，跳过该文件
        if (!confirmed) {
          log.debug(`用户取消覆盖: ${fileName}`);
          return;
        }

        log.debug(`用户确认覆盖: ${fileName}`);
      }
    } catch (checkErr) {
      // 检查失败，记录错误但继续上传（向后兼容）
      log.warn(`检查文件存在失败，直接上传:`, checkErr);
    }

    log.debug(`上传文件: ${localPath} -> ${remotePath}`);

    // 调用 Tauri 后端开始上传
    await invoke("transfer_file", {
      serverId: activeServerId,
      direction: "upload",
      remotePath,
      localPath,
    });
  }, [activeServerId, currentPath, activeSearch, entries, t]);

  /**
   * 上传单个本地文件（文件选择对话框与拖拽共用入口）
   *
   * 文件名超过 Linux 字节限制时弹出重命名对话框（用户确认后经
   * handleUploadWithNewName 续传），其余文件不受影响继续上传
   */
  const uploadSingleFile = useCallback(async (localPath: string) => {
    if (!activeServerId) {
      pushServerNotification(activeServerId, { title: t("common.error"), body: t("fileManager.notConnected"), source: t("fileManager.source"), urgency: "normal" });
      return;
    }

    const fileName = localPath.split(/[\\/]/).pop() || 'unknown';

    // 检查文件名长度（Linux 最大 255 字节，预留临时后缀）
    const fileNameBytes = new TextEncoder().encode(fileName).length;
    if (fileNameBytes > MAX_NAME_BYTES) {
      // 文件名超长，弹出重命名对话框；已有对话框打开时跳过该文件（一次只处理一个）
      log.warn(`文件名超长（${fileNameBytes} 字节），需重命名: ${fileName}`);
      setRenameDialog(prev => prev ? prev : {
        localPath,
        originalName: fileName,
        newName: fileName, // 显示完整原始文件名
        maxBytes: MAX_NAME_BYTES,
      });
      return;
    }

    await uploadFileWithName(localPath, fileName);
  }, [activeServerId, uploadFileWithName]);

  /**
   * 处理文件上传（右键菜单 / 工具栏入口）
   *
   * 触发 Windows 文件选择对话框，支持多选；
   * 逐个走 uploadSingleFile（含重命名检查与覆盖确认）
   */
  const handleUpload = useCallback(async () => {
    // 检查是否有活跃服务器
    if (!activeServerId) {
      pushServerNotification(activeServerId, { title: t("common.error"), body: t("fileManager.notConnected"), source: t("fileManager.source"), urgency: "normal" });
      return;
    }

    try {
      // 打开 Windows 文件选择对话框
      const selectedFiles = await open({
        multiple: true,  // 支持多选
        directory: false, // 选择文件（不是文件夹）
        title: t("fileManager.uploadTitle"),
        filters: [
          { name: t("fileManager.filterAllFiles"), extensions: ['*'] },
          { name: t("fileManager.filterTextFiles"), extensions: ['txt', 'md', 'json', 'toml', 'yaml', 'yml'] },
          { name: t("fileManager.filterScripts"), extensions: ['sh', 'py', 'js', 'ts', 'rs'] },
        ],
      });

      // 用户取消选择
      if (!selectedFiles) return;

      // selectedFiles 是字符串数组（多选）或字符串（单选）
      const files = Array.isArray(selectedFiles) ? selectedFiles : [selectedFiles];

      // 为每个文件创建传输任务（单个文件失败不影响其余文件）
      for (const localPath of files) {
        try {
          await uploadSingleFile(localPath);
        } catch (err) {
          log.error(`上传失败: ${localPath}`, err);
        }
      }

      // 关闭右键菜单
      setContextMenu(null);
    } catch (err) {
      log.error('上传失败:', err);
      pushServerNotification(activeServerId, { title: t("common.error"), body: t("fileManager.uploadFailed", { message: formatUnknownError(err) }), source: t("fileManager.source"), urgency: "normal" });
    }
  }, [activeServerId, uploadSingleFile]);

  /**
   * 处理拖拽上传（Tauri webview 级拖放事件）
   *
   * 拖放事件只提供本地路径（不区分文件/目录），目录无法按文件传输，
   * 调用 Rust 端 local_path_is_file 过滤后再逐个上传
   */
  const handleFilesDropped = useCallback(async (paths: string[]) => {
    if (!activeServerId) {
      pushServerNotification(activeServerId, { title: t("common.error"), body: t("fileManager.notConnected"), source: t("fileManager.source"), urgency: "normal" });
      return;
    }
    if (paths.length === 0) return;

    log.info(`拖拽上传 ${paths.length} 个文件到 ${currentPath}`);

    for (const localPath of paths) {
      try {
        // 过滤目录与不存在的路径（拖入文件夹时给出明确提示而非创建必败任务）
        const isFile = await invoke<boolean>("local_path_is_file", { path: localPath });
        if (!isFile) {
          const name = localPath.split(/[\\/]/).pop() || localPath;
          log.warn(`拖拽项不是文件，已跳过: ${name}`);
          pushServerNotification(activeServerId, { title: t("common.error"), body: t("fileManager.folderUploadUnsupported", { name }), source: t("fileManager.source"), urgency: "normal" });
          continue;
        }
        await uploadSingleFile(localPath);
      } catch (err) {
        log.error(`拖拽上传失败: ${localPath}`, err);
      }
    }
  }, [activeServerId, currentPath, uploadSingleFile]);

  /**
   * 处理重命名后的文件上传
   *
   * 用户在重命名弹窗中确认新文件名后，使用新名称继续上传
   */
  const handleUploadWithNewName = useCallback(async (localPath: string, newName: string) => {
    // 检查是否有活跃服务器
    if (!activeServerId) {
      pushServerNotification(activeServerId, { title: t("common.error"), body: t("fileManager.notConnected"), source: t("fileManager.source"), urgency: "normal" });
      return;
    }

    try {
      await uploadFileWithName(localPath, newName);

      // 关闭右键菜单
      setContextMenu(null);
    } catch (err) {
      log.error('上传失败:', err);
      pushServerNotification(activeServerId, { title: t("common.error"), body: t("fileManager.uploadFailed", { message: formatUnknownError(err) }), source: t("fileManager.source"), urgency: "normal" });
    }
  }, [activeServerId, uploadFileWithName]);

  // 拖拽上传监听：Tauri v2 webview 级拖放（HTML5 拖放事件被 dragDropEnabled 拦截，
  // 系统文件拖入时以 tauri://drag-* 事件提供文件路径，天然避开浏览器安全限制）
  // 回调经 ref 转发，目录切换（currentPath 变化）时无需重注册监听
  const uploadFnsRef = useRef({ uploadSingleFile, handleFilesDropped });
  useEffect(() => {
    uploadFnsRef.current = { uploadSingleFile, handleFilesDropped };
  }, [uploadSingleFile, handleFilesDropped]);

  useEffect(() => {
    // 未连接服务器或窗口非活动（最小化/失焦）时不响应拖拽（不显示覆盖层、不注册监听）
    if (!activeServerId || !isWindowActive) return;

    let disposed = false;
    let unlisten: (() => void) | null = null;

    getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type === 'enter' || event.payload.type === 'over') {
        // enter 携带 paths：空数组表示非文件拖拽（如窗口内文本），不显示覆盖层
        if (event.payload.type === 'enter' && event.payload.paths.length === 0) return;
        setDragActive(true);
      } else if (event.payload.type === 'leave') {
        setDragActive(false);
      } else if (event.payload.type === 'drop') {
        setDragActive(false);
        const paths = event.payload.paths;
        if (paths.length === 0) return;
        // 异步处理：事件回调内不 await，避免阻塞 webview 事件循环
        void uploadFnsRef.current.handleFilesDropped(paths);
      }
    }).then((fn) => {
      if (disposed) {
        fn(); // effect 已卸载（如断开连接/窗口失焦），立即注销
      } else {
        unlisten = fn;
      }
    }).catch((e) => {
      log.error("注册拖放监听失败:", e);
    });

    return () => {
      disposed = true;
      unlisten?.();
      setDragActive(false); // 兜底清除覆盖层
    };
  }, [activeServerId, isWindowActive]);

  /**
   * 处理文件下载
   *
   * 触发 Windows 保存对话框
   */
  const handleDownload = useCallback(async (entry: FileEntry) => {
    // 检查是否有活跃服务器
    if (!activeServerId) {
      pushServerNotification(activeServerId, { title: t("common.error"), body: t("fileManager.notConnected"), source: t("fileManager.source"), urgency: "normal" });
      return;
    }

    // 文件夹不能下载
    if (entry.is_dir) {
      pushServerNotification(activeServerId, { title: t("common.error"), body: t("fileManager.folderDownloadUnsupported"), source: t("fileManager.source"), urgency: "normal" });
      return;
    }

    try {
      // 构建远程文件路径（搜索命中自带 path，普通浏览按当前目录拼装）
      const remotePath = entryFullPath(entry);

      log.info(`下载文件: ${remotePath}`);

      // 打开 Windows 保存对话框
      const localPath = await save({
        defaultPath: entry.name,  // 默认文件名
        title: t("fileManager.saveTitle"),
        filters: [
          { name: t("fileManager.filterAllFiles"), extensions: ['*'] },
        ],
      });

      // 用户取消选择
      if (!localPath) return;

      // 调用 Tauri 后端开始下载
      await invoke("transfer_file", {
        serverId: activeServerId,
        direction: "download",
        remotePath,
        localPath,
      });

      // 关闭右键菜单
      setContextMenu(null);
    } catch (err) {
      log.error('下载失败:', err);
      pushServerNotification(activeServerId, { title: t("common.error"), body: t("fileManager.downloadFailed", { message: formatUnknownError(err) }), source: t("fileManager.source"), urgency: "normal" });
    }
  }, [activeServerId, entryFullPath]);

  // ── Context Menu ─────────────────────────────────────
  // 右键菜单：坐标 = 鼠标右下方 +3 紧贴；targets = 作用条目（null = 空白菜单）。
  // 视口越界翻转与钳制由共享 ContextMenu 组件内部处理（渲染后实测尺寸校正）
  // 右键仅作用于已选中项（菜单作用于整个选区）；未选中位置一律弹空白菜单且
  // 不改变选中——右键不是选择工具，误触不丢当前选区（原设计行为）
  const handleContextMenu = useCallback((e: React.MouseEvent, entry: FileEntry) => {
    e.preventDefault();
    e.stopPropagation();

    const path = entryFullPath(entry);
    if (selectedSet.has(path)) {
      setContextMenu({ x: e.clientX + 3, y: e.clientY + 3, targets: selectedEntries });
    } else {
      setContextMenu({ x: e.clientX + 3, y: e.clientY + 3, targets: null });
    }
  }, [selectedSet, selectedEntries, entryFullPath]);

  // 空白区域右键菜单
  const handleEmptyContextMenu = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    setContextMenu({ x: e.clientX + 3, y: e.clientY + 3, targets: null });
  }, []);

  // 框选（marquee）状态机：按下→追踪→命中检测→抬起，Ctrl/⌘ 反选在内
  // 框选条目盒提供者（审计 M25）：虚拟化下未挂载行不在 DOM——列表按
  // 行高几何计算全量矩形（含滚出视口的行，框选跨屏滚动也正确）；
  // 网格暂回退 DOM 查询（其窗口化接入后切换几何）
  const marqueeRectsProvider = useCallback((): EntryBox[] | null => {
    if (viewMode === "list" && listRowH > 0) {
      const cont = listContentRef.current;
      if (!cont) return null;
      const crect = cont.getBoundingClientRect();
      return visibleEntries.map((e) => ({
        path: entryFullPath(e),
        rect: {
          left: crect.left,
          right: crect.right,
          // 行步进含 row-gap（与虚拟窗口同口径）；content 的 rect 随外层
          // .fm-list 滚动一起移动，故无需再减 scrollTop
          top: crect.top + visibleEntries.indexOf(e) * listRowStride,
          bottom: crect.top + (visibleEntries.indexOf(e) + 1) * listRowStride,
        },
      }));
    }
    if (viewMode === "grid" && gridCols > 0 && gridItemH > 0 && gridTrackW.length === gridCols) {
      const cont = gridRef.current;
      if (!cont) return null;
      const crect = cont.getBoundingClientRect();
      const padX = 4; // .fm-grid padding 4px
      const gap = 8;
      return visibleEntries.map((e, i) => {
        const r = Math.floor(i / gridCols);
        const c = i % gridCols;
        return {
          path: entryFullPath(e),
          rect: {
            left: crect.left + padX + c * (gridTrackW[c] + gap),
            right: crect.left + padX + c * (gridTrackW[c] + gap) + gridTrackW[c],
            top: crect.top + padX + r * (gridItemH + gap) - cont.scrollTop,
            bottom: crect.top + padX + r * (gridItemH + gap) - cont.scrollTop + gridItemH,
          },
        };
      });
    }
    return null; // 回退 DOM 查询
  }, [viewMode, visibleEntries, listRowH, entryFullPath, gridCols, gridItemH, gridTrackW]);

  const { marquee, beginMarquee, justEndedRef: marqueeJustEndedRef } = useMarquee({
    editingEntry,
    selected,
    setSelected,
    rectsProvider: marqueeRectsProvider,
  });

  // 点击空白区域取消选中（框选刚结束时跳过一次：click 事件跟在 mouseup
  // 之后触发，会误清刚框选出的选区）
  const handleEmptyClick = useCallback((e: React.MouseEvent) => {
    const target = e.target as HTMLElement;
    if (target.closest(".fm-list-row") || target.closest(".fm-grid-item")) return;
    if (marqueeJustEndedRef.current) {
      marqueeJustEndedRef.current = false;
      return;
    }
    clearSelection();
  }, []);

  /** 行单击（列表/网格共用）：Ctrl 切换 / Shift 范围 / 普通单击；
   *  单击已选中项延迟 200ms 触发重命名（防双击误触，Adwaita 惯例） */
  const handleRowClick = useCallback((e: React.MouseEvent, entry: FileEntry, idx: number) => {
    if (clickTimerRef.current) {
      clearTimeout(clickTimerRef.current);
      clickTimerRef.current = null;
    }

    const path = entryFullPath(entry);
    const editingThis = editingEntry?.name === entry.name && editingEntry?.path === entry.path;

    if (e.ctrlKey || e.metaKey) {
      setSelected((prev) => toggleSelection(prev, path));
      setAnchorPath(path);
      setFocusIdx(idx);
      return;
    }
    if (e.shiftKey) {
      // Shift 范围选：锚点到当前行（无锚点回退单选当前行）
      setSelected(rangeSelection(visiblePaths, anchorPath ?? path, path));
      setFocusIdx(idx);
      return;
    }
    if (selected.length === 1 && selected[0] === path && !editingThis) {
      clickTimerRef.current = setTimeout(() => {
        handleRename(entry);
        clickTimerRef.current = null;
      }, 200);  // 200ms 延迟，确保双击不会触发重命名
    } else {
      setSelected([path]);
      setAnchorPath(path);
      setFocusIdx(idx);
    }
  }, [entryFullPath, editingEntry, visiblePaths, anchorPath, selected]);

  /** 行双击（列表/网格共用）：清定时器防误触重命名，打开条目 */
  const handleRowDoubleClick = useCallback((entry: FileEntry) => {
    if (clickTimerRef.current) {
      clearTimeout(clickTimerRef.current);
      clickTimerRef.current = null;
    }
    handleOpen(entry);
  }, [handleOpen]);

  /** 移动/复制单项结果补丁：移动移除源、目标按服务器条目新增/覆盖 */
  const applyTransferResult = useCallback((mode: 'move' | 'copy', src: string, dst: string, entry: FileEntry) => {
    const changes: FileViewChange[] = mode === 'move'
      ? [{ kind: 'remove', path: src }, { kind: 'upsert', path: dst, entry }]
      : [{ kind: 'upsert', path: dst, entry }];
    applyViewChanges(changes);
  }, [applyViewChanges]);

  // ── 拖拽移动与应用内剪贴板（hook：状态机 + 批量执行器，详见 useFileTransfer）──
  const {
    clip, setClip,
    dropTargetPath,
    transferConfirm, setTransferConfirm,
    riskConfirm, setRiskConfirm,
    checkConflictsAndExec,
    execTransfer,
    handlePaste,
    dragGhost, ghostPos,
    handleRowMouseDown,
  } = useFileTransfer({
    activeServerId,
    currentPath,
    selected,
    refreshAfterMutation,
    applyTransferResult,
    entryFullPath,
    navigateTo,
    editingEntry,
    setSelected,
    setAnchorPath,
  });

  /** 拖拽幽灵的图标描述：路径 → { 图标, 名称 }（最多 3 个，叠放展示；
   *  不在可见列表的路径按文件名回退图标） */
  const describeGhostItems = useCallback((paths: string[]) => {
    const byPath = new Map(visibleEntries.map((e) => [entryFullPath(e), e]));
    return paths.slice(0, 3).map((p) => {
      const e = byPath.get(p);
      return {
        icon: e ? getFileIcon(e) : getFileIconForName(entryName(p)),
        name: e?.name ?? entryName(p),
      };
    });
  }, [visibleEntries, entryFullPath]);

  /** 右键菜单项：空白菜单（刷新/新建/上传）与文件菜单（作用于选区，多选批量） */
  const contextMenuItems: ContextMenuEntry[] = useMemo(() => {
    if (!contextMenu) return [];
    const targets = contextMenu.targets;
    if (!targets || targets.length === 0) {
      return [
        { label: t("fileManager.ctxRefresh"), icon: <AppIcon name="monitor" size={14} />, onClick: handleRefresh },
        { separator: true },
        { label: t("fileManager.newFolder"), icon: <AppIcon name="folder-plus" size={14} />, onClick: () => createEntry("folder") },
        { label: t("fileManager.ctxNewFile"), icon: <AppIcon name="file" size={14} />, onClick: () => createEntry("file") },
        { label: t("fileManager.ctxUpload"), icon: <AppIcon name="file-code" size={14} />, onClick: handleUpload },
        { separator: true },
        { label: t("fileManager.ctxPaste"), shortcut: "Ctrl+V", disabled: !clip, onClick: handlePaste },
      ];
    }

    const single = targets.length === 1 ? targets[0] : null;
    const items: ContextMenuEntry[] = [];
    if (single) {
      items.push({ label: t("fileManager.ctxOpen"), icon: <AppIcon name="folder-open" size={14} />, onClick: () => handleOpen(single) });
      // 脚本运行（仅 .sh 文件）：双击=编辑器查看（方案 1），运行收右键显式入口
      if (!single.is_dir && single.name.toLowerCase().endsWith(".sh")) {
        items.push(
          { label: t("fileManager.ctxRunInTerminal"), icon: <AppIcon name="terminal" size={14} />, onClick: () => runScriptInTerminal(single, false) },
          { label: t("fileManager.ctxRunWithSudo"), icon: <AppIcon name="lock" size={14} />, onClick: () => runScriptInTerminal(single, true) },
        );
      }
      items.push({ separator: true });
      items.push({ label: t("fileManager.ctxRename"), icon: <AppIcon name="editor" size={14} />, onClick: () => handleRename(single) });
    }

    // 剪切/复制/粘贴（应用内剪贴板）：作用于选区，粘贴到当前目录
    items.push({ separator: true });
    items.push({
      label: t("fileManager.ctxCut"), shortcut: "Ctrl+X",
      onClick: () => setClip({ mode: "cut", paths: targets.map((en) => entryFullPath(en)) }),
    });
    items.push({
      label: t("fileManager.ctxCopy"), shortcut: "Ctrl+C",
      onClick: () => setClip({ mode: "copy", paths: targets.map((en) => entryFullPath(en)) }),
    });
    items.push({ label: t("fileManager.ctxPaste"), shortcut: "Ctrl+V", disabled: !clip, onClick: handlePaste });

    // 批量删除（Del 键同入口）：菜单标签附数量，确认弹窗列明
    items.push({
      label: targets.length > 1 ? `${t("fileManager.ctxDelete")} (${targets.length})` : t("fileManager.ctxDelete"),
      icon: <AppIcon name="trash" size={14} />,
      danger: true,
      shortcut: "Del",
      onClick: () => setDeleteTargets(targets),
    });

    items.push({ separator: true });
    items.push({
      // 复制完整路径到剪贴板（多选逐行换行拼接）
      label: t("fileManager.ctxCopyPath"),
      icon: <AppIcon name="file-text" size={14} />,
      onClick: () => navigator.clipboard.writeText(targets.map((en) => entryFullPath(en)).join("\n")),
    });
    if (single && !single.is_dir) {
      items.push({ label: t("fileManager.ctxDownload"), icon: <AppIcon name="file-archive" size={14} />, onClick: () => handleDownload(single) });
    }
    if (single) {
      items.push({ separator: true });
      items.push({ label: t("fileManager.ctxProperties"), icon: <AppIcon name="about" size={14} />, onClick: () => showProperties(single) });
    }
    return items;
  }, [contextMenu, t, entryFullPath, clip, handleOpen, handlePaste, handleRename, handleDownload, showProperties, handleRefresh, createEntry, handleUpload]);

  // ── Keyboard ─────────────────────────────────────────
  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    // 焦点在任何输入框（常驻搜索框/路径编辑/重命名）时：按键由输入框自身
    // 处理，不透传到目录键盘导航（否则 Backspace 删字会误触 goUp、
    // Delete 会误删选中文件）
    if (e.target instanceof HTMLInputElement) return;

    // 如果正在编辑路径，不处理任何全局键盘事件
    if (isEditingPath) return;

    // 如果正在编辑文件名，不处理任何键盘事件（编辑输入框会处理）
    if (editingEntry) return;

    // 表头下拉菜单打开时：Esc 关闭，其余按键不透传（菜单为当前键盘焦点层）
    if (headerMenu) {
      if (e.key === "Escape") setHeaderMenu(null);
      return;
    }

    // Ctrl/⌘+A 全选可见条目（路径制选中，跨筛选有效）
    if ((e.ctrlKey || e.metaKey) && (e.key === "a" || e.key === "A")) {
      e.preventDefault();
      setSelected(visiblePaths);
      return;
    }

    // Ctrl/⌘+C/X/V：应用内剪贴板（复制/剪切选区、粘贴到当前目录）
    if ((e.ctrlKey || e.metaKey) && (e.key === "c" || e.key === "C")) {
      if (selected.length > 0) setClip({ mode: "copy", paths: [...selected] });
      return;
    }
    if ((e.ctrlKey || e.metaKey) && (e.key === "x" || e.key === "X")) {
      if (selected.length > 0) setClip({ mode: "cut", paths: [...selected] });
      return;
    }
    if ((e.ctrlKey || e.metaKey) && (e.key === "v" || e.key === "V")) {
      if (clip) handlePaste();
      return;
    }

    // Del 批量删除选区（走确认弹窗）；Esc 清空选区
    if (e.key === "Delete" && selected.length > 0) {
      e.preventDefault();
      setDeleteTargets(selectedEntries);
      return;
    }
    if (e.key === "Escape") {
      clearSelection();
      return;
    }

    if (e.key === "Backspace") { e.preventDefault(); goUp(); }
    if (e.key === "Enter" && focusIdx >= 0 && focusIdx < visibleEntries.length) {
      handleOpen(visibleEntries[focusIdx]);
    }
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (visibleEntries.length === 0) return;
      const delta = e.key === "ArrowDown" ? 1 : -1;
      // 空选区时 ↓ 从首项进入、↑ 停在首项
      const base = focusIdx < 0 ? (delta > 0 ? -1 : 0) : focusIdx;
      const next = Math.max(0, Math.min(base + delta, visibleEntries.length - 1));
      setFocusIdx(next);
      // 焦点行/卡片跟随进可视区（虚拟化下未挂载行不在 DOM，scrollTop 数学）
      if (viewMode === "list") {
        listWin.ensureVisible(next);
      } else if (viewMode === "grid" && gridCols > 0) {
        gridWin.ensureVisible(Math.floor(next / gridCols));
      }
      const nextPath = visiblePaths[next];
      if (e.shiftKey) {
        // Shift 扩选：锚点不动（无锚点回退目标自身 = 单选）
        setSelected(rangeSelection(visiblePaths, anchorPath ?? nextPath, nextPath));
      } else {
        // 普通方向键：折叠为单选并重置锚点（Windows Explorer 惯例）
        setSelected([nextPath]);
        setAnchorPath(nextPath);
      }
    }
  }, [visibleEntries, visiblePaths, focusIdx, anchorPath, selected, selectedEntries, clip, goUp, handleOpen, handlePaste, editingEntry, isEditingPath, headerMenu]);

  // ── 监听上传完成事件 ─────────────────────────────────
  // 目录监听生效时跳过（最终条目由 file_changes 增量推送，省每文件一次 RPC）；
  // 未生效（旧 Agent/无权限）时定向探测该文件条目并增量插入，取不到条目再
  // 回退一次权威目录刷新（不做逐文件整目录读取）
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let disposed = false; // setupListener 是异步的：cleanup 先于 resolve 时补注销，防监听器泄漏

    const setupListener = async () => {
      const { listen } = await import('@tauri-apps/api/event');
      const fn = await listen<{ remote_path: string; dir_path: string; file_name: string }>('upload-completed', (event) => {
        const { remote_path, dir_path } = event.payload;
        if (!activeServerId) return;
        // 事件可能晚于导航到达：只处理“仍停留”的目录
        if (dir_path !== currentPathRef.current) return;
        // 目录监听已生效：watch 的 rename 增量会补上正式条目
        if (fsWatchActiveRef.current) return;

        void (async () => {
          try {
            const info = await invoke<{ entry: FileEntry | null }>('remote_file_info', {
              serverId: activeServerId,
              path: remote_path,
            });
            if (info.entry) {
              applyViewChanges([{ kind: 'upsert', path: remote_path, entry: info.entry }]);
              return;
            }
          } catch (e) {
            log.error('上传完成元数据探测失败:', e);
          }
          // 旧版 Agent 未回传条目：回退一次权威目录刷新（读此刻目录，防拉回旧目录）
          refreshAfterMutation();
        })();
      });
      if (disposed) {
        fn(); // effect 已清理：立即注销本次注册
      } else {
        unlisten = fn;
      }
    };

    setupListener().catch((e) => log.error('上传完成事件监听设置失败:', e));

    return () => {
      disposed = true;
      if (unlisten) {
        unlisten();
      }
    };
  }, [activeServerId, applyViewChanges, refreshAfterMutation]);

  // ── Stats（搜索词生效中统计命中结果，否则统计当前目录）──
  const statusEntries = activeSearch ? (searchResults ?? []) : entries;
  const dirCount = statusEntries.filter(e => e.is_dir).length;
  const fileCount = statusEntries.filter(e => !e.is_dir).length;
  const totalSize = statusEntries.filter(e => !e.is_dir).reduce((s, e) => s + e.size, 0);

  // 搜索视图的内容区占位（搜索中/错误/无命中）；有结果时 null → 走列表/网格渲染
  const searchEmptyState = activeSearch ? (
    searching ? (
      <div className="fm-empty">
        <div className="fm-empty-icon"><AppIcon name="search" size={40} /></div>
        <div className="fm-empty-text">{t("fileManager.searching")}</div>
      </div>
    ) : searchError ? (
      <div className="fm-empty">
        <div className="fm-empty-icon"><AppIcon name="about" size={40} /></div>
        <div className="fm-empty-text">{searchError}</div>
      </div>
    ) : (searchResults?.length ?? 0) === 0 ? (
      <div className="fm-empty" onContextMenu={handleEmptyContextMenu} onClick={handleEmptyClick} onMouseDown={beginMarquee}>
        <div className="fm-empty-icon"><AppIcon name="search" size={40} /></div>
        <div className="fm-empty-text">{t("fileManager.searchNoResults")}</div>
      </div>
    ) : null
  ) : null;

  return (
    <div className="fm-app" onKeyDown={handleKeyDown} tabIndex={0}>
      {/* 拖拽上传覆盖层：文件拖入窗口时提示上传目标目录（pointer-events:none 不拦截释放） */}
      {dragActive && (
        <div className="fm-drop-overlay">
          <div className="fm-drop-overlay-card">
            <div className="fm-drop-overlay-icon">⬆️</div>
            <div className="fm-drop-overlay-title">{t("fileManager.dropToUpload")}</div>
            <div className="fm-drop-overlay-path">{currentPath}</div>
          </div>
        </div>
      )}
      {/* ✅ 使用AppLayout抽象层，简化CSS层级 */}
      <AppLayout
        sidebar={
          /* Sidebar */
          <div className="fm-sidebar">
            {sidebarSections.map((section, sectionIdx) => (
              <div key={sectionIdx} className="fm-sidebar-section">
                <div className="fm-sidebar-section-title">{section.titleKey ? t(section.titleKey) : section.title}</div>
                {section.items.map((item, itemIdx) => {
                  // 书签/挂载点可作拖拽放置目标（网络项非本机路径不接）
                  const droppable = item.type === "bookmark" || item.type === "mount";
                  const isDropTarget = droppable && dropTargetPath === item.path;
                  return (
                  <div
                    key={`${sectionIdx}-${itemIdx}`}
                    className={`fm-sidebar-item${currentPath === item.path ? " fm-sidebar-item-active" : ""}${isDropTarget ? " drop-target" : ""}`}
                    data-drop-dir={droppable ? item.path : undefined}
                    onClick={() => navigateTo(item.path)}
                  >
                    <span className="fm-si-icon"><AppIcon name={item.icon} size={16} /></span>
                    <span className="fm-si-label">{item.labelKey ? t(item.labelKey) : item.label}</span>
                  </div>
                  );
                })}
              </div>
            ))}
          </div>
        }
        toolbar={
          /* Toolbar - 原HeaderBar功能移到这里 */
          <div className="fm-toolbar">
            {/* Navigation */}
            <div className="fm-toolbar-nav">
              <button className="fm-toolbar-btn" onClick={goBack} disabled={historyIdx <= 0} title={t("fileManager.navBack")}>←</button>
              <button className="fm-toolbar-btn" onClick={goForward} disabled={historyIdx >= history.length - 1} title={t("fileManager.navForward")}>→</button>
              <button className="fm-toolbar-btn" onClick={goUp} title={t("fileManager.navUp")}>↑</button>
            </div>

            {/* Path Input */}
            <div className="fm-toolbar-path">
              {/* 面包屑导航：始终可见（搜索框在地址栏右侧，不再替换地址栏） */}
              {!isEditingPath && (
                <div
                  className="fm-breadcrumb"
                  onClick={() => {
                    setIsEditingPath(true);
                    // 延迟聚焦，等待输入框显示
                    setTimeout(() => pathInputRef.current?.focus(), 0);
                  }}
                >
                  {parsePathSegments(currentPath).map((segment, index, array) => {
                    const isLast = index === array.length - 1;
                    return (
                      <React.Fragment key={segment.path}>
                        <button
                          className={`fm-breadcrumb-segment ${isLast ? 'fm-crumb-current' : ''}${dropTargetPath === segment.path ? ' drop-target' : ''}`}
                          data-drop-dir={segment.path}
                          data-spring=""
                          onClick={(e) => {
                            e.stopPropagation();
                            handleBreadcrumbClick(segment.path);
                          }}
                          title={segment.path}
                        >
                          {segment.name}
                        </button>
                        {/* 根目录后不显示分隔符，其他分段之间显示分隔符（除了最后一个） */}
                        {index >= 1 && index < array.length - 1 && (
                          <span className="fm-breadcrumb-separator">▸</span>
                        )}
                      </React.Fragment>
                    );
                  })}
                </div>
              )}

              {/* 路径输入框：编辑模式显示 */}
              <input
                ref={pathInputRef}
                type="text"
                className={`fm-path-input ${isEditingPath ? 'visible' : ''}`}
                value={pathInput}
                onChange={handlePathInputChange}
                onKeyDown={handlePathInputKeyDown}
                onFocus={handlePathInputFocus}
                onBlur={handlePathInputBlur}
                placeholder="/home"
                title={t("fileManager.addressHint")}
              />

              {/* Suggestions Dropdown */}
              {showSuggestions && suggestions.length > 0 && (
                <div className="fm-suggestions">
                  {suggestions.map((suggestion, idx) => (
                    <div
                      key={idx}
                      className={`fm-suggestion-item${selectedSuggestionIdx === idx ? " selected" : ""}`}
                      onMouseDown={(e) => {
                        e.preventDefault();
                        handleSelectSuggestion(suggestion);
                      }}
                      onMouseEnter={() => setSelectedSuggestionIdx(idx)}
                    >
                      <span className="fm-suggestion-icon"><AppIcon name="folder" size={14} /></span>
                      <span className="fm-suggestion-text">{suggestion}</span>
                    </div>
                  ))}
                </div>
              )}
            </div>

            {/* Search Box（地址栏右侧常驻，Ctrl+F 聚焦；下钻层数经右缘 ⌄ 菜单可调，
                菜单走 portal 不占布局，触发钮仅 20px） */}
            <div className="fm-search-bar">
              <span className="fm-search-icon"><AppIcon name="search" size={14} /></span>
              <input
                ref={searchInputRef}
                className="fm-search-input"
                value={searchInput}
                onChange={(e) => setSearchInput(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Escape") {
                    e.stopPropagation();
                    exitSearch();
                  }
                }}
                placeholder={fileManagerSearchDepth > 0
                  ? t("fileManager.searchPlaceholderDepth", { n: fileManagerSearchDepth })
                  : t("fileManager.searchPlaceholder")}
                disabled={!activeServerId}
              />
              {searchInput && (
                <button className="fm-search-exit" onClick={exitSearch} title={t("fileManager.exitSearch")}>✕</button>
              )}
              <ToolbarMenu
                icon="⌄"
                title={`${t("fileManager.searchScope")}：${searchScopeLabel}`}
                items={searchScopeItems}
              />
            </div>

            {/* Actions */}
            <div className="fm-toolbar-actions">
              <button
                className="fm-toolbar-btn"
                onClick={() => createEntry("folder")}
                title={t("fileManager.newFolder")}
                disabled={!activeServerId}
              ><AppIcon name="folder-plus" size={15} /></button>
            </div>

            {/* View Toggle */}
            <div className="fm-toolbar-view">
              <button
                className={`fm-toolbar-btn ${viewMode === "list" ? "active" : ""}`}
                onClick={() => setViewMode("list")}
                title={t("fileManager.viewList")}
              >☰</button>
              <button
                className={`fm-toolbar-btn ${viewMode === "grid" ? "active" : ""}`}
                onClick={() => setViewMode("grid")}
                title={t("fileManager.viewGrid")}
              >⊞</button>
            </div>
          </div>
        }
      >
        {/* File List Content（AppLayout自动处理滚动） */}
        {isOffline ? (
          /* ── 离线空状态：服务器列表登录界面 ───────────── */
          <div className="fm-offline">
            <div className="offline-icon">
              <SymbolicIcon name="network-offline" size={32} />
            </div>
            <div className="offline-title">{t("fileManager.offlineTitle")}</div>
            <div className="offline-desc">{t("fileManager.offlineDesc")}</div>

              {servers.length > 0 ? (
                <div className="offline-servers">
                  <div className="offline-servers-label">{t("fileManager.availableServers")}</div>
                  {servers.map((server) => (
                    <button
                      key={server.id}
                      className="offline-server-btn"
                      onClick={() => connectServer(server.id)}
                      disabled={server.status === "connecting"}
                    >
                      <span className="osb-status">
                        <span className={`osb-dot osb-dot-${server.status}`} />
                      </span>
                      <span className="osb-info">
                        <span className="osb-name">{server.name || server.host}</span>
                        <span className="osb-host">{server.host}:{server.port}</span>
                      </span>
                      <span className="osb-action">
                        {server.status === "connecting" ? t("fileManager.connecting") : t("fileManager.connect")}
                      </span>
                    </button>
                  ))}
                </div>
              ) : (
                <div className="offline-no-servers">
                  <span>{t("fileManager.noServers")}</span>
                  <span>{t("fileManager.goAddServer")}</span>
                </div>
              )}
            </div>
        ) : loading && !activeSearch ? (
            /* 目录加载 spinner：搜索词生效中不显示（手动刷新/兜底重载的后台
               加载不打断搜索视图，搜索进度由 searchEmptyState 呈现） */
            <div className="fm-loading">
              <div className="spinner" />
              {t("common.loading")}
            </div>
          ) : searchEmptyState ? (
            /* 搜索视图占位（搜索中/错误/无命中）优先于目录浏览态 */
            searchEmptyState
          ) : !activeSearch && error ? (
            <div className="fm-empty">
              <div className="fm-empty-icon"><AppIcon name="about" size={40} /></div>
              <div className="fm-empty-text">{error}</div>
            </div>
          ) : !activeSearch && entries.length === 0 ? (
            <div
              className="fm-empty"
              onContextMenu={handleEmptyContextMenu}
              onClick={handleEmptyClick}
            >
              <div className="fm-empty-icon"><AppIcon name={permissionDenied ? "lock" : "folder-open"} size={40} /></div>
              <div className="fm-empty-text">{permissionDenied ? t("fileManager.noPermission") : t("fileManager.emptyDir")}</div>
            </div>
          ) : visibleEntries.length === 0 ? (
            /* ── 筛选无匹配：目录非空，但当前列筛选下无可见项
                  （无筛选时不可达：filterByColumnFilters 无勾选原样返回，
                  真空目录由上面的 entries.length === 0 分支呈现）── */
            <div
              className="fm-empty"
              onContextMenu={handleEmptyContextMenu}
              onClick={handleEmptyClick}
            >
              <div className="fm-empty-icon"><AppIcon name="search" size={40} /></div>
              <div className="fm-empty-text">{t("fileManager.noFilterMatch")}</div>
            </div>
          ) : viewMode === "list" ? (
            <div
              className={`fm-list${hoveredCol ? ` col-hover-${hoveredCol}` : ""}`}
              ref={listAreaRef}
              style={{ "--fm-cols": buildGridTemplate(columnWidths) } as React.CSSProperties}
            >
              {/* ✅ 表头固定（flex-shrink: 0），不参与滚动
                  点击列头翻转排序；列间手柄拖拽调宽（宽度经 --fm-cols 下发给表头与行） */}
              <div className="fm-list-header">
                {FILE_COLUMNS.map(col => {
                  // 该列筛选是否激活（name/size/mtime/type 有筛选区）：列头显示漏斗指示
                  const filterActive = col.key === "size"
                    ? columnFilters.size.length > 0
                    : col.key === "mtime" ? columnFilters.mtime.length > 0
                    : col.key === "name" ? columnFilters.name.length > 0
                    : col.key === "type" ? columnFilters.type.length > 0 : false;
                  return (
                  <span
                    key={col.key}
                    data-col={col.key}
                    className={`fm-col-header${sortState.key === col.key ? " sorted" : ""}${resizing?.key === col.key ? " resizing" : ""}${headerMenu?.key === col.key ? " menu-open" : ""}${filterActive ? " filtered" : ""}`}
                    onClick={() => toggleSort(col.key)}
                    onMouseEnter={() => setHoveredCol(col.key)}
                    /* 拖拽调宽期间保持整列高亮（鼠标常会移出列头），结束时统一清理 */
                    onMouseLeave={() => { if (resizing?.key !== col.key) setHoveredCol(null); }}
                  >
                    <span className="fm-col-label">{t(col.labelKey)}</span>
                    {sortState.key === col.key && (
                      <SymbolicIcon
                        name={sortState.dir === "asc" ? "chevron-up" : "chevron-down"}
                        size={12}
                        className="fm-sort-indicator"
                      />
                    )}
                    {/* ▾ 下拉按钮：名称/大小/修改时间/类型列提供（可筛选），hover 列头浮现
                        （Win11 Explorer 同款）；筛选激活时按钮常驻并显示漏斗图标
                        （激活指示与入口合一，不再在文字后另放符号）；
                        点击弹筛选菜单，不触发表头点击排序 */}
                    {(col.key === "size" || col.key === "mtime" || col.key === "name" || col.key === "type") && (
                      <button
                        className="fm-col-menu-btn"
                        title={t("fileManager.filter")}
                        onClick={(e) => openHeaderMenu(e, col.key)}
                      >
                        <SymbolicIcon name={filterActive ? "funnel" : "chevron-down"} size={10} />
                      </button>
                    )}
                    {col.key !== "perm" && (
                      <span
                        className="fm-col-resizer"
                        onMouseDown={(e) => beginColumnResize(e, col.key)}
                        onClick={(e) => e.stopPropagation()}  // 手柄点击不触发排序
                      />
                    )}
                  </span>
                  );
                })}
              </div>
              {/* ✅ 文件列表滚动容器（flex: 1 + overflow-y: auto） */}
              <div
                className="fm-list-content"
                ref={listContentRef}
                onContextMenu={handleEmptyContextMenu}
                onClick={handleEmptyClick}
                onMouseDown={beginMarquee}
              >
                {/* 上下占位：撑起未挂载行的滚动高度（审计 M25） */}
                {viewMode === "list" && padTop > 0 && (
                  <div aria-hidden style={{ height: padTop }} />
                )}
                {windowedEntries.map((entry, idx) => {
                  const rowPath = entryFullPath(entry);
                  const isDropTarget = entry.is_dir && dropTargetPath === rowPath;
                  const isCut = clip?.mode === "cut" && clip.paths.includes(rowPath);
                  return (
                  <div
                    key={entry.path ?? entry.name}
                    data-path={rowPath}
                    data-drop-dir={entry.is_dir ? rowPath : undefined}
                    className={`fm-list-row${selectedSet.has(rowPath) ? " selected" : ""}${isCut ? " fm-cut" : ""}${isDropTarget ? " drop-target" : ""}`}
                    onMouseDown={(e) => handleRowMouseDown(e, entry)}
                    onClick={(e) => handleRowClick(e, entry, winStart + idx)}
                    onDoubleClick={() => handleRowDoubleClick(entry)}
                    onContextMenu={(e) => handleContextMenu(e, entry)}
                  >
                    <div className="fm-file-name" title={activeSearch ? entry.path : undefined}>
                      <span className="fm-fn-icon"><AppIcon name={getFileIcon(entry)} size={16} /></span>
                      {editingEntry?.name === entry.name && editingEntry?.path === entry.path ? (
                        <input
                          type="text"
                          ref={inlineEditRef}
                          className="fm-fn-edit-input"
                          value={editingName}
                          onChange={(e) => setEditingName(e.target.value)}
                          onKeyDown={handleEditKeyDown}
                          onBlur={finishRename}
                          autoFocus
                          onClick={(e) => e.stopPropagation()}
                          onDoubleClick={(e) => e.stopPropagation()}  // 阻止双击事件冒泡
                        />
                      ) : (
                        <>
                          <span className="fm-fn-text">{entry.name}</span>
                          {/* 符号链接标记：目标类型已由 Agent 跟随解析（目录链接可进入） */}
                          {entry.is_symlink && (
                            <span
                              className="fm-fn-link"
                              title={entry.link_target
                                ? t("fileManager.symlinkTo", { target: entry.link_target })
                                : t("fileManager.symlink")}
                            >
                              ↗
                            </span>
                          )}
                          {/* 搜索命中在嵌套目录：名称后缀相对路径（Windows/Nautilus 副标题样式） */}
                          {activeSearch && entry.path && hitDirLabel(entry.path) && (
                            <span className="fm-fn-subdir">{hitDirLabel(entry.path)}</span>
                          )}
                        </>
                      )}
                    </div>
                    <span className="fm-file-size">{entry.is_dir ? "—" : formatSize(entry.size)}</span>
                    <span className="fm-file-mtime">{formatDate(entry.mtime)}</span>
                    <span className="fm-file-type">
                      {entry.is_symlink ? t("fileManager.symlink") : typeKeyLabel(typeKeyOf(entry))}
                    </span>
                    <span className="fm-file-perm">{entry.permissions}</span>
                  </div>
                  );
                })}
                {viewMode === "list" && padBottom > 0 && (
                  <div aria-hidden style={{ height: padBottom }} />
                )}
              </div>
            </div>
          ) : (
            <div
              className="fm-grid"
              ref={gridRef}
              onContextMenu={handleEmptyContextMenu}
              onClick={handleEmptyClick}
              onMouseDown={beginMarquee}
            >
              {viewMode === "grid" && gPadTop > 0 && (
                <div aria-hidden style={{ gridColumn: "1 / -1", height: gPadTop }} />
              )}
              {windowedGridEntries.map((entry, idx) => {
                const rowPath = entryFullPath(entry);
                const isDropTarget = entry.is_dir && dropTargetPath === rowPath;
                const isCut = clip?.mode === "cut" && clip.paths.includes(rowPath);
                return (
                <div
                  key={entry.path ?? entry.name}
                  data-path={rowPath}
                  data-drop-dir={entry.is_dir ? rowPath : undefined}
                  className={`fm-grid-item${selectedSet.has(rowPath) ? " selected" : ""}${isCut ? " fm-cut" : ""}${isDropTarget ? " drop-target" : ""}`}
                  onMouseDown={(e) => handleRowMouseDown(e, entry)}
                  onClick={(e) => handleRowClick(e, entry, gStart + idx)}
                  onDoubleClick={() => handleRowDoubleClick(entry)}
                  onContextMenu={(e) => handleContextMenu(e, entry)}
                >
                  <div className="fm-gi-icon">
                    <AppIcon name={getFileIcon(entry)} size={36} />
                    {entry.is_symlink && (
                      <span
                        className="fm-gi-link"
                        title={entry.link_target
                          ? t("fileManager.symlinkTo", { target: entry.link_target })
                          : t("fileManager.symlink")}
                      >
                        ↗
                      </span>
                    )}
                  </div>
                  {editingEntry?.name === entry.name && editingEntry?.path === entry.path ? (
                    <input
                      type="text"
                      ref={inlineEditRef}
                      className="fm-gi-edit-input"
                      value={editingName}
                      onChange={(e) => setEditingName(e.target.value)}
                      onKeyDown={handleEditKeyDown}
                      onBlur={finishRename}
                      autoFocus
                      onClick={(e) => e.stopPropagation()}
                      onDoubleClick={(e) => e.stopPropagation()}  // 阻止双击事件冒泡
                    />
                  ) : (
                    <div className="fm-gi-label" title={activeSearch ? entry.path : undefined}>{entry.name}</div>
                  )}
                </div>
                );
              })}
              {viewMode === "grid" && gPadBottom > 0 && (
                <div aria-hidden style={{ gridColumn: "1 / -1", height: gPadBottom }} />
              )}
            </div>
          )}
        </AppLayout>

      {/* 框选矩形（marquee）：viewport 坐标 fixed 定位，不随内容滚动 */}
      {marquee && createPortal(
        <div
          className="fm-marquee"
          data-browser-overlay
          style={{
            left: Math.min(marquee.x0, marquee.x1),
            top: Math.min(marquee.y0, marquee.y1),
            width: Math.abs(marquee.x1 - marquee.x0),
            height: Math.abs(marquee.y1 - marquee.y0),
          }}
        />,
        document.body
      )}

      {/* Status Bar */}
      <div className="fm-statusbar">
        {/* 左侧：原有的文件数量、总大小信息 */}
        <div className="fm-statusbar-left">
          <span>{isOffline ? t("fileManager.dirFileCount", { dirs: PLACEHOLDER, files: PLACEHOLDER }) : t("fileManager.dirFileCount", { dirs: dirCount, files: fileCount })}</span>
          <span>{t("fileManager.totalSize")} {isOffline ? PLACEHOLDER : formatSize(totalSize)}</span>
          {/* 搜索触达上限：提示结果可能不全（可细化关键词缩小范围） */}
          {activeSearch && searchTruncated && (
            <span className="fm-truncated-hint">{t("fileManager.searchTruncated")}</span>
          )}
          {/* 已选计数（多选时显示） */}
          {selected.length > 0 && (
            <span className="fm-selected-count">{t("fileManager.itemsSelected", { n: selected.length })}</span>
          )}
        </div>

        {/* 右侧：传输状态（新功能） */}
        <div className="fm-statusbar-right">
          <TransferStatusBar />
        </div>
      </div>

      {/* Header Dropdown Menu - Win11 Explorer 风格列头下拉（列筛选），
          Portal 渲染到 body：fixed 定位相对视口，不受滚动容器影响。
          名称/大小/修改时间/类型列可打开（权限列无筛选区，不渲染下拉按钮） */}
      {headerMenu && createPortal(
        <div
          className="fm-header-menu"
          data-browser-overlay
          style={{ left: headerMenu.x, top: headerMenu.y }}
          /* 菜单内点击（含复选框连续勾选）不冒泡到 window 的外部关闭监听 */
          onClick={(e) => e.stopPropagation()}
        >
          {/* 筛选区：勾选为并集（OR），菜单保持打开可连续多选；全不勾 = 该列不过滤
              名称=字母/拼音组、大小=区间组、时间=日期组、类型=当前目录动态清单
              （类型桶按全量 entries 生成，不随其他列筛选跳变；按钮只在可筛选列渲染） */}
          {(headerMenu.key === "size" ? SIZE_BUCKETS.map((b) => ({ key: b.key, label: t(b.labelKey) }))
            : headerMenu.key === "mtime" ? MTIME_BUCKETS.map((b) => ({ key: b.key, label: t(b.labelKey) }))
            : headerMenu.key === "type" ? listFileTypeBuckets(entries)
            : NAME_BUCKETS.map((b) => ({ key: b.key, label: t(b.labelKey) }))).map(b => {
            const checked = headerMenu.key === "size"
              ? columnFilters.size.includes(b.key as SizeBucket)
              : headerMenu.key === "mtime"
                ? columnFilters.mtime.includes(b.key as MtimeBucket)
                : headerMenu.key === "type"
                  ? columnFilters.type.includes(b.key)
                  : columnFilters.name.includes(b.key as NameBucket);
            return (
              <label key={b.key} className="fm-hm-check">
                <input
                  type="checkbox"
                  checked={checked}
                  onChange={() => toggleColumnFilter(headerMenu.key, b.key)}
                />
                <span>{b.label}</span>
              </label>
            );
          })}
        </div>
      , document.body)}

      {/* 指针式拖拽幽灵：「图标被拿起」跟随层——真实文件/文件夹 AppIcon，
          多选扇形叠放 + 数量角标，名称胶囊悬于下方（HTML5 DnD 在 WebView2
          下不可用，内部拖拽为指针式实现，见 useFileTransfer 头注释） */}
      {dragGhost && createPortal((() => {
        const items = describeGhostItems(dragGhost.paths);
        return (
          <div
            className="fm-drag-ghost"
            data-browser-overlay
            /* 光标对齐图标堆叠中心：抓手的"手"落在图标中间（堆叠 38px，
               中心 = +19；名称胶囊垂在下方不挡落点高亮） */
            style={{ transform: `translate(${ghostPos.x - 19}px, ${ghostPos.y - 19}px)` }}
          >
            <span className="fm-drag-ghost-stack">
              {items.map((it, i, arr) => (
                <span
                  key={`${i}-${it.name}`}
                  className="fm-drag-ghost-icon"
                  style={{
                    transform: `translate(${-i * 6}px, ${-i * 4}px) rotate(${i * -7}deg)`,
                    opacity: 1 - i * 0.18,
                    zIndex: arr.length - i,
                  }}
                >
                  <AppIcon name={it.icon} size={30} />
                </span>
              ))}
              {dragGhost.count > 1 && (
                <span className="fm-drag-ghost-count">{dragGhost.count}</span>
              )}
            </span>
            <span className="fm-drag-ghost-name">
              {dragGhost.count > 1
                ? t("fileManager.dragItems", { n: dragGhost.count })
                : items[0]?.name ?? ""}
            </span>
          </div>
        );
      })(), document.body)}

      {/* 右键菜单：共享 ContextMenu 组件（portal + 实测尺寸视口钳制 +
          data-browser-overlay 让位机制）。targets = null 为空白菜单 */}
      {contextMenu && (
        <ContextMenu
          pos={{ x: contextMenu.x, y: contextMenu.y }}
          items={contextMenuItems}
          onClose={() => setContextMenu(null)}
        />
      )}

      {/* 删除确认弹窗：危险操作（红色警示图标 + 焦点默认在取消上防回车误触）。
          多选时列明数量，单选保留原文件名高亮文案 */}
      {deleteTargets.length > 0 && (
        <ConfirmDialog
          title={t("fileManager.ctxDelete")}
          message={
            deleteTargets.length === 1
              ? t("fileManager.deleteConfirm", { name: deleteTargets[0].name })
              : t("fileManager.deleteConfirmMulti", { n: deleteTargets.length })
          }
          highlight={deleteTargets.length === 1 ? deleteTargets[0].name : undefined}
          detail={
            deleteTargets.length === 1
              ? (deleteTargets[0].is_dir ? t("fileManager.deleteDetailDir") : t("fileManager.deleteDetailFile"))
              : t("fileManager.deleteDetailMulti")
          }
          confirmLabel={t("fileManager.ctxDelete")}
          danger
          onCancel={() => setDeleteTargets([])}
          onConfirm={() => { const targets = deleteTargets; setDeleteTargets([]); doDelete(targets); }}
        />
      )}

      {/* 防呆确认弹窗：src/dst 触及系统或敏感目录时先于重名确认弹出 */}
      {riskConfirm && (
        <ConfirmDialog
          title={t("fileManager.guardTitle", { op: riskConfirm.mode === "move" ? t("fileManager.opMove") : t("fileManager.opCopy") })}
          message={t("fileManager.guardMessage")}
          detail={riskConfirm.findings
            .map((f) => t(f.reason === "systemPath" ? "fileManager.guardSystemPath" : "fileManager.guardSensitiveDir", { sample: f.sample }))
            .join("；")}
          confirmLabel={riskConfirm.mode === "move" ? t("fileManager.opMove") : t("fileManager.opCopy")}
          danger
          onCancel={() => setRiskConfirm(null)}
          onConfirm={() => {
            const { mode, targets } = riskConfirm;
            setRiskConfirm(null);
            void checkConflictsAndExec(mode, targets);
          }}
        />
      )}

      {/* 移动/复制重名确认：目标位置存在同名项时列出并请求覆盖确认 */}
      {transferConfirm && (
        <ConfirmDialog
          title={transferConfirm.mode === "move" ? t("fileManager.moveConfirmTitle") : t("fileManager.copyConfirmTitle")}
          message={t("fileManager.conflictDetail", { n: transferConfirm.conflicts.length, names: transferConfirm.conflicts.slice(0, 3).map(entryName).join("、") })}
          detail={t("fileManager.conflictQuestion")}
          confirmLabel={transferConfirm.mode === "move" ? t("fileManager.opMove") : t("fileManager.opCopy")}
          danger
          onCancel={() => setTransferConfirm(null)}
          onConfirm={() => {
            const { mode, targets } = transferConfirm;
            setTransferConfirm(null);
            void execTransfer(mode, targets);
          }}
        />
      )}

      {/* Path Error Dialog — 路径不存在时弹窗提示（统一弹窗底座） */}
      {pathErrorDialog && (
        <Dialog onClose={() => setPathErrorDialog(null)}>
          <div className="dg-title">{t("fileManager.pathErrorTitle")}</div>
          <div className="dg-message">{pathErrorDialog}</div>
          <div className="dg-actions">
            <button className="dg-btn-primary" autoFocus onClick={() => setPathErrorDialog(null)}>{t("common.confirm")}</button>
          </div>
        </Dialog>
      )}

      {/* Rename Dialog - 文件名过长时弹窗重命名（统一弹窗底座）。
          事件 stopPropagation 防止按键冒泡到列表快捷键（原行为保留） */}
      {renameDialog && (
        <Dialog onClose={() => setRenameDialog(null)} width={480}>
          <div
            onClick={(e) => e.stopPropagation()}
            onKeyDown={(e) => e.stopPropagation()}
            onKeyUp={(e) => e.stopPropagation()}
            onKeyPress={(e) => e.stopPropagation()}
          >
            <div className="dg-title">{t("fileManager.renameTitle")}</div>
            <div className="rd-content">
              <div className="rd-original">
                <div className="rd-label">{t("fileManager.renameOriginal")}</div>
                <div className="rd-original-name">
                  {renameDialog.originalName.length > 60
                    ? renameDialog.originalName.substring(0, 60) + '...'
                    : renameDialog.originalName}
                </div>
              </div>
              <div className="rd-input-section">
                <div className="rd-label">{t("fileManager.renameNew")}</div>
                <input
                  ref={renameInputRef}
                  type="text"
                  className="rd-input"
                  value={renameDialog.newName}
                  onChange={(e) => {
                    const newName = e.target.value;
                    setRenameDialog(prev => prev ? { ...prev, newName } : null);
                  }}
                  onInput={(e) => {
                    // 阻止事件冒泡
                    e.stopPropagation();
                  }}
                  onKeyDown={(e) => {
                    // 阻止事件冒泡，防止触发文件管理器的快捷键
                    e.stopPropagation();
                    // Enter 键确认
                    if (e.key === 'Enter') {
                      const currentBytes = new TextEncoder().encode(renameDialog.newName).length;
                      if (currentBytes <= renameDialog.maxBytes && renameDialog.newName.trim() !== '') {
                        setRenameDialog(null);
                        handleUploadWithNewName(renameDialog.localPath, renameDialog.newName.trim());
                      }
                    }
                  }}
                  onKeyUp={(e) => e.stopPropagation()}
                  onKeyPress={(e) => e.stopPropagation()}
                  placeholder={t("fileManager.renamePlaceholder")}
                  autoFocus
                />
              </div>
              <div className="rd-byte-counter">
                {(() => {
                  const currentBytes = new TextEncoder().encode(renameDialog.newName).length;
                  const isValid = currentBytes <= renameDialog.maxBytes;
                  const remaining = renameDialog.maxBytes - currentBytes;

                  return (
                    <span className={isValid ? 'rd-byte-valid' : 'rd-byte-invalid'}>
                      {isValid ? (
                        <>{t("fileManager.renameBytesRemaining", { count: remaining })}</>
                      ) : (
                        <>{t("fileManager.renameBytesOver", { count: Math.abs(remaining) })}</>
                      )}
                      <span className="rd-byte-hint">{t("fileManager.renameByteHint", { current: currentBytes, max: renameDialog.maxBytes })}</span>
                    </span>
                  );
                })()}
              </div>
            </div>
            <div className="dg-actions">
              <button className="dg-btn" onClick={() => setRenameDialog(null)}>{t("common.cancel")}</button>
              <button
                className="dg-btn-primary"
                disabled={new TextEncoder().encode(renameDialog.newName).length > renameDialog.maxBytes || renameDialog.newName.trim() === ''}
                onClick={() => {
                  const newName = renameDialog.newName.trim();
                  if (newName && new TextEncoder().encode(newName).length <= renameDialog.maxBytes) {
                    // 用户确认重命名，继续上传流程
                    setRenameDialog(null);
                    // 调用上传逻辑（使用新文件名）
                    handleUploadWithNewName(renameDialog.localPath, newName);
                  }
                }}
              >
                {t("common.confirm")}
              </button>
            </div>
          </div>
        </Dialog>
      )}

      {/* Properties Dialog - 使用 Portal（分页：常规=只读信息 / 权限=可编辑 chmod+chown） */}
      {propertiesEntry && createPortal(
        <div className="pd-overlay" data-browser-overlay onClick={(e) => {
          // 保存进行中不允许点遮罩关闭（防丢失反馈）；其余状态点击遮罩 = 关闭（与 ed-overlay 一致）
          if (!permSaving && e.target === e.currentTarget) {
            setPropertiesEntry(null);
          }
        }}>
          <div className="fm-properties-dialog">
            <div className="pd-header">
              <span className="pd-icon"><AppIcon name={getFileIcon(propertiesEntry)} size={28} /></span>
              <span className="pd-name">{propertiesEntry.name}</span>
            </div>

            {/* 分页栏：按功能划分——「常规」看信息，「权限」改设置 */}
            <div className="pd-tabs" role="tablist">
              <button
                role="tab"
                aria-selected={permTab === 'general'}
                className={`pd-tab ${permTab === 'general' ? 'pd-tab-active' : ''}`}
                onClick={() => setPermTab('general')}
              >
                {t("fileManager.propsTabGeneral")}
                {/* 权限页有未保存更改时，常规页标题旁给出徽标提示 */}
                {permTab === 'general' && permHasChanges && <span className="pd-tab-badge" title={t("fileManager.propsUnsavedBadge")} />}
              </button>
              <button
                role="tab"
                aria-selected={permTab === 'permissions'}
                className={`pd-tab ${permTab === 'permissions' ? 'pd-tab-active' : ''}`}
                onClick={() => setPermTab('permissions')}
              >
                {t("fileManager.propsTabPermissions")}
                {permTab !== 'permissions' && permHasChanges && <span className="pd-tab-badge" title={t("fileManager.propsUnsavedBadge")} />}
              </button>
            </div>

            <div className="pd-content">
              {permTab === 'general' ? (<>
                {/* ── 常规页：只读信息 ── */}
                <div className="pd-row">
                  <span className="pd-label">{t("fileManager.propsType")}</span>
                  <span className="pd-value">{propertiesEntry.is_dir ? t("fileManager.propsFolder") : t("fileManager.propsFile")}</span>
                </div>
                <div className="pd-row">
                  <span className="pd-label">{t("fileManager.propsLocation")}</span>
                  {/* 完整路径（等宽字体，可选中复制） */}
                  <span className="pd-value pd-location">
                    {currentPath === '/' ? `/${propertiesEntry.name}` : `${currentPath}/${propertiesEntry.name}`}
                  </span>
                </div>
                <div className="pd-row">
                  <span className="pd-label">{t("fileManager.propsSize")}</span>
                  <span className="pd-value">{formatSize(propertiesEntry.size)}</span>
                </div>
                <div className="pd-row">
                  <span className="pd-label">{t("fileManager.propsModified")}</span>
                  <span className="pd-value">{formatDate(propertiesEntry.mtime)}</span>
                </div>
                {/* 访问控制速览（当前服务器状态；编辑请切到「权限」页） */}
                <div className="pd-row">
                  <span className="pd-label">{t("fileManager.propsPermissions")}</span>
                  <span className="pd-value pd-mono">
                    {propertiesEntry.permissions || "—"}
                    {permDirty.modeChanged && <span className="pd-badge-dirty">{t("fileManager.propsUnsaved")}</span>}
                  </span>
                </div>
                <div className="pd-row">
                  <span className="pd-label">{t("fileManager.propsOwner")}</span>
                  <span className="pd-value pd-mono">
                    {propertiesEntry.owner || propertiesEntry.group
                      ? `${propertiesEntry.owner || "—"}:${propertiesEntry.group || "—"}`
                      : "—"}
                    {permDirty.ownerChanged && <span className="pd-badge-dirty">{t("fileManager.propsUnsaved")}</span>}
                  </span>
                </div>
                <div className="pd-hint">{t("fileManager.propsGeneralHint")}</div>
              </>) : (<>
                {/* ── 权限页：可编辑（chmod + chown） ── */}
                {/* 快捷预设：一键填充矩阵（目录/文件各一组常用值） */}
                <div className="pd-preset-row">
                  <span className="pd-preset-label">{t("fileManager.propsPreset")}</span>
                  {(propertiesEntry.is_dir
                    ? [{ mode: 0o755, key: "preset755", hintKey: "presetHintStandard" }, { mode: 0o700, key: "preset700", hintKey: "presetHintPrivate" }]
                    : [{ mode: 0o644, key: "preset644", hintKey: "presetHintStandard" }, { mode: 0o600, key: "preset600", hintKey: "presetHintPrivate" }, { mode: 0o444, key: "preset444", hintKey: "presetHintReadonly" }]
                  ).map(p => (
                    <button
                      key={p.key}
                      className="pd-preset-btn"
                      disabled={permSaving}
                      title={t("fileManager.presetApply", { mode: p.mode.toString(8), hint: t(`fileManager.${p.hintKey}`) })}
                      onClick={() => setPermMatrix(parsePermissions(octalToPermString(p.mode)))}
                    >
                      {t(`fileManager.${p.key}`)}
                    </button>
                  ))}
                </div>

                {/* 3×3 复选框矩阵（行=所有者/组/其他，列=读取/写入/执行）+ 八进制只读展示 */}
                <div className="pd-perm">
                  <div className="pd-perm-grid">
                    <span className="pd-perm-head" />
                    {(["permRead", "permWrite", "permExecute"] as const).map(colKey => (
                      <span key={colKey} className="pd-perm-head">{t(`fileManager.${colKey}`)}</span>
                    ))}
                    {(["permOwnerRow", "permGroupRow", "permOthersRow"] as const).map((rowKey, r) => (
                      <React.Fragment key={rowKey}>
                        <span className="pd-perm-rowlabel">{t(`fileManager.${rowKey}`)}</span>
                        {[0, 1, 2].map(c => (
                          <label key={c} className="pd-perm-cell">
                            <input
                              type="checkbox"
                              aria-label={`${t(`fileManager.${rowKey}`)} ${t(`fileManager.${(["permRead", "permWrite", "permExecute"] as const)[c]}`)}`}
                              checked={permMatrix[r][c]}
                              disabled={permSaving}
                              onChange={() => setPermMatrix(prev => {
                                const next = prev.map(t => [...t]);
                                next[r][c] = !prev[r][c];
                                return next;
                              })}
                            />
                          </label>
                        ))}
                      </React.Fragment>
                    ))}
                  </div>
                  {/* 八进制 + 符号串随矩阵联动（只读）；mode 数值须按八进制显示（0o620=400 → "620"） */}
                  <div className="pd-perm-octal">
                    <span className="pd-perm-octal-value">{matrixToOctal(permMatrix).toString(8).padStart(3, "0")}</span>
                    <span className="pd-perm-octal-symbolic">{octalToPermString(matrixToOctal(permMatrix))}</span>
                  </div>
                </div>

                {/* 所有者区：owner / group 文本输入（初值 entry.owner/entry.group，缺失回退 "—" 占位） */}
                <div className="pd-owner-row">
                  <label className="pd-owner-field">
                    <span className="pd-owner-label">{t("fileManager.permOwnerRow")}</span>
                    <input
                      className="pd-input"
                      value={permOwner}
                      placeholder={propertiesEntry.owner || "—"}
                      disabled={permSaving}
                      onChange={e => setPermOwner(e.target.value)}
                    />
                  </label>
                  <label className="pd-owner-field">
                    <span className="pd-owner-label">{t("fileManager.propsGroup")}</span>
                    <input
                      className="pd-input"
                      value={permGroup}
                      placeholder={propertiesEntry.group || "—"}
                      disabled={permSaving}
                      onChange={e => setPermGroup(e.target.value)}
                    />
                  </label>
                </div>
                <div className="pd-hint">{t("fileManager.ownerRootHint")}</div>

                {/* 仅目录：递归应用 */}
                {propertiesEntry.is_dir && (
                  <label className="pd-recursive">
                    <input
                      type="checkbox"
                      checked={permRecursive}
                      disabled={permSaving}
                      onChange={e => setPermRecursive(e.target.checked)}
                    />
                    {t("fileManager.propsRecursive")}
                  </label>
                )}
              </>)}
            </div>
            {/* 内联错误（Adwaita error 红），请求失败时不关闭对话框 */}
            {permError && <div className="pd-error">{permError}</div>}
            <div className="pd-footer">
              <button className="pd-btn" disabled={permSaving} onClick={() => setPropertiesEntry(null)}>{t("fileManager.close")}</button>
              {/* 「应用」仅权限页显示；无更改时禁用（避免无意义请求） */}
              {permTab === 'permissions' && (
                <button className="pd-btn-primary" disabled={permSaving || !permHasChanges} onClick={applyProperties}>
                  {permSaving ? t("fileManager.applying") : t("fileManager.apply")}
                </button>
              )}
            </div>
          </div>
        </div>,
        document.body
      )}
    </div>
  );
}

/* ── Demo Data (for UI dev without backend) ──────────── */

function getDemoEntries(path: string): FileEntry[] {
  const DEMO: Record<string, FileEntry[]> = {
    "/": [
      { name: "bin",  is_dir: true,  size: 0,      mtime: "2026-01-15T08:00:00Z", permissions: "rwxr-xr-x" },
      { name: "etc",  is_dir: true,  size: 0,      mtime: "2026-05-20T12:30:00Z", permissions: "rwxr-xr-x" },
      { name: "home", is_dir: true,  size: 0,      mtime: "2026-05-28T10:00:00Z", permissions: "rwxr-xr-x" },
      { name: "opt",  is_dir: true,  size: 0,      mtime: "2026-03-01T09:00:00Z", permissions: "rwxr-xr-x" },
      { name: "tmp",  is_dir: true,  size: 0,      mtime: "2026-05-28T18:45:00Z", permissions: "rwxrwxrwt" },
      { name: "usr",  is_dir: true,  size: 0,      mtime: "2026-02-10T07:00:00Z", permissions: "rwxr-xr-x" },
      { name: "var",  is_dir: true,  size: 0,      mtime: "2026-05-28T19:00:00Z", permissions: "rwxr-xr-x" },
    ],
    "/home": [
      { name: "user",          is_dir: true,  size: 0,        mtime: "2026-05-28T10:00:00Z", permissions: "rwx------" },
      { name: "admin",         is_dir: true,  size: 0,        mtime: "2026-05-27T14:00:00Z", permissions: "rwxr-x---" },
      { name: "lost+found",    is_dir: true,  size: 0,        mtime: "2026-01-01T00:00:00Z", permissions: "rwx------" },
    ],
    "/home/user": [
      { name: "Documents",  is_dir: true,  size: 0,         mtime: "2026-05-28T09:30:00Z", permissions: "rwxr-xr-x" },
      { name: "Downloads",  is_dir: true,  size: 0,         mtime: "2026-05-28T18:00:00Z", permissions: "rwxr-xr-x" },
      { name: "Pictures",   is_dir: true,  size: 0,         mtime: "2026-05-25T11:00:00Z", permissions: "rwxr-xr-x" },
      { name: "Music",      is_dir: true,  size: 0,         mtime: "2026-05-20T16:00:00Z", permissions: "rwxr-xr-x" },
      { name: ".bashrc",    is_dir: false, size: 3771,      mtime: "2026-04-10T08:30:00Z", permissions: "rw-r--r--" },
      { name: ".profile",   is_dir: false, size: 807,       mtime: "2026-01-15T08:00:00Z", permissions: "rw-r--r--" },
      { name: ".ssh",       is_dir: true,  size: 0,         mtime: "2026-05-01T10:00:00Z", permissions: "rwx------" },
      { name: "config.toml", is_dir: false, size: 2048,     mtime: "2026-05-27T15:30:00Z", permissions: "rw-r--r--" },
      { name: "notes.md",   is_dir: false, size: 15360,    mtime: "2026-05-28T17:00:00Z", permissions: "rw-r--r--" },
      { name: "deploy.sh",  is_dir: false, size: 512,       mtime: "2026-05-26T09:00:00Z", permissions: "rwxr-xr-x" },
    ],
    "/home/user/Documents": [
      { name: "project-plan.md",  is_dir: false, size: 45056,   mtime: "2026-05-28T14:00:00Z", permissions: "rw-r--r--" },
      { name: "architecture.png", is_dir: false, size: 128000,  mtime: "2026-05-27T16:30:00Z", permissions: "rw-r--r--" },
      { name: "report-2026-q1.pdf", is_dir: false, size: 2048000, mtime: "2026-04-05T10:00:00Z", permissions: "rw-r--r--" },
      { name: "contracts",        is_dir: true,  size: 0,       mtime: "2026-03-15T12:00:00Z", permissions: "rwx------" },
      { name: "meeting-notes",    is_dir: true,  size: 0,       mtime: "2026-05-28T11:00:00Z", permissions: "rwxr-xr-x" },
    ],
    "/home/user/Downloads": [
      { name: "rust-1.95.0-x86_64-unknown-linux-gnu.tar.gz", is_dir: false, size: 256000000, mtime: "2026-05-28T16:00:00Z", permissions: "rw-r--r--" },
      { name: "node-v22.22.2-linux-x64.tar.xz",              is_dir: false, size: 28000000,  mtime: "2026-05-27T09:00:00Z", permissions: "rw-r--r--" },
      { name: "setup-guide.pdf",                              is_dir: false, size: 512000,   mtime: "2026-05-26T14:00:00Z", permissions: "rw-r--r--" },
    ],
    "/tmp": [
      { name: "build-output.log", is_dir: false, size: 8192,  mtime: "2026-05-28T19:30:00Z", permissions: "rw-rw-rw-" },
      { name: "session-abc123",   is_dir: true,  size: 0,     mtime: "2026-05-28T18:00:00Z", permissions: "rwx------" },
      { name: "cache",            is_dir: true,  size: 0,     mtime: "2026-05-28T19:00:00Z", permissions: "rwxrwxrwx" },
    ],
  };

  // Try to find a matching demo path, or generate a generic one
  const entries = DEMO[path];
  if (entries) return entries;

  // Generic fallback for unlisted paths
  return [
    { name: "..", is_dir: true, size: 0, mtime: "", permissions: "rwxr-xr-x" },
    { name: "README.md", is_dir: false, size: 1024, mtime: "2026-05-28T12:00:00Z", permissions: "rw-r--r--" },
    { name: "data", is_dir: true, size: 0, mtime: "2026-05-27T08:00:00Z", permissions: "rwxr-xr-x" },
  ];
}
