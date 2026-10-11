/**
 * ChromeTabStrip — chrome-tabs 库桥接组件
 *
 * 使用成熟的 chrome-tabs 库（adamschwartz/chrome-tabs，MIT）渲染
 * Chrome 式页签条：标签几何（SVG 连接弧）、拖拽排序、关闭按钮全部由库提供，
 * React 只负责状态，通过命令式 API 双向同步。
 *
 * 集成要点（踩坑总结）：
 * - 库要求固定骨架：.chrome-tabs > .chrome-tabs-content（缺省会在
 *   layoutTabs 处抛 clientWidth of null）
 * - 库没有 destroy API：resize 监听器与延迟回调永久存活。卸载时把实例
 *   重定向到带骨架的哑元素，让残留调用安全空转
 * - 组件只经 portal 渲染进标题栏槽位，不在应用内回退——树位置切换的
 *   重挂载会触发上述残留问题
 * - 错误边界隔离库的运行时异常，崩溃只损失页签条不炸窗口
 * - 库的"双击空白新建"与标题栏双击最大化冲突 → 捕获阶段禁用
 * - × 关闭走脏确认：捕获阶段拦截库的直接删除
 */

import React, { useEffect, useRef } from 'react';
import ChromeTabs from 'chrome-tabs';
import 'chrome-tabs/css/chrome-tabs.css';
import './ChromeTabStrip.css';

/** 未保存圆点（借用库的 favicon 槽位渲染，accent 色） */
const DIRTY_FAVICON =
  "data:image/svg+xml;utf8,<svg xmlns=%27http://www.w3.org/2000/svg%27 viewBox=%270 0 16 16%27><circle cx=%278%27 cy=%278%27 r=%273.2%27 fill=%27%2315aa70%27/></svg>";

export interface ChromeTabItem {
  id: string;
  label: string;
  dirty: boolean;
}

interface ChromeTabsInstance {
  el: HTMLElement;
  init(el: HTMLElement): void;
  layoutTabs(): void;
  addTab(
    props: { title?: string; favicon?: string | false; id?: string },
    opts?: { animate?: boolean; background?: boolean },
  ): void;
  removeTab(tabEl: HTMLElement): void;
  updateTab(
    tabEl: HTMLElement,
    props: { title?: string; favicon?: string | false; id?: string },
  ): void;
  setCurrentTab(tabEl: HTMLElement): void;
}

interface ChromeTabStripProps {
  tabs: ChromeTabItem[];
  activeTabId: string;

  /** 新建页签（+ 按钮） */
  onAdd(): void;

  onActivate(id: string): void;

  /** 页签关闭（库 × 与确认流程统一入口；脏页签由上层拦截确认） */
  onClose(id: string): void;

  /** 拖拽排序（库 originIndex → destinationIndex） */
  onReorder(from: number, to: number): void;
}

/**
 * 错误边界：隔离第三方库运行时异常，
 * 页签条崩溃时只隐藏自身，不炸整个应用窗口树
 */
class ChromeTabStripErrorBoundary extends React.Component<
  { children: React.ReactNode },
  { hasError: boolean }
> {
  state = { hasError: false };

  static getDerivedStateFromError() {
    return { hasError: true };
  }

  componentDidCatch(err: Error) {
    console.warn('[ChromeTabStrip] 崩溃已隔离:', err);
  }

  render() {
    return this.state.hasError ? null : this.props.children;
  }
}

/** 页签内容宽度上限（含标题区；整体宽度再加左右 9px 边距）——收窄默认页签 */
const TAB_CONTENT_MAX_WIDTH = 170;

/**
 * 放大版页签几何路径（viewBox 0 0 214 36）：
 * 库默认脚弧/顶角为 9/8px，视觉阶梯感明显；此处放大到 12px 并保持
 * 切线连续（脚弧起端水平、末端垂直衔接侧壁，顶角同理）
 */
const CHROME_TAB_GEOMETRY_D =
  'M22 0 H214 V36 H0 V34 C6 34 12 29.5 12 24 V10 C12 4.5 17.5 0 22 0 Z';

