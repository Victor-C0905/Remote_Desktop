/**
 * ChromeTabStrip — @sinm/react-chrome-tabs 桥接（共享组件）
 *
 * React 原生 Chrome 式页签组件（Adam Schwartz chrome-tabs 引擎的 React 移植，
 * MIT，持续维护）：标签几何（SVG 连接弧）、拖拽排序、关闭按钮、宽度自适应
 * 全部由库提供，本组件只做声明式对接：
 *
 * - tabs → TabProperties（脏标记借用 faviconClass 渲染 accent 圆点）
 * - onTabClose → 脏页签确认流程（库只发事件，移除由 React 状态驱动）
 * - onTabActive / onTabReorder → 直接映射上层回调
 * - "+ 新建页签"按钮走库官方 pinnedRight 插槽（流内渲染在页签 content 之后）
 * - 行 wrapper（.chrome-tabstrip-row）内建：portal 进标题栏槽位后 flex:1
 *   填满槽位；children 渲染在页签条之后（状态灯/菜单等行内附件）
 * - CSS 覆盖：几何配色走 --quirel-* 令牌（ChromeTabStrip.css）
 */

import type { ReactNode } from 'react';
import { Tabs } from '@sinm/react-chrome-tabs';
import '@sinm/react-chrome-tabs/css/chrome-tabs.css';
import './ChromeTabStrip.css';

export interface ChromeTabItem {
  id: string;
  label: string;
  dirty: boolean;
}

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

  /** 新建页签（+ 按钮，pinnedRight 插槽） */
  onAdd(): void;

  onActivate(id: string): void;

  /** 页签关闭（脏页签由上层拦截确认） */
  onClose(id: string): void;

  /** 拖拽排序（库 fromIndex → toIndex） */
  onReorder(from: number, to: number): void;

  /** 行内附件（状态灯、菜单等），渲染在页签条之后 */
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
  return (
    <div
      className="chrome-tabstrip-row"
      data-tabstrip-variant={variant}
    >
      <Tabs
        className="te-chrome-tabs"
        draggable
        tabs={tabs.map((t) => ({
          id: t.id,
          title: t.label,
          active: t.id === activeTabId,
          faviconClass: t.dirty ? 'te-tab-dirty' : undefined,
          isCloseIconVisible: true,
        }))}
        pinnedRight={
          <button
            type="button"
            className="te-tab-new"
            onClick={onAdd}
            title="新建页签"
          >
            +
          </button>
        }
        onTabActive={onActivate}
        onTabClose={onClose}
        onTabReorder={(_tabId, fromIndex, toIndex) => onReorder(fromIndex, toIndex)}
      />
      {children}
    </div>
  );
}
