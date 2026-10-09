import React from "react";
import styles from "./AppLayout.module.css";

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
  sidebarWidth = 240,
  sidebarCollapsible = false,
  toolbar,
  children,
}: AppLayoutProps) {
  return (
    <div className={styles.appLayout} style={{ '--app-sidebar-width': `${sidebarWidth}px` } as React.CSSProperties}>
      {/* Sidebar（可选） */}
      {sidebar && (
        <div
          className={styles.appSidebar}
          data-collapsible={sidebarCollapsible}
        >
          {sidebar}
        </div>
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