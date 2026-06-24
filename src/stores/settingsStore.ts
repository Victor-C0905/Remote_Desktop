import { create } from "zustand";
import { persist, createJSONStorage } from "zustand/middleware";
import { settingsStorage } from "../utils/storage";
import { ThemeId, AccentColorId } from "../config/themes";

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
      // 初始化时应用设置到 DOM
      onRehydrateStorage: () => (state) => {
        if (state) {
          // 迁移旧的主题 ID 到新的
          if (state.themeId === 'warmOriginal' || state.themeId === 'warmEnhanced') {
            state.themeId = 'paper';
          }

          document.documentElement.setAttribute("data-theme", state.theme);
          document.documentElement.style.setProperty("--accent-bg", state.accentColor);
          document.documentElement.style.setProperty("--font-body", `${state.fontSize}pt`);
          document.documentElement.style.setProperty("--font-title", `${state.fontSize + 1}pt`);
          document.documentElement.style.setProperty("--font-small", `${state.fontSize - 1}pt`);
        }
      },
    }
  )
);