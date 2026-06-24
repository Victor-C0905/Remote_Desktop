// src/hooks/useTheme.ts

import { useEffect } from 'react';
import { ThemeId, AccentColorId, themes, accentColors } from '../config/themes';

export function useTheme(
  themeId: ThemeId,
  accentColorId?: AccentColorId | null,
  isDarkMode: boolean = false
) {
  useEffect(() => {
    // 如果 themeId 不存在，使用默认主题 'paper'
    const theme = themes[themeId] || themes['paper'];

    // 判断是否使用暗色配置
    const useDarkColors = isDarkMode || themeId === 'dark';
    const colors = useDarkColors ? theme.darkColors : theme.lightColors;

    // 应用主题颜色到 CSS 变量
    const root = document.documentElement;

    root.style.setProperty('--window-bg', colors.windowBg);
    root.style.setProperty('--view-bg', colors.viewBg);
    root.style.setProperty('--card-bg', colors.cardBg);
    root.style.setProperty('--card-hover', colors.cardHover);
    root.style.setProperty('--headerbar-bg', colors.headerbarBg);
    root.style.setProperty('--sidebar-bg', colors.sidebarBg);
    root.style.setProperty('--sidebar-border', colors.sidebarBorder);

    // 应用滑块颜色
    root.style.setProperty('--slider-track-bg', colors.sliderTrackBg);
    root.style.setProperty('--slider-thumb-bg', colors.sliderThumbBg);
    root.style.setProperty('--slider-active-bg', colors.sliderActiveBg);

    // 如果主题支持可选强调色，应用用户选择的强调色
    if (theme.accentColorOptions && accentColorId) {
      const accentColor = accentColors[accentColorId];
      const accent = useDarkColors ? accentColor.dark : accentColor.light;

      root.style.setProperty('--accent-bg', accent);
      root.style.setProperty('--accent-hover', adjustBrightness(accent, -10));
      root.style.setProperty('--accent-active', adjustBrightness(accent, -20));
    } else {
      root.style.setProperty('--accent-bg', colors.accentBg);
      root.style.setProperty('--accent-hover', colors.accentHover);
      root.style.setProperty('--accent-active', colors.accentActive);
    }

    // 应用字体颜色（暗色模式使用白色文字）
    if (useDarkColors) {
      root.style.setProperty('--text-primary', 'rgba(255, 255, 255, 0.87)');
      root.style.setProperty('--text-secondary', 'rgba(255, 255, 255, 0.60)');
      root.style.setProperty('--text-disabled', 'rgba(255, 255, 255, 0.38)');
      root.style.setProperty('--sidebar-fg', '#cccccc');
      root.style.setProperty('--border-color', 'rgba(255, 255, 255, 0.12)');
      root.setAttribute('data-theme', 'dark');
    } else {
      root.style.setProperty('--text-primary', 'rgba(0, 0, 0, 0.87)');
      root.style.setProperty('--text-secondary', 'rgba(0, 0, 0, 0.60)');
      root.style.setProperty('--text-disabled', 'rgba(0, 0, 0, 0.38)');
      root.style.setProperty('--sidebar-fg', '#3f3f3f');
      root.style.setProperty('--border-color', 'rgba(0, 0, 0, 0.15)');
      root.removeAttribute('data-theme');
    }
  }, [themeId, accentColorId, isDarkMode]);
}

// 辅助函数：调整颜色亮度
function adjustBrightness(hex: string, percent: number): string {
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