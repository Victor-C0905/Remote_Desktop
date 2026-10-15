/**
 * useChromeTabsHost — chrome-tabs 引擎宿主（替换 @sinm/react-chrome-tabs
 * 的 <Tabs> React 同步层；引擎本身仍用库的 chrome-tabs.js）
 *
 * 为什么不能用库的 <Tabs> 组件（三个结构性缺陷咬合出「页签身份按位置调包」）：
 * 1. component.js 的 handleTabReorder 在每个拖拽中间事件「原地 splice 当前
 *    props 数组」（污染 usePrevious 快照，isEqual 防线失效）；
 * 2. 引擎拖拽是「先物理移动 DOM 元素」，React 状态要等 dragEnd 才更新，
 *    拖拽全程 DOM 顺序 ≠ React 数组顺序；
 * 3. component.js 的同步改写是「按位置索引」的（updateTabByIndex：DOM 第
 *    index 个元素被盖上数组第 index 项的 data-tab-id/标题）。任何以旧顺序
 *    数组跑的同步都会把身份盖到错的元素上 → 点击某位置激活的是别的会话，
 *    表象即「页签往后移后内容被重新加载」（前移恰好回正故无异常）。
 *
 * 本宿主的同步改用「按 id 识别 + 自愈式对齐」，结构上消灭整类错位：
 * - 事件只读「事件时刻元素身上的 data-tab-id」转发给上层；
 * - tabReorder 只累积 from/to（复刻库「dragEnd 才回调一次」的语义），
 *   绝不触碰 props 数组；
 * - 同步按 id 增删/搬移 DOM 元素（缺失则 addTab、位置不对则 insertBefore），
 *   把 DOM 顺序强制对齐 React 数组，任何错位都会被下一轮同步纠正；
 * - 引擎拖拽中（isDragging）只更新既有元素的标题/脏标记等元数据，
 *   结构性增删/搬移推迟到 dragEnd 之后补做，避免与拖拽动画打架。
 */

import { useCallback, useEffect, useRef } from 'react';
import ChromeTabsEngine from '@sinm/react-chrome-tabs/dist/chrome-tabs';
import type { TabProperties } from '@sinm/react-chrome-tabs/dist/chrome-tabs';

export interface ChromeTabsHostListeners {
  onTabActive(tabId: string | undefined): void;
  onTabClose(tabId: string | undefined): void;
  /** 右键菜单（可选；tabId 为元素事件时刻的真实 id） */
  onTabContextMenu?(tabId: string | undefined, event: MouseEvent): void;
  /** 拖拽排序（dragEnd 时回调一次，from/to 为累积的起止索引） */
  onTabReorder(tabId: string | undefined, fromIndex: number, toIndex: number): void;
}

