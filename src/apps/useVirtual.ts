// src/apps/useVirtual.ts
// 列表/网格窗口化（虚拟滚动）支持：自研固定行高窗口计算，无第三方依赖。
//
// 设计（审计 M25）：
// - 纯函数 computeWindow 可单测：scrollTop/viewportH/itemHeight → 渲染区间 + 上下占位
// - hook 订阅滚动容器 scroll（passive，rAF 合帧）与 ResizeObserver（视口高度）
// - 行高由调用方实测（首行 offsetHeight）后传入，初始值给经验默认；
//   实测值变化时窗口自动随 state 重算
// - ensureVisible：键盘导航把指定下标滚动进可视区（替代已不存在的
//   scrollIntoView——未挂载的行不在 DOM 里，必须用 scrollTop 数学）

import { useCallback, useEffect, useLayoutEffect, useState, type RefObject } from "react";

/** 窗口计算（纯函数）：返回应渲染的条目下标区间 [start, end) 与上下占位高度 */
export function computeWindow(
  scrollTop: number,
  viewportH: number,
  itemHeight: number,
  itemCount: number,
  overscan: number,
): { start: number; end: number; padTop: number; padBottom: number } {
  if (itemHeight <= 0 || itemCount <= 0) {
    return { start: 0, end: 0, padTop: 0, padBottom: 0 };
  }
  const start = Math.max(0, Math.floor(scrollTop / itemHeight) - overscan);
  const visible = Math.ceil(viewportH / itemHeight) + overscan * 2;
  const end = Math.min(itemCount, start + visible);
  return {
    start,
    end,
    padTop: start * itemHeight,
    padBottom: (itemCount - end) * itemHeight,
  };
}

export function useVirtualWindow(params: {
  /** 滚动容器（产生 scrollTop 的元素） */
  scrollRef: RefObject<HTMLElement | null>;
  itemCount: number;
  /** 单条目高度（px，统一行高；调用方实测） */
  itemHeight: number;
  /** 视口外上下各多渲染几条，滚动不闪 */
  overscan?: number;
  /** false 时不窗口化（视图未启用，返回全量窗口） */
  enabled?: boolean;
}) {
  const { scrollRef, itemCount, itemHeight, overscan = 6, enabled = true } = params;
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportH, setViewportH] = useState(0);
  const [rafRef] = useState<{ current: number | null }>(() => ({ current: null }));

  // 滚动元素身份同步：RefObject 本身永不变化，但元素可能晚于首次渲染才挂载
  // （列表首屏是 loading/空目录分支，数据到达后 .fm-list 才出现），也可能在
  // 刷新时先卸载再挂载。若监听 effect 只依赖 scrollRef/enabled，就会一直绑定
  // 在「当时还不存在」的元素上：viewportH 永远为 0 → 只渲染 overscan*2 行，
  // 目录后面全是空白（实测 12 行）。
  // 这里每次渲染后把 ref 指向的真实元素同步进 state，元素更换时重新绑定监听。
  const [scrollEl, setScrollEl] = useState<HTMLElement | null>(null);
  useLayoutEffect(() => {
    const el = enabled ? scrollRef.current : null;
    // 仅在元素身份变化时 setState（同一元素反复渲染不会触发重渲染/死循环）
    if (el !== scrollEl) setScrollEl(el);
  });

  // 滚动：rAF 合帧（高频 scroll 只保留最新值）
  useEffect(() => {
    if (!scrollEl) return;
    const onScroll = () => {
      if (rafRef.current !== null) return;
      rafRef.current = requestAnimationFrame(() => {
        rafRef.current = null;
        setScrollTop(scrollEl.scrollTop);
      });
    };
    scrollEl.addEventListener("scroll", onScroll, { passive: true });
    setScrollTop(scrollEl.scrollTop);
    return () => {
      scrollEl.removeEventListener("scroll", onScroll);
      if (rafRef.current !== null) {
        cancelAnimationFrame(rafRef.current);
        rafRef.current = null;
      }
    };
  }, [scrollEl, rafRef]);

  // 视口高度：ResizeObserver
  useEffect(() => {
    if (!scrollEl) return;
    const ro = new ResizeObserver(() => setViewportH(scrollEl.clientHeight));
    ro.observe(scrollEl);
    setViewportH(scrollEl.clientHeight);
    return () => ro.disconnect();
  }, [scrollEl]);

  const win = enabled
    ? computeWindow(scrollTop, viewportH, itemHeight, itemCount, overscan)
    : { start: 0, end: itemCount, padTop: 0, padBottom: 0 };

  /** 把下标滚动进可视区（贴边对齐） */
  const ensureVisible = useCallback(
    (index: number) => {
      const el = scrollRef.current;
      if (!el || itemHeight <= 0 || index < 0) return;
      const top = index * itemHeight;
      if (top < el.scrollTop) {
        el.scrollTop = top;
      } else if (top + itemHeight > el.scrollTop + el.clientHeight) {
        el.scrollTop = top + itemHeight - el.clientHeight;
      }
    },
    [scrollRef, itemHeight],
  );

  return { ...win, ensureVisible };
}
