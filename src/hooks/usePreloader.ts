import { useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";

/* ── Types (与 FileManager 共享) ─────────────────────── */

export interface FileEntry {
  name: string;
  is_dir: boolean;
  size: number;
  mtime: string;
  permissions: string;
}

export interface MountInfo {
  mount_point: string;
  device: string;
  filesystem: string;
  total_bytes: number;
  used_bytes: number;
}

export interface SidebarItem {
  icon: string;
  label: string;
  path: string;
  type: "bookmark" | "mount" | "network";
}

export interface SidebarSection {
  title: string;
  items: SidebarItem[];
}

/**
 * FileManager 预加载所需的全部初始数据
 * 由 Desktop 父级通过 usePreloader 获取后注入 FileManager 组件
 */
export interface FileManagerInitialData {
  username: string | null;
  currentPath: string;
  entries: FileEntry[];
  mounts: MountInfo[];
  sidebarSections: SidebarSection[];
}

type PreloadState = 'idle' | 'loading' | 'ready' | 'error';

interface PreloaderResult {
  state: PreloadState;
  data: FileManagerInitialData | null;
  error: string | null;
  preload: (serverId: string | null) => Promise<void>;
  reset: () => void;
}

/* ── Helper: 构建侧边栏数据 ──────────────────────────── */

function buildSidebarSections(
  username: string | null,
  mounts: MountInfo[],
  activeServerId: string | null,
  IS_WIN: boolean
): SidebarSection[] {
  if (activeServerId && username) {
    // 远程模式侧边栏
    return [
      {
        title: "位置",
        items: [
          { icon: "\u{1F3E0}", label: "主目录", path: `/home/${username}`, type: "bookmark" },
          { icon: "\u{1F4C4}", label: "文档", path: `/home/${username}/Documents`, type: "bookmark" },
          { icon: "\u{2B07}\uFE0F", label: "下载", path: `/home/${username}/Downloads`, type: "bookmark" },
          { icon: "\u{1F5BC}\uFE0F", label: "图片", path: `/home/${username}/Pictures`, type: "bookmark" },
          { icon: "\u{1F3B5}", label: "音乐", path: `/home/${username}/Music`, type: "bookmark" },
          { icon: "\u{1F3AC}", label: "视频", path: `/home/${username}/Videos`, type: "bookmark" },
        ],
      },
      {
        title: "设备",
        items: mounts.map((m) => ({
          icon: "\u{1F4BE}",
          label: m.mount_point,
          path: m.mount_point,
          type: "mount" as const,
        })),
      },
      {
        title: "其他位置",
        items: [{ icon: "\u{1F310}", label: "网络", path: "/network", type: "network" as const }],
      },
    ];
  }

  if (!activeServerId) {
    // 本地模式侧边栏
    const bookmarks = IS_WIN
      ? [
          { icon: "\u{1F3E0}", label: "主目录", path: "C:\\Users", type: "bookmark" as const },
          { icon: "\u{1F4C4}", label: "文档", path: "C:\\Users\\Public\\Documents", type: "bookmark" as const },
          { icon: "\u{2B07}\uFE0F", label: "下载", path: "C:\\Users\\Public\\Downloads", type: "bookmark" as const },
          { icon: "\u{1F5BC}\uFE0F", label: "图片", path: "C:\\Users\\Public\\Pictures", type: "bookmark" as const },
          { icon: "\u{1F3B5}", label: "音乐", path: "C:\\Users\\Public\\Music", type: "bookmark" as const },
          { icon: "\u{1F3AC}", label: "视频", path: "C:\\Users\\Public\\Videos", type: "bookmark" as const },
        ]
      : [
          { icon: "\u{1F3E0}", label: "主目录", path: "/home", type: "bookmark" as const },
          { icon: "\u{1F4C4}", label: "文档", path: "/home/Documents", type: "bookmark" as const },
          { icon: "\u{2B07}\uFE0F", label: "下载", path: "/home/Downloads", type: "bookmark" as const },
          { icon: "\u{1F5BC}\uFE0F", label: "图片", path: "/home/Pictures", type: "bookmark" as const },
          { icon: "\u{1F3B5}", label: "音乐", path: "/home/Music", type: "bookmark" as const },
          { icon: "\u{1F3AC}", label: "视频", path: "/home/Videos", type: "bookmark" as const },
        ];

    return [
      { title: "位置", items: bookmarks },
      {
        title: "其他位置",
        items: [{ icon: "\u{1F310}", label: "网络", path: "/network", type: "network" as const }],
      },
    ];
  }

  return [];
}

/* ── Hook 实现 ───────────────────────────────────────── */

/**
 * FileManager 数据预加载 Hook
 *
 * 封装 FileManager 所需的全部异步数据获取逻辑（用户名、目录列表、挂载点、侧边栏），
 * 由 Desktop 父级在窗口创建时调用，数据就绪后注入 FileManager 组件，
 * 避免组件内部的串行加载导致的页面闪动。
 *
 * @example
 * ```tsx
 * const preloader = usePreloader();
 *
 * // 窗口创建时调用：
 * await preloader.preload(activeServerId);
 *
 * // 数据就绪后注入：
 * <FileManager initialData={preloader.data} />
 * ```
 */
export function usePreloader(): PreloaderResult {
  const [state, setState] = useState<PreloadState>('idle');
  const [data, setData] = useState<FileManagerInitialData | null>(null);
  const [error, setError] = useState<string | null>(null);

  const preload = useCallback(async (serverId: string | null) => {
    setState('loading');
    setError(null);
    setData(null);

    const IS_WIN_LOCAL = typeof navigator !== "undefined" && navigator.platform.startsWith("Win");
    const IS_WIN = !serverId && IS_WIN_LOCAL;

    try {
      if (serverId) {
        // ── 远程模式：并行获取 username 和 mounts ──
        const [username, mounts] = await Promise.all([
          invoke<string>("remote_get_current_user", { serverId })
            .catch(() => "user"),
          invoke<MountInfo[]>("remote_get_mounts", { serverId })
            .catch(() => []),
        ]);

        // ── 用 username 加载初始目录 ──
        const homePath = `/home/${username}`;
        const dirResp = await invoke<{ path: string; entries: FileEntry[] }>(
          "remote_read_dir",
          { serverId, path: homePath }
        );

        const entries = (dirResp?.entries || []).sort((a, b) => {
          if (a.is_dir !== b.is_dir) return a.is_dir ? -1 : 1;
          return a.name.localeCompare(b.name);
        });

        // ── 组装完整数据 ──
        const sidebarSections = buildSidebarSections(username, mounts, serverId, false);
        const result: FileManagerInitialData = {
          username,
          currentPath: homePath,
          entries,
          mounts,
          sidebarSections,
        };

        setData(result);
        setState('ready');
      } else {
        // ── 本地模式 ──
        const HOME_PATH = IS_WIN_LOCAL ? "C:\\Users" : "/home";
        const dirResp = await invoke<{ path: string; entries: FileEntry[] }>(
          "read_dir",
          { path: HOME_PATH }
        );

        const entries = (dirResp?.entries || []).sort((a, b) => {
          if (a.is_dir !== b.is_dir) return a.is_dir ? -1 : 1;
          return a.name.localeCompare(b.name);
        });

        const sidebarSections = buildSidebarSections(null, [], null, IS_WIN);
        const result: FileManagerInitialData = {
          username: null,
          currentPath: HOME_PATH,
          entries,
          mounts: [],
          sidebarSections,
        };

        setData(result);
        setState('ready');
      }
    } catch (err) {
      console.error("[usePreload] 预加载失败:", err);
      setError(err instanceof Error ? err.message : String(err));
      setState('error');
    }
  }, []);

  const reset = useCallback(() => {
    setState('idle');
    setData(null);
    setError(null);
  }, []);

  return { state, data, error, preload, reset };
}
