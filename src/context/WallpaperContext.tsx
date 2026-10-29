import { createContext, useContext, useCallback, useEffect, ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { isTauri } from "../utils/environment";
import { useNotificationStore } from "../stores/notificationStore";
import { useWallpaperStore, getWallpaperStyle, wallpaperImageUrl } from "../stores/wallpaperStore";
import type { WallpaperConfig } from "../stores/wallpaperStore";

/* ── Types ─────────────────────────────────────────────── */

interface WallpaperState {
  wallpaper: WallpaperConfig;
}

interface WallpaperActions {
  setBuiltinWallpaper: (builtinId: string) => void;
  setCustomWallpaper: (path: string) => void;
  clearWallpaper: () => void;
  importWallpaper: () => Promise<void>;
}

type WallpaperContextType = WallpaperState & WallpaperActions;

/* ── Context ──────────────────────────────────────────── */

const WallpaperContext = createContext<WallpaperContextType | null>(null);

/* ── Provider ────────────────────────────────────────── */

export function WallpaperProvider({ children }: { children: ReactNode }) {
  // 使用 Zustand store
  const { wallpaper, objectUrl, setBuiltinWallpaper, setCustomWallpaper, setCachedWallpaper, clearWallpaper } = useWallpaperStore();

  // 主窗口背景色同步：当前壁纸 64×64 平均色 → Rust（暴露区 WM_ERASEBKGND
  // 填充 + WebView2 DefaultBackgroundColor）。最大化/缩放的坏帧窗口在
  // WebView2 新帧就位前不可避免，以其内容主色填充后从「闪黑」退化为
  // 与壁纸几乎不可分的色差（探针实测与方案见 debug-maximize-black-flash.md）。
  // asset 协议图会污染 canvas（SecurityError），失败保持上次颜色。
  useEffect(() => {
    if (!isTauri()) return;
    const url = wallpaperImageUrl(wallpaper, objectUrl);
    if (!url) return;
    let cancelled = false;
    const img = new Image();
    img.onload = () => {
      if (cancelled) return;
      try {
        const N = 64;
        const cv = document.createElement("canvas");
        cv.width = N;
        cv.height = N;
        const ctx = cv.getContext("2d", { willReadFrequently: true });
        if (!ctx) return;
        ctx.drawImage(img, 0, 0, N, N);
        const d = ctx.getImageData(0, 0, N, N).data;
        let r = 0, g = 0, b = 0;
        const n = d.length / 4;
        for (let i = 0; i < d.length; i += 4) {
          r += d[i]; g += d[i + 1]; b += d[i + 2];
        }
        invoke("set_window_background_color", {
          r: Math.round(r / n),
          g: Math.round(g / n),
          b: Math.round(b / n),
        }).catch(() => {});
      } catch {
        /* canvas 污染：保持上次颜色 */
      }
    };
    img.src = url;
    return () => {
      cancelled = true;
    };
  }, [wallpaper, objectUrl]);

  const importWallpaper = useCallback(async () => {
    // Tauri 环境：dialog 拿绝对路径 → Rust 复制为单一缓存文件 → asset 协议加载。
    // 相比 dataURL：store 里只存几十字节路径（rehydrate 快），WebView 原生解码图片。
    if (isTauri()) {
      try {
        const selected = await open({
          multiple: false,
          filters: [{ name: "图片", extensions: ["png", "jpg", "jpeg", "webp"] }],
        });
        if (typeof selected === "string") {
          const cachedPath = await invoke<string>("wallpaper_cache_write", {
            sourcePath: selected,
          });
          setCachedWallpaper(cachedPath);
          // 立即读回字节生成 blob URL（渲染主通道；asset 协议仅作回退）
          await useWallpaperStore.getState().loadCachedWallpaper();
        }
      } catch (err) {
        // 静默失败 = "导入失效"的假象：任何一步出错都必须让用户看见
        useNotificationStore.getState().pushNotification({
          title: "壁纸导入失败",
          body: String(err),
          action: "请确认图片为 png/jpg/webp 且不超过 32MB；若刚更新过应用，请完全重启后再试",
          urgency: "critical",
          source: "设置",
        });
      }
      return;
    }
    // 浏览器调试模式兜底：dataURL（无文件系统访问）
    const input = document.createElement("input");
    input.type = "file";
    input.accept = "image/*";
    input.onchange = (ev) => {
      const file = (ev.target as HTMLInputElement).files?.[0];
      if (file) {
        const reader = new FileReader();
        reader.onload = (e) => {
          const dataUrl = e.target?.result as string;
          setCustomWallpaper(dataUrl);
        };
        reader.readAsDataURL(file);
      }
    };
    input.click();
  }, [setCachedWallpaper]);

  return (
    <WallpaperContext.Provider
      value={{
        wallpaper,
        setBuiltinWallpaper,
        setCustomWallpaper,
        clearWallpaper,
        importWallpaper,
      }}
    >
      {children}
    </WallpaperContext.Provider>
  );
}

/* ── Hook ────────────────────────────────────────────── */

export function useWallpaper(): WallpaperContextType {
  const context = useContext(WallpaperContext);
  if (!context) {
    throw new Error("useWallpaper must be used within WallpaperProvider");
  }
  return context;
}

/* ── Re-export Utility Functions ─────────────────────── */

export { getWallpaperStyle };