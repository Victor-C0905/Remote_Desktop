import { create } from "zustand";
import { persist, createJSONStorage } from "zustand/middleware";
import { settingsStorage } from "../utils/storage";
import { createLogger } from "../utils/logger";

const log = createLogger('WallpaperStore');

/* ── Types ─────────────────────────────────────────────── */

export interface WallpaperConfig {
  type: "preset" | "custom";
  presetId?: string;
  customPath?: string;
}

export interface WallpaperState {
  wallpaper: WallpaperConfig;
}

export interface WallpaperActions {
  setPresetWallpaper: (presetId: string) => void;
  setCustomWallpaper: (path: string) => void;
  clearWallpaper: () => void;
}

/* ── Preset Wallpapers ──────────────────────────────── */

export const PRESET_WALLPAPERS: Record<string, string> = {
  "adwaita-blue": "linear-gradient(135deg, #3584e4 0%, #1a5fb4 100%)",
  "adwaita-dark": "linear-gradient(135deg, #1e1e1e 0%, #2a2a2a 100%)",
  "adwaita-green": "linear-gradient(135deg, #33d17a 0%, #26a269 100%)",
  "adwaita-orange": "linear-gradient(135deg, #e66100 0%, #c64600 100%)",
  "adwaita-purple": "linear-gradient(135deg, #9141ac 0%, #613583 100%)",
  "quirel-default": "linear-gradient(180deg, #3584e4 0%, #1a5fb4 50%, #0d1b3d 100%)",
};

/* ── Default Wallpaper ───────────────────────────────── */

const DEFAULT_WALLPAPER: WallpaperConfig = {
  type: "preset",
  presetId: "quirel-default",
};

/* ── Store ────────────────────────────────────────────── */

export const useWallpaperStore = create<WallpaperState & WallpaperActions>()(
  persist(
    (set) => ({
      wallpaper: DEFAULT_WALLPAPER,

      setPresetWallpaper: (presetId) => {
        set({ wallpaper: { type: "preset", presetId } });
      },

      setCustomWallpaper: (path) => {
        set({ wallpaper: { type: "custom", customPath: path } });
      },

      clearWallpaper: () => {
        set({ wallpaper: DEFAULT_WALLPAPER });
      },
    }),
    {
      name: "quirel-wallpaper",
      storage: createJSONStorage(() => settingsStorage),
      // Hydration 完成后的回调
      onRehydrateStorage: () => (state) => {
        if (state) {
          log.debug("Rehydrated:", state.wallpaper);
        }
      },
    }
  )
);

/* ── Utility Functions ──────────────────────────────── */

export function getWallpaperStyle(wallpaper: WallpaperConfig): React.CSSProperties {
  if (wallpaper.type === "preset" && wallpaper.presetId) {
    return {
      backgroundImage: PRESET_WALLPAPERS[wallpaper.presetId] || PRESET_WALLPAPERS["quirel-default"],
      backgroundSize: "cover",
      backgroundPosition: "center",
      backgroundRepeat: "no-repeat",
    };
  }

  if (wallpaper.type === "custom" && wallpaper.customPath) {
    return {
      backgroundImage: `url(${wallpaper.customPath})`,
      backgroundSize: "cover",
      backgroundPosition: "center",
      backgroundRepeat: "no-repeat",
    };
  }

  return {
    backgroundImage: PRESET_WALLPAPERS["quirel-default"],
    backgroundSize: "cover",
    backgroundPosition: "center",
    backgroundRepeat: "no-repeat",
  };
}

export function getPresetWallpaperName(presetId: string): string {
  const names: Record<string, string> = {
    "adwaita-blue": "Adwaita 蓝",
    "adwaita-dark": "Adwaita 暗色",
    "adwaita-green": "Adwaita 绿",
    "adwaita-orange": "Adwaita 橙",
    "adwaita-purple": "Adwaita 紫",
    "quirel-default": "Quirel 默认",
  };
  return names[presetId] || presetId;
}