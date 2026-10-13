/**
 * ChromeTabStrip — chrome-tabs 引擎桥接（共享组件）
 *
 * Chrome 式页签组件（Adam Schwartz chrome-tabs 引擎，@sinm/react-chrome-tabs
 * 提供引擎类与基础 CSS，MIT，持续维护）：标签几何（SVG 连接弧）、拖拽排序、
 * 关闭按钮、宽度自适应全部由引擎提供。库自带的 <Tabs> React 同步层因
 * 「按位置索引盖章 + 原地 mutate props 数组」存在页签身份错位的结构性缺陷
 * （详见 useChromeTabsHost.tsx 头注释），本组件改用自研宿主 hook 直连引擎：
 *
 * - tabs → TabProperties（脏标记借用 faviconClass 渲染 accent 圆点）
 * - onTabClose → 脏页签确认流程（引擎只发事件，移除由 React 状态驱动）
 * - onActivate / onReorder → 直接映射上层回调
 * - "+ 新建页签"按钮：绝对定位于行内、x 坐标实测紧跟末尾页签（Chrome 行为）；
 *   不用库的 pinnedRight 插槽——页签收窄溢出时绝对定位页签会画到条内透明
 *   按钮背后直至条面边缘，按钮挪到条外并钳制在条右缘预留槽（条面
 *   margin-right: 30px，裁剪边之外），永不被溢出页签侵占
 * - 行 wrapper（.chrome-tabstrip-row）内建：portal 进标题栏槽位后 flex:1
 *   填满槽位；children 渲染在行尾附件区（状态灯/菜单等）
 * - CSS 覆盖：几何配色走 --quirel-* 令牌（ChromeTabStrip.css）
 */

import type { CSSProperties, ReactNode } from 'react';
import { useLayoutEffect, useMemo, useRef, useState } from 'react';
import '@sinm/react-chrome-tabs/css/chrome-tabs.css';
import './ChromeTabStrip.css';
import { useChromeTabsHost } from './useChromeTabsHost';

export interface ChromeTabItem {
  id: string;
  label: string;
  dirty: boolean;
}

/** + 与末尾页签的间距 */
const NEW_TAB_GAP = 2;
/** + 距行尾附件的间距（24px 按钮宽 + 4px） */
const NEW_TAB_TAIL_GAP = 28;
/** 相邻页签盒子的真实重叠（chrome-tabs.js 几何）：每只页签两侧各有 9px
    隐藏边距（--tab-content-margin，页签几何 SVG 把它收进弧线里），引擎
    定位步进 = 页签内容宽 − 1，故盒子重叠 = 2×9 + 1 = 19px。
    ⚠️ 不能复刻引擎 getTabsWidth() 的记账式（Σ盒宽 + 8 − (n−1)×1）——那是
    content div 的记账宽（供 wheel 平移/滚入视界用），比页签真实右缘多出
    8 + 18×(n−1) px，照抄会让 + 随页签数越偏越远（实测 1 页签差 8px、
    2 页签差 26px） */
const TAB_BOX_OVERLAP = 19;

interface ChromeTabStripProps {
  tabs: ChromeTabItem[];
  activeTabId: string;

  /**
   * 活动页签融合的下方表面（Chrome 形态：活动页签 = 页签条正下方表面的颜色）：
   * - 'light'：内容区浅色 --quirel-window-bg（TextEditor，页签伸入 te-topbar 与内容连成一体）
   * - 'dark' ：终端黑 --quirel-terminal-bg（Terminal，页签伸入黑色终端区）
   * - 缺省  ：应用内顶栏 --quirel-headerbar-bg（历史行为）
   * 同时以 data-tabstrip-variant 标在行 wrapper 上，供 WindowShell 调整底缝桥接色
   */
  variant?: 'light' | 'dark';

  /** 新建页签（+ 按钮） */
  onAdd(): void;

  onActivate(id: string): void;

  /** 页签关闭（脏页签由上层拦截确认） */
  onClose(id: string): void;

  /** 拖拽排序（库 fromIndex → toIndex） */
  onReorder(from: number, to: number): void;

  /** 行尾附件（状态灯、菜单等） */
  children?: ReactNode;
}

