/**
 * AppIcon —— 全站统一内容图标组件（单一图标真相源）
 *
 * 设计约定（AGENTS 补充：图标语言）：
 * - 双档构建（settingsStore.iconTheme 驱动，getIcons 按档缓存）：
 *   classic「GNOME 经典」：彩色渐变磁贴 + 白色图形 + 体积描边 + 内描边高光
 *   paper  「纸墨」：平涂淡彩磁贴 + 本色实边 + 深墨图形（暗色 UI 反转为暗卡浅墨）；
 *           文件类型为着色纸形直接落地（无磁贴），文件夹为双色调全出血轮廓
 * - 色彩全部引用 --ic-* 变量 + 渐变 defs，data-icon-theme / data-icon-scheme
 *   双维联动（variables.css）
 * - 语义 key 对外暴露，底层图形可整体替换
 * - 24 视图等比缩放；14px（菜单）~ 48px（空状态）通用
 */
import type { AppIconName } from './iconNames';
import { useSettingsStore } from '../stores/settingsStore';

export type { AppIconName } from './iconNames';
export type IconThemeKind = 'classic' | 'paper';

/* 渐变 defs：每色两档 + 纸底渐变；stop 引 CSS 变量随主题换装。
   folder/paper 供全出血文件夹前板与白纸形使用——曾长期被引用却未定义，
   按 SVG 规范按 fill:none 渲染（文件夹只剩页签、纸身透明），务必与引用成对维护。
   stop 必须用 stop-color 属性而非 style="…"：release 的 CSP 会被 Tauri 追加
   nonce/hash，'unsafe-inline' 随之失效，style 属性整条被拦 → stop-color 回落黑色 */
const G = (id: string, a: string, b: string) =>
  `<linearGradient id="${id}" x1="0" y1="0" x2="0" y2="1">` +
  `<stop offset="0" stop-color="var(${a})"/><stop offset="1" stop-color="var(${b})"/></linearGradient>`;
const DEFS =
  '<defs>' +
  ['blue', 'green', 'orange', 'purple', 'red', 'yellow', 'brown', 'gray', 'folder', 'paper']
    .map((c) => G(`icg-${c}`, `--ic-${c}-light`, `--ic-${c}-surface`) + G(`icg-${c}d`, `--ic-${c}-surface`, `--ic-${c}-deep`))
    .join('') +
  '</defs>';

/* 徽章底：classic = 渐变方块 + 体积描边 + 内描边高光（与 AppBadge classic 同构）；
   paper = 平涂淡彩方块 + 本色实边（纸墨语言：无渐变、无高光，淡彩由
   --ic-<c>-surface 的 color-mix 派生，自动跟随三套 UI 主题） */
const B = (c: string, t: IconThemeKind) =>
  t === 'classic'
    ? `<rect x="1.5" y="1.5" width="21" height="21" rx="7" fill="url(#icg-${c})" stroke="var(--ic-${c}-deep)" stroke-width="1.2" stroke-opacity=".9"/>` +
      `<rect x="2.4" y="2.4" width="19.2" height="19.2" rx="6.1" fill="none" stroke="var(--ic-hi)" stroke-width="1"/>`
    : `<rect x="1.5" y="1.5" width="21" height="21" rx="7" fill="var(--ic-${c}-surface)" stroke="var(--ic-${c}-deep)" stroke-width="1.3"/>`;

/* 白纸形（文件类型图形底）：白纸 + 折角 + 投影 + 实色描边，
   在任何颜色的磁贴上都清晰可读（此前纸形与磁贴同渐变导致内容隐形） */
const SHEET =
  '<path d="M9.2 5.6h4.2l2.8 2.8v9.2a1.2 1.2 0 0 1-1.2 1.2H9.2a1.2 1.2 0 0 1-1.2-1.2z" fill="rgba(0,0,0,0.18)"/>' +
  '<path d="M8.6 5h4.6l3 3v9.4a1.3 1.3 0 0 1-1.3 1.3H8.6a1.3 1.3 0 0 1-1.3-1.3V6.3A1.3 1.3 0 0 1 8.6 5z" fill="url(#icg-paper)" stroke="var(--ic-paper-edge)"/>' +
  '<path d="M12.9 5.1 16 8.2V8h-2.6a.5.5 0 0 1-.5-.5z" fill="var(--ic-paper-fold)"/>';

