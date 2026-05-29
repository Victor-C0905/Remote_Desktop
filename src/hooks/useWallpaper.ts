import { useState, useEffect, useCallback } from "react";

/* ── Types ─────────────────────────────────────────────── */

export interface WallpaperConfig {
  type: "preset" | "custom";
  presetId?: string;
  customPath?: string;
}

interface WallpaperState {
  wallpaper: WallpaperConfig;
  presetWallpapers: string[];
}

interface WallpaperManagerActions {
  setPresetWallpaper: (presetId: string) => void;
  setCustomWallpaper: (path: string) => void;
  clearWallpaper: () => void;
  importWallpaper: () => Promise<void>;
}

type WallpaperManagerHook = WallpaperState & WallpaperManagerActions;

/* ── Preset Wallpapers ──────────────────────────────── */

const PRESET_WALLPAPERS: Record<string, string> = {
  "adwaita-blue": "linear-gradient(135deg, #3584e4 0%, #1a5fb4 100%)",
  "adwaita-dark": "linear-gradient(135deg, #1e1e1e 0%, #2a2a2a 100%)",
  "adwaita-green": "linear-gradient(135deg, #33d17a 0%, #26a269 100%)",
  "adwaita-orange": "linear-gradient(135deg, #e66100 0%, #c64600 100%)",
  "adwaita-purple": "linear-gradient(135deg, #9141ac 0%, #613583 100%)",
  "gnome-default": "linear-gradient(180deg, #3584e4 0%, #1a5fb4 50%, #0d1b3d 100%)",
};

const STORAGE_KEY = "gnome-remote-wallpaper";

/* ── Hook ────────────────────────────────────────────── */

export function useWallpaper(): WallpaperManagerHook {
  const [wallpaper, setWallpaper] = useState<WallpaperConfig>({
    type: "preset",
    presetId: "gnome-default",
  });

  // Load from localStorage on mount
  useEffect(() => {
    try {
      const stored = localStorage.getItem(STORAGE_KEY);
      console.log("[DEBUG] useWallpaper - loading from localStorage:", stored);
      if (stored) {
        const parsed = JSON.parse(stored) as WallpaperConfig;
        console.log("[DEBUG] useWallpaper - parsed:", parsed);
        setWallpaper(parsed);
      }
    } catch (e) {
      console.warn("[WallpaperManager] Failed to load wallpaper:", e);
    }
  }, []);

  // Save to localStorage on change
  useEffect(() => {
    console.log("[DEBUG] useWallpaper - wallpaper changed:", wallpaper);
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(wallpaper));
      console.log("[DEBUG] useWallpaper - saved to localStorage:", localStorage.getItem(STORAGE_KEY));
    } catch (e) {
      console.warn("[WallpaperManager] Failed to save wallpaper:", e);
    }
  }, [wallpaper]);

  const setPresetWallpaper = useCallback((presetId: string) => {
    console.log("[DEBUG] useWallpaper - setPresetWallpaper called:", presetId);
    setWallpaper({ type: "preset", presetId });
  }, []);

  const setCustomWallpaper = useCallback((path: string) => {
    console.log("[DEBUG] useWallpaper - setCustomWallpaper called:", path);
    setWallpaper({ type: "custom", customPath: path });
  }, []);

  const clearWallpaper = useCallback(() => {
    console.log("[DEBUG] useWallpaper - clearWallpaper called");
    setWallpaper({ type: "preset", presetId: "gnome-default" });
  }, []);

  const importWallpaper = useCallback(async () => {
    try {
      // Use Tauri's file dialog API
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
      // Fallback: use browser file input (for web dev mode)
      console.warn("[WallpaperManager] Tauri dialog not available, using fallback");
      
      const input = document.createElement("input");
      input.type = "file";
      input.accept = "image/*";
      input.onchange = (e) => {
        const file = (e.target as HTMLInputElement).files?.[0];
        if (file) {
          const reader = new FileReader();
          reader.onload = (ev) => {
            const dataUrl = ev.target?.result as string;
            setCustomWallpaper(dataUrl);
          };
          reader.readAsDataURL(file);
        }
      };
      input.click();
    }
  }, [setCustomWallpaper]);

  return {
    wallpaper,
    presetWallpapers: Object.keys(PRESET_WALLPAPERS),
    setPresetWallpaper,
    setCustomWallpaper,
    clearWallpaper,
    importWallpaper,
  };
}

/* ── Utility Functions ─────────────────────────────── */

export function getWallpaperStyle(wallpaper: WallpaperConfig): React.CSSProperties {
  if (wallpaper.type === "preset" && wallpaper.presetId) {
    return { 
      backgroundImage: PRESET_WALLPAPERS[wallpaper.presetId] || PRESET_WALLPAPERS["gnome-default"],
      backgroundSize: "cover",
      backgroundPosition: "center",
      backgroundRepeat: "no-repeat",
    };
  }

  if (wallpaper.type === "custom" && wallpaper.customPath) {
    if (wallpaper.customPath.startsWith("data:") || wallpaper.customPath.startsWith("http")) {
      return {
        backgroundImage: `url(${wallpaper.customPath})`,
        backgroundSize: "cover",
        backgroundPosition: "center",
        backgroundRepeat: "no-repeat",
      };
    }
    return {
      backgroundImage: `url(${wallpaper.customPath})`,
      backgroundSize: "cover",
      backgroundPosition: "center",
      backgroundRepeat: "no-repeat",
    };
  }

  return { 
    backgroundImage: PRESET_WALLPAPERS["gnome-default"],
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
    "gnome-default": "GNOME 默认",
  };
  return names[presetId] || presetId;
}

export function getPresetWallpaperPreview(presetId: string): string {
  return PRESET_WALLPAPERS[presetId] || PRESET_WALLPAPERS["gnome-default"];
}