export function useChromeTabsHost(
  tabs: TabProperties[],
  activeTabId: string | undefined,
  listeners: ChromeTabsHostListeners,
) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const engineRef = useRef<ChromeTabsEngine | null>(null);
  // 渲染期直写 ref，保证引擎回调（异步的 dragEnd/rAF 链）读到的永远是最新值
  const listenersRef = useRef(listeners);
  listenersRef.current = listeners;
  const tabsRef = useRef(tabs);
  tabsRef.current = tabs;
  // activeTabId 也走 ref：sync 是 useCallback([]) 的稳定闭包，直接读参数会
  // 永远停在首帧旧值（新增页签后激活对齐会被过期值跳过）
  const activeTabIdRef = useRef(activeTabId);
  activeTabIdRef.current = activeTabId;

  // 同步引擎 DOM ↔ React 状态（幂等，可随时重复执行）。全部读取走 ref，
  // 便于 dragEnd 等引擎回调在任意时机补做同步。
  const sync = useCallback(() => {
    const engine = engineRef.current;
    if (!engine) return;
    const tabsNow = tabsRef.current;
    const content = engine.tabContentEl;
    if (!content) return;

    const dragging = engine.isDragging;
    const byId = new Map<string, HTMLElement>();
    engine.tabEls.forEach((el) => {
      const id = el.getAttribute('data-tab-id');
      if (id) byId.set(id, el);
    });

    let structural = false;
    if (!dragging) {
      // ① 移除 React 已删除的页签（引擎 removeTab 内部处理激活移交/重布局）
      byId.forEach((el, id) => {
        if (!tabsNow.some((t) => t.id === id)) {
          engine.removeTab(el);
          byId.delete(id);
          structural = true;
        }
      });
      // ② 按 React 顺序补齐缺失页签、把既有元素搬到正确位置（自愈核心：
      //    addTab/insertBefore 全部按 id 定位，绝不按位置索引盖章）
      tabsNow.forEach((tab, index) => {
        let el = byId.get(tab.id);
        if (!el) {
          el = engine.addTab({ ...tab, active: false }, { background: true });
          byId.set(tab.id, el);
          structural = true;
        }
        if (el !== content.children[index]) {
          content.insertBefore(el, content.children[index] ?? null);
          structural = true;
        }
      });
    }

    // ③ 结构变化后重算布局（addTab/removeTab 内部已各自 layoutTabs，
    //    这里只为 insertBefore 补一次；重复调用幂等）
    if (structural) engine.layoutTabs();

    // ④ 激活态按 id 对齐（setCurrentTab 幂等：目标已激活直接返回）
    const activeId = activeTabIdRef.current;
    const activeEl = engine.activeTabEl;
    if (activeEl?.getAttribute('data-tab-id') !== activeId) {
      const wanted = activeId
        ? engine.tabEls.find((el) => el.getAttribute('data-tab-id') === activeId)
        : undefined;
      if (wanted) engine.setCurrentTab(wanted);
    }
  }, []);

  const syncRef = useRef(sync);
  syncRef.current = sync;

  // 挂载：创建引擎并接线。清理时移除全部页签元素，让 StrictMode 下的
  // 重挂载从干净容器重建（引擎 destroy 只清 window/document 监听）。
  useEffect(() => {
    const engine = new ChromeTabsEngine({ draggable: true });
    engineRef.current = engine;
    engine.init(hostRef.current!);
    return () => {
      engineRef.current = null;
      const content = engine.tabContentEl;
      content?.querySelectorAll('.chrome-tab').forEach((el) => el.remove());
      engine.destroy();
    };
  }, []);

  // 引擎事件接线：一律以「事件时刻元素身上的 data-tab-id」为准转发。
  // tabReorder 只累积起止索引（复刻库「dragEnd 才回调一次」的语义），绝不
  // 触碰 props 数组；dragEnd 回调后补做一次同步，收尾 isDragging 期间被
  // 推迟的结构性变更（如拖拽中改了标题/关闭了页签的场景）。
  useEffect(() => {
    const engine = engineRef.current;
    if (!engine) return;
    const el = engine.el;

    const idOf = (event: Event) =>
      (event as CustomEvent<{ tabEl: HTMLElement }>).detail.tabEl.getAttribute('data-tab-id') ??
      undefined;

    const onTabClick = (event: Event) => {
      listenersRef.current.onTabActive(idOf(event));
    };
    const onTabClose = (event: Event) => listenersRef.current.onTabClose(idOf(event));
    const onContextMenu = (event: Event) =>
      listenersRef.current.onTabContextMenu?.(
        idOf(event),
        (event as CustomEvent<{ event: MouseEvent }>).detail.event,
      );

    let moveFrom: number | null = null;
    let moveTo = -1;
    let movedId: string | undefined;
    const onTabReorder = (event: Event) => {
      const detail = (event as CustomEvent<{ tabEl: HTMLElement; originIndex: number; destinationIndex: number }>)
        .detail;
      if (moveFrom === null) moveFrom = detail.originIndex;
      moveTo = detail.destinationIndex;
      movedId = detail.tabEl.getAttribute('data-tab-id') ?? undefined;
    };
    // 引擎在发出 dragEnd 前已把 isDragging 置 false，但 rAF 收尾链还在跑，
    // 这里的兜底同步必须推迟到 React 微任务提交之后，且 sync 幂等，常规
    // 路径下 effects 已对齐、此调用为空转
    const onDragEnd = () => {
      if (moveFrom !== null) {
        listenersRef.current.onTabReorder(movedId, moveFrom, moveTo);
        moveFrom = null;
        moveTo = -1;
      }
      window.setTimeout(() => {
        syncRef.current();
      }, 0);
    };

    el.addEventListener('tabClick', onTabClick);
    el.addEventListener('tabClose', onTabClose);
    el.addEventListener('contextmenu', onContextMenu);
    el.addEventListener('tabReorder', onTabReorder);
    el.addEventListener('dragEnd', onDragEnd);
    return () => {
      el.removeEventListener('tabClick', onTabClick);
      el.removeEventListener('tabClose', onTabClose);
      el.removeEventListener('contextmenu', onContextMenu);
      el.removeEventListener('tabReorder', onTabReorder);
      el.removeEventListener('dragEnd', onDragEnd);
    };
  }, []);

  // React 状态 → 引擎 DOM（自愈式同步）
  useEffect(() => {
    sync();
  }, [tabs, activeTabId, sync]);

  return { hostRef };
}