function ChromeTabStripImpl({
  tabs,
  activeTabId,
  onActivate,
  onClose,
  onReorder,
  onAdd,
}: ChromeTabStripProps) {
  const elRef = useRef<HTMLDivElement>(null);
  const instanceRef = useRef<ChromeTabsInstance | null>(null);
  const updateTabsEndRef = useRef<(() => void) | null>(null);
  /** 程序化同步期间屏蔽库事件，防止状态回环 */
  const suppressRef = useRef(false);
  const cbRef = useRef({ onActivate, onClose, onReorder, onAdd });
  cbRef.current = { onActivate, onClose, onReorder, onAdd };

  // 初始化库 + 事件桥接（仅挂载一次）
  useEffect(() => {
    const el = elRef.current;
    if (!el) return;

    const ct = new (ChromeTabs as unknown as { new (): ChromeTabsInstance })();
    ct.init(el);

    // 收窄默认页签：实例级遮蔽 tabContentWidths getter（宽度与位置都由此派生，
    // 收窄一处即全局一致）。在 init 之后定义（el 已就绪），并手动重排一次
    const proto = Object.getPrototypeOf(ct) as Record<string, unknown>;
    // ✅ 用 getOwnPropertyDescriptor 取 getter（直接访问属性会以原型为 this 触发它）
    const widthsGet = Object.getOwnPropertyDescriptor(proto, 'tabContentWidths')?.get?.bind(ct);
    if (widthsGet) {
      Object.defineProperty(ct, 'tabContentWidths', {
        get() {
          return widthsGet().map((w: number) => Math.min(w, TAB_CONTENT_MAX_WIDTH));
        },
      });
      ct.layoutTabs();
    }

    // 放大连接弧与顶部圆角（库默认 9/8px 偏小，阶梯感明显 → 12px 更圆润）。
    // 库的页签模板内嵌 <symbol id="chrome-tab-geometry-left">（右脚镜像复用左脚），
    // 同 id 符号以文档中首个为准——包装 createNewTabEl 让每个新页签都写入放大后的路径
    const patchGeometry = (root: HTMLElement) => {
      root.querySelectorAll('symbol[id="chrome-tab-geometry-left"] path').forEach((p) => {
        p.setAttribute('d', CHROME_TAB_GEOMETRY_D);
      });
    };
    const origCreate = proto.createNewTabEl as
      | ((this: ChromeTabsInstance) => HTMLElement)
      | undefined;
    if (origCreate) {
      proto.createNewTabEl = function (this: ChromeTabsInstance) {
        const tabEl = origCreate.call(this);
        patchGeometry(tabEl);
        return tabEl;
      } as unknown as typeof proto.createNewTabEl;
    }
    patchGeometry(el);

    // 维护 --tabs-end（最后一个页签右缘）：+ 按钮紧随其后
    const updateTabsEnd = () => {
      const content = el.querySelector('.chrome-tabs-content');
      const tabEls = [...(content?.querySelectorAll('.chrome-tab') ?? [])];
      const last = tabEls[tabEls.length - 1] as HTMLElement | undefined;
      if (!content || !last) return;
      const contentRect = content.getBoundingClientRect();
      const lastRect = last.getBoundingClientRect();
      el.style.setProperty('--tabs-end', `${Math.round(lastRect.right - contentRect.left)}px`);
    };
    updateTabsEndRef.current = updateTabsEnd;

    // 容器尺寸变化（窗口缩放）后库会重排页签，跟随更新
    const ro = new ResizeObserver(() => {
      requestAnimationFrame(() => updateTabsEndRef.current?.());
    });
    ro.observe(el);
    instanceRef.current = ct;

    const idOf = (target: EventTarget | null): string | null =>
      (target as HTMLElement | null)?.dataset?.tabId ?? null;

    const onActive = (e: Event) => {
      if (suppressRef.current) return;
      const id = idOf((e as CustomEvent).detail?.tabEl);
      if (id) cbRef.current.onActivate(id);
    };
    const onRemove = (e: Event) => {
      if (suppressRef.current) return;
      const id = idOf((e as CustomEvent).detail?.tabEl);
      // 库的 × 已直接删除 DOM；脏页签被确认取消时，同步 effect 会按状态重建
      if (id) cbRef.current.onClose(id);
    };
    const onReorderEv = (e: Event) => {
      if (suppressRef.current) return;
      const { originIndex, destinationIndex } = (e as CustomEvent).detail ?? {};
      if (typeof originIndex === 'number' && typeof destinationIndex === 'number') {
        cbRef.current.onReorder(originIndex, destinationIndex);
      }
    };
    // × 点击：捕获阶段拦截（脏确认），阻止库的直接删除
    const onClickCapture = (e: MouseEvent) => {
      const target = e.target as Element;
      if (target.closest?.('.chrome-tab-close')) {
        e.stopPropagation();
        e.preventDefault();
        const id = idOf(target.closest('.chrome-tab'));
        if (id) cbRef.current.onClose(id);
      }
    };
    // 双击空白新建与标题栏双击最大化冲突 → 禁用
    const onDblCapture = (e: MouseEvent) => e.stopImmediatePropagation();

    el.addEventListener('activeTabChange', onActive);
    el.addEventListener('tabRemove', onRemove);
    el.addEventListener('tabReorder', onReorderEv);
    el.addEventListener('click', onClickCapture, true);
    el.addEventListener('dblclick', onDblCapture, true);

    return () => {
      ro.disconnect();
      el.removeEventListener('activeTabChange', onActive);
      el.removeEventListener('tabRemove', onRemove);
      el.removeEventListener('tabReorder', onReorderEv);
      el.removeEventListener('click', onClickCapture, true);
      el.removeEventListener('dblclick', onDblCapture, true);
      // 库没有 destroy API：resize 监听器与延迟回调会永久存活。
      // 把实例重定向到带骨架的哑元素，让残留调用永远安全空转，
      // 避免打在已分离/已清空的 DOM 上抛错。
      const dummy = document.createElement('div');
      dummy.innerHTML = '<div class="chrome-tabs-content"></div>';
      (ct as unknown as { el: HTMLElement }).el = dummy;
      instanceRef.current = null;
    };
  }, []);

  // 状态 → 库 同步（最小 diff：删多余/改标题/保激活；顺序不符时整表重建）
  useEffect(() => {
    const ct = instanceRef.current;
    const el = elRef.current;
    if (!ct || !el) return;

    const domEls = () => [...el.querySelectorAll<HTMLElement>('.chrome-tab')];

    // 删除：DOM 有但状态没有
    domEls().forEach((tabEl) => {
      if (!tabs.some((t) => t.id === tabEl.dataset.tabId)) {
        suppressRef.current = true;
        ct.removeTab(tabEl);
        suppressRef.current = false;
      }
    });

    // 顺序校验：不符（新增/拖拽后的状态回流）→ 整表重建保序
    let els = domEls();
    const orderOk =
      tabs.every((t, i) => els[i]?.dataset.tabId === t.id) && els.length === tabs.length;
    if (!orderOk) {
      suppressRef.current = true;
      els.forEach((tabEl) => ct.removeTab(tabEl));
      tabs.forEach((t) => {
        ct.addTab(
          { title: t.label, favicon: t.dirty ? DIRTY_FAVICON : false, id: t.id },
          { animate: false, background: t.id !== activeTabId },
        );
      });
      const activeEl = el.querySelector<HTMLElement>(`[data-tab-id="${activeTabId}"]`);
      if (activeEl) ct.setCurrentTab(activeEl);
      suppressRef.current = false;
      return;
    }

    // 原位更新：标题 / 脏点 / 激活态
    suppressRef.current = true;
    tabs.forEach((t) => {
      const tabEl = el.querySelector<HTMLElement>(`[data-tab-id="${t.id}"]`);
      if (tabEl) {
        ct.updateTab(tabEl, {
          title: t.label,
          favicon: t.dirty ? DIRTY_FAVICON : false,
          id: t.id,
        });
      }
    });
    const activeEl = el.querySelector<HTMLElement>(`[data-tab-id="${activeTabId}"]`);
    if (activeEl && !activeEl.hasAttribute('active')) ct.setCurrentTab(activeEl);
    suppressRef.current = false;
    updateTabsEndRef.current?.();
  }, [tabs, activeTabId]);

  // 库要求的固定骨架：.chrome-tabs > .chrome-tabs-content；
  // + 按钮内嵌其中，绝对定位紧随最后一个页签（--tabs-end）
  return (
    <div ref={elRef} className="te-chrome-tabs chrome-tabs">
      <div className="chrome-tabs-content">
        <button
          type="button"
          className="te-tab-new"
          onClick={() => cbRef.current.onAdd()}
          title="新建页签"
          style={{ left: 'var(--tabs-end, 0px)' }}
        >
          +
        </button>
      </div>
    </div>
  );
}

export function ChromeTabStrip(props: ChromeTabStripProps) {
  return (
    <ChromeTabStripErrorBoundary>
      <ChromeTabStripImpl {...props} />
    </ChromeTabStripErrorBoundary>
  );
}
