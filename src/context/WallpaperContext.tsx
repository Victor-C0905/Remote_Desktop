import { createContext, useContext, useState, useEffect, useCallback, ReactNode } from "react";

/* ── Types ─────────────────────────────────────────────── */

export interface WallpaperConfig {
  type: "preset" | "custom";
  presetId?: string;
  customPath?: string;
}

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

/* ── Preset Wallpapers ──────────────────────────────── */

export const PRESET_WALLPAPERS: Record<string, string> = {
  "adwaita-blue": "linear-gradient(135deg, #3584e4 0%, #1a5fb4 100%)",
  "adwaita-dark": "linear-gradient(135deg, #1e1e1e 0%, #2a2a2a 100%)",
  "adwaita-green": "linear-gradient(135deg, #33d17a 0%, #26a269 100%)",
  "adwaita-orange": "linear-gradient(135deg, #e66100 0%, #c64600 100%)",
  "adwaita-purple": "linear-gradient(135deg, #9141ac 0%, #613583 100%)",
  "gnome-default": "linear-gradient(180deg, #3584e4 0%, #1a5fb4 50%, #0d1b3d 100%)",
};

const STORAGE_KEY = "gnome-remote-wallpaper";

/* ── Context ──────────────────────────────────────────── */

const WallpaperContext = createContext<WallpaperContextType | null>(null);

/* ── Provider ────────────────────────────────────────── */

export function WallpaperProvider({ children }: { children: ReactNode }) {
  const [wallpaper, setWallpaper] = useState<WallpaperConfig>({
    type: "preset",
    presetId: "gnome-default",
  });

  // Load from localStorage on mount
  useEffect(() => {
    try {
      const stored = localStorage.getItem(STORAGE_KEY);
      if (stored) {
        const parsed = JSON.parse(stored) as WallpaperConfig;
        setWallpaper(parsed);
      }
    } catch (e) {
      console.warn("[WallpaperContext] Failed to load:", e);
    }
  }, []);

  // Save to localStorage on change
  useEffect(() => {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(wallpaper));
    } catch (e) {
      console.warn("[WallpaperContext] Failed to save:", e);
    }
  }, [wallpaper]);

  const setPresetWallpaper = useCallback((presetId: string) => {
    setWallpaper({ type: "preset", presetId });
  }, []);

  const setCustomWallpaper = useCallback((path: string) => {
    setWallpaper({ type: "custom", customPath: path });
  }, []);

  const clearWallpaper = useCallback(() => {
    setWallpaper({ type: "preset", presetId: "gnome-default" });
  }, []);

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
      console.warn("[WallpaperContext] Tauri dialog not available, using fallback");
      
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
    <WallpaperContext.Provider value={{
      wallpaper,
      setPresetWallpaper,
      setCustomWallpaper,
      clearWallpaper,
      importWallpaper,
    }}>
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