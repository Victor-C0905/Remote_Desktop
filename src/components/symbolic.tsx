// src/components/symbolic.tsx
// Adwaita Symbolic 图标集 —— 填充剪影版
// 设计语言与 AppIcon/AppBadge 统一：物体类 = 实底剪影 + 镂空细节（evenodd），
// 方向指示类（chevron/pan/go-*）保留描边/实心三角（箭头天然是线条语言）。
// 统一 16×16 viewBox；fill/stroke = currentColor，颜色由父元素控制

import type { CSSProperties, ReactNode } from "react";

export type SymbolicIconName =
  | "dialog-error" | "dialog-warning" | "dialog-information"
  | "view-refresh" | "network-offline"
  | "mail-unread" | "mailbox"
  | "emblem-ok" | "clock" | "link" | "user-trash" | "window-close"
  | "window-minimize" | "window-maximize" | "window-restore"
  | "view-fullscreen" | "view-restore"
  | "bell" | "bell-outline"
  | "pan-up" | "pan-down"
  | "funnel"
  | "chevron-up" | "chevron-down"
  | "go-previous" | "go-next" | "go-home" | "tab-new"
  | "go-up" | "go-down"
  | "media-play" | "media-pause"
  | "folder-open"
  | "external-link"
  | "system-search"
  | "emblem-system" | "view-private";

