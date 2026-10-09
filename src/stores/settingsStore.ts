import { create } from "zustand";
import { persist, createJSONStorage } from "zustand/middleware";
import { settingsStorage } from "../utils/storage";
import { ThemeId, AccentColorId } from "../config/themes";
import { applyThemeColors } from "../utils/themeUtils";
import { DEFAULT_COLUMN_WIDTHS, ColumnWidths } from "../apps/fileTable";

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

  // 远程浏览默认浏览器（"auto" 自动探测，或浏览器 id / 自定义路径）
  browserId: string;

  // 远程浏览 SOCKS5 固定端口（断线重连后浏览器无需重开的前提）
  browserProxyPort: number;

  // 启动时恢复上次窗口（桌面会话恢复）
  restoreWindowsOnStartup: boolean;

  // 文件管理器列表列宽（用户拖拽调节后持久化；name 为 null 表示名称列弹性默认态）
  fileManagerColumns: ColumnWidths;
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

  // 远程浏览默认浏览器
  setBrowserId: (browserId: string) => void;

  // 远程浏览 SOCKS5 固定端口
  setBrowserProxyPort: (port: number) => void;

  // 启动时恢复上次窗口
  setRestoreWindowsOnStartup: (enabled: boolean) => void;

  // 文件管理器列宽（拖拽结束时提交一次，避免拖动过程频繁写盘）
  setFileManagerColumns: (widths: ColumnWidths) => void;
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

  // 远程浏览默认浏览器：自动探测
  browserId: "auto",

  // SOCKS5 固定端口（SOCKS 惯用端口；被占用时可在设置中更换）
  browserProxyPort: 1080,

  // 启动时恢复上次窗口（默认开，保留桌面会话恢复行为）
  restoreWindowsOnStartup: true,

  // 文件管理器列宽默认值（与 CSS 历史列宽一致，名称列弹性）
  fileManagerColumns: DEFAULT_COLUMN_WIDTHS,
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
        document.documentElement.style.setProperty("--quirel-accent-bg", color);
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

      setBrowserId: (browserId) => {
        set({ browserId });
      },

      setBrowserProxyPort: (port) => {
        set({ browserProxyPort: port });
      },

      setRestoreWindowsOnStartup: (enabled) => {
        set({ restoreWindowsOnStartup: enabled });
      },

      setFileManagerColumns: (widths) => {
        set({ fileManagerColumns: widths });
      },
    }),
    {
      name: "quirel-settings",
      storage: createJSONStorage(() => settingsStorage),
      // 初始化时应用设置到 DOM（在 React 渲染前提供正确的初始值，避免闪烁）
      onRehydrateStorage: () => (state) => {
        if (state) {
          // 迁移旧的主题 ID 到新的
          if ((state.themeId as string) === 'warmOriginal' || (state.themeId as string) === 'warmEnhanced') {
            state.themeId = 'paper';
          }

          // 应用主题颜色到CSS变量
          applyThemeColors(state.themeId, state.accentColorId);

          // 字体大小
          const root = document.documentElement;
          root.style.setProperty("--font-body", `${state.fontSize}pt`);
          root.style.setProperty("--font-title", `${state.fontSize + 1}pt`);
          root.style.setProperty("--font-small", `${state.fontSize - 1}pt`);
        }
      },
    }
  )
);