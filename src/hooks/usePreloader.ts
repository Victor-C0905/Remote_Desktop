import { useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { createLogger } from "../utils/logger";

const log = createLogger('Preloader');

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

/* ── Helper: 构建侧边栏数据（仅远程模式） ──────────────── */

function buildSidebarSections(
  username: string | null,
  mounts: MountInfo[],
  activeServerId: string | null,
): SidebarSection[] {
  if (activeServerId && username) {
    // 远程模式侧边栏
    return [
      {
        title: "位置",
        items: [
          { icon: "🏠", label: "主目录", path: `/home/${username}`, type: "bookmark" },
          { icon: "📄", label: "文档", path: `/home/${username}/Documents`, type: "bookmark" },
          { icon: "⬇️", label: "下载", path: `/home/${username}/Downloads`, type: "bookmark" },
          { icon: "🖼️", label: "图片", path: `/home/${username}/Pictures`, type: "bookmark" },
          { icon: "🎵", label: "音乐", path: `/home/${username}/Music`, type: "bookmark" },
          { icon: "🎬", label: "视频", path: `/home/${username}/Videos`, type: "bookmark" },
        ],
      },
      {
        title: "设备",
        items: mounts.map((m) => ({
          icon: "💾",
          label: m.mount_point,
          path: m.mount_point,
          type: "mount" as const,
        })),
      },
      {
        title: "其他位置",
        items: [{ icon: "🌐", label: "网络", path: "/network", type: "network" as const }],
      },
    ];
  }

  // 离线模式：使用默认 Linux 侧边栏（保持 UI 结构一致）
  return [
    {
      title: "位置",
      items: [
        { icon: "🏠", label: "主目录", path: "/home", type: "bookmark" as const },
        { icon: "📄", label: "文档", path: "/home/Documents", type: "bookmark" as const },
        { icon: "⬇️", label: "下载", path: "/home/Downloads", type: "bookmark" as const },
        { icon: "🖼️", label: "图片", path: "/home/Pictures", type: "bookmark" as const },
        { icon: "🎵", label: "音乐", path: "/home/Music", type: "bookmark" as const },
        { icon: "🎬", label: "视频", path: "/home/Videos", type: "bookmark" as const },
      ],
    },
    {
      title: "其他位置",
      items: [{ icon: "🌐", label: "网络", path: "/network", type: "network" as const }],
    },
  ];
}

/* ── Hook 实现 ───────────────────────────────────────── */

/**
 * FileManager 数据预加载 Hook（仅远程模式）
 *
 * 封装 FileManager 所需的全部异步数据获取逻辑（用户名、目录列表、挂载点、侧边栏），
 * 由 Desktop 父级在窗口创建时调用，数据就绪后注入 FileManager 组件，
 * 避免组件内部的串行加载导致的页面闪动。
 *
 * 注意：未连接远程服务器时不加载本地文件系统，FileManager 自行处理离线状态展示。
 */
export function usePreloader(): PreloaderResult {
  const [state, setState] = useState<PreloadState>('idle');
  const [data, setData] = useState<FileManagerInitialData | null>(null);
  const [error, setError] = useState<string | null>(null);

  const preload = useCallback(async (serverId: string | null) => {
    if (!serverId) {
      // 无远程连接，不预加载（FileManager 会显示离线状态）
      setData(null);
      setState('idle');
      return;
    }

    setState('loading');
    setError(null);
    setData(null);

    try {
      // 并行获取 username 和 mounts
      const [username, mounts] = await Promise.all([
        invoke<string>("remote_get_current_user", { serverId })
          .catch((e) => { log.warn('获取用户名失败，使用默认值:', e); return "user"; }),
        invoke<MountInfo[]>("remote_get_mounts", { serverId })
          .catch((e) => { log.warn('获取挂载点失败，返回空数组:', e); return []; }),
      ]);

      // 用 username 加载初始目录
      const homePath = `/home/${username}`;
      const dirResp = await invoke<{ path: string; entries: FileEntry[] }>(
        "remote_read_dir",
        { serverId, path: homePath }
      );

      const entries = (dirResp?.entries || []).sort((a, b) => {
        if (a.is_dir !== b.is_dir) return a.is_dir ? -1 : 1;
        return a.name.localeCompare(b.name);
      });

      // 组装完整数据
      const sidebarSections = buildSidebarSections(username, mounts, serverId);
      const result: FileManagerInitialData = {
        username,
        currentPath: homePath,
        entries,
        mounts,
        sidebarSections,
      };

      setData(result);
      setState('ready');
    } catch (err) {
      log.error("预加载失败:", err);
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
