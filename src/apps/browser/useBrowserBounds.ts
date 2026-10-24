// src/apps/browser/useBrowserBounds.ts
// content 区布局上报：ResizeObserver + 窗口几何变化 → rAF 节流 → browser_set_bounds
// （spec 第 3 节：桌面内窗口移动/缩放时的唯一热点，rAF 保证一帧最多一次）

import { useEffect, useRef, type RefObject } from "react";
import { invoke } from "@tauri-apps/api/core";
import { createLogger } from "../../utils/logger";

const log = createLogger("BrowserBounds");

/** webview 与窗口左/右/下边缘的内缩（逻辑 px，实测定值 5）。
 *  画面贴边；小于边缘 8px resize 手柄宽度，手柄被 webview 盖住、
 *  左右/下方拖拽缩放不可用——airspace 下贴边视觉优先于边缘 resize。 */
export const WEBVIEW_EDGE_INSET = 5;

/** gBCR → webview bounds：左/右/下各内缩 WEBVIEW_EDGE_INSET。
 *  顶部在 toolbar 下方、不贴窗口边，无需内缩。两处上报共用
 *  （useBrowserBounds 常规上报 + BrowserApp 拖拽结束恢复），保证口径一致。 */
export function insetWebviewRect(r: DOMRect): {
  x: number;
  y: number;
  width: number;
  height: number;
} {
  return {
    x: r.left + WEBVIEW_EDGE_INSET,
    y: r.top,
    width: r.width - WEBVIEW_EDGE_INSET * 2,
    height: r.height - WEBVIEW_EDGE_INSET,
  };
}

/** 窗口几何快照（引用值变化即触发重新上报；来自 useWindowState） */
export interface WindowGeometry {
  x: number;
  y: number;
  width: number;
  height: number;
}

export function useBrowserBounds(
  serverId: string | null,
  contentRef: RefObject<HTMLElement | null>,
  geometry: WindowGeometry,
  enabled: boolean,
  /** 会话纪元：browser_open 成功后递增，强制重报一次 bounds。
   *  host 创建是异步慢操作，就绪前发出的 browser_set_bounds 会被后端
   *  host_of 静默丢弃（rAF 与 ResizeObserver 初始回调都早于就绪），
   *  需以纪元递增驱动 effect 在 host 就绪后补发。 */
  epoch: number = 0,
) {
  const rafRef = useRef(0);

  useEffect(() => {
    const el = contentRef.current;
    if (!el || !serverId || !enabled) return;

    const report = () => {
      rafRef.current = 0;
      const r = el.getBoundingClientRect();
      const b = insetWebviewRect(r);
      // 面积为 0（最小化/隐藏）不上报，Rust 端保留旧 bounds
      if (b.width < 1 || b.height < 1) {
        // 诊断：零面积跳过是"webview 停在初始 800×600"的候选原因（闭环后移除）
        log.info(`跳过上报（面积 0）: ${Math.round(b.width)}x${Math.round(b.height)}`);
        return;
      }
      // 诊断：bounds 上报值（webview 不铺满/占位常显定位；闭环后可移除）
      log.info(
        `上报 bounds: ${Math.round(b.x)},${Math.round(b.y)} ` +
          `${Math.round(b.width)}x${Math.round(b.height)}`,
      );
      invoke("browser_set_bounds", {
        serverId,
        x: b.x,
        y: b.y,
        width: b.width,
        height: b.height,
      }).catch(() => {}); // 窗口刚关时布局事件迟到，静默
    };
    // rAF 节流：一帧最多一次
    const schedule = () => {
      if (!rafRef.current) rafRef.current = requestAnimationFrame(report);
    };

    schedule();
    const ro = new ResizeObserver(schedule);
    ro.observe(el);
    return () => {
      ro.disconnect();
      if (rafRef.current) {
        cancelAnimationFrame(rafRef.current);
        rafRef.current = 0;
      }
    };
  }, [serverId, enabled, epoch, geometry.x, geometry.y, geometry.width, geometry.height]);
}
