import { useEffect, useCallback } from "react";

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