/* 按类型着色的纸形：浅色底（--ic-<c>-light）+ 同色深图形（--ic-<c>-solid）。
   修复"所有文件类型都是白纸"的辨识度问题——每个类型一个色相，16px 一眼可辨 */
const sheetTinted = (c: string) =>
  '<path d="M9.2 5.6h4.2l2.8 2.8v9.2a1.2 1.2 0 0 1-1.2 1.2H9.2a1.2 1.2 0 0 1-1.2-1.2z" fill="rgba(0,0,0,0.18)"/>' +
  '<path d="M8.6 5h4.6l3 3v9.4a1.3 1.3 0 0 1-1.3 1.3H8.6a1.3 1.3 0 0 1-1.3-1.3V6.3A1.3 1.3 0 0 1 8.6 5z" fill="var(--ic-' + c + '-light)" stroke="var(--ic-' + c + '-solid)" stroke-opacity=".55"/>' +
  '<path d="M12.9 5.1 16 8.2V8h-2.6a.5.5 0 0 1-.5-.5z" fill="var(--ic-' + c + '-solid)" opacity=".45"/>';

const makeIcons = (t: IconThemeKind): Record<AppIconName, string> => ({
  /* ── 应用 ── */
  terminal: B('gray', t) +
    '<path d="M7.2 9.4 10.4 12l-3.2 2.6" stroke="var(--ic-gray-ink)" stroke-width="1.9" fill="none" stroke-linecap="round" stroke-linejoin="round"/>' +
    '<path d="M12.4 15h4.6" stroke="var(--ic-green-cursor)" stroke-width="2" stroke-linecap="round"/>',
  files: B('blue', t) +
    '<path d="M7 9.2a1.2 1.2 0 0 1 1.2-1.2h2.6l1.4 1.6h3.6A1.2 1.2 0 0 1 17 10.8v4.6a1.2 1.2 0 0 1-1.2 1.2H8.2A1.2 1.2 0 0 1 7 15.4z" fill="var(--ic-blue-ink)"/>',
  monitor: B('green', t) +
    '<path d="M6.4 14.2h2.9l1.7-4.4 2.7 7.4 1.5-4.2h2.7" stroke="var(--ic-green-ink)" stroke-width="1.9" fill="none" stroke-linecap="round" stroke-linejoin="round"/>',
  editor: B('orange', t) +
    '<path d="M13.6 7.9l2.7 2.7-5.1 5.1-3.3 1 1-3.3z" fill="var(--ic-orange-ink)"/>' +
    '<path d="M15.1 6.4l1.4-1.4a1.1 1.1 0 0 1 1.6 1.6l-1.4 1.4z" fill="var(--ic-orange-ink)" opacity=".8"/>',
  settings: B('gray', t) +
    '<g transform="translate(12 12)"><circle r="2.7" fill="none" stroke="var(--ic-gray-ink)" stroke-width="1.7"/>' +
    '<g stroke="var(--ic-gray-ink)" stroke-width="1.7" stroke-linecap="round">' +
    '<path d="M0-5.6v-1.7"/><path d="M0 5.6v1.7"/><path d="M-5.6 0h-1.7"/><path d="M5.6 0h1.7"/>' +
    '<path d="M-4-4l-1.2-1.2"/><path d="M4 4l1.2 1.2"/><path d="M4-4l1.2-1.2"/><path d="M-4 4l-1.2-1.2"/></g></g>',
  browser: B('blue', t) +
    '<circle cx="12" cy="12" r="5.6" fill="none" stroke="var(--ic-blue-ink)" stroke-width="1.4"/>' +
    '<path d="M6.4 12h11.2M12 6.4c-3.4 3.4-3.4 7.8 0 11.2 3.4-3.4 3.4-7.8 0-11.2z" stroke="var(--ic-blue-ink)" stroke-width="1.1" fill="none"/>',
  image: B('purple', t) +
    '<path d="M7.2 16l2.9-3.6 2.2 2.4 1.5-1.9 2.6 3.1z" fill="var(--ic-purple-ink)"/>' +
    '<circle cx="14.8" cy="9" r="1.5" fill="var(--ic-purple-ink)"/>',
  hex: B('gray', t) +
    '<g stroke="var(--ic-gray-ink)" stroke-width="1.6" stroke-linecap="round">' +
    '<path d="M10 7.5l-1.5 9"/><path d="M14 7.5l-1.5 9"/><path d="M8.5 10.75h8"/><path d="M7.5 13.75h8"/></g>',
  pdf: t === 'classic'
    ? B('red', t) + SHEET +
      '<rect x="8.6" y="11.4" width="6.8" height="1.4" rx=".7" fill="var(--ic-red-solid)"/>' +
      '<rect x="8.6" y="14" width="4.6" height="1.4" rx=".7" fill="var(--ic-red-solid)" opacity=".6"/>'
    : sheetTinted('red') +
      '<rect x="8.6" y="11.4" width="6.8" height="1.4" rx=".7" fill="var(--ic-red-ink)"/>' +
      '<rect x="8.6" y="14" width="4.6" height="1.4" rx=".7" fill="var(--ic-red-ink)" opacity=".6"/>',
  archive: B('brown', t) +
    '<rect x="7.2" y="9" width="9.6" height="8" rx="1.3" fill="var(--ic-brown-ink)"/>' +
    '<path d="M7.2 11.6h9.6" stroke="var(--ic-brown-surface)" stroke-width="1" opacity=".7"/>' +
    '<rect x="10.4" y="13.2" width="3.2" height="2.2" rx=".6" fill="var(--ic-brown-surface)" opacity=".7"/>',
  package: B('brown', t) +
    '<rect x="7" y="8.4" width="10" height="8.6" rx="1.3" fill="var(--ic-brown-ink)"/>' +
    '<path d="M12 8.4v8.6M7 11h10" stroke="var(--ic-brown-surface)" stroke-width="1" opacity=".7"/>',

  /* ── 设置区块 ── */
  connection: B('blue', t) +
    '<rect x="5.4" y="9.9" width="8.4" height="4.6" rx="2.3" fill="none" stroke="var(--ic-blue-ink)" stroke-width="1.7"/>' +
    '<rect x="10.2" y="9.9" width="8.4" height="4.6" rx="2.3" fill="none" stroke="var(--ic-blue-ink)" stroke-width="1.7"/>',
  appearance: B('purple', t) +
    '<path d="M12 6.4a5.6 5.6 0 0 0 0 11.2c1 0 1.4-.7 1-1.4-.5-.9.1-1.7 1.1-1.7h1.2a2.4 2.4 0 0 0 2.3-2.5A5.6 5.6 0 0 0 12 6.4z" fill="var(--ic-purple-ink)"/>' +
    '<g fill="var(--ic-purple-surface)"><circle cx="9.6" cy="10.6" r="1"/><circle cx="12.4" cy="9" r="1"/><circle cx="14.8" cy="11.2" r="1"/></g>',
  keyboard: B('gray', t) +
    '<rect x="6" y="8.6" width="12" height="7" rx="1.3" fill="var(--ic-gray-ink)"/>' +
    '<g fill="var(--ic-gray-surface)"><rect x="7.4" y="10" width="1.5" height="1.3" rx=".4"/><rect x="9.9" y="10" width="1.5" height="1.3" rx=".4"/><rect x="12.4" y="10" width="1.5" height="1.3" rx=".4"/><rect x="14.9" y="10" width="1.5" height="1.3" rx=".4"/><rect x="8.6" y="12.4" width="6.8" height="1.3" rx=".4"/></g>',
  notifications: B('blue', t) +
    '<path d="M12 6.6c-2.2 0-3.6 1.7-3.6 3.9v2.7L6.9 14.6a.8.8 0 0 0 .6 1.2h9a.8.8 0 0 0 .6-1.2l-1.5-1.4v-2.7c0-2.2-1.4-3.9-3.6-3.9z" fill="var(--ic-blue-ink)"/>' +
    '<path d="M10.5 16.4a1.5 1.5 0 0 0 3 0z" fill="var(--ic-blue-ink)"/>',
  stats: B('green', t) +
    '<rect x="7.4" y="12.4" width="2.4" height="4.6" rx=".9" fill="var(--ic-green-ink)"/>' +
    '<rect x="10.8" y="9.8" width="2.4" height="7.2" rx=".9" fill="var(--ic-green-ink)"/>' +
    '<rect x="14.2" y="7.4" width="2.4" height="9.6" rx=".9" fill="var(--ic-green-ink)"/>',
  about: B('blue', t) +
    '<rect x="11.2" y="8.4" width="1.6" height="4.6" rx=".8" fill="var(--ic-blue-ink)"/>' +
    '<circle cx="12" cy="15.2" r="1" fill="var(--ic-blue-ink)"/>',

  /* ── 侧栏书签 ── */
  documents: B('gray', t) +
    '<path d="M8.2 6h3.4l2.4 2.4v7.4a1.2 1.2 0 0 1-1.2 1.2H8.2A1.2 1.2 0 0 1 7 15.8V7.2A1.2 1.2 0 0 1 8.2 6z" fill="var(--ic-gray-ink)"/>' +
    '<g stroke="var(--ic-gray-surface)" stroke-width="1.2" stroke-linecap="round"><path d="M9.2 10.4h3.6"/><path d="M9.2 12.6h3.6"/><path d="M9.2 14.8h2.2"/></g>',
  pictures: B('purple', t) +
    '<path d="M7.2 16l2.9-3.6 2.2 2.4 1.5-1.9 2.6 3.1z" fill="var(--ic-purple-ink)"/>' +
    '<circle cx="14.8" cy="9" r="1.5" fill="var(--ic-purple-ink)"/>',
  music: B('purple', t) +
    '<circle cx="9.4" cy="15.4" r="1.9" fill="var(--ic-purple-ink)"/>' +
    '<rect x="11" y="8.4" width="1.4" height="7.2" fill="var(--ic-purple-ink)"/>' +
    '<circle cx="14.9" cy="14.2" r="1.9" fill="var(--ic-purple-ink)"/>' +
    '<rect x="15.6" y="7.4" width="1.4" height="7" fill="var(--ic-purple-ink)"/>' +
    '<path d="M12.4 8.4l4.6 1.2v1.7l-4.6-1.2z" fill="var(--ic-purple-ink)"/>',
  videos: B('orange', t) +
    '<path d="M9.4 8.8 16 12l-6.6 3.2z" fill="var(--ic-orange-ink)"/>',
  network: B('blue', t) +
    '<circle cx="12" cy="12" r="5.6" fill="none" stroke="var(--ic-blue-ink)" stroke-width="1.4"/>' +
    '<path d="M6.4 12h11.2M12 6.4c-3.4 3.4-3.4 7.8 0 11.2 3.4-3.4 3.4-7.8 0-11.2z" stroke="var(--ic-blue-ink)" stroke-width="1.1" fill="none"/>',
  home: B('orange', t) +
    '<path d="M12 6.4 18.8 12l-1 1.2L12 8.1l-5.8 5.1-1-1.2z" fill="var(--ic-orange-ink)"/>' +
    '<path d="M8.6 12.6h6.8v3.6a1 1 0 0 1-1 1H9.6a1 1 0 0 1-1-1z" fill="var(--ic-orange-ink)"/>' +
    '<rect x="11" y="13.9" width="2" height="3.3" rx=".5" fill="var(--ic-orange-surface)"/>',
  download: B('green', t) +
    '<path d="M10.9 7h2.2v5l2.2-2.1 1.4 1.4-4.7 4.4-4.7-4.4L8.7 9.9l2.2 2.1z" fill="var(--ic-green-ink)"/>',
  disk: B('gray', t) +
    '<rect x="6.6" y="9" width="10.8" height="6.6" rx="1.3" fill="var(--ic-gray-ink)"/>' +
    '<rect x="8" y="10.4" width="8" height="1.6" rx=".6" fill="var(--ic-gray-surface)"/>' +
    '<circle cx="14.8" cy="13.8" r="1" fill="var(--ic-gray-surface)"/>',

  /* ── 文件类型 ── */
  // 文件夹：classic = 全彩经典轮廓（页签+薄搁板+大圆角前板，icg-folder 渐变前板）；
  // paper = 纸墨双色调（深墨页签 + 淡彩前板 + 墨色勾边），folder 色板在
  // variables.css 两组 paper 覆盖里派生为 tint。三变体均撑满 24 画布。
  folder: t === 'classic'
    ? '<path d="M2.6 8.6V5.7a1.4 1.4 0 0 1 1.4-1.4h4.2a1.3 1.3 0 0 1 1 .5l1.6 1.9h9.1a1.4 1.4 0 0 1 1.4 1.4v.5z" fill="var(--ic-folder-deep)"/>' +
      '<path d="M1.9 8.4h20.2v9.8a1.7 1.7 0 0 1-1.7 1.7H3.6a1.7 1.7 0 0 1-1.7-1.7z" fill="url(#icg-folder)"/>' +
      '<path d="M1.9 8.4h20.2" stroke="var(--ic-hi)" stroke-width="1"/>' +
      '<path d="M1.9 18.2a1.7 1.7 0 0 0 1.7 1.7h16.8" stroke="var(--ic-folder-deep)" stroke-opacity=".4" stroke-width="1" fill="none"/>'
    : '<path d="M2.6 8.6V5.7a1.4 1.4 0 0 1 1.4-1.4h4.2a1.3 1.3 0 0 1 1 .5l1.6 1.9h9.1a1.4 1.4 0 0 1 1.4 1.4v.5z" fill="var(--ic-folder-deep)"/>' +
      '<path d="M1.9 8.4h20.2v9.8a1.7 1.7 0 0 1-1.7 1.7H3.6a1.7 1.7 0 0 1-1.7-1.7z" fill="var(--ic-folder-surface)" stroke="var(--ic-folder-deep)" stroke-width="1.4"/>',
  'folder-open': t === 'classic'
    ? '<path d="M2.6 8.6V5.7a1.4 1.4 0 0 1 1.4-1.4h4.2a1.3 1.3 0 0 1 1 .5l1.6 1.9h9.1a1.4 1.4 0 0 1 1.4 1.4v1.1z" fill="var(--ic-folder-deep)"/>' +
      '<path d="M1.9 8.4h20.2v2.6L13.9 9.9l-2-1.5H3.6a1.7 1.7 0 0 0-1.7 1.7z" fill="url(#icg-folder)" stroke="var(--ic-folder-deep)" stroke-opacity=".3"/>' +
      '<path d="M2.3 8.9h20.1L23.2 17.6a1.7 1.7 0 0 1-1.7 2H3.4a1.7 1.7 0 0 1-1.7-1.4z" fill="url(#icg-folder)" stroke="var(--ic-folder-deep)" stroke-opacity=".35"/>' +
      '<path d="M2.5 9.6h20.6" stroke="var(--ic-hi)" stroke-width="1"/>'
    : '<path d="M2.6 8.6V5.7a1.4 1.4 0 0 1 1.4-1.4h4.2a1.3 1.3 0 0 1 1 .5l1.6 1.9h9.1a1.4 1.4 0 0 1 1.4 1.4v1.1z" fill="var(--ic-folder-deep)"/>' +
      '<path d="M1.9 8.4h20.2v2.6L13.9 9.9l-2-1.5H3.6a1.7 1.7 0 0 0-1.7 1.7z" fill="var(--ic-folder-light)" stroke="var(--ic-folder-deep)" stroke-width="1"/>' +
      '<path d="M2.3 8.9h20.1L23.2 17.6a1.7 1.7 0 0 1-1.7 2H3.4a1.7 1.7 0 0 1-1.7-1.4z" fill="var(--ic-folder-surface)" stroke="var(--ic-folder-deep)" stroke-width="1.2"/>',
  'folder-plus': t === 'classic'
    ? '<path d="M2.6 8.6V5.7a1.4 1.4 0 0 1 1.4-1.4h4.2a1.3 1.3 0 0 1 1 .5l1.6 1.9h9.1a1.4 1.4 0 0 1 1.4 1.4v.5z" fill="var(--ic-folder-deep)"/>' +
      '<path d="M1.9 8.4h20.2v9.8a1.7 1.7 0 0 1-1.7 1.7H3.6a1.7 1.7 0 0 1-1.7-1.7z" fill="url(#icg-folder)"/>' +
      '<path d="M1.9 8.4h20.2" stroke="var(--ic-hi)" stroke-width="1"/>' +
      '<path d="M11.1 11.4h1.8v2.1h2.1v1.8h-2.1v2.1h-1.8v-2.1H9v-1.8h2.1z" fill="var(--ic-folder-deep)"/>'
    : '<path d="M2.6 8.6V5.7a1.4 1.4 0 0 1 1.4-1.4h4.2a1.3 1.3 0 0 1 1 .5l1.6 1.9h9.1a1.4 1.4 0 0 1 1.4 1.4v.5z" fill="var(--ic-folder-deep)"/>' +
      '<path d="M1.9 8.4h20.2v9.8a1.7 1.7 0 0 1-1.7 1.7H3.6a1.7 1.7 0 0 1-1.7-1.7z" fill="var(--ic-folder-surface)" stroke="var(--ic-folder-deep)" stroke-width="1.4"/>' +
      '<path d="M11.1 11.4h1.8v2.1h2.1v1.8h-2.1v2.1h-1.8v-2.1H9v-1.8h2.1z" fill="var(--ic-folder-deep)"/>',
  // paper 档文件类型 = 着色纸形直接落地（无磁贴），classic 档 = 磁贴 + 纸形
  file: t === 'classic' ? B('gray', t) + SHEET : sheetTinted('gray'),
  'file-text': t === 'classic'
    ? B('gray', t) + SHEET +
      '<g stroke="var(--ic-gray-deep)" stroke-width="1.2" stroke-linecap="round"><path d="M9 11h6"/><path d="M9 13h6"/><path d="M9 15h3.6"/></g>'
    : sheetTinted('gray') +
      '<g stroke="var(--ic-gray-ink)" stroke-width="1.2" stroke-linecap="round"><path d="M9 11h6"/><path d="M9 13h6"/><path d="M9 15h3.6"/></g>',
  'file-code': t === 'classic'
    ? B('orange', t) + sheetTinted('orange') +
      '<path d="M11 10.6 9.2 12.4l1.8 1.8M13 10.6l1.8 1.8-1.8 1.8" stroke="var(--ic-orange-solid)" stroke-width="1.4" fill="none" stroke-linecap="round" stroke-linejoin="round"/>'
    : sheetTinted('orange') +
      '<path d="M11 10.6 9.2 12.4l1.8 1.8M13 10.6l1.8 1.8-1.8 1.8" stroke="var(--ic-orange-ink)" stroke-width="1.4" fill="none" stroke-linecap="round" stroke-linejoin="round"/>',
  'file-json': t === 'classic'
    ? B('green', t) + sheetTinted('green') +
      '<path d="M10.8 10.4c-.9.2-1.1.7-1.1 1.4s-.2 1.2-.7 1.2c.5 0 .7.4.7 1.1s.2 1.3 1.1 1.5M13.2 10.4c.9.2 1.1.7 1.1 1.4s.2 1.2.7 1.2c-.5 0-.7.4-.7 1.1s-.2 1.3-1.1 1.5" stroke="var(--ic-green-solid)" stroke-width="1.3" fill="none" stroke-linecap="round"/>'
    : sheetTinted('green') +
      '<path d="M10.8 10.4c-.9.2-1.1.7-1.1 1.4s-.2 1.2-.7 1.2c.5 0 .7.4.7 1.1s.2 1.3 1.1 1.5M13.2 10.4c.9.2 1.1.7 1.1 1.4s.2 1.2.7 1.2c-.5 0-.7.4-.7 1.1s-.2 1.3-1.1 1.5" stroke="var(--ic-green-ink)" stroke-width="1.3" fill="none" stroke-linecap="round"/>',
  'file-image': t === 'classic'
    ? B('purple', t) + sheetTinted('purple') +
      '<path d="M8.6 15.4l2.2-2.7 1.6 1.8 1.1-1.4 2 2.3z" fill="var(--ic-purple-solid)"/>' +
      '<circle cx="13.6" cy="11.2" r="1" fill="var(--ic-purple-solid)"/>'
    : sheetTinted('purple') +
      '<path d="M8.6 15.4l2.2-2.7 1.6 1.8 1.1-1.4 2 2.3z" fill="var(--ic-purple-ink)"/>' +
      '<circle cx="13.6" cy="11.2" r="1" fill="var(--ic-purple-ink)"/>',
  'file-audio': t === 'classic'
    ? B('blue', t) + sheetTinted('blue') +
      '<circle cx="10.2" cy="14.4" r="1.4" fill="var(--ic-blue-solid)"/>' +
      '<rect x="11.4" y="9.6" width="1.2" height="5" fill="var(--ic-blue-solid)"/>' +
      '<path d="M12.6 9.6l2.8.9v1.4l-2.8-.9z" fill="var(--ic-blue-solid)"/>'
    : sheetTinted('purple') +
      '<circle cx="10.2" cy="14.4" r="1.4" fill="var(--ic-purple-ink)"/>' +
      '<rect x="11.4" y="9.6" width="1.2" height="5" fill="var(--ic-purple-ink)"/>' +
      '<path d="M12.6 9.6l2.8.9v1.4l-2.8-.9z" fill="var(--ic-purple-ink)"/>',
  'file-video': t === 'classic'
    ? B('blue', t) + sheetTinted('blue') +
      '<path d="M10 11.2 14.6 13.4 10 15.6z" fill="var(--ic-blue-solid)"/>'
    : sheetTinted('orange') +
      '<path d="M10 11.2 14.6 13.4 10 15.6z" fill="var(--ic-orange-ink)"/>',
  'file-archive': t === 'classic'
    ? B('yellow', t) + sheetTinted('yellow') +
      '<rect x="11" y="7.6" width="2" height="8.8" rx=".7" fill="var(--ic-yellow-deep)"/>' +
      '<g fill="var(--quirel-card-bg)"><rect x="11.3" y="8.6" width="1.4" height=".9" rx=".35"/><rect x="11.3" y="10.4" width="1.4" height=".9" rx=".35"/><rect x="11.3" y="12.2" width="1.4" height=".9" rx=".35"/></g>'
    : sheetTinted('yellow') +
      '<rect x="11" y="7.6" width="2" height="8.8" rx=".7" fill="var(--ic-yellow-ink)"/>' +
      '<g fill="var(--quirel-card-bg)"><rect x="11.3" y="8.6" width="1.4" height=".9" rx=".35"/><rect x="11.3" y="10.4" width="1.4" height=".9" rx=".35"/><rect x="11.3" y="12.2" width="1.4" height=".9" rx=".35"/></g>',
  config: t === 'classic'
    ? B('gray', t) + SHEET +
      '<g transform="translate(12 13.6)"><circle r="1.5" fill="none" stroke="var(--ic-gray-solid)" stroke-width="1.1"/>' +
      '<g stroke="var(--ic-gray-solid)" stroke-width="1" stroke-linecap="round"><path d="M0-3v-.9"/><path d="M0 3v.9"/><path d="M-3 0h-.9"/><path d="M3 0h.9"/><path d="M-2.1-2.1l-.6-.6"/><path d="M2.1 2.1l.6.6"/><path d="M2.1-2.1l.6-.6"/><path d="M-2.1 2.1l-.6.6"/></g></g>'
    : sheetTinted('gray') +
      '<g transform="translate(12 13.6)"><circle r="1.5" fill="none" stroke="var(--ic-gray-ink)" stroke-width="1.1"/>' +
      '<g stroke="var(--ic-gray-ink)" stroke-width="1" stroke-linecap="round"><path d="M0-3v-.9"/><path d="M0 3v.9"/><path d="M-3 0h-.9"/><path d="M3 0h.9"/><path d="M-2.1-2.1l-.6-.6"/><path d="M2.1 2.1l.6.6"/><path d="M2.1-2.1l.6-.6"/><path d="M-2.1 2.1l-.6.6"/></g></g>',
  database: B('blue', t) +
    '<ellipse cx="12" cy="8.6" rx="4.4" ry="1.8" fill="var(--ic-blue-ink)"/>' +
    '<path d="M7.6 8.6v6.8c0 1 2 1.8 4.4 1.8s4.4-.8 4.4-1.8V8.6z" fill="var(--ic-blue-ink)"/>' +
    '<path d="M7.6 12c0 1 2 1.8 4.4 1.8s4.4-.8 4.4-1.8" stroke="var(--ic-blue-surface)" stroke-width="1" fill="none" opacity=".7"/>',
  lock: B('yellow', t) +
    '<path d="M9.4 10.4V9a2.6 2.6 0 0 1 5.2 0v1.4" stroke="var(--ic-yellow-ink)" stroke-width="1.6" fill="none"/>' +
    '<rect x="7.6" y="10.2" width="8.8" height="6.6" rx="1.5" fill="var(--ic-yellow-ink)"/>' +
    '<circle cx="12" cy="13.2" r="1.1" fill="var(--ic-yellow-surface)"/>',

  /* ── 动作 ── */
  trash: B('red', t) +
    '<rect x="7.2" y="8" width="1.6" height="1.4" rx=".6" fill="none"/>' +
    '<path d="M8.6 7.8V7a1 1 0 0 1 1-1h4.8a1 1 0 0 1 1 1v.8" stroke="var(--ic-red-ink)" stroke-width="1.4" fill="none"/>' +
    '<rect x="7.2" y="8" width="9.6" height="1.6" rx=".8" fill="var(--ic-red-ink)"/>' +
    '<path d="M8.6 10.2h6.8v4.9a1.6 1.6 0 0 1-1.6 1.6h-3.6a1.6 1.6 0 0 1-1.6-1.6z" fill="var(--ic-red-ink)"/>' +
    '<g stroke="var(--ic-red-surface)" stroke-width="1.1" stroke-linecap="round"><path d="M10.8 11.6v3.6"/><path d="M13.2 11.6v3.6"/></g>',
  search: B('blue', t) +
    '<circle cx="11" cy="11" r="4.2" fill="none" stroke="var(--ic-blue-ink)" stroke-width="1.8"/>' +
    '<path d="M14.2 14.2 17.4 17.4" stroke="var(--ic-blue-ink)" stroke-width="2.1" stroke-linecap="round"/>',
});

/* 按档缓存：makeIcons 结果与主题一一对应，切档只重建一次 */
const iconsCache: Partial<Record<IconThemeKind, Record<AppIconName, string>>> = {};
function getIcons(t: IconThemeKind): Record<AppIconName, string> {
  return (iconsCache[t] ??= makeIcons(t));
}

export function AppIcon({
  name,
  size = 16,
  className,
  iconTheme,
}: {
  name: AppIconName;
  size?: number;
  className?: string;
  /** 强制指定图标主题（设置预览卡实况预览用）；缺省跟随 settingsStore.iconTheme */
  iconTheme?: IconThemeKind;
}) {
  const storeTheme = useSettingsStore((s) => s.iconTheme);
  const theme = iconTheme ?? storeTheme;
  const icons = getIcons(theme);
  const body = icons[name] ?? icons.file;
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      className={className}
      aria-hidden="true"
      dangerouslySetInnerHTML={{ __html: DEFS + body }}
    />
  );
}
