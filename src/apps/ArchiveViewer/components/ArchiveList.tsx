/**
 * 压缩包内容列表（平面导航，对齐 7-Zip / WinRAR）
 *
 * - 只显示当前目录内容，`..` 返回上一级
 * - 强调色行选中（非复选框），支持 Ctrl/Shift、Ctrl+A、Esc
 * - 空白处或行上拖动形成矩形框选，贴边自动滚动
 * - 双击目录进入、双击文件打开；Enter/Backspace 键盘导航
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { AppIcon } from '../../../components/AppIcon';
import type { ArchiveTreeNode } from '../tree';
import { fmtSize } from '../utils/format';

interface ArchiveListProps {
  /** 当前目录的子节点 */
  nodes: ArchiveTreeNode[];
  /** 当前目录内部路径（'' 表示包根目录） */
  currentPath: string;
  selected: Set<string>;
  extractingPath: string | null;
  onNavigate: (dirPath: string) => void;
  onOpenFile: (node: ArchiveTreeNode) => void;
  onSelectionChange: (next: Set<string>) => void;
}

/** 拖动超过该像素才进入框选，避免和点击/双击冲突 */
const DRAG_THRESHOLD = 4;
/** 贴边自动滚动速度（px/帧） */
const AUTO_SCROLL_STEP = 12;
/** 触发自动滚动的边缘距离 */
const AUTO_SCROLL_EDGE = 24;

interface MarqueeRect { left: number; top: number; width: number; height: number }

interface DragState {
  startX: number;
  startY: number;
  pointerX: number;
  pointerY: number;
  /** Ctrl 拖动：保留拖动前已选内容 */
  additive: boolean;
  base: Set<string>;
  active: boolean;
}

