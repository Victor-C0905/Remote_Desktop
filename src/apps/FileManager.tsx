import React, { useState, useEffect, useCallback, useRef } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open, save, ask } from "@tauri-apps/plugin-dialog";
import { useServerManager } from "../context/ServerManager";
import { PLACEHOLDER } from "../utils/offlineDefaults";
import { useWindowManager, useWindowState } from "../window-system/WindowManagerContext";
import { AppLayout } from "../components/app-shell";
import { TransferStatusBar } from "../components/TransferStatusBar";
import { createLogger } from '../utils/logger';
import "./FileManager.css";

const log = createLogger('FileManager');

export interface FileEntry {
  name: string;
  is_dir: boolean;
  size: number;
  mtime: string;
  permissions: string;
}

interface ReadDirResponse {
  path: string;
  entries: FileEntry[];
}

// 挂载点信息
interface MountInfo {
  mount_point: string;
  device: string;
  filesystem: string;
  total_bytes: number;
  used_bytes: number;
}

// 侧边栏项
interface SidebarItem {
  icon: string;
  label: string;
  path: string;
  type: "bookmark" | "mount" | "network";
}

// 侧边栏部分
interface SidebarSection {
  title: string;
  items: SidebarItem[];
}

/* ── Icon Map ──────────────────────────────────────────── */

const FOLDER_ICON = "📁";
const FILE_ICONS: Record<string, string> = {
  ".txt": "📄", ".md": "📝", ".json": "📋", ".toml": "⚙️",
  ".yaml": "⚙️", ".yml": "⚙️", ".rs": "🦀", ".ts": "📘",
  ".tsx": "📘", ".js": "📙", ".py": "🐍", ".css": "🎨",
  ".html": "🌐", ".sh": "🔧", ".conf": "⚙️", ".log": "📜",
  ".png": "🖼️", ".jpg": "🖼️", ".jpeg": "🖼️", ".svg": "🖼️",
  ".mp4": "🎬", ".mp3": "🎵", ".zip": "📦", ".tar": "📦",
  ".gz": "📦", ".db": "🗃️", ".sql": "🗃️",
};

function getFileIcon(entry: FileEntry): string {
  if (entry.is_dir) return FOLDER_ICON;
  const ext = entry.name.lastIndexOf(".");
  if (ext >= 0) {
    const icon = FILE_ICONS[entry.name.slice(ext).toLowerCase()];
    if (icon) return icon;
  }
  // Hidden files
  if (entry.name.startsWith(".")) return "🔒";
  return "📄";
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
  return d.toLocaleDateString("zh-CN", {
    year: "numeric", month: "2-digit", day: "2-digit",
    hour: "2-digit", minute: "2-digit",
  });
}

/* ── Component ─────────────────────────────────────────── */

