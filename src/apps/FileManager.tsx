import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useServerManager } from "../context/ServerManager";
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

/* ── Sidebar Bookmarks ─────────────────────────────────── */

interface Bookmark {
  icon: string;
  label: string;
  path: string;
}

const IS_WIN = typeof navigator !== "undefined" && navigator.platform.startsWith("Win");

const BOOKMARKS: Bookmark[] = IS_WIN ? [
  { icon: "🏠", label: "用户目录", path: "C:\\Users" },
  { icon: "📄", label: "文档", path: "C:\\Users\\Public\\Documents" },
  { icon: "⬇️", label: "下载", path: "C:\\Users\\Public\\Downloads" },
  { icon: "🖼️", label: "图片", path: "C:\\Users\\Public\\Pictures" },
] : [
  { icon: "🏠", label: "主目录", path: "/home" },
  { icon: "📄", label: "文档", path: "/home/user/Documents" },
  { icon: "⬇️", label: "下载", path: "/home/user/Downloads" },
  { icon: "🖼️", label: "图片", path: "/home/user/Pictures" },
  { icon: "🎵", label: "音乐", path: "/home/user/Music" },
];

const PLACES: Bookmark[] = IS_WIN ? [
  { icon: "💻", label: "C 盘", path: "C:\\" },
  { icon: "💾", label: "D 盘", path: "D:\\" },
] : [
  { icon: "💻", label: "计算机", path: "/" },
  { icon: "📁", label: "临时文件", path: "/tmp" },
];

/* ── Component ─────────────────────────────────────────── */

type ViewMode = "list" | "grid";

