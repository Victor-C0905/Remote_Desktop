import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useServerManager } from "../context/ServerManager";
import { PLACEHOLDER } from "../utils/offlineDefaults";
import { useWindowManager } from "../window-system/WindowManagerContext";
import { AppLayout } from "../components/app-shell";
import "./FileManager.css";

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

export function FileManager({ preloadData }: FileManagerProps) {
  // ── 窗口系统集成 ─────────────────────────────────────
  // 获取窗口管理器
  const { manager } = useWindowManager();

  // 从 props 获取预加载数据
  const initialData = preloadData;

  const { activeServerId, servers, connectServer } = useServerManager();

  // ── 离线状态判断 ─────────────────────────────────────
  // 无活跃连接 = 离线模式（显示空状态占位符，而非空白或本地文件）
  const isOffline = !activeServerId;

  // 路径系统：始终使用 Linux 远程路径格式（远程 Linux 服务器控制工具）
  const [currentPath, setCurrentPath] = useState(() => "/home");
  const [entries, setEntries] = useState<FileEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selectedIdx, setSelectedIdx] = useState<number | null>(null);
  const [username, setUsername] = useState<string | null>(null);
  const [viewMode, setViewMode] = useState<ViewMode>("list");
  const [history, setHistory] = useState<string[]>([currentPath]);
  const [historyIdx, setHistoryIdx] = useState(0);
  const [contextMenu, setContextMenu] = useState<{
    x: number; y: number; entry: FileEntry;
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

  // 输入框 ref（用于检测点击位置）
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
    setSelectedIdx(null);
    setContextMenu(null);

    // 调试：打印当前状态
    console.log("[FileManager] loadDir 调用:", {
      path,
      activeServerId,
      isRemote: !!activeServerId,
    });

    try {
      let resp: ReadDirResponse | null = null;

      if (activeServerId) {
        // 远程模式：通过 QUIC 从 Agent 获取
        console.log("[FileManager] 远程模式，调用 remote_read_dir");
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
      setError(e.toString());
      setEntries([]);
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

      console.log("[FileManager] fetchSuggestions: 输入路径:", path, "父目录:", parentPath, "部分名称:", partialName);

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

      console.log("[FileManager] fetchSuggestions: 建议列表:", suggestions);

      setSuggestions(suggestions);
      setShowSuggestions(suggestions.length > 0);
      setSelectedSuggestionIdx(null);
    } catch (err) {
      console.error("[FileManager] 获取建议失败:", err);
      setSuggestions([]);
      setShowSuggestions(false);
    }
  }, [activeServerId]);

  // ── 父级数据注入：优先使用预加载数据 ─────────────────
  // 当 Desktop 通过 usePreloader 预加载完成后注入 initialData 时，
  // 直接初始化所有状态，跳过后续的首次加载 useEffect
  useEffect(() => {
    if (initialData) {
      console.log("[FileManager] 使用父级注入的初始数据");
      setUsername(initialData.username);
      setCurrentPath(initialData.currentPath);
      setEntries(initialData.entries);
      setMounts(initialData.mounts);
      setSidebarSections(initialData.sidebarSections);
      setHistory([initialData.currentPath]);
      setPathInput(initialData.currentPath);
      setLoading(false);
    }
  }, [initialData]);

  // HOME_PATH：远程 Linux 默认路径
  const HOME_PATH = "/home";

  // 获取远程服务器用户名
  useEffect(() => {
    if (initialData) return; // 有注入数据时跳过，已通过 initialData 初始化
    console.log("[FileManager] 获取用户名 useEffect, activeServerId:", activeServerId);
    if (activeServerId) {
      // 连接建立后，获取用户名
      console.log("[FileManager] 调用 remote_get_current_user, serverId:", activeServerId);
      invoke<string>("remote_get_current_user", { serverId: activeServerId })
        .then((username) => {
          console.log("[FileManager] 获取用户名成功:", username);
          setUsername(username);
        })
        .catch(err => {
          console.error("[FileManager] 获取用户名失败:", err);
          setUsername("user");  // 默认值
        });
    } else {
      // 离线模式：清空用户名
      setUsername(null);
    }
  }, [activeServerId, initialData]);

  // Initial load
  useEffect(() => {
    if (initialData) return; // 有注入数据时跳过，已通过 initialData 初始化
    console.log("[FileManager] 初始加载 useEffect, activeServerId:", activeServerId, "username:", username);
    if (activeServerId && username) {
      // 远程模式：有用户名后，加载初始目录
      const homePath = `/home/${username}`;
      console.log("[FileManager] 远程模式，加载路径:", homePath);
      loadDir(homePath);
    } else if (!activeServerId) {
      // 离线模式：不加载（显示离线占位符）
      console.log("[FileManager] 离线模式，不加载");
    } else {
      console.log("[FileManager] 等待用户名...");
    }
  }, [activeServerId, username, loadDir, HOME_PATH, initialData]);

  // 获取挂载点列表
  useEffect(() => {
    if (initialData) return; // 有注入数据时跳过，已通过 initialData 初始化
    console.log("[FileManager] 获取挂载点 useEffect, activeServerId:", activeServerId);
    if (activeServerId) {
      // 远程模式：获取挂载点列表
      console.log("[FileManager] 调用 remote_get_mounts, serverId:", activeServerId);
      invoke<MountInfo[]>("remote_get_mounts", { serverId: activeServerId })
        .then((mounts) => {
          console.log("[FileManager] 获取挂载点成功:", mounts);
          setMounts(mounts);
        })
        .catch(err => {
          console.error("[FileManager] 获取挂载点失败:", err);
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
    console.log("[FileManager] 生成侧边栏 useEffect, activeServerId:", activeServerId, "username:", username, "mounts:", mounts.length);
    if (activeServerId && username) {
      // 远程模式：生成远程侧边栏
      const sections: SidebarSection[] = [
        {
          title: "位置",
          items: [
            { icon: "🏠", label: "主目录", path: `/home/${username}`, type: "bookmark" },
            { icon: "📄", label: "文档", path: `/home/${username}/Documents`, type: "bookmark" },
            { icon: "⬇️", label: "下载", path: `/home/${username}/Downloads`, type: "bookmark" },
            { icon: "🖼️", label: "图片", path: `/home/${username}/Pictures`, type: "bookmark" },
            { icon: "🎵", label: "音乐", path: `/home/${username}/Music`, type: "bookmark" },
            { icon: "🎬", label: "视频", path: `/home/${username}/Videos`, type: "bookmark" },
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
      console.log("[FileManager] 远程侧边栏生成完成:", sections);
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
  }, [activeServerId, username, mounts, initialData]);

  // ── 断连即时响应 ─────────────────────────────────────
  // 当 activeServerId 从有值变为 null（断连），立即清空数据并显示离线占位符
  useEffect(() => {
    if (!activeServerId) {
      // 断连：立即重置为离线状态（不清空侧边栏，保留 UI 结构）
      setEntries([]);
      setError(null);
      setLoading(false);
      setSelectedIdx(null);
      setContextMenu(null);
      console.log("[FileManager] 检测到断连，切换到离线模式");
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
      setLoading(false);
      setSelectedIdx(null);
      setContextMenu(null);
      setUsername(null);  // 清空，让 username useEffect 重新获取
      setMounts([]);
      setSidebarSections([]);
      // 重置路径为 Linux 远程格式（避免 C:\ 残留）
      setCurrentPath("/home");
      setHistory(["/home"]);
      setHistoryIdx(0);
      setPathInput("/home");
      console.log("[FileManager] 检测到新连接，已清空本地残留数据");
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

  const handlePathInputFocus = () => {
    setIsEditingPath(true);
  };

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

    console.log("[FileManager] navigateTo: 输入路径:", path, "清理后:", cleanPath);

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

      console.log(`[FileManager] Open file in editor: ${filePath}`);

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

    console.log("[FileManager] mkdir:", newPath);

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
      console.error("[FileManager] mkdir 失败:", err);
      alert(`创建文件夹失败: ${err}`);
    }
  }, [currentPath, activeServerId, loadDir]);

  const handleRename = useCallback((entry: FileEntry) => {
    // GNOME-style inline editing：直接在文件名上编辑
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

    console.log("[FileManager] rename:", oldPath, "->", newPath);

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
      console.error("[FileManager] rename 失败:", err);
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

    console.log("[FileManager] delete:", path);

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
      console.error("[FileManager] delete 失败:", err);
      alert(`删除失败: ${err}`);
    }
  }, [currentPath, activeServerId, loadDir]);

  // ── Context Menu ─────────────────────────────────────
  const handleContextMenu = useCallback((e: React.MouseEvent, entry: FileEntry, idx: number) => {
    e.preventDefault();
    setSelectedIdx(idx);
    setContextMenu({ x: e.clientX, y: e.clientY, entry });
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

  // ── Stats ────────────────────────────────────────────
  const dirCount = entries.filter(e => e.is_dir).length;
  const fileCount = entries.filter(e => !e.is_dir).length;
  const totalSize = entries.filter(e => !e.is_dir).reduce((s, e) => s + e.size, 0);

  return (
    <div className="fm-app" onKeyDown={handleKeyDown} tabIndex={0}>
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
              <input
                ref={pathInputRef}
                type="text"
                className="fm-path-input"
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
            <div className="fm-empty">
              <div className="fm-empty-icon">📂</div>
              <div className="fm-empty-text">空目录</div>
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
              <div className="fm-list-content">
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

                      // GNOME-style: 如果文件已被选中，延迟判断是否为单击（防止双击误触发）
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
            <div className="fm-grid">
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

                    // GNOME-style: 如果文件已被选中，延迟判断是否为单击（防止双击误触发）
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
        <span>{isOffline ? `${PLACEHOLDER} 个文件夹, ${PLACEHOLDER} 个文件` : `${dirCount} 个文件夹, ${fileCount} 个文件`}</span>
        <span>总大小: {isOffline ? PLACEHOLDER : formatSize(totalSize)}</span>
      </div>

      {/* Context Menu */}
      {contextMenu && (
        <div
          className="fm-context-menu"
          style={{ left: contextMenu.x, top: contextMenu.y }}
        >
          <div className="fm-fm-ctx-item" onClick={() => { handleOpen(contextMenu.entry); setContextMenu(null); }}>
            <span className="fm-fm-ctx-icon">📂</span> 打开
          </div>
          <div className="fm-ctx-separator" />

          {/* 文件操作 */}
          <div className="fm-ctx-item" onClick={() => { handleRename(contextMenu.entry); setContextMenu(null); }}>
            <span className="fm-ctx-icon">✏️</span> 重命名
          </div>
          <div className="fm-ctx-item" onClick={() => { handleDelete(contextMenu.entry); setContextMenu(null); }}>
            <span className="fm-ctx-icon">🗑️</span> 删除
          </div>

          <div className="fm-ctx-separator" />

          {/* 其他操作 */}
          <div className="fm-ctx-item" onClick={() => {
            // 复制路径到剪贴板（Linux 格式）
            const sep = "/";
            const path = currentPath === "/"
              ? `${currentPath}${sep}${contextMenu.entry.name}`
              : `${currentPath}${sep}${contextMenu.entry.name}`;
            navigator.clipboard.writeText(path);
            setContextMenu(null);
          }}>
            <span className="fm-ctx-icon">📋</span> 复制路径
          </div>
          <div className="fm-ctx-item" onClick={() => setContextMenu(null)}>
            <span className="fm-ctx-icon">⬇️</span> 下载
          </div>

          <div className="fm-ctx-separator" />
          <div className="fm-ctx-item" onClick={() => { showProperties(contextMenu.entry); setContextMenu(null); }}>
            <span className="fm-ctx-icon">ℹ️</span> 属性
          </div>
        </div>
      )}

      {/* Path Error Dialog — 路径不存在时弹窗提示（模态） */}
      {pathErrorDialog && (
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
        </div>
      )}

      {/* Properties Dialog */}
      {propertiesEntry && (
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
        </div>
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
