import { create } from "zustand";
import { persist, createJSONStorage } from "zustand/middleware";
import { settingsStorage } from "../utils/storage";
import { createLogger } from "../utils/logger";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";

/**
 * 缓存文件路径 → asset 协议 URL。
 * Windows 路径含反斜杠：encodeURIComponent 会把 `\` 编成 %5C，
 * asset 端解码后与 scope 匹配时可能失败（导入"看似无效"的已知坑）——
 * 统一先归一为正斜杠（Windows 文件系统两种分隔符等价）。
 */
export function wallpaperAssetUrl(cachedPath: string): string {
  return convertFileSrc(cachedPath.replace(/\\/g, "/"));
}

const log = createLogger('WallpaperStore');

/* ── Types ─────────────────────────────────────────────── */

export interface WallpaperConfig {
  /** 内置壁纸（public/wallpaper/*.jpg，随安装包分发，同源直接加载） */
  type: "builtin" | "custom" | "cached";
  /** 内置壁纸 id（见 BUILTIN_WALLPAPERS；缺省取第 0 张） */
  builtinId?: string;
  presetId?: string; /* 旧数据遗留字段 */
  /** 旧方案遗留：base64 dataURL（新导入一律走 cached 文件路径） */
  customPath?: string;
  /** 缓存文件绝对路径（Rust 侧 wallpaper_cache_write 复制到 appData/wallpaper/） */
  cachedPath?: string;
}

export interface WallpaperState {
  wallpaper: WallpaperConfig;
  /**
   * 会话级 blob URL（wallpaper_read_cached 读字节后生成）。
   * blob URL 跨会话无效，只存在内存、不持久化；渲染优先于 asset URL——
   * asset 协议在部分环境下拦截器未注册（ERR_CONNECTION_REFUSED），
   * blob 通道不依赖任何协议注册，保证可用。
   */
  objectUrl: string | null;
}

export interface WallpaperActions {
  setBuiltinWallpaper: (builtinId: string) => void;
  setCustomWallpaper: (path: string) => void;
  setCachedWallpaper: (path: string) => void;
  /** 读取磁盘缓存生成 blob URL（启动/导入后调用） */
  loadCachedWallpaper: () => Promise<void>;
  clearWallpaper: () => void;
}

/* ── 内置壁纸（纯色/渐变预设已移除：多张主题图 + 用户上传缓存位） ──
   生成脚本见仓库历史；2560×1440 渐变柔光，单张 ~40KB */

export interface BuiltinWallpaper {
  id: string;
  url: string;
  /** 词典 key（i18n wallpaper.builtin.<id>） */
  nameKey: string;
}

export const BUILTIN_WALLPAPERS: BuiltinWallpaper[] = [
  { id: "dusk", url: "/wallpaper/dusk.jpg", nameKey: "wallpaper.builtin.dusk" },
  { id: "forest", url: "/wallpaper/forest.jpg", nameKey: "wallpaper.builtin.forest" },
  { id: "dawn", url: "/wallpaper/dawn.jpg", nameKey: "wallpaper.builtin.dawn" },
  { id: "aurora", url: "/wallpaper/aurora.jpg", nameKey: "wallpaper.builtin.aurora" },
];

export function builtinWallpaperUrl(id?: string): string {
  return BUILTIN_WALLPAPERS.find((w) => w.id === id)?.url ?? BUILTIN_WALLPAPERS[0].url;
}

/* ── Default Wallpaper ───────────────────────────────── */

const DEFAULT_WALLPAPER: WallpaperConfig = {
  type: "builtin",
  builtinId: "dusk",
};

/* ── Store ────────────────────────────────────────────── */

export const useWallpaperStore = create<WallpaperState & WallpaperActions>()(
  persist(
    (set, get) => ({
      wallpaper: DEFAULT_WALLPAPER,
      objectUrl: null,

      setBuiltinWallpaper: (builtinId) => {
        revokeObjectUrl(get().objectUrl);
        set({ wallpaper: { type: "builtin", builtinId }, objectUrl: null });
      },

      setCustomWallpaper: (path) => {
        revokeObjectUrl(get().objectUrl);
        set({ wallpaper: { type: "custom", customPath: path }, objectUrl: null });
      },

      setCachedWallpaper: (path) => {
        set({ wallpaper: { type: "cached", cachedPath: path } });
      },

      loadCachedWallpaper: async () => {
        const { wallpaper } = get();
        if (wallpaper.type !== "cached" || !wallpaper.cachedPath) return;
        try {
          const b64 = await invoke<string>("wallpaper_read_cached");
          const bin = atob(b64);
          const bytes = new Uint8Array(bin.length);
          for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
          const mime = wallpaper.cachedPath.endsWith(".png") ? "image/png"
            : wallpaper.cachedPath.endsWith(".webp") ? "image/webp" : "image/jpeg";
          const url = URL.createObjectURL(new Blob([bytes], { type: mime }));
          revokeObjectUrl(get().objectUrl);
          set({ objectUrl: url });
          log.info("壁纸 blob 已加载");
        } catch (e) {
          log.warn("壁纸缓存读取失败:", e);
        }
      },

      clearWallpaper: () => {
        revokeObjectUrl(get().objectUrl);
        set({ wallpaper: DEFAULT_WALLPAPER, objectUrl: null });
      },
    }),
    {
      name: "quirel-wallpaper",
      storage: createJSONStorage(() => settingsStorage),
      // blob URL 跨会话无效：只持久化 wallpaper 配置
      partialize: (state) => ({ wallpaper: state.wallpaper }),
      // Hydration 完成后的回调
      onRehydrateStorage: () => (state) => {
        if (state) {
          // 旧版 preset / 无 id 的旧 builtin 迁移为内置默认（第 0 张）
          if ((state.wallpaper.type as string) === "preset" ||
              (state.wallpaper.type === "builtin" && !state.wallpaper.builtinId)) {
            state.wallpaper = DEFAULT_WALLPAPER;
          }
          log.debug("Rehydrated:", state.wallpaper);
        }
      },
    }
  )
);

/** 释放旧 blob URL（会话内换壁纸时防内存滞留） */
function revokeObjectUrl(url: string | null) {
  if (url) URL.revokeObjectURL(url);
}

/* ── Utility Functions ──────────────────────────────── */

export function getWallpaperStyle(
  wallpaper: WallpaperConfig,
  objectUrl?: string | null,
): React.CSSProperties {
  const base = {
    backgroundSize: "cover",
    backgroundPosition: "center",
    backgroundRepeat: "no-repeat",
    /* 兜底底色：图片尚未解码/加载失败时不透出白色 body 背景 */
    backgroundColor: "#241f28",
  } as React.CSSProperties;

  if (wallpaper.type === "cached" && wallpaper.cachedPath) {
    // 渲染优先级：会话 blob URL（保证可用）→ asset 协议 URL（回退）
    return { ...base, backgroundImage: `url(${objectUrl ?? wallpaperAssetUrl(wallpaper.cachedPath)})` };
  }

  if (wallpaper.type === "custom" && wallpaper.customPath) {
    return { ...base, backgroundImage: `url(${wallpaper.customPath})` };
  }

  // builtin（默认）/ 未知：内置图随安装包分发，webview 同源直接加载
  return { ...base, backgroundImage: `url(${builtinWallpaperUrl(wallpaper.builtinId)})` };
}
