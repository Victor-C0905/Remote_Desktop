import { useSettingsStore } from "../stores/settingsStore";

/* ── Hook: 直接使用 settingsStore ─────────────────────── */

/**
 * 主题 Hook - 简化版，直接使用 Zustand store
 */
export function useTheme() {
  const { theme, setTheme } = useSettingsStore();
  
  return { theme, setTheme };
}