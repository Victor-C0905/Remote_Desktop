// src/components/symbolic.tsx
// Adwaita Symbolic 风格内联 SVG 图标集
// 统一 16×16 viewBox；fill/stroke = currentColor，颜色由父元素 className/style 控制

import type { CSSProperties, ReactNode } from "react";

export type SymbolicIconName =
  | "dialog-error" | "dialog-warning" | "dialog-information"
  | "view-refresh" | "network-offline"
  | "mail-unread" | "mailbox"
  | "emblem-ok" | "user-trash" | "window-close"
  | "bell" | "bell-outline";

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
