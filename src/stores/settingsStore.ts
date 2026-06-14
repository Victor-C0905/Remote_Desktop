import { create } from "zustand";
import { persist, createJSONStorage } from "zustand/middleware";
import { settingsStorage } from "../utils/storage";

/* ── Types ─────────────────────────────────────────────── */

export interface SettingsState {
  theme: "light" | "dark";
  accentColor: string;
  fontSize: number;
  terminalFontSize: number;
}

export interface SettingsActions {
  setTheme: (theme: "light" | "dark") => void;
  setAccentColor: (color: string) => void;
  setFontSize: (size: number) => void;
  setTerminalFontSize: (size: number) => void;
}

/* ── Default Settings ──────────────────────────────────── */

const DEFAULT_SETTINGS: SettingsState = {
  theme: "light",
  accentColor: "#3584e4",
  fontSize: 10,
  terminalFontSize: 13,
};

/* ── Store ────────────────────────────────────────────── */

export const useSettingsStore = create<SettingsState & SettingsActions>()(
  persist(
    (set) => ({
      ...DEFAULT_SETTINGS,

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
    }),
    {
      name: "gnome-remote-settings",
      storage: createJSONStorage(() => settingsStorage),
      // 初始化时应用设置到 DOM
      onRehydrateStorage: () => (state) => {
        if (state) {
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