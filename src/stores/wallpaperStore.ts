import { create } from "zustand";
import { persist, createJSONStorage } from "zustand/middleware";
import { settingsStorage } from "../utils/storage";
import { createLogger } from "../utils/logger";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { isTauri } from "../utils/environment";

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

/** 壁纸当前生效的图像 URL（渲染与窗口背景色同步共用同一取值口径） */
export function wallpaperImageUrl(
  wallpaper: WallpaperConfig,
  objectUrl?: string | null,
): string | null {
  if (wallpaper.type === "cached" && wallpaper.cachedPath) {
    return objectUrl ?? wallpaperAssetUrl(wallpaper.cachedPath);
  }
  if (wallpaper.type === "custom" && wallpaper.customPath) {
    return wallpaper.customPath;
  }
  return builtinWallpaperUrl(wallpaper.builtinId);
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
          // 读失败将回退默认壁纸，与 storage 读失败同级，升 error 便于定位
          log.error("壁纸缓存读取失败:", e);
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
          // 旧版 dataURL 壁纸迁移（异步，见函数注释）
          void migrateLegacyDataUrlWallpaper();
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

/* 旧方案 dataURL 迁移（P0：settings.bin 膨胀 5MB）：
   旧版导入把图片整串 base64 存进 customPath 并持久化——单图可达 5MB，
   任意设置变更触发 persist 都全量重写 settings.bin，启动 rehydrate 也要
   parse 这串数据。落盘为缓存文件后改存 cachedPath（setCachedWallpaper
   触发 persist 自动覆写旧结构）；失败保持原状下次启动重试，期间 dataURL
   分支照常渲染，不产生可见中断。导出仅供测试 */
export async function migrateLegacyDataUrlWallpaper(): Promise<void> {
  if (!isTauri()) return; // 浏览器调试模式无 Rust 侧，dataURL 是唯一载体
  const w = useWallpaperStore.getState().wallpaper;
  if (w.type !== "custom" || !w.customPath?.startsWith("data:image/")) return;
  try {
    const cachedPath = await invoke<string>("wallpaper_cache_write_data", {
      dataUrl: w.customPath,
    });
    useWallpaperStore.getState().setCachedWallpaper(cachedPath);
    await useWallpaperStore.getState().loadCachedWallpaper();
    log.info("旧版 dataURL 壁纸已迁移为缓存文件");
  } catch (e) {
    log.error("dataURL 壁纸迁移失败（保持原状，下次启动重试）:", e);
  }
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

  const url = wallpaperImageUrl(wallpaper, objectUrl);
  /* cached 类型理论上 url 恒非空（cachedPath 缺失时退回 builtin），
     url 全空仅剩防御分支 */
  return url ? { ...base, backgroundImage: `url(${url})` } : base;
}
