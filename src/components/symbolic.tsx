// src/components/symbolic.tsx
// Adwaita Symbolic 风格内联 SVG 图标集
// 统一 16×16 viewBox；fill/stroke = currentColor，颜色由父元素 className/style 控制

import type { CSSProperties, ReactNode } from "react";

export type SymbolicIconName =
  | "dialog-error" | "dialog-warning" | "dialog-information"
  | "view-refresh" | "network-offline"
  | "mail-unread" | "mailbox"
  | "emblem-ok" | "user-trash" | "window-close"
  | "bell" | "bell-outline"
  | "pan-up" | "pan-down"
  | "funnel"
  | "chevron-up" | "chevron-down"
  | "go-previous" | "go-next" | "go-home" | "tab-new"
  | "external-link"
  | "system-search"
  | "emblem-system" | "view-private";

/** 图标内容（16×16 网格，几何近似 Adwaita symbolic 造型） */
const ICONS: Record<SymbolicIconName, ReactNode> = {
  // 圆环 + 叹号
  "dialog-error": (<>
    <circle cx="8" cy="8" r="6.25" fill="none" stroke="currentColor" strokeWidth="1.5" />
    <rect x="7.25" y="4.2" width="1.5" height="4.8" rx="0.75" fill="currentColor" />
    <rect x="7.25" y="10.4" width="1.5" height="1.5" rx="0.75" fill="currentColor" />
  </>),
  // 三角 + 叹号
  "dialog-warning": (<>
    <path d="M8 2.2 L14.3 13.3 a0.8 0.8 0 0 1 -0.7 1.2 H2.4 a0.8 0.8 0 0 1 -0.7 -1.2 Z"
      fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinejoin="round" />
    <rect x="7.25" y="5.8" width="1.5" height="3.6" rx="0.75" fill="currentColor" />
    <rect x="7.25" y="10.4" width="1.5" height="1.5" rx="0.75" fill="currentColor" />
  </>),
  // 圆环 + i
  "dialog-information": (<>
    <circle cx="8" cy="8" r="6.25" fill="none" stroke="currentColor" strokeWidth="1.5" />
    <rect x="7.25" y="4.2" width="1.5" height="1.5" rx="0.75" fill="currentColor" />
    <rect x="7.25" y="6.8" width="1.5" height="4.8" rx="0.75" fill="currentColor" />
  </>),
  // 环形箭头（重试/刷新）
  "view-refresh": (<>
    <path d="M13.2 8 a5.2 5.2 0 1 1 -1.5 -3.7" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
    <path d="M13.9 1.6 v3.6 h-3.6" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
  </>),
  // 显示器 + 斜杠（断开）
  "network-offline": (<>
    <rect x="1.8" y="2.2" width="12.4" height="8.4" rx="1.2" fill="none" stroke="currentColor" strokeWidth="1.5" />
    <path d="M5.5 13.4 h5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
    <path d="M3.4 3.4 L12.6 12.6" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
  </>),
  // 信封 + 未读点
  "mail-unread": (<>
    <rect x="1.5" y="4" width="13" height="9" rx="1.2" fill="none" stroke="currentColor" strokeWidth="1.5" />
    <path d="M2.2 4.8 L8 9 L13.8 4.8" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinejoin="round" />
    <circle cx="13.2" cy="2.8" r="1.8" fill="currentColor" />
  </>),
  // 信封
  "mailbox": (<>
    <rect x="1.5" y="3.5" width="13" height="9.5" rx="1.2" fill="none" stroke="currentColor" strokeWidth="1.5" />
    <path d="M2.2 4.3 L8 8.7 L13.8 4.3" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinejoin="round" />
  </>),
  // 对勾
  "emblem-ok": (<>
    <path d="M3 8.6 L6.5 12 L13 4.6" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
  </>),
  // 垃圾桶
  "user-trash": (<>
    <path d="M2.8 4.6 h10.4" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
    <path d="M6.6 2.4 h2.8 v1.4 h-2.8 Z" fill="currentColor" />
    <path d="M4 4.6 v8 a1.4 1.4 0 0 0 1.4 1.4 h5.2 a1.4 1.4 0 0 0 1.4 -1.4 v-8"
      fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinejoin="round" />
    <path d="M6.5 7 v4.5 M9.5 7 v4.5" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
  </>),
  // X
  "window-close": (<>
    <path d="M4 4 L12 12 M12 4 L4 12" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
  </>),
  // 铃铛（实底）
  "bell": (<>
    <path d="M8 1.6 c-2.4 0 -3.8 1.9 -3.8 4.3 v2.8 l-1.4 2.2 a0.6 0.6 0 0 0 0.5 0.9 h9.4 a0.6 0.6 0 0 0 0.5 -0.9 l-1.4 -2.2 v-2.8 c0 -2.4 -1.4 -4.3 -3.8 -4.3 Z" fill="currentColor" />
    <path d="M6.6 13.2 a1.4 1.4 0 0 0 2.8 0 Z" fill="currentColor" />
  </>),
  // 铃铛（描边）
  "bell-outline": (<>
    <path d="M8 2.1 c-2.1 0 -3.3 1.7 -3.3 3.9 v2.9 l-1.3 2 a0.6 0.6 0 0 0 0.5 0.9 h8.2 a0.6 0.6 0 0 0 0.5 -0.9 l-1.3 -2 v-2.9 c0 -2.2 -1.2 -3.9 -3.3 -3.9 Z"
      fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinejoin="round" />
    <path d="M6.7 13.2 a1.3 1.3 0 0 0 2.6 0" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
  </>),
  // 实心小三角朝上（排序指示：降序）
  "pan-up": (<>
    <path d="M8 4.4 L12.4 9.6 H3.6 Z" fill="currentColor" />
  </>),
  // 实心小三角朝下（排序指示：升序，桌面文件管理器惯例）
  "pan-down": (<>
    <path d="M8 11.6 L3.6 6.4 H12.4 Z" fill="currentColor" />
  </>),
  // 漏斗（列筛选激活指示，文件管理器表头）
  "funnel": (<>
    <path d="M2.2 2.8 H13.8 L9.4 8.2 V12.6 L6.6 13.8 V8.2 Z"
      fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinejoin="round" />
  </>),
  // 细线 chevron（排序方向指示/表头下拉按钮，Win11 Explorer 风格）
  "chevron-up": (<>
    <path d="M3.5 10 L8 5.5 L12.5 10"
      fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
  </>),
  "chevron-down": (<>
    <path d="M3.5 6 L8 10.5 L12.5 6"
      fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
  </>),
  // 实心左箭头（浏览后退）
  "go-previous": (
    <path d="M10.5 2.5 4 8l6.5 5.5V2.5Z" fill="currentColor" />
  ),
  // 实心右箭头（浏览前进）
  "go-next": (
    <path d="M5.5 2.5 12 8l-6.5 5.5V2.5Z" fill="currentColor" />
  ),
  // 小房子（返回首页；Adwaita go-home 简化造型：坡顶 + 房身 + 门）
  "go-home": (
    <>
      <path d="M2.6 8.3 8 3.2l5.4 5.1" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
      <path d="M4.1 7.6 V13 a0.7 0.7 0 0 0 0.7 0.7 h6.4 a0.7 0.7 0 0 0 0.7 -0.7 V7.6" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinejoin="round" />
      <path d="M6.7 13.7 V9.9 h2.6 v3.8" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinejoin="round" />
    </>
  ),
  // 新建标签页（窗口轮廓 + 加号）
  "tab-new": (
    <>
      <path d="M2 3.5C2 2.7 2.7 2 3.5 2h9c.8 0 1.5.7 1.5 1.5v9c0 .8-.7 1.5-1.5 1.5h-9C2.7 14 2 13.3 2 12.5v-9Z" fill="none" stroke="currentColor" strokeWidth="1.2" />
      <path d="M8 5v6M5 8h6" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
    </>
  ),
  // 齿轮（设置入口；中环 + 8 齿刻线的简化造型）
  "emblem-system": (<>
    <circle cx="8" cy="8" r="2.1" fill="none" stroke="currentColor" strokeWidth="1.5" />
    <path
      d="M8 1.6v2.1M8 12.3v2.1M1.6 8h2.1M12.3 8h2.1M3.5 3.5l1.5 1.5M11 11l1.5 1.5M12.5 3.5 11 5M5 11l-1.5 1.5"
      stroke="currentColor" strokeWidth="1.5" strokeLinecap="round"
    />
  </>),
  // 无痕浏览（帽檐 + 双圆眼镜；Adwaita view-private 简化造型）
  "view-private": (<>
    <path d="M3.2 6 C3.2 4.2 5.3 2.8 8 2.8 s4.8 1.4 4.8 3.2" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
    <path d="M2.4 7.2 h11.2" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
    <circle cx="5.1" cy="10.4" r="1.9" fill="none" stroke="currentColor" strokeWidth="1.4" />
    <circle cx="10.9" cy="10.4" r="1.9" fill="none" stroke="currentColor" strokeWidth="1.4" />
    <path d="M7 10.4 h2" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
  </>),
  // 外部打开（方框缺右上角 + 箭头跳出；Adwaita external-link 简化造型）
  "external-link": (<>
    <path
      d="M13 8.8 v2.9 a1.3 1.3 0 0 1 -1.3 1.3 H4.3 A1.3 1.3 0 0 1 3 11.7 V4.3 A1.3 1.3 0 0 1 4.3 3 H7.2"
      fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round"
    />
    <path d="M9.6 2.5 h3.9 V6.4" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
    <path d="M13.1 2.9 L7.9 8.1" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
  </>),
  // 搜索（放大镜；联想下拉模板项用；Adwaita magnifying-glass 简化造型）
  "system-search": (<>
    <circle cx="6.9" cy="6.9" r="3.9" fill="none" stroke="currentColor" strokeWidth="1.5" />
    <path d="M9.9 9.9 L13.5 13.5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
  </>),
};

export interface SymbolicIconProps {
  name: SymbolicIconName;
  size?: number;
  className?: string;
  style?: CSSProperties;
}

export function SymbolicIcon({ name, size = 16, className, style }: SymbolicIconProps) {
  return (
    <svg
      className={`symbolic-icon${className ? ` ${className}` : ""}`}
      width={size}
      height={size}
      viewBox="0 0 16 16"
      aria-hidden="true"
      style={style}
    >
      {ICONS[name] ?? ICONS["dialog-information"]}
    </svg>
  );
}
