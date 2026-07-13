/**
 * 主题工具函数
 * 提供共享的主题相关逻辑，消除代码重复
 */

import { ThemeId, AccentColorId, themes, accentColors } from '../config/themes';

/**
 * 调整颜色亮度
 * @param hex 十六进制颜色值（格式：#RRGGBB）
 * @param percent 亮度调整百分比（负数变暗，正数变亮）
 * @returns 调整后的十六进制颜色值
 */
export function adjustBrightness(hex: string, percent: number): string {
  // 验证hex格式：必须是#RRGGBB格式
  if (!/^#[0-9A-Fa-f]{6}$/.test(hex)) {
    console.error(`Invalid hex color format: ${hex}`);
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
 * 应用主题颜色到CSS变量（统一设置ovelis前缀变量）
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

    // 设置基础颜色变量（ovelis前缀）
    root.style.setProperty('--ovelis-window-bg', colors.windowBg);
    root.style.setProperty('--ovelis-view-bg', colors.viewBg);
    root.style.setProperty('--ovelis-card-bg', colors.cardBg);
    root.style.setProperty('--ovelis-card-hover', colors.cardHover);
    root.style.setProperty('--ovelis-headerbar-bg', colors.headerbarBg);
    root.style.setProperty('--ovelis-sidebar-bg', colors.sidebarBg);
    root.style.setProperty('--ovelis-sidebar-border', colors.sidebarBorder);

    // 设置毛玻璃颜色（跟随主题的 headerbarBg，透明度 80%）
    const headerbarRgb = hexToRgb(colors.headerbarBg);
    if (headerbarRgb) {
      root.style.setProperty('--ovelis-frosted-bg', `rgba(${headerbarRgb.r}, ${headerbarRgb.g}, ${headerbarRgb.b}, 0.80)`);
    }

    // 设置滑块颜色（ovelis前缀）
    root.style.setProperty('--ovelis-slider-track-bg', colors.sliderTrackBg);
    root.style.setProperty('--ovelis-slider-thumb-bg', colors.sliderThumbBg);
    root.style.setProperty('--ovelis-slider-active-bg', colors.sliderActiveBg);

    // 设置强调色（ovelis前缀）
    if (theme.accentColorOptions && accentColorId) {
      const accentColor = accentColors[accentColorId];
      if (accentColor) {
        const accent = accentColor.light; // 直接使用light版本
        root.style.setProperty('--ovelis-accent-bg', accent);
        root.style.setProperty('--ovelis-accent-hover', adjustBrightness(accent, -10));
        root.style.setProperty('--ovelis-accent-active', adjustBrightness(accent, -20));
      } else {
        // 如果强调色ID无效，使用主题默认强调色
        root.style.setProperty('--ovelis-accent-bg', colors.accentBg);
        root.style.setProperty('--ovelis-accent-hover', colors.accentHover);
        root.style.setProperty('--ovelis-accent-active', colors.accentActive);
      }
    } else {
      root.style.setProperty('--ovelis-accent-bg', colors.accentBg);
      root.style.setProperty('--ovelis-accent-hover', colors.accentHover);
      root.style.setProperty('--ovelis-accent-active', colors.accentActive);
    }

    // 设置字体颜色和边框颜色（ovelis前缀）
    root.style.setProperty('--ovelis-text-primary', colors.textPrimary);
    root.style.setProperty('--ovelis-text-secondary', colors.textSecondary);
    root.style.setProperty('--ovelis-text-disabled', colors.textDisabled);
    root.style.setProperty('--ovelis-border-color', colors.borderColor);
  } catch (error) {
    console.error('应用主题颜色失败:', error);
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