export function ChromeTabStrip({
  tabs,
  activeTabId,
  variant,
  onAdd,
  onActivate,
  onClose,
  onReorder,
  children,
}: ChromeTabStripProps) {
  const rowRef = useRef<HTMLDivElement>(null);
  const tailRef = useRef<HTMLDivElement>(null);
  const [newTabLeft, setNewTabLeft] = useState<number | null>(null);

  // 传给引擎宿主的 TabProperties（useMemo 钉住标识：仅 tabs/activeTabId 真
  // 变化时才产出新数组，无关重渲染不触发宿主同步 effect 重跑——同步本身
  // 幂等，但没必要为无关渲染空转）。
  //
  // ⚠️ 这里刻意不用库的 <Tabs> 组件：它的同步层有三个结构性缺陷咬合出
  // 「页签身份按位置调包」——① handleTabReorder 在拖拽中间事件原地 splice
  // 当前 props 数组（污染 isEqual 快照防线）；② 引擎拖拽先物理移动 DOM，
  // React 状态等 dragEnd 才更新，期间 DOM 顺序 ≠ 数组顺序；③ 同步改写按
  // 位置索引（updateTabByIndex 把 DOM 第 index 个元素盖上数组第 index 项
  // 的 id/标题）→ 拖拽中任何以旧顺序数组跑的同步都会把身份盖到错的元素
  // 上，表象即「页签往后移后内容被重新加载」（前移恰好回正故无异常）。
  // 现由 useChromeTabsHost 自研宿主按 data-tab-id 识别 + 自愈式对齐 DOM
  // 顺序，结构上消灭整类错位。
  const engineTabs = useMemo(
    () =>
      tabs.map((t) => ({
        id: t.id,
        title: t.label,
        active: t.id === activeTabId,
        faviconClass: t.dirty ? 'te-tab-dirty' : undefined,
        isCloseIconVisible: true,
      })),
    [tabs, activeTabId],
  );

  // 引擎宿主：事件一律按「事件时刻元素身上的 data-tab-id」转发；tabReorder
  // 在 dragEnd 收敛为一次回调（from/to 为累积起止索引），拖拽中间绝不触碰
  // tabs 数组（这是库同步层页签身份被调包的源头，见上方注释）
  const { hostRef } = useChromeTabsHost(engineTabs, activeTabId, {
    onTabActive: (tabId) => {
      if (tabId) onActivate(tabId);
    },
    onTabClose: (tabId) => {
      if (tabId) onClose(tabId);
    },
    onTabReorder: (_tabId, fromIndex, toIndex) => onReorder(fromIndex, toIndex),
  });

  // + 按钮 x 坐标：末尾页签右缘 + 6px（Chrome 行为）。测量必须用页签元素的
  // 布局盒宽按真实几何求和（Σ offsetWidth − 19×(n−1)，见 TAB_BOX_OVERLAP），
  // 不能读 content div 内联宽度——那是引擎 getTabsWidth() 的记账宽（多算
  // 8 + 18×(n−1)），也不能跟引擎一样用 gBCR——开窗 scale(0.96→1) 期间写入
  // 缩水 4% 的值且归位前不重算；页签元素自身的 offsetWidth 是布局值，
  // translate/scale 免疫（scale 只在窗口层），从首帧起就是正确值，+ 全程不动。
  // 定位链：row(relative) → strip(relative, 库) → content，offsetLeft 均为
  // 行坐标系；观测 content/行/行尾尺寸变化重算（页签增删/窗口缩放）；
  // jsdom 无 ResizeObserver 也不跑布局，静态计算一次即可（测试只查存在性）
  useLayoutEffect(() => {
    const row = rowRef.current;
    const tail = tailRef.current;
    if (!row || !tail) return;
    const strip = row.querySelector<HTMLElement>('.chrome-tabs');
    const content = strip?.querySelector<HTMLElement>('.chrome-tabs-content');
    if (!strip || !content) return;

    const update = () => {
      const tabEls = content.querySelectorAll<HTMLElement>('.chrome-tab');
      let span: number;
      if (tabEls.length === 0) {
        // 引擎尚未建页签（挂载首帧，窗口 opacity 0 不可见）：退回 content
        // 宽度，结果被 limit 钳到右槽，窗口可见前由 RO 纠正为正确位置
        span = content.offsetWidth;
      } else {
        span = 0;
        tabEls.forEach((el, i) => {
          span += el.offsetWidth - (i > 0 ? TAB_BOX_OVERLAP : 0);
        });
      }
      const tabsEnd = strip.offsetLeft + content.offsetLeft + span;
      const limit = tail.offsetLeft - NEW_TAB_TAIL_GAP;
      setNewTabLeft(Math.min(tabsEnd + NEW_TAB_GAP, limit));
    };
    update();
    if (typeof ResizeObserver === 'undefined') return;
    const ro = new ResizeObserver(update);
    ro.observe(content);
    ro.observe(row);
    ro.observe(tail);
    return () => ro.disconnect();
  }, []);

  return (
    <div
      ref={rowRef}
      className="chrome-tabstrip-row"
      data-tabstrip-variant={variant}
    >
      {/* 引擎宿主 DOM：三层结构与库的 ChromeTabsWrapper 一致，引擎 init
          在 effect 中挂到该节点并在此建页签 */}
      <div
        ref={hostRef}
        className="chrome-tabs te-chrome-tabs"
        style={{ '--tab-content-margin': '9px', display: 'flex' } as CSSProperties}
      >
        <div className="chrome-tabs-content" />
        <div className="chrome-tabs-toolbar-right" style={{ zIndex: 1, position: 'relative' }} />
        <div className="chrome-tabs-bottom-bar" />
      </div>
      <button
        type="button"
        className="te-tab-new"
        style={newTabLeft !== null ? { left: newTabLeft } : undefined}
        onClick={onAdd}
        title="新建页签"
        aria-label="新建页签"
      />
      <div ref={tailRef} className="chrome-tabstrip-tail">
        {children}
      </div>
    </div>
  );
}