type ViewMode = "list" | "grid";

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

  // ── 离线状态判断 ─────────────────────────────────────
  // 无活跃连接 = 离线模式（显示空状态占位符，而非空白或本地文件）
  const isOffline = !activeServerId;

  // 路径系统：始终使用 Linux 远程路径格式（远程 Linux 服务器控制工具）
  const [currentPath, setCurrentPath] = useState(() => "/");
  const [entries, setEntries] = useState<FileEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // 权限不足目录：进入目录但内容不可见（空列表+轻微提示）
  // 存储无权限目录路径，null 表示当前目录可正常访问
  const [permissionDenied, setPermissionDenied] = useState<string | null>(null);
  const [selectedIdx, setSelectedIdx] = useState<number | null>(null);
  const [username, setUsername] = useState<string | null>(null);
  // 用户真实家目录（root 为 /root，普通用户从 /etc/passwd 读取，不再硬编码 /home/${username}）
  const [homeDir, setHomeDir] = useState<string | null>(null);
  const [viewMode, setViewMode] = useState<ViewMode>("list");
  const [history, setHistory] = useState<string[]>([currentPath]);
  const [historyIdx, setHistoryIdx] = useState(0);
  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    type: 'file' | 'empty';  // 菜单类型：文件菜单或空白区域菜单
    entry?: FileEntry;       // 文件信息（仅文件菜单）
  } | null>(null);
  const [propertiesEntry, setPropertiesEntry] = useState<FileEntry | null>(null);
  const [editingEntry, setEditingEntry] = useState<FileEntry | null>(null);  // 正在编辑的文件
  const [editingName, setEditingName] = useState<string>("");  // 编辑中的新名称
  // ✅ 移除 mainRef，因为fm-main容器已被AppLayout替代
  const clickTimerRef = useRef<number | null>(null);  // 单击延迟定时器（防止双击误触发）

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

  const loadDir = useCallback(async (path: string): Promise<boolean> => {
    setLoading(true);
    setError(null);
    setPermissionDenied(null);
    setSelectedIdx(null);
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

      if (resp && resp.entries) {
        const sorted = [...resp.entries].sort((a, b) => {
          if (a.is_dir !== b.is_dir) return a.is_dir ? -1 : 1;
          return a.name.localeCompare(b.name);
        });
        setEntries(sorted);
      } else {
        setEntries(getDemoEntries(path));
      }

      // 成功后才更新当前路径（确保路径确实存在）
      setCurrentPath(path);
      return true;
    } catch (e: any) {
      log.error('加载目录失败:', e);
      const errMsg = e?.toString() ?? '';
      // 权限不足：当作空目录处理（进入目录但内容不可见）
      // 其他错误（路径不存在等）：显示错误提示，不更新路径
      if (errMsg.includes('权限不足') || errMsg.includes('Permission denied')) {
        setEntries([]);
        setPermissionDenied(path);
        setCurrentPath(path);
      } else {
        setError(errMsg);
        setEntries([]);
      }
      return false;
    } finally {
      setLoading(false);
    }
  }, [activeServerId]);

  // ── Path Suggestions ───────────────────────────────────
  const fetchSuggestions = useCallback(async (path: string) => {
    if (!activeServerId || path === "") {
      setSuggestions([]);
      setShowSuggestions(false);
      return;
    }

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

      setSuggestions(suggestions);
      setShowSuggestions(suggestions.length > 0);
      setSelectedSuggestionIdx(null);
    } catch (err) {
      log.error("获取建议失败:", err);
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
          title: "位置",
          items: [
            { icon: "🏠", label: "主目录", path: homeDir, type: "bookmark" },
            { icon: "📄", label: "文档", path: `${homeDir}/Documents`, type: "bookmark" },
            { icon: "⬇️", label: "下载", path: `${homeDir}/Downloads`, type: "bookmark" },
            { icon: "🖼️", label: "图片", path: `${homeDir}/Pictures`, type: "bookmark" },
            { icon: "🎵", label: "音乐", path: `${homeDir}/Music`, type: "bookmark" },
            { icon: "🎬", label: "视频", path: `${homeDir}/Videos`, type: "bookmark" },
          ]
        },
        {
          title: "设备",
          items: mounts.map(m => ({
            icon: "💾",
            label: m.mount_point,
            path: m.mount_point,
            type: "mount"
          }))
        },
        {
          title: "其他位置",
          items: [
            { icon: "🌐", label: "网络", path: "/network", type: "network" }
          ]
        }
      ];
      log.debug("远程侧边栏生成完成:", sections);
      setSidebarSections(sections);
    } else {
      // 离线模式：使用默认 Linux 侧边栏（保持 UI 结构一致）
      const sections: SidebarSection[] = [
        {
          title: "位置",
          items: [
            { icon: "🏠", label: "主目录", path: "/home", type: "bookmark" },
            { icon: "📄", label: "文档", path: "/home/Documents", type: "bookmark" },
            { icon: "⬇️", label: "下载", path: "/home/Downloads", type: "bookmark" },
            { icon: "🖼️", label: "图片", path: "/home/Pictures", type: "bookmark" },
            { icon: "🎵", label: "音乐", path: "/home/Music", type: "bookmark" },
            { icon: "🎬", label: "视频", path: "/home/Videos", type: "bookmark" },
          ]
        },
        {
          title: "其他位置",
          items: [
            { icon: "🌐", label: "网络", path: "/network", type: "network" }
          ]
        }
      ];
      setSidebarSections(sections);
    }
  }, [activeServerId, username, homeDir, mounts, initialData]);

  // ── 断连即时响应 ─────────────────────────────────────
  // 当 activeServerId 从有值变为 null（断连），立即清空数据并显示离线占位符
  useEffect(() => {
    if (!activeServerId) {
      // 断连：立即重置为离线状态（不清空侧边栏，保留 UI 结构）
      setEntries([]);
      setError(null);
      setPermissionDenied(null);
      setLoading(false);
      setSelectedIdx(null);
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
      setSelectedIdx(null);
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

      try {
        await invoke<ReadDirResponse>("remote_read_dir", {
          serverId: activeServerId,
          path: cleanPath,
        });
        // 路径存在：正常导航（navigateTo → loadDir → setCurrentPath 更新完毕）
        navigateTo(cleanPath);
        // 导航完成后主动失焦，由 handlePathInputBlur 统一清理（同步 pathInput + 关闭编辑模式）
        pathInputRef.current?.blur();
      } catch {
        // 路径不存在：弹窗提示，恢复为当前路径（正确格式）
        setPathErrorDialog(`路径不存在: ${cleanPath}`);
        // 同样通过失焦统一处理，blur handler 会将 pathInput 恢复为 currentPath
        pathInputRef.current?.blur();
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
      setHistoryIdx(historyIdx - 1);
      loadDir(history[historyIdx - 1]);
    }
  }, [history, historyIdx, loadDir]);

  const goUp = useCallback(() => {
    const parent = currentPath.split("/").slice(0, -1).join("/") || "/";
    navigateTo(parent);
  }, [currentPath, navigateTo]);

  const goForward = useCallback(() => {
    if (historyIdx < history.length - 1) {
      setHistoryIdx(historyIdx + 1);
      loadDir(history[historyIdx + 1]);
    }
  }, [history, historyIdx, loadDir]);

  // Double-click entry
  const handleOpen = useCallback((entry: FileEntry) => {
    if (entry.is_dir) {
      const sep = "/";
      const newPath = currentPath === "/"
        ? `${currentPath}${sep}${entry.name}`
        : `${currentPath}${sep}${entry.name}`;
      navigateTo(newPath);
    } else {
      // 双击文件时，打开编辑器窗口
      const sep = "/";
      const filePath = currentPath === "/"
        ? `${currentPath}${sep}${entry.name}`
        : `${currentPath}${sep}${entry.name}`;

      log.debug(`Open file in editor: ${filePath}`);

      // 创建编辑器窗口，传递文件路径和服务器 ID
      manager.create('editor', {
        serverId: activeServerId || undefined,
        preloadData: { path: filePath, serverId: activeServerId || undefined }
      });
    }
  }, [currentPath, navigateTo, manager, activeServerId]);

  // ── Properties Dialog ────────────────────────────────
  const showProperties = useCallback((entry: FileEntry) => {
    setPropertiesEntry(entry);
  }, []);

  // ── File Operations ───────────────────────────────────
  const handleMkdir = useCallback(async () => {
    // 使用简单的 prompt（后续可以改为对话框）
    const name = prompt("新建文件夹名称:");
    if (!name || name.trim() === "") return;

    // 构建新路径（Linux 格式）
    const sep = "/";
    const newPath = currentPath === "/"
      ? `${currentPath}${sep}${name.trim()}`
      : `${currentPath}${sep}${name.trim()}`;

    log.debug("mkdir:", newPath);

    try {
      if (activeServerId) {
        await invoke("remote_mkdir", { serverId: activeServerId, path: newPath });
      } else {
        alert("请先连接到远程服务器");
        return;
      }

      // 刷新当前目录
      loadDir(currentPath);
    } catch (err) {
      log.error("mkdir 失败:", err);
      alert(`创建文件夹失败: ${err}`);
    }
  }, [currentPath, activeServerId, loadDir]);

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

    // 构建路径（Linux 格式）
    const sep = "/";
    const oldPath = currentPath === "/"
      ? `${currentPath}${sep}${editingEntry.name}`
      : `${currentPath}${sep}${editingEntry.name}`;

    const newPath = currentPath === "/"
      ? `${currentPath}${sep}${trimmedName}`
      : `${currentPath}${sep}${trimmedName}`;

    log.debug("rename:", oldPath, "->", newPath);

    try {
      if (activeServerId) {
        await invoke("remote_rename", {
          serverId: activeServerId,
          oldPath,
          newPath
        });
      } else {
        alert("请先连接到远程服务器");
        setEditingEntry(null);
        setEditingName("");
        return;
      }

      // 刷新当前目录
      loadDir(currentPath);
    } catch (err) {
      log.error("rename 失败:", err);
      alert(`重命名失败: ${err}`);
    }

    // 清除编辑状态
    setEditingEntry(null);
    setEditingName("");
  }, [editingEntry, editingName, currentPath, activeServerId, loadDir]);

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

  const handleDelete = useCallback(async (entry: FileEntry) => {
    // 使用简单的 confirm
    const confirmed = confirm(`确定删除 "${entry.name}"?\n\n${entry.is_dir ? "这将删除文件夹及其所有内容。" : "此操作无法撤销。"}`);
    if (!confirmed) return;

    // 构建路径（Linux 格式）
    const sep = "/";
    const path = currentPath === "/"
      ? `${currentPath}${sep}${entry.name}`
      : `${currentPath}${sep}${entry.name}`;

    log.debug("delete:", path);

    try {
      if (activeServerId) {
        await invoke("remote_delete", { serverId: activeServerId, path });
      } else {
        alert("请先连接到远程服务器");
        return;
      }

      // 刷新当前目录
      loadDir(currentPath);
    } catch (err) {
      log.error("delete 失败:", err);
      alert(`删除失败: ${err}`);
    }
  }, [currentPath, activeServerId, loadDir]);

  // 刷新当前目录
  const handleRefresh = useCallback(() => {
    setContextMenu(null);
    loadDir(currentPath);
  }, [currentPath, loadDir]);

  // 新建文件夹
  const handleNewFolder = useCallback(async () => {
    setContextMenu(null);
    const folderName = prompt("请输入文件夹名称：");
    if (!folderName) return;

    const sep = "/";
    const folderPath = currentPath === "/"
      ? `${currentPath}${sep}${folderName}`
      : `${currentPath}${sep}${folderName}`;

    try {
      if (activeServerId) {
        await invoke("remote_mkdir", { serverId: activeServerId, path: folderPath });
        loadDir(currentPath);
      } else {
        alert("请先连接到远程服务器");
      }
    } catch (err) {
      log.error("mkdir 失败:", err);
      alert(`创建文件夹失败: ${err}`);
    }
  }, [currentPath, activeServerId, loadDir]);

  // 新建文件
  const handleNewFile = useCallback(async () => {
    setContextMenu(null);
    const fileName = prompt("请输入文件名称：");
    if (!fileName) return;

    const sep = "/";
    const filePath = currentPath === "/"
      ? `${currentPath}${sep}${fileName}`
      : `${currentPath}${sep}${fileName}`;

    try {
      if (activeServerId) {
        // 创建空文件
        await invoke("remote_write_file", {
          serverId: activeServerId,
          path: filePath,
          content: ""
        });
        loadDir(currentPath);
      } else {
        alert("请先连接到远程服务器");
      }
    } catch (err) {
      log.error("new file 失败:", err);
      alert(`创建文件失败: ${err}`);
    }
  }, [currentPath, activeServerId, loadDir]);

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
      alert("请先连接到远程服务器");
      return;
    }

    const remotePath = currentPath === "/"
      ? `/${fileName}`
      : `${currentPath}/${fileName}`;

    log.debug(`检查文件是否存在: ${remotePath}`);

    // 检查远程文件是否存在
    try {
      const fileInfo = await invoke<{ exists: boolean; size?: number; mtime?: number } | null>(
        "check_file_exists",
        { serverId: activeServerId, path: remotePath }
      );

      // 如果文件存在，询问用户是否覆盖
      if (fileInfo && fileInfo.exists) {
        const size = fileInfo.size ? formatSize(fileInfo.size) : '未知';
        const mtime = fileInfo.mtime
          ? new Date(fileInfo.mtime * 1000).toLocaleString('zh-CN')
          : '未知';

        const confirmed = await ask(
          `文件已存在：${fileName}\n\n大小：${size}\n修改时间：${mtime}\n\n是否覆盖？`,
          {
            title: '确认覆盖',
            kind: 'warning',
            okLabel: '是',
            cancelLabel: '否',
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
  }, [activeServerId, currentPath]);

  /**
   * 上传单个本地文件（文件选择对话框与拖拽共用入口）
   *
   * 文件名超过 Linux 字节限制时弹出重命名对话框（用户确认后经
   * handleUploadWithNewName 续传），其余文件不受影响继续上传
   */
  const uploadSingleFile = useCallback(async (localPath: string) => {
    if (!activeServerId) {
      alert("请先连接到远程服务器");
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
      alert("请先连接到远程服务器");
      return;
    }

    try {
      // 打开 Windows 文件选择对话框
      const selectedFiles = await open({
        multiple: true,  // 支持多选
        directory: false, // 选择文件（不是文件夹）
        title: '选择要上传的文件',
        filters: [
          { name: '所有文件', extensions: ['*'] },
          { name: '文本文件', extensions: ['txt', 'md', 'json', 'toml', 'yaml', 'yml'] },
          { name: '脚本文件', extensions: ['sh', 'py', 'js', 'ts', 'rs'] },
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
      alert(`上传失败: ${err}`);
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
      alert("请先连接到远程服务器");
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
          alert(`暂不支持上传文件夹：${name}\n\n请仅拖入文件。`);
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
      alert("请先连接到远程服务器");
      return;
    }

    try {
      await uploadFileWithName(localPath, newName);

      // 关闭右键菜单
      setContextMenu(null);
    } catch (err) {
      log.error('上传失败:', err);
      alert(`上传失败: ${err}`);
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
      alert("请先连接到远程服务器");
      return;
    }

    // 文件夹不能下载
    if (entry.is_dir) {
      alert("暂不支持文件夹下载");
      return;
    }

    try {
      // 构建远程文件路径
      const remotePath = currentPath === "/"
        ? `/${entry.name}`
        : `${currentPath}/${entry.name}`;

      log.info(`下载文件: ${remotePath}`);

      // 打开 Windows 保存对话框
      const localPath = await save({
        defaultPath: entry.name,  // 默认文件名
        title: '保存文件',
        filters: [
          { name: '所有文件', extensions: ['*'] },
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
      alert(`下载失败: ${err}`);
    }
  }, [activeServerId, currentPath]);

  // ── Context Menu ─────────────────────────────────────
  // 智能计算右键菜单位置，紧贴鼠标右下角，避免超出屏幕边界
  const calculateMenuPosition = useCallback((mouseX: number, mouseY: number) => {
    const offset = 3;  // 距离鼠标的距离（紧贴）

    // 简化：直接使用鼠标位置 + offset，不做边界检测
    // 让 CSS 的 position: fixed 自动处理
    const x = mouseX + offset;
    const y = mouseY + offset;

    return { x, y };
  }, []);

  const handleContextMenu = useCallback((e: React.MouseEvent, entry: FileEntry, idx: number) => {
    e.preventDefault();
    e.stopPropagation();  // 阻止事件冒泡到父容器

    // 只有当前已选中该文件时，才显示文件菜单
    // 否则显示刷新菜单（和空白区域一样）
    if (selectedIdx === idx) {
      setContextMenu({ x: e.clientX + 3, y: e.clientY + 3, type: 'file', entry });
    } else {
      // 未选中该文件，显示刷新菜单
      setContextMenu({ x: e.clientX + 3, y: e.clientY + 3, type: 'empty' });
    }
  }, [selectedIdx]);

  // 空白区域右键菜单
  const handleEmptyContextMenu = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    const position = calculateMenuPosition(e.clientX, e.clientY);
    setContextMenu({ ...position, type: 'empty' });
  }, [calculateMenuPosition]);

  // 点击空白区域取消选中
  const handleEmptyClick = useCallback((e: React.MouseEvent) => {
    // 检查点击目标，如果是空白区域则取消选中
    const target = e.target as HTMLElement;
    if (!target.closest('.fm-list-row') && !target.closest('.fm-grid-item')) {
      setSelectedIdx(-1);
    }
  }, []);

  // Close context menu on click anywhere
  useEffect(() => {
    const close = () => setContextMenu(null);
    if (contextMenu) {
      window.addEventListener("click", close);
      return () => window.removeEventListener("click", close);
    }
  }, [contextMenu]);

  // ── Keyboard ─────────────────────────────────────────
  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    // 如果正在编辑路径，不处理任何全局键盘事件
    if (isEditingPath) return;

    // 如果正在编辑文件名，不处理任何键盘事件（编辑输入框会处理）
    if (editingEntry) return;

    if (e.key === "Backspace") { e.preventDefault(); goUp(); }
    if (e.key === "Enter" && selectedIdx !== null) {
      handleOpen(entries[selectedIdx]);
    }
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setSelectedIdx(prev => Math.min((prev ?? -1) + 1, entries.length - 1));
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      setSelectedIdx(prev => Math.max((prev ?? 0) - 1, 0));
    }
  }, [entries, selectedIdx, goUp, handleOpen, editingEntry, isEditingPath]);

  // ── 监听上传完成事件 ─────────────────────────────────
  useEffect(() => {
    let unlisten: (() => void) | null = null;

    const setupListener = async () => {
      const { listen } = await import('@tauri-apps/api/event');
      unlisten = await listen<{ dir_path: string; file_name: string }>('upload-completed', (event) => {
        log.debug('收到上传完成事件:', event.payload);
        const { dir_path } = event.payload;

        // 如果上传的目录是当前目录，刷新
        if (dir_path === currentPath) {
          log.debug('上传目录匹配，刷新当前目录');
          loadDir(currentPath);
        }
      });
    };

    setupListener().catch((e) => log.error('上传完成事件监听设置失败:', e));

    return () => {
      if (unlisten) {
        unlisten();
      }
    };
  }, [currentPath, loadDir]);

  // ── Stats ────────────────────────────────────────────
  const dirCount = entries.filter(e => e.is_dir).length;
  const fileCount = entries.filter(e => !e.is_dir).length;
  const totalSize = entries.filter(e => !e.is_dir).reduce((s, e) => s + e.size, 0);

  return (
    <div className="fm-app" onKeyDown={handleKeyDown} tabIndex={0}>
      {/* 拖拽上传覆盖层：文件拖入窗口时提示上传目标目录（pointer-events:none 不拦截释放） */}
      {dragActive && (
        <div className="fm-drop-overlay">
          <div className="fm-drop-overlay-card">
            <div className="fm-drop-overlay-icon">⬆️</div>
            <div className="fm-drop-overlay-title">释放以上传到当前目录</div>
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
                <div className="fm-sidebar-section-title">{section.title}</div>
                {section.items.map((item, itemIdx) => (
                  <div
                    key={`${sectionIdx}-${itemIdx}`}
                    className={`fm-sidebar-item${currentPath === item.path ? " fm-sidebar-item-active" : ""}`}
                    onClick={() => navigateTo(item.path)}
                  >
                    <span className="fm-si-icon">{item.icon}</span>
                    <span className="fm-si-label">{item.label}</span>
                  </div>
                ))}
              </div>
            ))}
          </div>
        }
        toolbar={
          /* Toolbar - 原HeaderBar功能移到这里 */
          <div className="fm-toolbar">
            {/* Navigation */}
            <div className="fm-toolbar-nav">
              <button className="fm-toolbar-btn" onClick={goBack} disabled={historyIdx <= 0} title="后退">←</button>
              <button className="fm-toolbar-btn" onClick={goForward} disabled={historyIdx >= history.length - 1} title="前进">→</button>
              <button className="fm-toolbar-btn" onClick={goUp} title="上级目录">↑</button>
            </div>

            {/* Path Input */}
            <div className="fm-toolbar-path">
              {/* 面包屑导航：非编辑模式显示 */}
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
                          className={`fm-breadcrumb-segment ${isLast ? 'fm-crumb-current' : ''}`}
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
                title="输入路径并按 Enter 跳转"
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
                      <span className="fm-suggestion-icon">📁</span>
                      <span className="fm-suggestion-text">{suggestion}</span>
                    </div>
                  ))}
                </div>
              )}
            </div>

            {/* Actions */}
            <div className="fm-toolbar-actions">
              <button
                className="fm-toolbar-btn"
                onClick={handleMkdir}
                title="新建文件夹"
                disabled={!activeServerId}
              >📁+</button>
              <button className="fm-toolbar-btn" title="搜索">🔍</button>
            </div>

            {/* View Toggle */}
            <div className="fm-toolbar-view">
              <button
                className={`fm-toolbar-btn ${viewMode === "list" ? "active" : ""}`}
                onClick={() => setViewMode("list")}
                title="列表视图"
              >☰</button>
              <button
                className={`fm-toolbar-btn ${viewMode === "grid" ? "active" : ""}`}
                onClick={() => setViewMode("grid")}
                title="网格视图"
              >⊞</button>
            </div>
          </div>
        }
      >
        {/* File List Content（AppLayout自动处理滚动） */}
        {isOffline ? (
            /* ── 离线空状态 ─────────────────────────────── */
            <div className="fm-offline">
              <div className="offline-icon">📡</div>
              <div className="offline-title">无远程连接</div>
              <div className="offline-desc">请先连接到远程服务器以浏览远程文件系统。</div>

              {servers.length > 0 ? (
                <div className="offline-servers">
                  <div className="offline-servers-label">可用服务器</div>
                  {servers.map((server) => (
                    <button
                      key={server.id}
                      className="offline-server-btn"
                      onClick={() => connectServer(server.id)}
                      disabled={server.status === "connecting"}
                    >
                      <span className="osb-status">
                        {server.status === "connected" ? "🟢" :
                         server.status === "connecting" ? "🟡" :
                         server.status === "error" ? "🔴" : "⚪"}
                      </span>
                      <span className="osb-info">
                        <span className="osb-name">{server.name || server.host}</span>
                        <span className="osb-host">{server.host}:{server.port}</span>
                      </span>
                      <span className="osb-action">
                        {server.status === "connecting" ? "连接中..." : "连接"}
                      </span>
                    </button>
                  ))}
                </div>
              ) : (
                <div className="offline-no-servers">
                  <span>暂无已配置的服务器</span>
                  <span>请前往设置面板添加远程服务器</span>
                </div>
              )}
            </div>
          ) : loading ? (
            <div className="fm-loading">
              <div className="spinner" />
              加载中...
            </div>
          ) : error ? (
            <div className="fm-empty">
              <div className="fm-empty-icon">⚠️</div>
              <div className="fm-empty-text">{error}</div>
            </div>
          ) : entries.length === 0 ? (
            <div
              className="fm-empty"
              onContextMenu={handleEmptyContextMenu}
              onClick={handleEmptyClick}
            >
              <div className="fm-empty-icon">{permissionDenied ? "🔒" : "📂"}</div>
              <div className="fm-empty-text">{permissionDenied ? "无权限读取此目录内容" : "空目录"}</div>
            </div>
          ) : viewMode === "list" ? (
            <div className="fm-list">
              {/* ✅ 表头固定（flex-shrink: 0），不参与滚动 */}
              <div className="fm-list-header">
                <span>名称</span>
                <span>大小</span>
                <span>修改时间</span>
                <span>权限</span>
              </div>
              {/* ✅ 文件列表滚动容器（flex: 1 + overflow-y: auto） */}
              <div
                className="fm-list-content"
                onContextMenu={handleEmptyContextMenu}
                onClick={handleEmptyClick}
              >
                {entries.map((entry, idx) => (
                  <div
                    key={entry.name}
                    className={`fm-list-row${selectedIdx === idx ? " selected" : ""}`}
                    onClick={() => {
                      // 清除之前的定时器
                      if (clickTimerRef.current) {
                        clearTimeout(clickTimerRef.current);
                        clickTimerRef.current = null;
                      }

                      // Adwaita-style: 如果文件已被选中，延迟判断是否为单击（防止双击误触发）
                      if (selectedIdx === idx && editingEntry?.name !== entry.name) {
                        clickTimerRef.current = setTimeout(() => {
                          handleRename(entry);
                          clickTimerRef.current = null;
                        }, 200);  // 200ms 延迟，更快的响应，确保双击不会触发重命名
                      } else {
                        setSelectedIdx(idx);
                      }
                    }}
                    onDoubleClick={() => {
                      // 双击时，清除单击的定时器，防止触发重命名
                      if (clickTimerRef.current) {
                        clearTimeout(clickTimerRef.current);
                        clickTimerRef.current = null;
                      }
                      handleOpen(entry);
                    }}
                    onContextMenu={(e) => handleContextMenu(e, entry, idx)}
                  >
                    <div className="fm-file-name">
                      <span className="fm-fn-icon">{getFileIcon(entry)}</span>
                      {editingEntry?.name === entry.name ? (
                        <input
                          type="text"
                          className="fm-fn-edit-input"
                          value={editingName}
                          onChange={(e) => setEditingName(e.target.value)}
                          onKeyDown={handleEditKeyDown}
                          onBlur={finishRename}
                          autoFocus
                          onClick={(e) => e.stopPropagation()}
                          onDoubleClick={(e) => e.stopPropagation()}  // 阻止双击事件冒泡
                          style={{ width: `${Math.max(editingName.length + 0.5, 4)}ch` }}  // 动态宽度：文本长度 + 0.5字符，最小4字符
                        />
                      ) : (
                        <span className="fm-fn-text">{entry.name}</span>
                      )}
                    </div>
                    <span className="fm-file-size">{entry.is_dir ? "—" : formatSize(entry.size)}</span>
                    <span className="fm-file-mtime">{formatDate(entry.mtime)}</span>
                    <span className="fm-file-perm">{entry.permissions}</span>
                  </div>
                ))}
              </div>
            </div>
          ) : (
            <div
              className="fm-grid"
              onContextMenu={handleEmptyContextMenu}
              onClick={handleEmptyClick}
            >
              {entries.map((entry, idx) => (
                <div
                  key={entry.name}
                  className={`fm-grid-item${selectedIdx === idx ? " selected" : ""}`}
                  onClick={() => {
                    // 清除之前的定时器
                    if (clickTimerRef.current) {
                      clearTimeout(clickTimerRef.current);
                      clickTimerRef.current = null;
                    }

                    // Adwaita-style: 如果文件已被选中，延迟判断是否为单击（防止双击误触发）
                    if (selectedIdx === idx && editingEntry?.name !== entry.name) {
                      clickTimerRef.current = setTimeout(() => {
                        handleRename(entry);
                        clickTimerRef.current = null;
                      }, 200);  // 200ms 延迟，更快的响应，确保双击不会触发重命名
                    } else {
                      setSelectedIdx(idx);
                    }
                  }}
                  onDoubleClick={() => {
                    // 双击时，清除单击的定时器，防止触发重命名
                    if (clickTimerRef.current) {
                      clearTimeout(clickTimerRef.current);
                      clickTimerRef.current = null;
                    }
                    handleOpen(entry);
                  }}
                  onContextMenu={(e) => handleContextMenu(e, entry, idx)}
                >
                  <div className="fm-gi-icon">{getFileIcon(entry)}</div>
                  {editingEntry?.name === entry.name ? (
                    <input
                      type="text"
                      className="fm-gi-edit-input"
                      value={editingName}
                      onChange={(e) => setEditingName(e.target.value)}
                      onKeyDown={handleEditKeyDown}
                      onBlur={finishRename}
                      autoFocus
                      onClick={(e) => e.stopPropagation()}
                      onDoubleClick={(e) => e.stopPropagation()}  // 阻止双击事件冒泡
                      style={{ width: `${Math.max(editingName.length + 0.5, 4)}ch` }}  // 动态宽度：文本长度 + 0.5字符，最小4字符
                    />
                  ) : (
                    <div className="fm-gi-label">{entry.name}</div>
                  )}
                </div>
              ))}
            </div>
          )}
        </AppLayout>

      {/* Status Bar */}
      <div className="fm-statusbar">
        {/* 左侧：原有的文件数量、总大小信息 */}
        <div className="fm-statusbar-left">
          <span>{isOffline ? `${PLACEHOLDER} 个文件夹, ${PLACEHOLDER} 个文件` : `${dirCount} 个文件夹, ${fileCount} 个文件`}</span>
          <span>总大小: {isOffline ? PLACEHOLDER : formatSize(totalSize)}</span>
        </div>

        {/* 右侧：传输状态（新功能） */}
        <div className="fm-statusbar-right">
          <TransferStatusBar />
        </div>
      </div>

      {/* Context Menu - 使用 Portal 渲染到 body，确保 position: fixed 相对于视口 */}
      {contextMenu && createPortal(
        <div
          className="fm-context-menu"
          style={{ left: contextMenu.x, top: contextMenu.y }}
        >
          {contextMenu.type === 'file' && contextMenu.entry && (
            // 文件/文件夹菜单
            <>
              <div className="fm-ctx-item" onClick={() => { handleOpen(contextMenu.entry!); setContextMenu(null); }}>
                <span className="fm-ctx-icon">📂</span> 打开
              </div>
              <div className="fm-ctx-separator" />

              {/* 文件操作 */}
              <div className="fm-ctx-item" onClick={() => { handleRename(contextMenu.entry!); setContextMenu(null); }}>
                <span className="fm-ctx-icon">✏️</span> 重命名
              </div>
              <div className="fm-ctx-item" onClick={() => { handleDelete(contextMenu.entry!); setContextMenu(null); }}>
                <span className="fm-ctx-icon">🗑️</span> 删除
              </div>

              <div className="fm-ctx-separator" />

              {/* 其他操作 */}
              <div className="fm-ctx-item" onClick={() => {
                // 复制路径到剪贴板（Linux 格式）
                const sep = "/";
                const path = currentPath === "/"
                  ? `${currentPath}${sep}${contextMenu.entry!.name}`
                  : `${currentPath}${sep}${contextMenu.entry!.name}`;
                navigator.clipboard.writeText(path);
                setContextMenu(null);
              }}>
                <span className="fm-ctx-icon">📋</span> 复制路径
              </div>

              {/* 下载功能（仅文件） */}
              {!contextMenu.entry!.is_dir && (
                <div className="fm-ctx-item" onClick={() => {
                  handleDownload(contextMenu.entry!);
                }}>
                  <span className="fm-ctx-icon">⬇️</span> 下载
                </div>
              )}

              <div className="fm-ctx-separator" />
              <div className="fm-ctx-item" onClick={() => { showProperties(contextMenu.entry!); setContextMenu(null); }}>
                <span className="fm-ctx-icon">ℹ️</span> 属性
              </div>
            </>
          )}

          {contextMenu.type === 'empty' && (
            // 空白区域菜单
            <>
              <div className="fm-ctx-item" onClick={handleRefresh}>
                <span className="fm-ctx-icon">🔄</span> 刷新
              </div>
              <div className="fm-ctx-separator" />
              <div className="fm-ctx-item" onClick={handleNewFolder}>
                <span className="fm-ctx-icon">📁</span> 新建文件夹
              </div>
              <div className="fm-ctx-item" onClick={handleNewFile}>
                <span className="fm-ctx-icon">📄</span> 新建文件
              </div>

              {/* 上传功能 */}
              <div className="fm-ctx-item" onClick={handleUpload}>
                <span className="fm-ctx-icon">⬆️</span> 上传文件
              </div>
            </>
          )}
        </div>,
        document.body
      )}

      {/* Path Error Dialog — 路径不存在时弹窗提示（模态）- 使用 Portal */}
      {pathErrorDialog && createPortal(
        <div className="fm-path-error-overlay">
          <div className="fm-path-error-dialog">
            <div className="ped-header">
              <span className="ped-icon">⚠️</span>
              <span className="ped-title">路径不存在</span>
            </div>
            <div className="ped-message">{pathErrorDialog}</div>
            <div className="ped-footer">
              <button autoFocus onClick={() => setPathErrorDialog(null)}>确定</button>
            </div>
          </div>
        </div>,
        document.body
      )}

      {/* Rename Dialog - 文件名过长时弹窗重命名（模态）- 使用 Portal */}
      {renameDialog && createPortal(
        <div
          className="fm-path-error-overlay"
          onClick={(e) => e.stopPropagation()}
          onKeyDown={(e) => e.stopPropagation()}
          onKeyUp={(e) => e.stopPropagation()}
          onKeyPress={(e) => e.stopPropagation()}
        >
          <div className="fm-rename-dialog">
            <div className="rd-header">
              <span className="rd-icon">📝</span>
              <span className="rd-title">文件名过长，请重命名</span>
            </div>
            <div className="rd-content">
              <div className="rd-original">
                <div className="rd-label">原始文件名：</div>
                <div className="rd-original-name">
                  {renameDialog.originalName.length > 60
                    ? renameDialog.originalName.substring(0, 60) + '...'
                    : renameDialog.originalName}
                </div>
              </div>
              <div className="rd-input-section">
                <div className="rd-label">新文件名：</div>
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
                    // Escape 键取消
                    if (e.key === 'Escape') {
                      setRenameDialog(null);
                    }
                  }}
                  onKeyUp={(e) => e.stopPropagation()}
                  onKeyPress={(e) => e.stopPropagation()}
                  placeholder="请输入新文件名"
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
                        <>剩余 {remaining} 字节</>
                      ) : (
                        <>超出 {Math.abs(remaining)} 字节</>
                      )}
                      <span className="rd-byte-hint">（当前 {currentBytes} / 最大 {renameDialog.maxBytes} 字节）</span>
                    </span>
                  );
                })()}
              </div>
            </div>
            <div className="rd-footer">
              <button className="rd-btn-cancel" onClick={() => setRenameDialog(null)}>
                取消
              </button>
              <button
                className="rd-btn-confirm"
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
                确定
              </button>
            </div>
          </div>
        </div>,
        document.body
      )}

      {/* Properties Dialog - 使用 Portal */}
      {propertiesEntry && createPortal(
        <div className="fm-properties-dialog">
          <div className="pd-header">
            <span className="pd-icon">{getFileIcon(propertiesEntry)}</span>
            <span className="pd-name">{propertiesEntry.name}</span>
          </div>
          <div className="pd-content">
            <div className="pd-row">
              <span className="pd-label">类型:</span>
              <span className="pd-value">{propertiesEntry.is_dir ? "文件夹" : "文件"}</span>
            </div>
            <div className="pd-row">
              <span className="pd-label">大小:</span>
              <span className="pd-value">{formatSize(propertiesEntry.size)}</span>
            </div>
            <div className="pd-row">
              <span className="pd-label">修改时间:</span>
              <span className="pd-value">{formatDate(propertiesEntry.mtime)}</span>
            </div>
            <div className="pd-row">
              <span className="pd-label">权限:</span>
              <span className="pd-value">{propertiesEntry.permissions}</span>
            </div>
          </div>
          <div className="pd-footer">
            <button onClick={() => setPropertiesEntry(null)}>关闭</button>
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