export function ArchiveList({
  nodes, currentPath, selected, extractingPath,
  onNavigate, onOpenFile, onSelectionChange,
}: ArchiveListProps) {
  const { t } = useTranslation();
  const scrollRef = useRef<HTMLDivElement>(null);
  const selectedRef = useRef(selected);
  selectedRef.current = selected;
  const nodesRef = useRef(nodes);
  nodesRef.current = nodes;
  const selectionCbRef = useRef(onSelectionChange);
  selectionCbRef.current = onSelectionChange;

  const [marquee, setMarquee] = useState<MarqueeRect | null>(null);
  const [focusIndex, setFocusIndex] = useState<number | null>(null);
  const anchorRef = useRef<string | null>(null);
  const dragRef = useRef<DragState | null>(null);
  const rafRef = useRef<number | null>(null);
  const hasUp = currentPath !== '';
  // 包内路径没有前导斜杠：顶层目录的上一级是包根（''），不是 '/'
  const upPath = hasUp && currentPath.includes('/')
    ? currentPath.slice(0, currentPath.lastIndexOf('/'))
    : '';

  const stopAutoScroll = useCallback(() => {
    if (rafRef.current !== null) {
      cancelAnimationFrame(rafRef.current);
      rafRef.current = null;
    }
  }, []);

  // 目录切换/内容刷新后清空键盘焦点与 Shift 锚点
  useEffect(() => {
    anchorRef.current = null;
    setFocusIndex(null);
  }, [currentPath, nodes]);

  useEffect(() => () => stopAutoScroll(), [stopAutoScroll]);

  /** 点击/键盘选中：Shift 按可见顺序连选，Ctrl 切换，普通单选 */
  const applySelection = useCallback((node: ArchiveTreeNode, mods: { ctrl: boolean; shift: boolean }) => {
    const list = nodesRef.current;
    const current = selectedRef.current;
    // Ctrl 保留原选择（Shift+Ctrl 为并集）；普通/单纯 Shift 从空集合开始，Shift 连选替换原选择
    const next = new Set(mods.ctrl ? current : []);
    const anchor = anchorRef.current;

    if (mods.shift && anchor) {
      const from = list.findIndex((n) => n.path === anchor);
      const to = list.findIndex((n) => n.path === node.path);
      if (from >= 0 && to >= 0) {
        const [lo, hi] = from <= to ? [from, to] : [to, from];
        if (!mods.ctrl) next.clear();
        for (let i = lo; i <= hi; i++) next.add(list[i].path);
        selectionCbRef.current(next);
        return;
      }
    }
    if (mods.ctrl) {
      if (next.has(node.path)) next.delete(node.path);
      else next.add(node.path);
    } else {
      next.add(node.path);
    }
    selectionCbRef.current(next);
    anchorRef.current = node.path;
  }, []);

  /** 按当前拖动指针位置计算矩形框选命中行 */
  const updateMarquee = useCallback(() => {
    const drag = dragRef.current;
    const el = scrollRef.current;
    if (!drag || !drag.active || !el) return;
    const rect = el.getBoundingClientRect();
    const { startX, startY, pointerX, pointerY } = drag;

    const left = Math.min(startX, pointerX) - rect.left + el.scrollLeft;
    const top = Math.min(startY, pointerY) - rect.top + el.scrollTop;
    const width = Math.abs(pointerX - startX);
    const height = Math.abs(pointerY - startY);
    setMarquee({ left, top, width, height });

    const hitLeft = Math.min(startX, pointerX);
    const hitRight = Math.max(startX, pointerX);
    const hitTop = Math.min(startY, pointerY);
    const hitBottom = Math.max(startY, pointerY);
    const hits = new Set(drag.base);
    el.querySelectorAll<HTMLElement>('[data-index]').forEach((row) => {
      const r = row.getBoundingClientRect();
      if (r.right > hitLeft && r.left < hitRight && r.bottom > hitTop && r.top < hitBottom) {
        const index = Number(row.dataset.index);
        const node = nodesRef.current[index];
        if (node) hits.add(node.path);
      }
    });
    selectionCbRef.current(hits);
  }, []);

  const startAutoScroll = useCallback(() => {
    if (rafRef.current !== null) return;
    const tick = () => {
      const drag = dragRef.current;
      const el = scrollRef.current;
      if (!drag || !drag.active || !el) {
        rafRef.current = null;
        return;
      }
      const rect = el.getBoundingClientRect();
      let dy = 0;
      if (drag.pointerY < rect.top + AUTO_SCROLL_EDGE) dy = -AUTO_SCROLL_STEP;
      else if (drag.pointerY > rect.bottom - AUTO_SCROLL_EDGE) dy = AUTO_SCROLL_STEP;
      if (dy !== 0) {
        el.scrollTop += dy;
        updateMarquee();
      }
      rafRef.current = requestAnimationFrame(tick);
    };
    rafRef.current = requestAnimationFrame(tick);
  }, [updateMarquee]);

  const handlePointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    const el = scrollRef.current;
    if (!el) return;
    // `..` 行不参与选择/框选，保留双击返回
    if ((e.target as HTMLElement).closest('[data-up]')) return;

    const row = (e.target as HTMLElement).closest<HTMLElement>('[data-index]');
    const index = row ? Number(row.dataset.index) : null;
    if (index !== null) {
      const node = nodesRef.current[index];
      if (node) applySelection(node, { ctrl: e.ctrlKey || e.metaKey, shift: e.shiftKey });
    } else if (!e.ctrlKey && !e.metaKey && !e.shiftKey) {
      selectionCbRef.current(new Set());
    }

    dragRef.current = {
      startX: e.clientX,
      startY: e.clientY,
      pointerX: e.clientX,
      pointerY: e.clientY,
      additive: e.ctrlKey || e.metaKey,
      base: e.ctrlKey || e.metaKey ? new Set(selectedRef.current) : new Set(),
      active: false,
    };
    // 注意：此处不能立刻 setPointerCapture——指针捕获会把 click/dblclick
    // 重定向到列表容器，导致行上的双击事件永远不触发（文件夹进不去）。
    // 只有确认进入拖拽后才捕获，保证框选移出列表时仍能继续。
  };

  const handlePointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    const drag = dragRef.current;
    if (!drag) return;
    drag.pointerX = e.clientX;
    drag.pointerY = e.clientY;
    if (!drag.active) {
      if (Math.max(Math.abs(e.clientX - drag.startX), Math.abs(e.clientY - drag.startY)) < DRAG_THRESHOLD) {
        return;
      }
      drag.active = true;
      scrollRef.current?.setPointerCapture(e.pointerId);
      startAutoScroll();
    }
    updateMarquee();
  };

  const handlePointerUp = (e: React.PointerEvent<HTMLDivElement>) => {
    const drag = dragRef.current;
    dragRef.current = null;
    setMarquee(null);
    stopAutoScroll();
    if (e.currentTarget.hasPointerCapture(e.pointerId)) {
      e.currentTarget.releasePointerCapture(e.pointerId);
    }
    // 框选结束后更新 Shift 锚点为指针所在行，便于后续连选
    if (drag?.active) {
      const row = document.elementFromPoint(e.clientX, e.clientY)?.closest<HTMLElement>('[data-index]');
      if (row) {
        const node = nodesRef.current[Number(row.dataset.index)];
        if (node) anchorRef.current = node.path;
      }
    }
  };

  /**
   * 双击导航。挂在滚动容器而非行上：指针捕获（框选）会把 click/dblclick
   * 重定向到容器，行上的 onDoubleClick 可能收不到；这里先看事件目标，
   * 必要时再用指针坐标命中行。
   */
  const handleDoubleClick = (e: React.MouseEvent<HTMLDivElement>) => {
    const direct = (e.target as HTMLElement).closest<HTMLElement>('[data-up], [data-index]');
    const row = direct ?? document.elementFromPoint(e.clientX, e.clientY)
      ?.closest<HTMLElement>('[data-up], [data-index]');
    if (!row) return;
    if (row.dataset.up !== undefined) {
      if (hasUp) onNavigate(upPath);
      return;
    }
    const node = nodesRef.current[Number(row.dataset.index)];
    if (!node) return;
    if (node.isDir) onNavigate(node.path);
    else onOpenFile(node);
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'a') {
      e.preventDefault();
      const next = new Set(nodesRef.current.map((n) => n.path));
      selectionCbRef.current(next);
      anchorRef.current = nodesRef.current[0]?.path ?? null;
      return;
    }
    if (e.key === 'Escape') {
      e.preventDefault();
      selectionCbRef.current(new Set());
      anchorRef.current = null;
      return;
    }
    if (e.key === 'Backspace' && hasUp) {
      e.preventDefault();
      onNavigate(upPath);
      return;
    }
    if (e.key === 'Enter') {
      if (focusIndex === -1 && hasUp) {
        e.preventDefault();
        onNavigate(upPath);
        return;
      }
      const node = focusIndex === null ? undefined : nodesRef.current[focusIndex];
      if (!node) return;
      e.preventDefault();
      if (node.isDir) onNavigate(node.path);
      else onOpenFile(node);
      return;
    }
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
    const count = nodesRef.current.length;
    if (count === 0 && !hasUp) return;
    e.preventDefault();
    const min = hasUp ? -1 : 0;
    const delta = e.key === 'ArrowDown' ? 1 : -1;
    let nextIndex: number;
    if (focusIndex === null) {
      nextIndex = delta > 0 ? Math.max(min, 0) : Math.max(min, count - 1);
    } else {
      nextIndex = Math.min(Math.max(focusIndex + delta, min), count - 1);
    }
    setFocusIndex(nextIndex);
    if (nextIndex === -1) return;
    const node = nodesRef.current[nextIndex];
    if (node) applySelection(node, { ctrl: false, shift: e.shiftKey });
  };

  return (
    <div className="av-list">
      <div className="av-list-head">
        <span className="av-col-name">{t('archive.colName')}</span>
        <span className="av-col-size">{t('archive.colSize')}</span>
      </div>
      <div
        ref={scrollRef}
        className="av-list-body"
        tabIndex={0}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerUp}
        onPointerCancel={handlePointerUp}
        onDoubleClick={handleDoubleClick}
        onKeyDown={handleKeyDown}
      >
        {hasUp && (
          <div
            className="av-row av-up-row"
            data-up
            title={t('archive.parentDirectory')}
          >
            <span className="av-up-icon">↑</span>
            <span className="av-row-name">..</span>
          </div>
        )}
        {nodes.map((node, index) => (
          <div
            key={node.path}
            data-index={index}
            className={`av-row${selected.has(node.path) ? ' av-selected' : ''}${
              extractingPath === node.path ? ' av-extracting' : ''
            }`}
            title={node.path}
          >
            <span className="av-icon">
              <AppIcon
                name={node.isDir ? 'folder' : node.kind === 'file' ? 'file' : 'file-archive'}
                size={15}
              />
            </span>
            <span className="av-row-name">{node.name}</span>
            <span className="av-row-size">{node.isDir ? '' : fmtSize(node.size)}</span>
          </div>
        ))}
        {nodes.length === 0 && (
          <div className="av-empty">{t('archive.folderEmpty')}</div>
        )}
        {marquee && (
          <div
            className="av-marquee"
            style={{
              left: marquee.left, top: marquee.top,
              width: marquee.width, height: marquee.height,
            }}
          />
        )}
      </div>
    </div>
  );
}