export function FileManager() {
  const { activeServerId } = useServerManager();
  
  const [currentPath, setCurrentPath] = useState(() => {
    if (navigator.platform.startsWith("Win")) {
      return "C:\\Users";
    }
    return "/home";
  });
  const [entries, setEntries] = useState<FileEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selectedIdx, setSelectedIdx] = useState<number | null>(null);
  const [viewMode, setViewMode] = useState<ViewMode>("list");
  const [history, setHistory] = useState<string[]>([currentPath]);
  const [historyIdx, setHistoryIdx] = useState(0);
  const pathSep = IS_WIN ? "\\" : "/";
  const [contextMenu, setContextMenu] = useState<{
    x: number; y: number; entry: FileEntry;
  } | null>(null);
  const mainRef = useRef<HTMLDivElement>(null);

  const loadDir = useCallback(async (path: string) => {
    setLoading(true);
    setError(null);
    setSelectedIdx(null);
    setContextMenu(null);

    try {
      let resp: ReadDirResponse | null = null;
      
      if (activeServerId) {
        // 远程模式：通过 QUIC 从 Agent 获取
        resp = await invoke<ReadDirResponse>("remote_read_dir", {
          serverId: activeServerId,
          path,
        });
      } else {
        // 本地模式：直接读取本地文件系统
        resp = await invoke<ReadDirResponse>("read_dir", { path });
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

      setCurrentPath(path);
    } catch (e: any) {
      setError(e.toString());
      setEntries([]);
    } finally {
      setLoading(false);
    }
  }, [activeServerId]);

  const HOME_PATH = navigator.platform.startsWith("Win") ? "C:\\Users" : "/home";

  // Initial load
  useEffect(() => { loadDir(HOME_PATH); }, [loadDir, HOME_PATH]);

  // ── Navigation ──────────────────────────────────────
  const navigateTo = useCallback((path: string) => {
    const newHistory = history.slice(0, historyIdx + 1);
    newHistory.push(path);
    setHistory(newHistory);
    setHistoryIdx(newHistory.length - 1);
    loadDir(path);
  }, [history, historyIdx, loadDir]);

  const goBack = useCallback(() => {
    if (historyIdx > 0) {
      setHistoryIdx(historyIdx - 1);
      loadDir(history[historyIdx - 1]);
    }
  }, [history, historyIdx, loadDir]);

  const goUp = useCallback(() => {
    if (IS_WIN) {
      // Windows: "C:\Users\foo" -> "C:\Users"
      const parts = currentPath.split("\\").filter(Boolean);
      if (parts.length <= 1) return; // Already at root like "C:"
      const parent = parts.slice(0, -1).join("\\");
      navigateTo(parent);
    } else {
      const parent = currentPath.split("/").slice(0, -1).join("/") || "/";
      navigateTo(parent);
    }
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
      const sep = IS_WIN ? "\\" : "/";
      const newPath = currentPath === "/" || (IS_WIN && currentPath.endsWith(":"))
        ? `${currentPath}${sep}${entry.name}`
        : `${currentPath}${sep}${entry.name}`;
      navigateTo(newPath);
    } else {
      // TODO: file preview / download
      console.log(`[FileManager] Open file: ${currentPath}/${entry.name}`);
    }
  }, [currentPath, navigateTo]);

  // Breadcrumb click
  const handleCrumbClick = useCallback((idx: number) => {
    const targetPath = pathParts.slice(0, idx + 1).join(pathSep);
    navigateTo(IS_WIN ? targetPath : "/" + targetPath);
  }, [currentPath, navigateTo, pathSep]);

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
  }, [entries, selectedIdx, goUp, handleOpen]);

  // ── Breadcrumb parts ─────────────────────────────────
  const pathParts = currentPath.split(pathSep).filter(Boolean);

  // ── Stats ────────────────────────────────────────────
  const dirCount = entries.filter(e => e.is_dir).length;
  const fileCount = entries.filter(e => !e.is_dir).length;
  const totalSize = entries.filter(e => !e.is_dir).reduce((s, e) => s + e.size, 0);

  return (
    <div className="fm" onKeyDown={handleKeyDown} tabIndex={0}>
      {/* Header Bar */}
      <div className="fm-headerbar">
        <button className="nav-btn" onClick={goBack} disabled={historyIdx <= 0} title="后退">←</button>
        <button className="nav-btn" onClick={goForward} disabled={historyIdx >= history.length - 1} title="前进">→</button>
        <button className="nav-btn" onClick={goUp} title="上级目录">↑</button>

        {/* Breadcrumb */}
        <div className="fm-breadcrumb">
          <button className="crumb" onClick={() => navigateTo(IS_WIN ? "C:\\" : "/")}>
            {IS_WIN ? "C:" : "/"}
          </button>
          {pathParts.map((_, i) => (
            <span key={`sep-${i}`} className="sep">{pathSep}</span>
          ))}
          {pathParts.map((part, i) => (
            <button
              key={i}
              className={`crumb${i === pathParts.length - 1 ? " current" : ""}`}
              onClick={() => handleCrumbClick(i)}
            >
              {part}
            </button>
          ))}
        </div>

        {/* View Toggle */}
        <div className="view-toggle">
          <button
            className={viewMode === "list" ? "active" : ""}
            onClick={() => setViewMode("list")}
            title="列表视图"
          >☰</button>
          <button
            className={viewMode === "grid" ? "active" : ""}
            onClick={() => setViewMode("grid")}
            title="网格视图"
          >⊞</button>
        </div>

        <button className="search-btn" title="搜索">🔍</button>
      </div>

      {/* Content */}
      <div className="fm-content">
        {/* Sidebar */}
        <div className="fm-sidebar">
          <div className="sidebar-section">
            <div className="sidebar-section-title">位置</div>
            {BOOKMARKS.map((bm) => (
              <div
                key={bm.path}
                className={`sidebar-item${currentPath === bm.path ? " active" : ""}`}
                onClick={() => navigateTo(bm.path)}
              >
                <span className="si-icon">{bm.icon}</span>
                <span className="si-label">{bm.label}</span>
              </div>
            ))}
          </div>
          <div className="sidebar-section">
            <div className="sidebar-section-title">设备</div>
            {PLACES.map((p) => (
              <div
                key={p.path}
                className={`sidebar-item${currentPath === p.path ? " active" : ""}`}
                onClick={() => navigateTo(p.path)}
              >
                <span className="si-icon">{p.icon}</span>
                <span className="si-label">{p.label}</span>
              </div>
            ))}
          </div>
        </div>

        {/* Main Area */}
        <div className="fm-main" ref={mainRef}>
          {loading ? (
            <div className="fm-loading">
              <div className="spinner" />
              加载中...
            </div>
          ) : error ? (
            <div className="fm-empty">
              <div className="empty-icon">⚠️</div>
              <div className="empty-text">{error}</div>
            </div>
          ) : entries.length === 0 ? (
            <div className="fm-empty">
              <div className="empty-icon">📂</div>
              <div className="empty-text">空目录</div>
            </div>
          ) : viewMode === "list" ? (
            <div className="fm-list">
              <div className="fm-list-header">
                <span>名称</span>
                <span>大小</span>
                <span>修改时间</span>
                <span>权限</span>
              </div>
              {entries.map((entry, idx) => (
                <div
                  key={entry.name}
                  className={`fm-list-row${selectedIdx === idx ? " selected" : ""}`}
                  onClick={() => setSelectedIdx(idx)}
                  onDoubleClick={() => handleOpen(entry)}
                  onContextMenu={(e) => handleContextMenu(e, entry, idx)}
                >
                  <div className="file-name">
                    <span className="fn-icon">{getFileIcon(entry)}</span>
                    <span className="fn-text">{entry.name}</span>
                  </div>
                  <span className="file-size">{entry.is_dir ? "—" : formatSize(entry.size)}</span>
                  <span className="file-mtime">{formatDate(entry.mtime)}</span>
                  <span className="file-perm">{entry.permissions}</span>
                </div>
              ))}
            </div>
          ) : (
            <div className="fm-grid">
              {entries.map((entry, idx) => (
                <div
                  key={entry.name}
                  className={`fm-grid-item${selectedIdx === idx ? " selected" : ""}`}
                  onClick={() => setSelectedIdx(idx)}
                  onDoubleClick={() => handleOpen(entry)}
                  onContextMenu={(e) => handleContextMenu(e, entry, idx)}
                >
                  <div className="gi-icon">{getFileIcon(entry)}</div>
                  <div className="gi-label">{entry.name}</div>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>

      {/* Status Bar */}
      <div className="fm-statusbar">
        <span>{dirCount} 个文件夹, {fileCount} 个文件</span>
        <span>总大小: {formatSize(totalSize)}</span>
      </div>

      {/* Context Menu */}
      {contextMenu && (
        <div
          className="fm-context-menu"
          style={{ left: contextMenu.x, top: contextMenu.y }}
        >
          <div className="ctx-item" onClick={() => { handleOpen(contextMenu.entry); setContextMenu(null); }}>
            <span className="ctx-icon">📂</span> 打开
          </div>
          <div className="ctx-separator" />
          <div className="ctx-item" onClick={() => setContextMenu(null)}>
            <span className="ctx-icon">📋</span> 复制路径
          </div>
          <div className="ctx-item" onClick={() => setContextMenu(null)}>
            <span className="ctx-icon">⬇️</span> 下载
          </div>
          <div className="ctx-separator" />
          <div className="ctx-item" onClick={() => setContextMenu(null)}>
            <span className="ctx-icon">🗑️</span> 删除
          </div>
          <div className="ctx-item" onClick={() => setContextMenu(null)}>
            <span className="ctx-icon">✏️</span> 重命名
          </div>
          <div className="ctx-separator" />
          <div className="ctx-item" onClick={() => setContextMenu(null)}>
            <span className="ctx-icon">ℹ️</span> 属性
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
