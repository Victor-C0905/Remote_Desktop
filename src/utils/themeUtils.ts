/**
 * 主题工具函数
 * 提供共享的主题相关逻辑，消除代码重复
 */

import { ThemeId, AccentColorId, themes, accentColors } from '../config/themes';
import { createLogger } from './logger';

const log = createLogger('ThemeUtils');

/**
 * 调整颜色亮度
 * @param hex 十六进制颜色值（格式：#RRGGBB）
 * @param percent 亮度调整百分比（负数变暗，正数变亮）
 * @returns 调整后的十六进制颜色值
 */
export function adjustBrightness(hex: string, percent: number): string {
  // 验证hex格式：必须是#RRGGBB格式
  if (!/^#[0-9A-Fa-f]{6}$/.test(hex)) {
    log.error(`Invalid hex color format: ${hex}`);
    return hex; // 返回原值，避免破坏应用
  }

  const num = parseInt(hex.replace('#', ''), 16);
  const amt = Math.round(2.55 * percent);
  const R = (num >> 16) + amt;
  const G = (num >> 8 & 0x00FF) + amt;
  const B = (num & 0x0000FF) + amt;

  return '#' + (
    0x1000000 +
    (R < 255 ? (R < 1 ? 0 : R) : 255) * 0x10000 +
    (G < 255 ? (G < 1 ? 0 : G) : 255) * 0x100 +
    (B < 255 ? (B < 1 ? 0 : B) : 255)
  ).toString(16).slice(1);
}

/**
 * 十六进制转 RGB
 * @param hex 十六进制颜色值（格式：#RRGGBB）
 * @returns { r, g, b } 或 null（格式错误时）
 */
function hexToRgb(hex: string): { r: number; g: number; b: number } | null {
  if (!/^#[0-9A-Fa-f]{6}$/.test(hex)) {
    return null;
  }
  const num = parseInt(hex.replace('#', ''), 16);
  return {
    r: (num >> 16) & 255,
    g: (num >> 8) & 255,
    b: num & 255,
  };
}

/**
 * 应用主题颜色到CSS变量（统一设置quirel前缀变量）
 * @param themeId 主题ID
 * @param accentColorId 强调色ID（可选）
 */
export function applyThemeColors(
  themeId: ThemeId,
  accentColorId?: AccentColorId | null
): void {
  try {
    const theme = themes[themeId] || themes['paper'];
    const colors = theme.lightColors; // dark主题的lightColors已包含暗色配置
    const root = document.documentElement;

    // 设置基础颜色变量（quirel前缀）
    root.style.setProperty('--quirel-window-bg', colors.windowBg);
    root.style.setProperty('--quirel-view-bg', colors.viewBg);
    root.style.setProperty('--quirel-card-bg', colors.cardBg);
    root.style.setProperty('--quirel-card-hover', colors.cardHover);
    root.style.setProperty('--quirel-headerbar-bg', colors.headerbarBg);
    root.style.setProperty('--quirel-sidebar-bg', colors.sidebarBg);
    root.style.setProperty('--quirel-sidebar-border', colors.sidebarBorder);

    // 设置毛玻璃颜色（跟随主题的 headerbarBg，透明度 80%）
    const headerbarRgb = hexToRgb(colors.headerbarBg);
    if (headerbarRgb) {
      // ✅ 设置 RGB 分离值,供 dock 等组件使用
      root.style.setProperty('--quirel-headerbar-bg-rgb', `${headerbarRgb.r}, ${headerbarRgb.g}, ${headerbarRgb.b}`);
      // ✅ 设置毛玻璃背景色 (rgba 格式)
      root.style.setProperty('--quirel-frosted-bg', `rgba(${headerbarRgb.r}, ${headerbarRgb.g}, ${headerbarRgb.b}, 0.80)`);
    }

    // 设置滑块颜色（quirel前缀）
    root.style.setProperty('--quirel-slider-track-bg', colors.sliderTrackBg);
    root.style.setProperty('--quirel-slider-thumb-bg', colors.sliderThumbBg);
    root.style.setProperty('--quirel-slider-active-bg', colors.sliderActiveBg);

    // 设置强调色（quirel前缀）
    if (theme.accentColorOptions && accentColorId) {
      const accentColor = accentColors[accentColorId];
      if (accentColor) {
        const accent = accentColor.light; // 直接使用light版本
        root.style.setProperty('--quirel-accent-bg', accent);
        root.style.setProperty('--quirel-accent-hover', adjustBrightness(accent, -10));
        root.style.setProperty('--quirel-accent-active', adjustBrightness(accent, -20));
      } else {
        // 如果强调色ID无效，使用主题默认强调色
        root.style.setProperty('--quirel-accent-bg', colors.accentBg);
        root.style.setProperty('--quirel-accent-hover', colors.accentHover);
        root.style.setProperty('--quirel-accent-active', colors.accentActive);
      }
    } else {
      root.style.setProperty('--quirel-accent-bg', colors.accentBg);
      root.style.setProperty('--quirel-accent-hover', colors.accentHover);
      root.style.setProperty('--quirel-accent-active', colors.accentActive);
    }

    // 设置字体颜色和边框颜色（quirel前缀）
    root.style.setProperty('--quirel-text-primary', colors.textPrimary);
    root.style.setProperty('--quirel-text-secondary', colors.textSecondary);
    root.style.setProperty('--quirel-text-disabled', colors.textDisabled);
    root.style.setProperty('--quirel-border-color', colors.borderColor);

    // 设置通知严重度圆点色板（quirel前缀，emoji 观感球体三段渐变）
    root.style.setProperty('--quirel-dot-critical-hi', colors.dotCriticalHi);
    root.style.setProperty('--quirel-dot-critical-mid', colors.dotCriticalMid);
    root.style.setProperty('--quirel-dot-critical-lo', colors.dotCriticalLo);
    root.style.setProperty('--quirel-dot-normal-hi', colors.dotNormalHi);
    root.style.setProperty('--quirel-dot-normal-mid', colors.dotNormalMid);
    root.style.setProperty('--quirel-dot-normal-lo', colors.dotNormalLo);
    root.style.setProperty('--quirel-dot-low-hi', colors.dotLowHi);
    root.style.setProperty('--quirel-dot-low-mid', colors.dotLowMid);
    root.style.setProperty('--quirel-dot-low-lo', colors.dotLowLo);
  } catch (error) {
    log.error('应用主题颜色失败:', error);
  }
}

/**
 * 获取主题对象（带默认后备）
 * @param themeId 主题ID
 * @returns 主题对象
 */
export function getTheme(themeId: ThemeId) {
  return themes[themeId] || themes['paper'];
}