/**
 * AppShell - 应用层抽象组件导出
 *
 * 提供应用层的通用UI组件，用于构建FileManager、Settings等应用。
 */

export { AppLayout } from "./AppLayout";
export type { AppLayoutProps } from "./AppLayout";

export { AppTopbar } from "./AppTopbar";
export type { AppTopbarProps } from "./AppTopbar";

// 未来将添加更多抽象组件：
// export { SidebarItem } from "./SidebarItem";
// export { SidebarSection } from "./SidebarSection";