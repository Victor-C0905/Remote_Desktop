/* ── 统一导出所有 Stores ─────────────────────────────── */

export { useSettingsStore } from "./settingsStore";
export type { SettingsState, SettingsActions } from "./settingsStore";

export { useServersStore, formatLastConnected, getStatusColor, getStatusIcon } from "./serversStore";
export type { ServerConfig, ServersState, ServersActions } from "./serversStore";

export { 
  useWallpaperStore, 
  PRESET_WALLPAPERS, 
  getWallpaperStyle, 
  getPresetWallpaperName 
} from "./wallpaperStore";
export type { WallpaperConfig, WallpaperState, WallpaperActions } from "./wallpaperStore";