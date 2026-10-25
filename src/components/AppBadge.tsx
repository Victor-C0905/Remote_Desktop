/**
 * AppBadge —— 应用级图标徽章（桌面 / Dock / 概览）
 *
 * 两套可切换主题（设置 → 外观 → 图标主题，settingsStore.iconTheme）：
 * - classic「GNOME 经典」：彩色圆角徽章 + 白色图形 + 顶部高光/内描边
 *   （GNOME Circle 应用图标的正统做法）
 * - paper「纸张」：白卡徽章 + 深一档本色图形（放大 8%）+ 细边
 *   （与 UI 白卡层级同族，深浅壁纸上都干净）
 *
 * 文件类型 / 动作小图标不走这里——那层用 AppIcon（lucide 单色线），
 * 两层各司其职（GNOME 的 app icon 与 symbolic icon 也是两层体系）。
 */
import { useSettingsStore } from '../stores/settingsStore';

export type AppBadgeTheme = 'classic' | 'paper';

type BadgeAppId =
  | 'terminal' | 'files' | 'monitor' | 'editor' | 'settings' | 'browser'
  | 'image-viewer' | 'hex-viewer' | 'pdf-viewer' | 'archive-viewer';

/** 每应用配色：c = classic 徽章底色（--ic-* 令牌，随配色主题与亮暗换挡；
 *  此前为硬编码 hex，paper/neutral/dark 三套 UI 下桌面徽章恒同色，与内容图标脱节）。
 *  ic = paper 图形引用的 --ic-<色系> 变量。
 *  paper 的底/边/图形全部走主题令牌与 --ic-* 变量（随配色主题+亮暗联动），
 *  与内容图标同一色彩语言，任何背景下自动保证对比色差 */
const APP_COLORS: Record<BadgeAppId, { c: string; ic: string }> = {
  'terminal':       { c: 'var(--ic-gray-deep)',    ic: 'gray' },
  'files':          { c: 'var(--ic-blue-surface)', ic: 'blue' },
  'monitor':        { c: 'var(--ic-green-surface)', ic: 'green' },
  'editor':         { c: 'var(--ic-orange-surface)', ic: 'orange' },
  'settings':       { c: 'var(--ic-gray-surface)', ic: 'gray' },
  'browser':        { c: 'var(--ic-blue-deep)',    ic: 'blue' },
  'image-viewer':   { c: 'var(--ic-purple-surface)', ic: 'purple' },
  'hex-viewer':     { c: 'var(--ic-gray-surface)', ic: 'gray' },
  'pdf-viewer':     { c: 'var(--ic-red-surface)',  ic: 'red' },
  'archive-viewer': { c: 'var(--ic-brown-surface)', ic: 'brown' },
};