/** 图标内容（16×16 网格；剪影用 fillRule=evenodd 镂空细节） */
const ICONS: Record<SymbolicIconName, ReactNode> = {
  // 实心圆环 + 镂空叹号
  "dialog-error": (
    <path
      fillRule="evenodd"
      d="M8 1.75a6.25 6.25 0 1 1 0 12.5 6.25 6.25 0 0 1 0-12.5ZM7.25 4.2h1.5v4.8h-1.5ZM7.25 10.4h1.5v1.5h-1.5Z"
      fill="currentColor"
    />
  ),
  // 实心三角 + 镂空叹号
  "dialog-warning": (
    <path
      fillRule="evenodd"
      d="M8 1.9 14.5 13.4a.95.95 0 0 1-.83 1.4H2.3a.95.95 0 0 1-.83-1.4ZM7.25 5.8h1.5v3.8h-1.5ZM7.25 10.9h1.5v1.6h-1.5Z"
      fill="currentColor"
    />
  ),
  // 实心圆环 + 镂空 i
  "dialog-information": (
    <path
      fillRule="evenodd"
      d="M8 1.75a6.25 6.25 0 1 1 0 12.5 6.25 6.25 0 0 1 0-12.5ZM7.25 4.2h1.5v1.6h-1.5ZM7.25 6.9h1.5v4.7h-1.5Z"
      fill="currentColor"
    />
  ),
  // 粗环箭头（重试/刷新）
  "view-refresh": (
    <>
      <path d="M13.4 8 a5.4 5.4 0 1 1 -1.6 -3.8" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" />
      <path d="M14.2 1.4 v4 h-4" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round" />
    </>
  ),
  // 实心显示器 + 镂空斜杠（断开）
  "network-offline": (
    <>
      <path
        fillRule="evenodd"
        d="M3 2.2h10a1.4 1.4 0 0 1 1.4 1.4v6.8a1.4 1.4 0 0 1-1.4 1.4H3a1.4 1.4 0 0 1-1.4-1.4V3.6A1.4 1.4 0 0 1 3 2.2ZM12.9 3.4 11.8 2.3 3.1 11 4.2 12.1Z"
        fill="currentColor"
      />
      <path d="M5.5 13.6h5" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" />
    </>
  ),
  // 实心信封 + 镂空盖 + 未读点
  "mail-unread": (
    <>
      <path
        fillRule="evenodd"
        d="M2.7 3.8h10.6a1.3 1.3 0 0 1 1.3 1.3v7.8a1.3 1.3 0 0 1-1.3 1.3H2.7a1.3 1.3 0 0 1-1.3-1.3V5.1a1.3 1.3 0 0 1 1.3-1.3ZM7 5.4 8 6.2l1-0.8h-2Z"
        fill="currentColor"
      />
      <circle cx="13.4" cy="2.8" r="1.9" fill="currentColor" />
    </>
  ),
  // 实心信封 + 镂空盖
  "mailbox": (
    <path
      fillRule="evenodd"
      d="M2.7 3.8h10.6a1.3 1.3 0 0 1 1.3 1.3v7.8a1.3 1.3 0 0 1-1.3 1.3H2.7a1.3 1.3 0 0 1-1.3-1.3V5.1a1.3 1.3 0 0 1 1.3-1.3ZM7 5.4 8 6.2l1-0.8h-2Z"
      fill="currentColor"
    />
  ),
  // 对勾（加粗）
  "emblem-ok": (
    <path d="M3 8.6 L6.5 12 L13 4.6" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round" />
  ),
  // 实心钟面 + 镂空指针（连接中/等待）
  "clock": (
    <path
      fillRule="evenodd"
      d="M8 1.9a6.1 6.1 0 1 1 0 12.2A6.1 6.1 0 0 1 8 1.9Zm-.7 2.4h1.4v4l3 1.8-.7 1.2-3.7-2.2z"
      fill="currentColor"
    />
  ),
  // 双链环（连接；动作类描边）
  "link": (
    <g fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
      <path d="M6.67 8.67a3.33 3.33 0 0 0 5.03.36l2-2a3.33 3.33 0 0 0-4.71-4.71l-1.15 1.14" />
      <path d="M9.33 7.33a3.33 3.33 0 0 0-5.03-.36l-2 2a3.33 3.33 0 0 0 4.71 4.71l1.14-1.14" />
    </g>
  ),
  // 实心垃圾桶 + 镂空竖条
  "user-trash": (
    <path
      fillRule="evenodd"
      d="M6.1 2.6a1 1 0 0 1 1-1h1.8a1 1 0 0 1 1 1v.8h3.3a.9.9 0 0 1 .9.9v.7a.9.9 0 0 1-.9.9H2.8a.9.9 0 0 1-.9-.9v-.7a.9.9 0 0 1 .9-.9h3.3ZM4.2 6.4h7.6v7a1.6 1.6 0 0 1-1.6 1.6H5.8a1.6 1.6 0 0 1-1.6-1.6ZM6.7 8h1.4v4.6H6.7ZM7.9 8h1.4v4.6H7.9Z"
      fill="currentColor"
    />
  ),
  // X（窗口关闭）
  "window-close": (
    <path d="M4 4 L12 12 M12 4 L4 12" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
  ),
  // 一横（窗口最小化；与 window-close 同线宽同端点）
  "window-minimize": (
    <path d="M3.5 8 H12.5" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
  ),
  // 圆角方框（窗口最大化）
  "window-maximize": (
    <rect x="3.5" y="3.5" width="9" height="9" rx="1.2" fill="none" stroke="currentColor" strokeWidth="2" />
  ),
  // 双框·后框右上段悬空（窗口还原；两线不相交保持单色干净）
  "window-restore": (
    <>
      <path d="M6.2 3.5 H10.6 A1.4 1.4 0 0 1 12 4.9 V9.3" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
      <rect x="3" y="5.5" width="7.5" height="7.5" rx="1.2" fill="none" stroke="currentColor" strokeWidth="2" />
    </>
  ),
  // 四角外扩 L（进入全屏）
  "view-fullscreen": (
    <path
      d="M6 3 H3 V6 M10 3 H13 V6 M13 10 V13 H10 M6 13 H3 V10"
      fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"
    />
  ),
  // 中心圆角框 + 四角收拢短线（退出全屏，与 view-fullscreen 成开合对偶）
  "view-restore": (
    <>
      <rect x="6.1" y="6.1" width="3.8" height="3.8" rx="0.9" fill="none" stroke="currentColor" strokeWidth="2" />
      <path
        d="M4.3 4.3 L2.7 2.7 M11.7 4.3 L13.3 2.7 M4.3 11.7 L2.7 13.3 M11.7 11.7 L13.3 13.3"
        stroke="currentColor" strokeWidth="2" strokeLinecap="round"
      />
    </>
  ),
  // 铃铛（实底剪影）
  "bell": (
    <>
      <path d="M8 1.6 c-2.4 0 -3.8 1.9 -3.8 4.3 v2.8 l-1.4 2.2 a0.6 0.6 0 0 0 0.5 0.9 h9.4 a0.6 0.6 0 0 0 0.5 -0.9 l-1.4 -2.2 v-2.8 c0 -2.4 -1.4 -4.3 -3.8 -4.3 Z" fill="currentColor" />
      <path d="M6.6 13.2 a1.4 1.4 0 0 0 2.8 0 Z" fill="currentColor" />
    </>
  ),
  // 铃铛（描边轮廓：无未读/安静态，与实底 bell 形成开合区分）
  "bell-outline": (
    <>
      <path
        d="M8 2.1C5.8 2.1 4.5 3.8 4.5 6.1v2.6L3.2 10.9a.55.55 0 0 0 .47.85h8.66a.55.55 0 0 0 .47-.85l-1.3-2.2V6.1c0-2.3-1.3-4-3.5-4Z"
        fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinejoin="round"
      />
      <path d="M6.6 13.3a1.4 1.4 0 0 0 2.8 0" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
    </>
  ),
  // 实心小三角朝上（排序指示：降序）
  "pan-up": (
    <path d="M8 4.4 L12.4 9.6 H3.6 Z" fill="currentColor" />
  ),
  // 实心小三角朝下（排序指示：升序，桌面文件管理器惯例）
  "pan-down": (
    <path d="M8 11.6 L3.6 6.4 H12.4 Z" fill="currentColor" />
  ),
  // 实心漏斗（列筛选激活指示，文件管理器表头）
  "funnel": (
    <path d="M2.2 2.8 H13.8 L9.4 8.2 V12.6 L6.6 13.8 V8.2 Z" fill="currentColor" strokeLinejoin="round" />
  ),
  // 细线 chevron（排序方向指示/表头下拉按钮，Win11 Explorer 风格）
  "chevron-up": (
    <path d="M3.5 10 L8 5.5 L12.5 10"
      fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
  ),
  "chevron-down": (
    <path d="M3.5 6 L8 10.5 L12.5 6"
      fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
  ),
  // 实心左箭头（浏览后退）
  "go-previous": (
    <path d="M10.5 2.5 4 8l6.5 5.5V2.5Z" fill="currentColor" />
  ),
  // 实心右箭头（浏览前进）
  "go-next": (
    <path d="M5.5 2.5 12 8l-6.5 5.5V2.5Z" fill="currentColor" />
  ),
  // 实心上箭头（传输上传方向，Adwaita go-up）
  "go-up": (
    <path d="M8 4l4 4H9.5v4h-3V8H4z" fill="currentColor" />
  ),
  // 实心下箭头（传输下载方向，Adwaita go-down）
  "go-down": (
    <path d="M8 12l-4-4h2.5V4h3v4H12z" fill="currentColor" />
  ),
  // 实心播放三角（继续传输）
  "media-play": (
    <path d="M5.5 4.2v7.6L12.4 8z" fill="currentColor" />
  ),
  // 实心双竖条（暂停传输）
  "media-pause": (
    <g fill="currentColor">
      <rect x="4.2" y="3.5" width="2.8" height="9" rx="0.7" />
      <rect x="9" y="3.5" width="2.8" height="9" rx="0.7" />
    </g>
  ),
  // 实心小房子（返回首页）
  "go-home": (
    <path d="M8 2.6 14.6 8.6 h-2 v4.8 a0.8 0.8 0 0 1 -0.8 0.8 H4.2 a0.8 0.8 0 0 1 -0.8 -0.8 V8.6 h-2 Z" fill="currentColor" />
  ),
  // 实心打开文件夹（打开文件；12 网格 Adwaita 路径等比放大至 16）
  "folder-open": (
    <path d="M1.3 2.7h5.4l1.3 1.3h5.3v1.4H2.7v6.6l1.3-4h10.7l-2 5.4H1.3z" fill="currentColor" />
  ),
  // 实心标签页 + 镂空加号
  "tab-new": (
    <path
      fillRule="evenodd"
      d="M3.4 2h9.2a1.6 1.6 0 0 1 1.6 1.6v8.8a1.6 1.6 0 0 1-1.6 1.6H3.4a1.6 1.6 0 0 1-1.6-1.6V3.6A1.6 1.6 0 0 1 3.4 2ZM7.3 4.6h1.4v2.7h2.7v1.4H8.7v2.7H7.3V8.7H4.6V7.3h2.7Z"
      fill="currentColor"
    />
  ),
  // 实心齿轮 + 镂空中孔 + 实心齿
  "emblem-system": (
    <g fill="currentColor">
      <path
        fillRule="evenodd"
        d="M8 2.8a5.2 5.2 0 1 1 0 10.4 5.2 5.2 0 0 1 0-10.4Zm0 3a2.2 2.2 0 1 0 0 4.4 2.2 2.2 0 0 0 0-4.4Z"
      />
      <rect x="7.25" y="0.9" width="1.5" height="2.1" rx="0.4" />
      <rect x="7.25" y="13" width="1.5" height="2.1" rx="0.4" />
      <rect x="0.9" y="7.25" width="2.1" height="1.5" rx="0.4" />
      <rect x="13" y="7.25" width="2.1" height="1.5" rx="0.4" />
      <rect x="7.25" y="0.9" width="1.5" height="2.1" rx="0.4" transform="rotate(45 8 8)" />
      <rect x="7.25" y="13" width="1.5" height="2.1" rx="0.4" transform="rotate(45 8 8)" />
      <rect x="0.9" y="7.25" width="2.1" height="1.5" rx="0.4" transform="rotate(45 8 8)" />
      <rect x="13" y="7.25" width="2.1" height="1.5" rx="0.4" transform="rotate(45 8 8)" />
    </g>
  ),
  // 无痕浏览（实心帽檐 + 实心双镜片）
  "view-private": (
    <g fill="currentColor">
      <path d="M2.4 7.4 C2.4 4.6 4.9 2.6 8 2.6 s5.6 2 5.6 4.8 a6.2 6.2 0 0 0 -11.2 0 Z" />
      <rect x="2" y="7" width="12" height="1.6" rx="0.8" />
      <circle cx="5.1" cy="11" r="2.1" />
      <circle cx="10.9" cy="11" r="2.1" />
      <rect x="6.9" y="10.3" width="2.2" height="1.3" />
    </g>
  ),
  // 外部打开（描边方框 + 实心箭头）
  "external-link": (
    <>
      <path
        d="M13 8.8 v2.9 a1.3 1.3 0 0 1 -1.3 1.3 H4.3 A1.3 1.3 0 0 1 3 11.7 V4.3 A1.3 1.3 0 0 1 4.3 3 H7.2"
        fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round"
      />
      <path d="M9.6 2.5 h3.9 V6.4" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
      <path d="M13.1 2.9 L7.9 8.1" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
    </>
  ),
  // 实心放大镜 + 粗柄（搜索）
  "system-search": (
    <>
      <path
        fillRule="evenodd"
        d="M6.9 2.5a4.4 4.4 0 1 1 0 8.8 4.4 4.4 0 0 1 0-8.8Zm0 1.5a2.9 2.9 0 1 0 0 5.8 2.9 2.9 0 0 0 0-5.8Z"
        fill="currentColor"
      />
      <path d="M10.2 10.2 13.5 13.5" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" />
    </>
  ),
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
