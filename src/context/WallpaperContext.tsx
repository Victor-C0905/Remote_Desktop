import { createContext, useContext, useCallback, ReactNode } from "react";
import { createLogger } from "../utils/logger";
import {
  useWallpaperStore,
  PRESET_WALLPAPERS,
  getWallpaperStyle,
  getPresetWallpaperName,
} from "../stores/wallpaperStore";
import type { WallpaperConfig } from "../stores/wallpaperStore";

/* ── Types ─────────────────────────────────────────────── */

interface WallpaperState {
  wallpaper: WallpaperConfig;
}

interface WallpaperActions {
  setPresetWallpaper: (presetId: string) => void;
  setCustomWallpaper: (path: string) => void;
  clearWallpaper: () => void;
  importWallpaper: () => Promise<void>;
}

type WallpaperContextType = WallpaperState & WallpaperActions;

/* ── Context ──────────────────────────────────────────── */

const WallpaperContext = createContext<WallpaperContextType | null>(null);

/* ── Logger ───────────────────────────────────────────── */

const log = createLogger('WallpaperContext');

/* ── Provider ────────────────────────────────────────── */

export function WallpaperProvider({ children }: { children: ReactNode }) {
  // 使用 Zustand store
  const { wallpaper, setPresetWallpaper, setCustomWallpaper, clearWallpaper } = useWallpaperStore();

  const importWallpaper = useCallback(async () => {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({
        multiple: false,
        filters: [
          { name: "Images", extensions: ["jpg", "jpeg", "png", "webp", "gif", "bmp"] },
        ],
      });

      if (selected && typeof selected === "string") {
        setCustomWallpaper(selected);
      }
    } catch (e) {
      log.warn('Tauri dialog not available, using fallback');

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
    }
  }, [setCustomWallpaper]);

  return (
    <WallpaperContext.Provider
      value={{
        wallpaper,
        setPresetWallpaper,
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

export { PRESET_WALLPAPERS, getWallpaperStyle, getPresetWallpaperName };