/** 图形（48 视图居中约 24×24），currentColor 取色 */
const GLYPHS: Record<BadgeAppId, string> = {
  'terminal': '<path d="M15 17l8 7-8 7" fill="none" stroke="currentColor" stroke-width="3.2" stroke-linecap="round" stroke-linejoin="round"/><path d="M27 32h8" stroke="currentColor" stroke-width="3.2" stroke-linecap="round"/>',
  'files': '<path d="M12 17.5h8.2l3 4H36a2.5 2.5 0 0 1 2.5 2.5v9A2.5 2.5 0 0 1 36 35.5H12A2.5 2.5 0 0 1 9.5 33V20A2.5 2.5 0 0 1 12 17.5z" fill="currentColor"/>',
  'monitor': '<path d="M10.5 26.5h6l4-9 5.5 13 4-8h8" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"/>',
  'editor': '<path d="M18.5 33.5l-1.8 5.3 5.3-1.8L34.6 24.6a2.6 2.6 0 0 0-3.7-3.7z" fill="currentColor"/>',
  'settings': '<circle cx="24" cy="24" r="5.2" fill="none" stroke="currentColor" stroke-width="3"/><g stroke="currentColor" stroke-width="3" stroke-linecap="round"><path d="M24 11.5v3.4"/><path d="M24 33.1v3.4"/><path d="M11.5 24h3.4"/><path d="M33.1 24h3.4"/><path d="M15.2 15.2l2.4 2.4"/><path d="M30.4 30.4l2.4 2.4"/><path d="M32.8 15.2l-2.4 2.4"/><path d="M17.6 30.4l-2.4 2.4"/></g>',
  'browser': '<circle cx="24" cy="24" r="11.5" fill="none" stroke="currentColor" stroke-width="2.8"/><path d="M12.5 24h23" stroke="currentColor" stroke-width="2.4"/><ellipse cx="24" cy="24" rx="5.2" ry="11.5" fill="none" stroke="currentColor" stroke-width="2.4"/>',
  'image-viewer': '<path d="M12 33l7.5-8.5 4.8 5 3.7-4.5L36 33z" fill="currentColor"/><circle cx="31.5" cy="17.5" r="3" fill="currentColor"/>',
  'hex-viewer': '<g stroke="currentColor" stroke-width="2.9" stroke-linecap="round"><path d="M18.5 15l-3 18"/><path d="M26.5 15l-3 18"/><path d="M15.5 21.5h16"/><path d="M13.5 27.5h16"/></g>',
  'pdf-viewer': '<path d="M16.5 11h9.5l7 7v18a2 2 0 0 1-2 2h-14.5a2 2 0 0 1-2-2V13a2 2 0 0 1 2-2z" fill="currentColor"/><g stroke="#ffffff" stroke-width="2" stroke-linecap="round" opacity="0.85"><path d="M19.5 25.5h9"/><path d="M19.5 30h6"/></g>',
  'archive-viewer': '<rect x="14" y="17" width="20" height="18" rx="2.5" fill="currentColor"/><path d="M14 23h20" stroke="#ffffff66" stroke-width="1.6"/><path d="M21 27h6v3.5h-6z" fill="#ffffff" opacity="0.9"/>',
};

export function appBadgeSvg(appId: string, theme: AppBadgeTheme): string {
  const id = (appId in APP_COLORS ? appId : 'files') as BadgeAppId;
  const colors = APP_COLORS[id];
  const glyph = GLYPHS[id];

  if (theme === 'paper') {
    // 白卡徽章（主题令牌派生）：底=主题卡色、边=主题边框色、图形=--ic 色系
    // ink 变量（随配色主题与亮暗联动，与内容图标同一色彩语言）
    const k = 1.08, off = (-24 * (k - 1)).toFixed(2);
    return (
      `<svg viewBox="0 0 48 48" xmlns="http://www.w3.org/2000/svg">` +
      `<rect x="2" y="2" width="44" height="44" rx="13" fill="var(--ic-${colors.ic}-surface)"/>` +
      `<rect x="2" y="2" width="44" height="44" rx="13" fill="none" stroke="var(--ic-${colors.ic}-deep)" stroke-width="2"/>` +
      `<g color="var(--ic-${colors.ic}-ink)" transform="translate(${off} ${off}) scale(${k})">${glyph}</g>` +
      `</svg>`
    );
  }

  // classic：彩色徽章 + 顶部高光 + 内描边 + 白图形
  return (
    `<svg viewBox="0 0 48 48" xmlns="http://www.w3.org/2000/svg">` +
    `<rect x="2" y="2" width="44" height="44" rx="13" fill="${colors.c}"/>` +
    `<path d="M4 26 Q24 8 44 26 L44 33 Q24 42 4 33 Z" fill="#ffffff" opacity="0.07"/>` +
    `<path d="M4 14 Q24 4 44 14 L44 20 Q24 10 4 20 Z" fill="#ffffff" opacity="0.12"/>` +
    `<rect x="2" y="2" width="44" height="44" rx="13" fill="none" stroke="#ffffff" stroke-opacity="0.28" stroke-width="1.4"/>` +
    `<g color="#ffffff">${glyph}</g>` +
    `</svg>`
  );
}

/** React 组件：订阅设置中的图标主题，切主题全站即时生效。
 *  theme 传入时强制覆盖（设置预览卡各自固定展示自己的方案用）。 */
export function AppBadge({ app, size = 44, theme }: { app: string; size?: number; theme?: AppBadgeTheme }) {
  const storeTheme = useSettingsStore((s) => s.iconTheme);
  const iconTheme = theme ?? storeTheme;
  const svg = appBadgeSvg(app, iconTheme);
  return (
    <div
      style={{ width: size, height: size }}
      dangerouslySetInnerHTML={{ __html: svg }}
      aria-hidden="true"
    />
  );
}
