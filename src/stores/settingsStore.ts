import { create } from "zustand";
import { persist, createJSONStorage } from "zustand/middleware";
import { settingsStorage } from "../utils/storage";
import { ThemeId, AccentColorId, themes, accentColors } from "../config/themes";

/* ── Types ─────────────────────────────────────────────── */

export interface SettingsState {
  // 现有设置
  theme: "light" | "dark";
  accentColor: string;
  fontSize: number;
  terminalFontSize: number;

  // 新增主题设置
  themeId: ThemeId;
  accentColorId: AccentColorId | null;
  isDarkMode: boolean;
}

export interface SettingsActions {
  // 现有操作
  setTheme: (theme: "light" | "dark") => void;
  setAccentColor: (color: string) => void;
  setFontSize: (size: number) => void;
  setTerminalFontSize: (size: number) => void;

  // 新增主题操作
  setThemeId: (themeId: ThemeId) => void;
  setAccentColorId: (accentColorId: AccentColorId | null) => void;
  toggleDarkMode: () => void;
}

/* ── Default Settings ──────────────────────────────────── */

const DEFAULT_SETTINGS: SettingsState = {
  // 现有设置
  theme: "light",
  accentColor: "#15aa70",   // 纸张主题默认强调色 - RGB(21,170,112)
  fontSize: 10,
  terminalFontSize: 13,

  // 新增主题设置
  themeId: "paper", // 默认主题
  accentColorId: "paperAccent", // 纸张主题默认选中的强调色（绿色）
  isDarkMode: false, // 默认亮色模式（已弃用，暗色由 themeId === 'dark' 控制）
};

/* ── Store ────────────────────────────────────────────── */

export const useSettingsStore = create<SettingsState & SettingsActions>()(
  persist(
    (set) => ({
      ...DEFAULT_SETTINGS,

      // 现有操作
      setTheme: (theme) => {
        set({ theme });
        // 同步更新 DOM
        document.documentElement.setAttribute("data-theme", theme);
      },

      setAccentColor: (color) => {
        set({ accentColor: color });
        // 同步更新 CSS 变量
        document.documentElement.style.setProperty("--accent-bg", color);
      },

      setFontSize: (size) => {
        set({ fontSize: size });
        // 同步更新 CSS 变量
        document.documentElement.style.setProperty("--font-body", `${size}pt`);
        document.documentElement.style.setProperty("--font-title", `${size + 1}pt`);
        document.documentElement.style.setProperty("--font-small", `${size - 1}pt`);
      },

      setTerminalFontSize: (size) => {
        set({ terminalFontSize: size });
      },

      // 新增主题操作
      setThemeId: (themeId) => {
        set({ themeId });
      },

      setAccentColorId: (accentColorId) => {
        set({ accentColorId });
      },

      toggleDarkMode: () => {
        set((state) => ({ isDarkMode: !state.isDarkMode }));
      },
    }),
    {
      name: "gnome-remote-settings",
      storage: createJSONStorage(() => settingsStorage),
      // 初始化时应用设置到 DOM（在 React 渲染前提供正确的初始值，避免闪烁）
      onRehydrateStorage: () => (state) => {
        if (state) {
          // 迁移旧的主题 ID 到新的
          if (state.themeId === 'warmOriginal' || state.themeId === 'warmEnhanced') {
            state.themeId = 'paper';
          }

          // 通过 themeId 应用完整主题 CSS 变量（与 useTheme 逻辑一致）
          const theme = themes[state.themeId] || themes['paper'];
          const useDarkColors = state.themeId === 'dark';
          const colors = useDarkColors ? theme.darkColors : theme.lightColors;
          const root = document.documentElement;

          root.style.setProperty('--window-bg', colors.windowBg);
          root.style.setProperty('--view-bg', colors.viewBg);
          root.style.setProperty('--card-bg', colors.cardBg);
          root.style.setProperty('--card-hover', colors.cardHover);
          root.style.setProperty('--headerbar-bg', colors.headerbarBg);
          root.style.setProperty('--sidebar-bg', colors.sidebarBg);
          root.style.setProperty('--sidebar-border', colors.sidebarBorder);
          root.style.setProperty('--slider-track-bg', colors.sliderTrackBg);
          root.style.setProperty('--slider-thumb-bg', colors.sliderThumbBg);
          root.style.setProperty('--slider-active-bg', colors.sliderActiveBg);

          // 强调色
          if (theme.accentColorOptions && state.accentColorId) {
            const accentColor = accentColors[state.accentColorId];
            const accent = useDarkColors ? accentColor.dark : accentColor.light;
            root.style.setProperty('--accent-bg', accent);
            root.style.setProperty('--accent-hover', adjustBrightness(accent, -10));
            root.style.setProperty('--accent-active', adjustBrightness(accent, -20));
          } else {
            root.style.setProperty('--accent-bg', colors.accentBg);
            root.style.setProperty('--accent-hover', colors.accentHover);
            root.style.setProperty('--accent-active', colors.accentActive);
          }

          // 字体颜色
          if (useDarkColors) {
            root.style.setProperty('--text-primary', 'rgba(255, 255, 255, 0.87)');
            root.style.setProperty('--text-secondary', 'rgba(255, 255, 255, 0.60)');
            root.style.setProperty('--text-disabled', 'rgba(255, 255, 255, 0.38)');
            root.style.setProperty('--sidebar-fg', '#cccccc');
            root.style.setProperty('--border-color', 'rgba(255, 255, 255, 0.12)');
            root.setAttribute('data-theme', 'dark');
          } else {
            root.style.setProperty('--text-primary', 'rgba(0, 0, 0, 0.87)');
            root.style.setProperty('--text-secondary', 'rgba(0, 0, 0, 0.60)');
            root.style.setProperty('--text-disabled', 'rgba(0, 0, 0, 0.38)');
            root.style.setProperty('--sidebar-fg', '#3f3f3f');
            root.style.setProperty('--border-color', 'rgba(0, 0, 0, 0.15)');
            root.removeAttribute('data-theme');
          }

          // 字体大小
          root.style.setProperty("--font-body", `${state.fontSize}pt`);
          root.style.setProperty("--font-title", `${state.fontSize + 1}pt`);
          root.style.setProperty("--font-small", `${state.fontSize - 1}pt`);
        }
      },
    }
  )
);

// 辅助函数：调整颜色亮度（与 useTheme.ts 保持一致）
function adjustBrightness(hex: string, percent: number): string {
  const num = parseInt(hex.replace('#', ''), 16);
  const amt = Math.round(2.55 * percent);
  const R = (num >> 16) + amt;
  const G = (num >> 8 & 0x00FF) + amt;
  const B = (num & 0x0000FF) + amt;

  return '#' + (
    0x1000000 +
    (R < 255 ? (R < 1 ? 0 : R) : 255) * 0x10000 +
    (G < 255 ? (G < 1 ? 0 : G) : 255) * 0x100 +
    (B < 255 ? (B < 1 ? 0 : B) : 255)
  ).toString(16).slice(1);
}