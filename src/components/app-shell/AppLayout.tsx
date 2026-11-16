import React, { useRef, useState } from "react";
import styles from "./AppLayout.module.css";
import { SIDEBAR_DEFAULT_WIDTH, clampSidebarWidth } from "./sidebarWidth";

/**
 * AppLayout - 应用布局组件
 *
 * 提供标准的 Sidebar + Main 布局，用于 FileManager、Settings 等应用。
 *
 * 布局结构：
 * ┌─────────────────────────────────────┐
 * │ Sidebar │ Toolbar（可选）            │
 * │         ├─────────────────────────────│
 * │         │ Main Content               │
 * └─────────────────────────────────────┘
 *
 * 特性：
 * - Sidebar 可配置宽度（默认 240px）
 * - Sidebar 可选拖拽调宽（sidebarResizable，R12-FM-1：分隔柄 + 夹取，
 *   双击重置；FM 场景对齐 Windows 资源管理器的侧栏行为）
 * - Main 区域包含可选 Toolbar 和 Content
 * - 自动处理 CSS flex 约束链，确保滚动正常工作
 * - 所有样式使用 CSS 变量，自动跟随主题设置
 */

export interface AppLayoutProps {
  /**
   * Sidebar 内容（可选）
   * 如果不提供，则不显示 Sidebar
   */
  sidebar?: React.ReactNode;

  /**
   * Sidebar 宽度（像素）
   * 默认：240px
   */
  sidebarWidth?: number;

  /**
   * Sidebar 是否可折叠
   * 默认：false
   * （当前版本未实现折叠功能，预留接口）
   */
  sidebarCollapsible?: boolean;

  /**
   * 启用侧栏拖拽调宽（在侧栏与主区之间渲染分隔柄）
   * 默认：false
   */
  sidebarResizable?: boolean;

  /**
   * 拖拽回报的夹取后宽度（sidebarResizable 时必配）
   */
  onSidebarWidthChange?: (width: number) => void;

  /**
   * 双击分隔柄时的重置回调（可选）
   */
  onSidebarReset?: () => void;

  /**
   * Toolbar 内容（可选）
   * 如果提供，插入在 Main 区域顶部
   */
  toolbar?: React.ReactNode;

  /**
   * Main 区域内容（必选）
   * 这是应用的主要内容区域
   */
  children: React.ReactNode;
}

/**
 * AppLayout 组件实现
 */
export function AppLayout({
  sidebar,
  sidebarWidth = SIDEBAR_DEFAULT_WIDTH,
  sidebarCollapsible = false,
  sidebarResizable = false,
  onSidebarWidthChange,
  onSidebarReset,
  toolbar,
  children,
}: AppLayoutProps) {
  const layoutRef = useRef<HTMLDivElement | null>(null);
  const [resizing, setResizing] = useState(false);

  // 拖拽期间的全局 mousemove/mouseup（移出分隔柄仍连续跟手；滑过 iframe/
  // 文本时不会丢事件）。clientX − 布局左缘 = 请求宽，夹取后回报。
  const startResize = (e: React.MouseEvent) => {
    if (!onSidebarWidthChange) return;
    e.preventDefault();
    const left = layoutRef.current?.getBoundingClientRect().left ?? 0;
    const onMove = (ev: MouseEvent) => {
      onSidebarWidthChange(clampSidebarWidth(ev.clientX - left));
    };
    const onUp = () => {
      setResizing(false);
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
    };
    setResizing(true);
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
  };

  return (
    <div ref={layoutRef} className={styles.appLayout} style={{ '--app-sidebar-width': `${sidebarWidth}px` } as React.CSSProperties}>
      {/* Sidebar（可选） */}
      {sidebar && (
        <div
          className={styles.appSidebar}
          data-collapsible={sidebarCollapsible}
        >
          {sidebar}
        </div>
      )}

      {/* 拖拽分隔柄（opt-in）：竖向细条，悬停/拖拽高亮，双击重置 */}
      {sidebar && sidebarResizable && onSidebarWidthChange && (
        <div
          className={`${styles.appSidebarResizer}${resizing ? ` ${styles.appSidebarResizerActive}` : ""}`}
          role="separator"
          aria-orientation="vertical"
          onMouseDown={startResize}
          onDoubleClick={onSidebarReset}
          data-resizing={resizing || undefined}
        />
      )}

      {/* Main 区域 */}
      <div className={styles.appMain}>
        {/* Toolbar（可选） */}
        {toolbar && <div className={styles.appToolbarContainer}>{toolbar}</div>}

        {/* Content（必选） */}
        <div className={styles.appContent}>{children}</div>
      </div>
    </div>
  );
}
