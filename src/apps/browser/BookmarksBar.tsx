// src/apps/browser/BookmarksBar.tsx
// 书签栏：导航行下方的细条。点击条目在当前页签导航，右键 ContextMenu
// （打开 / 在新标签页打开 / 复制网址 / 删除书签）。横向溢出滚动；
// 无书签时整条隐藏。数据在 bookmarksStore（持久化），无痕会话同样展示
// （对齐 Chrome：无痕可见书签，只是不产生新数据）。

import { useCallback, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { useBookmarksStore } from "../../stores/bookmarksStore";
import { ContextMenu, type ContextMenuEntry } from "../../components/ContextMenu/ContextMenu";
import { createLogger } from "../../utils/logger";
import "./BookmarksBar.css";

const log = createLogger("BookmarksBar");

interface BookmarksBarProps {
  serverId: string | null;
  /** 当前页签导航（地址栏同款规范化不在此做——书签存的就是 URL） */
  onNavigate: (url: string) => void;
}

export function BookmarksBar({ serverId, onNavigate }: BookmarksBarProps) {
  const { t } = useTranslation();
  const bookmarks = useBookmarksStore((s) => s.bookmarks);
  const barVisible = useBookmarksStore((s) => s.barVisible);
  const removeBookmark = useBookmarksStore((s) => s.removeBookmark);

  const [menu, setMenu] = useState<{ x: number; y: number; url: string; title: string } | null>(null);

  const newTabNavigate = useCallback((url: string) => {
    if (serverId) invoke("browser_create_tab", { serverId, url }).catch((e) => log.error("书签新标签打开失败:", e));
  }, [serverId]);

  const menuItems: ContextMenuEntry[] = (() => {
    if (!menu) return [];
    return [
      { label: t("browser.open"), onClick: () => onNavigate(menu.url) },
      { label: t("browser.ctxOpenLinkNewTab"), onClick: () => newTabNavigate(menu.url) },
      { label: t("browser.copyPageUrl"), onClick: () => navigator.clipboard.writeText(menu.url) },
      { separator: true },
      { label: t("browser.removeBookmark"), danger: true, onClick: () => {
        const target = useBookmarksStore.getState().bookmarks.find((b) => b.url === menu.url);
        if (target) removeBookmark(target.id);
      } },
    ];
  })();

  if (!barVisible || bookmarks.length === 0) return null;

  return (
    <>
      <div className="br-bookmarks-bar">
        {bookmarks.map((b) => (
          <button
            key={b.id}
            type="button"
            className="br-bookmark-chip"
            title={`${b.title}\n${b.url}`}
            onClick={() => onNavigate(b.url)}
            onAuxClick={(e) => {
              // 中键 = 新标签页打开（浏览器惯例）
              if (e.button === 1) newTabNavigate(b.url);
            }}
            onContextMenu={(e) => {
              e.preventDefault();
              e.stopPropagation();
              setMenu({ x: e.clientX + 3, y: e.clientY + 3, url: b.url, title: b.title });
            }}
          >
            <span className="br-bm-title">{b.title}</span>
          </button>
        ))}
      </div>

      {menu && (
        <ContextMenu pos={{ x: menu.x, y: menu.y }} items={menuItems} onClose={() => setMenu(null)} />
      )}
    </>
  );
}
