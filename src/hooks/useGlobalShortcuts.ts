import { useEffect, useCallback } from "react";

/* ── Types ─────────────────────────────────────────────── */

interface ShortcutConfig {
  key: string;
  ctrl?: boolean;
  alt?: boolean;
  shift?: boolean;
  meta?: boolean;
  action: () => void;
  description?: string;
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
          shortcut.action();
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

/* ── Preset Shortcuts for GNOME Remote ────────────────── */

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
      description: "打开活动概览",
    },
    {
      key: "OS",
      meta: true,
      action: toggleOverview,
      description: "打开活动概览",
    },
    {
      key: "1",
      meta: true,
      action: () => openApp("files"),
      description: "打开文件管理器",
    },
    {
      key: "2",
      meta: true,
      action: () => openApp("terminal"),
      description: "打开终端",
    },
    {
      key: "3",
      meta: true,
      action: () => openApp("monitor"),
      description: "打开系统监控",
    },
    {
      key: "4",
      meta: true,
      action: () => openApp("settings"),
      description: "打开设置",
    },
    {
      key: "Escape",
      action: () => {
        closeOverview();
        closeNotifications();
      },
      description: "关闭概览/通知中心",
    },
    {
      key: "F1",
      action: () => openApp("settings"),
      description: "打开设置",
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