import { createContext, useContext, useCallback, ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { isTauri } from "../utils/environment";
import { useNotificationStore } from "../stores/notificationStore";
import { useWallpaperStore, getWallpaperStyle } from "../stores/wallpaperStore";
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
  const { wallpaper, setBuiltinWallpaper, setCustomWallpaper, setCachedWallpaper, clearWallpaper } = useWallpaperStore();

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