import { useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";

/* ── Types ─────────────────────────────────────────────── */

interface ShortcutConfig {
  key: string;
  ctrl?: boolean;
  alt?: boolean;
  shift?: boolean;
  meta?: boolean;
  action: () => void;
  /** 描述的词典 key（渲染处 t()，P7） */
  descriptionKey?: string;
}

interface UseGlobalShortcutsOptions {
  enabled?: boolean;
}

/* ── Hook ─────────────────────────────────────────────── */

export function useGlobalShortcuts(
  shortcuts: ShortcutConfig[],
  options: UseGlobalShortcutsOptions = {}
) {
  const { enabled = true } = options;

  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if (!enabled) return;

      for (const shortcut of shortcuts) {
        const keyMatch = e.key.toLowerCase() === shortcut.key.toLowerCase();
        const ctrlMatch = shortcut.ctrl ? e.ctrlKey : !e.ctrlKey;
        const altMatch = shortcut.alt ? e.altKey : !e.altKey;
        const shiftMatch = shortcut.shift ? e.shiftKey : !e.shiftKey;
        const metaMatch = shortcut.meta ? (e.metaKey || e.key === "Meta" || e.key === "OS") : !e.metaKey;

        if (keyMatch && ctrlMatch && altMatch && shiftMatch && metaMatch) {
          e.preventDefault();
          // async action（如 F11 全屏）失败不至于静默——"快捷键没反应"可归因
          Promise.resolve(shortcut.action()).catch((err) =>
            console.warn("[GlobalShortcuts] action 失败:", shortcut.key, err)
          );
          return;
        }
      }
    },
    [shortcuts, enabled]
  );

  useEffect(() => {
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [handleKeyDown]);
}

/* ── Preset Shortcuts for Quirel ────────────────── */

export function createAppShortcuts(
  openApp: (appId: string) => void,
  toggleOverview: () => void,
  closeOverview: () => void,
  closeNotifications: () => void
): ShortcutConfig[] {
  return [
    {
      key: "Meta",
      meta: true,
      action: toggleOverview,
      descriptionKey: "shortcuts.overview",
    },
    {
      key: "OS",
      meta: true,
      action: toggleOverview,
      descriptionKey: "shortcuts.overview",
    },
    {
      key: "1",
      meta: true,
      action: () => openApp("files"),
      descriptionKey: "shortcuts.files",
    },
    {
      key: "2",
      meta: true,
      action: () => openApp("terminal"),
      descriptionKey: "shortcuts.terminal",
    },
    {
      key: "3",
      meta: true,
      action: () => openApp("monitor"),
      descriptionKey: "shortcuts.monitor",
    },
    {
      key: "4",
      meta: true,
      action: () => openApp("settings"),
      descriptionKey: "shortcuts.settings",
    },
    {
      key: "Escape",
      action: () => {
        closeOverview();
        closeNotifications();
      },
      descriptionKey: "shortcuts.closeOverlays",
    },
    {
      key: "F1",
      action: () => openApp("settings"),
      descriptionKey: "shortcuts.settings",
    },
    {
      // F11 全屏切换（Windows 桌面惯例）：Rust 侧 Win32 自控实现真全屏
      // （铺满显示器全 rect + TOPMOST 压过任务栏，浏览器 F11 语义），状态
      // 经 fullscreen://changed 事件广播（TopBar 据此禁用 OS 窗口拖拽区）。
      // 不用 tauri set_fullscreen：tao 在透明无边框窗口上任务栏不覆盖、
      // 状态脱节。焦点在 webview child HWND 内时按键不经过前端 DOM，
      // F11 由网页层处置（airspace 固有边界）
      key: "F11",
      action: async () => {
        await invoke("toggle_fullscreen");
      },
      descriptionKey: "shortcuts.fullscreen",
    },
  ];
}

/* ── Shortcut Display Helper ─────────────────────────── */

export function formatShortcut(shortcut: ShortcutConfig): string {
  const parts: string[] = [];

  if (shortcut.ctrl) parts.push("Ctrl");
  if (shortcut.alt) parts.push("Alt");
  if (shortcut.shift) parts.push("Shift");
  if (shortcut.meta) parts.push("Super");

  parts.push(shortcut.key.toUpperCase());

  return parts.join(" + ");
}