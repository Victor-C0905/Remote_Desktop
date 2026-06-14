import { useWallpaperStore, getWallpaperStyle } from "../stores/wallpaperStore";

/* ── Hook: 直接使用 wallpaperStore ───────────────────── */

/**
 * 壁纸 Hook - 简化版，直接使用 Zustand store
 */
export function useWallpaper() {
  const { wallpaper, setPresetWallpaper, setCustomWallpaper, clearWallpaper } = useWallpaperStore();
  
  return {
    wallpaper,
    wallpaperStyle: getWallpaperStyle(wallpaper),
    setPresetWallpaper,
    setCustomWallpaper,
    clearWallpaper,
  };
}