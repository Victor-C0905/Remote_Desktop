// src/window-system/hooks/useEscCloseWindow.ts

import { useEffect } from 'react';
import { useWindowManager } from './useWindowManager';

/**
 * 查看器类窗口：按 Esc 关闭窗口（BUG-VIEW-2）。
 *
 * 常规窗口管理器不处理 Esc；图片/Hex/PDF/压缩包等「打开即看」的查看器
 * 沿用桌面应用习惯（Esc = 关闭/退出）。
 *
 * @param windowId 窗口 id；为空（异常形态）时不启用
 * @param blocked  内部对话框/模态打开时传 true——Esc 让位给对话框自身处理；
 *                 焦点在输入框/可编辑区时同样不接管（输入中的 Esc 优先）
 */
export function useEscCloseWindow(
  windowId: string | null | undefined,
  blocked = false,
): void {
  const { manager } = useWindowManager();

  useEffect(() => {
    if (!windowId || blocked) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      // 仅响应本窗口处于活动态时的 Esc：所有窗口共用同一 document，
      // 无焦点判断会让一次 Esc 关闭全部查看器窗口（实测 2 个同时关闭）
      if (manager.getActive()?.id !== windowId) return;
      const ae = document.activeElement;
      // 焦点在「其它窗口」内时不算输入态：所有窗口共用同一 document，
      // 点击查看器标题栏不会自动让后台终端的隐藏 textarea 失焦；不排除
      // 他窗焦点的话，Esc 会被 INPUT/TEXTAREA 守卫永久吞掉（实测）
      const aeShell = ae instanceof HTMLElement ? ae.closest('[data-window-id]') : null;
      const aeInOtherWindow =
        aeShell instanceof HTMLElement && aeShell.getAttribute('data-window-id') !== windowId;
      if (
        !aeInOtherWindow &&
        ae instanceof HTMLElement &&
        (ae.tagName === 'INPUT' || ae.tagName === 'TEXTAREA' || ae.isContentEditable)
      ) {
        return;
      }
      e.preventDefault();
      manager.close(windowId);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [manager, windowId, blocked]);
}
