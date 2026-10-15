import React from "react";
import { MenuBar, type MenuBarMenu } from "../ToolbarMenu/ToolbarMenu";
import styles from "./AppTopbar.module.css";

/**
 * AppTopbar - 应用内顶栏（菜单栏 + 上下文槽位）
 *
 * 统一各应用窗口的顶栏形态：左侧传统菜单栏（文件/编辑/视图…），
 * 右侧为应用自己的上下文内容（路径面包屑、视图页签等），中间以
 * 分隔竖线区隔。外观对齐 TextEditor 顶栏（30px 浅色条 + 底缝），
 * 固定在内容区上方，不随内容滚动。
 *
 * 布局结构：
 * ┌──────────────────────────────────────────┐
 * │ 文件 编辑 视图 │ 应用上下文内容（可选）      │
 * └──────────────────────────────────────────┘
 * │ 应用主内容区（独立滚动）                    │
 *
 * @example 基本用法（菜单栏 + 面包屑，见 TextEditor）
 * ```tsx
 * <AppTopbar menus={[{ label: '文件', items: [{ label: '保存', onClick: save }] }]}>
 *   <div className="my-breadcrumb">/home/user/file.txt</div>
 * </AppTopbar>
 * ```
 *
 * @example 仅菜单栏（children 省略时不渲染分隔竖线）
 * ```tsx
 * <AppTopbar menus={menus} />
 * ```
 *
 * 配合 AppLayout 使用时作为 toolbar 传入；无侧边栏的应用可直接
 * 置于自身 flex column 容器顶部（见 SystemMonitor）。
 */
export interface AppTopbarProps {
  /**
   * 菜单栏内容（文件/编辑/视图…），交给共享 MenuBar 渲染
   * 可选：不传或空数组时不渲染菜单栏与分隔竖线（顶栏仅剩上下文内容）
   */
  menus?: MenuBarMenu[];

  /**
   * 菜单栏右侧的上下文内容（可选）
   * 面包屑、视图页签等应用专属内容；提供且菜单栏存在时自动插入分隔竖线
   */
  children?: React.ReactNode;
}

export function AppTopbar({ menus, children }: AppTopbarProps) {
  const hasChildren = children !== undefined && children !== null && children !== false;
  const hasMenus = !!menus && menus.length > 0;

  return (
    <div className={styles.topbar}>
      {hasMenus && <MenuBar menus={menus} />}
      {hasMenus && hasChildren && <div className={styles.divider} aria-hidden="true" />}
      {hasChildren && children}
    </div>
  );
}
