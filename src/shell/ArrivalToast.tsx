// src/shell/ArrivalToast.tsx
// 新通知到达预览条：TopBar 下方居中悬浮，短暂展示最新一条通知
// 行为：显示 3.5s 自动收起；hover 暂停计时；点击打开通知中心；
//       连发替换内容并重置计时，期间错过的条目累计为「+N」

import { useEffect, useRef, useState, useCallback } from "react";
import { useNotificationStore } from "../stores/notificationStore";
import type { AppNotification, NotificationUrgency } from "../stores/notificationStore";
import { SymbolicIcon } from "../components/symbolic";
import "./ArrivalToast.css";

const DISPLAY_MS = 3500;
const EXIT_MS = 200;

/** urgency → Symbolic 图标 */
function urgencyIcon(urgency: NotificationUrgency) {
  switch (urgency) {
    case "critical": return "dialog-error" as const;
    case "normal": return "dialog-warning" as const;
    case "low": return "dialog-information" as const;
  }
}

export function ArrivalToast({ onOpen }: { onOpen: () => void }) {
  const notifications = useNotificationStore((s) => s.notifications);
  const [current, setCurrent] = useState<AppNotification | null>(null);
  const [missed, setMissed] = useState(0);
  const [visible, setVisible] = useState(false);
  const [leaving, setLeaving] = useState(false);
  const shownIdRef = useRef<string | null>(null);
  const hideTimerRef = useRef<number | null>(null);
  const leaveTimerRef = useRef<number | null>(null);

  const clearTimers = useCallback(() => {
    if (hideTimerRef.current !== null) {
      window.clearTimeout(hideTimerRef.current);
      hideTimerRef.current = null;
    }
    if (leaveTimerRef.current !== null) {
      window.clearTimeout(leaveTimerRef.current);
      leaveTimerRef.current = null;
    }
  }, []);

  /** 收起：进入 200ms 退出动画后完全隐藏 */
  const dismiss = useCallback(() => {
    clearTimers();
    setLeaving(true);
    leaveTimerRef.current = window.setTimeout(() => {
      setVisible(false);
      setLeaving(false);
      setMissed(0);
      setCurrent(null);
    }, EXIT_MS);
  }, [clearTimers]);

  const scheduleHide = useCallback(() => {
    if (hideTimerRef.current !== null) window.clearTimeout(hideTimerRef.current);
    hideTimerRef.current = window.setTimeout(dismiss, DISPLAY_MS);
  }, [dismiss]);

  // 监听新通知（store 头部插入最新）
  useEffect(() => {
    if (notifications.length === 0) return;
    const latest = notifications[0];
    if (shownIdRef.current === latest.id) return;
    if (visible) setMissed((m) => m + 1); // 展示期间连发 → 累计
    shownIdRef.current = latest.id;
    setCurrent(latest);
    setLeaving(false);
    setVisible(true);
    scheduleHide();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [notifications]);

  // 卸载清理
  useEffect(() => clearTimers, [clearTimers]);

  if (!visible || !current) return null;

  return (
    <div
      className={`at-toast${leaving ? " at-toast-leaving" : ""}`}
      role="status"
      onClick={() => {
        dismiss();
        onOpen();
      }}
      onMouseEnter={() => {
        // hover 暂停自动隐藏计时
        if (hideTimerRef.current !== null) {
          window.clearTimeout(hideTimerRef.current);
          hideTimerRef.current = null;
        }
      }}
      onMouseLeave={() => {
        if (!leaving) scheduleHide();
      }}
    >
      <SymbolicIcon
        name={urgencyIcon(current.urgency)}
        size={16}
        className={`at-sev at-sev-${current.urgency}`}
      />
      <span className="at-title">{current.title}</span>
      {current.source && <span className="at-source">{current.source}</span>}
      {missed > 0 && <span className="at-missed">+{missed}</span>}
      <button
        className="at-close"
        aria-label="关闭预览"
        onClick={(e) => {
          e.stopPropagation();
          dismiss();
        }}
      >
        <SymbolicIcon name="window-close" size={12} />
      </button>
    </div>
  );
}
