// src/apps/browser/HistoryPanel.tsx
// 历史记录面板：导航栏时钟按钮下方的下拉面板（锚定按钮，与设置菜单同模式）。
// 列表点击导航、右键 ContextMenu（打开/新标签/复制/删除此条目）、底部清空。
// 数据在 historyStore（持久化）；无痕会话不产生记录，但仍可查看既往历史。

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useHistoryStore } from "../../stores/historyStore";
import { ContextMenu, type ContextMenuEntry } from "../../components/ContextMenu/ContextMenu";
import "./HistoryPanel.css";

/** 面板展示条数上限（完整浏览/搜索是后续任务，这里只做最近回访入口） */
const PANEL_LIMIT = 50;

const pad = (n: number) => String(n).padStart(2, "0");

/** 访问时间展示：今天只给时分，今年给月日+时分，更早补年份（纯数字，免 i18n） */
function formatVisited(ts: number): string {
  const d = new Date(ts);
  const now = new Date();
  const sameDay = d.toDateString() === now.toDateString();
  const hm = `${pad(d.getHours())}:${pad(d.getMinutes())}`;
  if (sameDay) return hm;
  const md = `${d.getMonth() + 1}/${d.getDate()} ${hm}`;
  return d.getFullYear() === now.getFullYear() ? md : `${d.getFullYear()}/${md}`;
}

interface HistoryPanelProps {
  open: boolean;
  onClose(): void;
  /** 当前页签导航 */
  onNavigate(url: string): void;
  /** 新标签页导航 */
  onNewTab(url: string): void;
}

export function HistoryPanel({ open, onClose, onNavigate, onNewTab }: HistoryPanelProps) {
  const { t } = useTranslation();
  const entries = useHistoryStore((s) => s.entries);
  const remove = useHistoryStore((s) => s.remove);
  const clear = useHistoryStore((s) => s.clear);

  const [menu, setMenu] = useState<{ x: number; y: number; url: string } | null>(null);

  // 外点 / Esc 关闭。capture 阶段监听（同 ContextMenu 理由：标题栏组件会
  // stopPropagation 截断冒泡）；切换按钮与面板自身点击不关
  useEffect(() => {
    if (!open) return;
    const onPointerDown = (e: MouseEvent) => {
      const el = e.target as Element;
      if (el.closest?.(".br-history-menu") || el.closest?.(".br-history-btn")) return;
      onClose();
    };
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("mousedown", onPointerDown, true);
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("mousedown", onPointerDown, true);
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [open, onClose]);

  const menuItems: ContextMenuEntry[] = (() => {
    if (!menu) return [];
    return [
      { label: t("browser.open"), onClick: () => onNavigate(menu.url) },
      { label: t("browser.ctxOpenLinkNewTab"), onClick: () => onNewTab(menu.url) },
      { label: t("browser.copyPageUrl"), onClick: () => navigator.clipboard.writeText(menu.url) },
      { separator: true },
      { label: t("browser.removeHistoryEntry"), danger: true, onClick: () => remove(menu.url) },
    ];
  })();

  if (!open) return null;

  const visible = entries.slice(0, PANEL_LIMIT);

  return (
    <div className="br-history-menu" role="dialog" aria-label={t("browser.history")}>
      <div className="br-history-head">
        <span className="br-history-title">{t("browser.history")}</span>
        {entries.length > 0 && (
          <button
            type="button"
            className="br-history-clear"
            onClick={() => {
              clear();
              onClose();
            }}
          >
            {t("browser.clearHistory")}
          </button>
        )}
      </div>

      {visible.length === 0 ? (
        <div className="br-history-empty">{t("browser.historyEmpty")}</div>
      ) : (
        <div className="br-history-list">
          {visible.map((e) => (
            <button
              key={`${e.visitedAt}-${e.url}`}
              type="button"
              className="br-history-item"
              title={`${e.title}\n${e.url}`}
              onClick={() => {
                onNavigate(e.url);
                onClose();
              }}
              onAuxClick={(ev) => {
                if (ev.button === 1) onNewTab(e.url);
              }}
              onContextMenu={(ev) => {
                ev.preventDefault();
                ev.stopPropagation();
                setMenu({ x: ev.clientX + 3, y: ev.clientY + 3, url: e.url });
              }}
            >
              <span className="br-history-text">
                <span className="br-history-entry-title">{e.title}</span>
                <span className="br-history-entry-url">{e.url}</span>
              </span>
              <span className="br-history-time">{formatVisited(e.visitedAt)}</span>
            </button>
          ))}
        </div>
      )}

      {menu && <ContextMenu pos={{ x: menu.x, y: menu.y }} items={menuItems} onClose={() => setMenu(null)} />}
    </div>
  );
}
