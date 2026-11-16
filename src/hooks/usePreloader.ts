import { useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { createLogger } from "../utils/logger";
import { formatUnknownError } from "../types/errors";
import { buildSidebarSections } from "../apps/fileSidebar";
import type { CommonLocation, SidebarSection } from "../apps/fileSidebar";
import { selectFrequentPaths, useFilePathUsageStore } from "../stores/filePathUsageStore";

const log = createLogger('Preloader');

/* ── Types (与 FileManager 共享) ─────────────────────── */

export interface FileEntry {
  name: string;
  is_dir: boolean;
  size: number;
  mtime: string;
  permissions: string;
}

/**
 * FileManager 预加载所需的全部初始数据
 * 由 Desktop 父级通过 usePreloader 获取后注入 FileManager 组件
 */
export interface FileManagerInitialData {
  username: string | null;
  homeDir: string | null;  // 用户真实家目录（root 为 /root，普通用户从 /etc/passwd 读取）
  currentPath: string;
  entries: FileEntry[];
  /** 服务器标准位置（空数组 = 家目录下无 XDG 目录，侧边栏仅展示主目录） */
  commonLocations: CommonLocation[];
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

/* ── Hook 实现 ───────────────────────────────────────── */

/**
 * FileManager 数据预加载 Hook（仅远程模式）
 *
 * 封装 FileManager 所需的全部异步数据获取逻辑（用户名、目录列表、侧边栏），
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
      const user = await invoke<{ username: string; home_dir: string; common_locations?: CommonLocation[] }>(
        "remote_get_current_user",
        { serverId }
      ).catch((e) => {
        log.warn('获取用户信息失败，使用 / 作为回退:', e);
        return { username: "user", home_dir: "/", common_locations: [] };
      });

      // 用服务端返回的真实家目录加载初始目录
      const homePath = user.home_dir;
      const dirResp = await invoke<{ path: string; entries: FileEntry[] }>(
        "remote_read_dir",
        { serverId, path: homePath }
      );

      const entries = (dirResp?.entries || []).sort((a, b) => {
        if (a.is_dir !== b.is_dir) return a.is_dir ? -1 : 1;
        return a.name.localeCompare(b.name);
      });

      // 组装完整数据
      const commonLocations = user.common_locations ?? [];
      const frequentPaths = selectFrequentPaths(
        useFilePathUsageStore.getState().byServer,
        serverId
      );
      const sidebarSections = buildSidebarSections({
        username: user.username,
        homeDir: user.home_dir,
        activeServerId: serverId,
        commonLocations,
        frequentPaths,
      });
      const result: FileManagerInitialData = {
        username: user.username,
        homeDir: user.home_dir,
        currentPath: homePath,
        entries,
        commonLocations,
        sidebarSections,
      };

      setData(result);
      setState('ready');
    } catch (err) {
      log.error("预加载失败:", err);
      setError(formatUnknownError(err));
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
