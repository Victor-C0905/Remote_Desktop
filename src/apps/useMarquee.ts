// src/apps/useMarquee.ts
// 文件管理器框选（marquee）状态机：按下 → 追踪 → 命中检测 → 抬起的完整
// 生命周期，列表/网格容器共用。命中与选区纯函数在 fileSelection.ts；
// Ctrl/⌘ 反选语义（命中项相对起拖选区取反）也在此。

import { useCallback, useRef, useState, type Dispatch, type SetStateAction } from 'react';
import { marqueeHit, normalizeRect, type Rect } from './fileSelection';
import type { FileEntry } from './fileTypes';

/** 框选矩形（viewport 坐标） */
export interface MarqueeRect {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
}

/** 命中检测用的条目盒（行元素实测 rect） */
export interface EntryBox {
  path: string;
  rect: Rect;
}

export function useMarquee(params: {
  /** 正在内联重命名的条目（重命名期间不启动框选） */
  editingEntry: FileEntry | null;
  /** 起拖时的选区（Ctrl 反选基准） */
  selected: string[];
  setSelected: Dispatch<SetStateAction<string[]>>;
  /** 条目盒提供者（审计 M25）：虚拟化下未挂载行不在 DOM，调用方按
   *  几何计算全量条目矩形；缺省回退 DOM 查询（兼容非虚拟化容器） */
  rectsProvider?: () => EntryBox[] | null;
}) {
  const { editingEntry, selected, setSelected, rectsProvider } = params;
  const [marquee, setMarquee] = useState<MarqueeRect | null>(null);
  const rectsRef = useRef<EntryBox[] | null>(null);
  const rafRef = useRef<number | null>(null);
  const startRef = useRef<{ x: number; y: number } | null>(null);
  const latestRef = useRef<{ x: number; y: number } | null>(null);
  const ctrlRef = useRef(false);
  const baseRef = useRef<string[]>([]);
  /** 框选刚结束标记：容器 click 跟在 mouseup 之后触发，需跳过一次以防误清选区 */
  const justEndedRef = useRef(false);

  /** 框选入口：绑在列表/网格容器的 onMouseDown（空白处左键） */
  const beginMarquee = useCallback((e: React.MouseEvent) => {
    // 仅左键、且起点不在条目上（条目点击/拖拽走各自的处理器）
    if (e.button !== 0 || editingEntry) return;
    const target = e.target as HTMLElement;
    if (target.closest('.fm-list-row') || target.closest('.fm-grid-item')) return;

    const container = e.currentTarget as HTMLElement;
    const collectRects = () => {
      const provided = rectsProvider?.();
      if (provided) {
        rectsRef.current = provided;
        return;
      }
      const rects: EntryBox[] = [];
      container.querySelectorAll<HTMLElement>('[data-path]').forEach((el) => {
        const p = el.dataset.path;
        if (p) rects.push({ path: p, rect: el.getBoundingClientRect() });
      });
      rectsRef.current = rects;
    };
    collectRects();
    ctrlRef.current = e.ctrlKey || e.metaKey;
    baseRef.current = selected;
    startRef.current = { x: e.clientX, y: e.clientY };
    latestRef.current = { x: e.clientX, y: e.clientY };

    // mousemove 只记点 + 排帧：命中检测与 setState 合并到每帧一次（大目录不掉帧）
    const onMove = (ev: MouseEvent) => {
      latestRef.current = { x: ev.clientX, y: ev.clientY };
      if (rafRef.current === null) {
        rafRef.current = requestAnimationFrame(() => {
          rafRef.current = null;
          const start = startRef.current;
          const latest = latestRef.current;
          const cached = rectsRef.current;
          if (!start || !latest || !cached) return;
          // 点击/拖动判别：未超 3px 不画框不改选区（原地点击的清选区由
          // 容器 click 路径负责）
          const mdx = latest.x - start.x;
          const mdy = latest.y - start.y;
          if (mdx * mdx + mdy * mdy <= 9) return;
          setMarquee({ x0: start.x, y0: start.y, x1: latest.x, y1: latest.y });
          const mq = normalizeRect(start.x, start.y, latest.x, latest.y);
          const hits = marqueeHit(cached.map((c) => c.path), cached.map((c) => c.rect), mq);
          if (ctrlRef.current) {
            // Ctrl 反选：命中项相对起拖选区取反（Set 保序：基准顺序在前）
            const next = new Set(baseRef.current);
            for (const p of hits) {
              if (next.has(p)) next.delete(p);
              else next.add(p);
            }
            setSelected([...next]);
          } else {
            setSelected(hits);
          }
        });
      }
    };
    const onWheel = () => collectRects();
    const onUp = () => {
      document.removeEventListener('mousemove', onMove);
      document.removeEventListener('mouseup', onUp);
      document.removeEventListener('wheel', onWheel);
      if (rafRef.current !== null) {
        cancelAnimationFrame(rafRef.current);
        rafRef.current = null;
      }
      // 实际拖动过（>3px）才抑制后续 click 的清选区；原地点击放行
      const start = startRef.current;
      const latest = latestRef.current;
      if (start && latest && (Math.abs(latest.x - start.x) > 3 || Math.abs(latest.y - start.y) > 3)) {
        justEndedRef.current = true;
      }
      startRef.current = null;
      rectsRef.current = null;
      setMarquee(null);
    };
    document.addEventListener('mousemove', onMove);
    document.addEventListener('mouseup', onUp);
    document.addEventListener('wheel', onWheel, { passive: true });
  }, [editingEntry, selected, setSelected]);

  return { marquee, beginMarquee, justEndedRef };
}
