import { create } from "zustand";
import { persist, createJSONStorage } from "zustand/middleware";
import { settingsStorage } from "../utils/storage";
import { ThemeId, AccentColorId } from "../config/themes";
import { applyThemeColors } from "../utils/themeUtils";
import { DEFAULT_COLUMN_WIDTHS, ColumnWidths } from "../apps/fileTable";
import { applyLocale, type Locale } from "../i18n";

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

  // 内置浏览器：后台标签睡眠超时（分钟；0 = 永不睡眠）
  browserSleepMinutes: number;

  // 远程浏览 SOCKS5 固定端口（断线重连后浏览器无需重开的前提）
  browserProxyPort: number;

  // 系统浏览器打开网页时使用的浏览器（auto/edge/chrome/firefox/自定义 exe 路径；
  // 与旧版浏览器功能对齐，Rust 端 proxy_list_browsers 枚举候选）
  browserId: string;

  // 启动时恢复上次窗口（桌面会话恢复）
  restoreWindowsOnStartup: boolean;

  // 毛玻璃效果（backdrop-filter 采样壁纸模糊，GPU 常驻开销大，默认关闭）
  frostedGlassEnabled: boolean;

  // 文件管理器列表列宽（用户拖拽调节后持久化；name 为 null 表示名称列弹性默认态）
  fileManagerColumns: ColumnWidths;

  // 界面语言（持久化；启动时 onRehydrateStorage 应用到 i18next）
  locale: Locale;
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

  // 内置浏览器：后台标签睡眠超时
  setBrowserSleepMinutes: (minutes: number) => void;

  // 远程浏览 SOCKS5 固定端口
  setBrowserProxyPort: (port: number) => void;

  // 系统浏览器选择（auto/edge/chrome/firefox/自定义 exe 路径）
  setBrowserId: (browserId: string) => void;

  // 启动时恢复上次窗口
  setRestoreWindowsOnStartup: (enabled: boolean) => void;

  // 毛玻璃效果开关
  setFrostedGlassEnabled: (enabled: boolean) => void;

  // 文件管理器列宽（拖拽结束时提交一次，避免拖动过程频繁写盘）
  setFileManagerColumns: (widths: ColumnWidths) => void;

  setLocale: (locale: Locale) => void;
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

  // 后台标签睡眠超时（对齐 Edge sleeping tabs 的默认量级）
  browserSleepMinutes: 120,

  // SOCKS5 固定端口（SOCKS 惯用端口；被占用时可在设置中更换）
  browserProxyPort: 1080,

  // 系统浏览器选择：auto = 按候选顺序自动探测（Edge → Chrome → Firefox）
  browserId: "auto",

  // 启动时恢复上次窗口（默认开，保留桌面会话恢复行为）
  restoreWindowsOnStartup: true,

  // 毛玻璃效果默认关闭（GPU 常驻开销大；开启后窗口 chrome 采样壁纸模糊）
  frostedGlassEnabled: false,

  // 文件管理器列宽默认值（与 CSS 历史列宽一致，名称列弹性）
  fileManagerColumns: DEFAULT_COLUMN_WIDTHS,

  // 界面语言默认 zh-CN 与当前应用语言一致，老用户升级零感知
  locale: "zh-CN",
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

      setBrowserSleepMinutes: (minutes) => {
        set({ browserSleepMinutes: minutes });
      },

      setBrowserProxyPort: (port) => {
        set({ browserProxyPort: port });
      },

      setBrowserId: (browserId) => {
        set({ browserId });
      },

      setRestoreWindowsOnStartup: (enabled) => {
        set({ restoreWindowsOnStartup: enabled });
      },

      setFrostedGlassEnabled: (enabled) => {
        set({ frostedGlassEnabled: enabled });
        // 开关即时生效：根元素 data-frosted 驱动 variables.css 的变量覆盖
        //（毛玻璃消费方统一引用 --quirel-frosted-* 变量，无需逐处修改）
        document.documentElement.setAttribute("data-frosted", enabled ? "on" : "off");
      },

      setFileManagerColumns: (widths) => {
        set({ fileManagerColumns: widths });
      },

      setLocale: (locale) => {
        set({ locale });
        // 与 setTheme 同风格：action 内直接同步 DOM/i18n，切换即时生效
        applyLocale(locale);
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

          // 毛玻璃效果开关（在 applyThemeColors 之后设置：关闭态用
          // !important 覆盖主题 inline 写入的透明玻璃变量；属性缺失按关闭处理）
          root.setAttribute("data-frosted", state.frostedGlassEnabled ? "on" : "off");

          // 界面语言（持久化值可能来自上次会话；首次启动用默认 zh-CN）
          applyLocale(state.locale);
        }
      },
    }
  )
);