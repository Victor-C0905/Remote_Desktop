// src/hooks/useContrastColor.ts
// 根据背景色自动计算高对比度文本颜色

import { useState, useEffect, RefObject } from 'react';

/**
 * 计算颜色的相对亮度
 * 基于 W3C WCAG 2.0 标准
 */
function getLuminance(r: number, g: number, b: number): number {
  const [rs, gs, bs] = [r, g, b].map(c => {
    c = c / 255;
    return c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
  });
  return 0.2126 * rs + 0.7152 * gs + 0.0722 * bs;
}

/**
 * 计算两个颜色之间的对比度
 * 返回值范围: 1 到 21 (1=无对比, 21=最高对比)
 */
function getContrastRatio(luminance1: number, luminance2: number): number {
  const lighter = Math.max(luminance1, luminance2);
  const darker = Math.min(luminance1, luminance2);
  return (lighter + 0.05) / (darker + 0.05);
}

/**
 * 根据背景色选择最佳文本颜色
 * 返回: 'light' | 'dark'
 */
function getBestTextColor(bgColor: { r: number; g: number; b: number }): 'light' | 'dark' {
  const bgLuminance = getLuminance(bgColor.r, bgColor.g, bgColor.b);
  const whiteLuminance = getLuminance(255, 255, 255);
  const blackLuminance = getLuminance(0, 0, 0);

  const whiteContrast = getContrastRatio(bgLuminance, whiteLuminance);
  const blackContrast = getContrastRatio(bgLuminance, blackLuminance);

  // WCAG AA 标准要求对比度至少 4.5:1
  // 选择对比度更高的颜色
  return whiteContrast > blackContrast ? 'light' : 'dark';
}

/**
 * useContrastColor Hook
 *
 * 根据 DOM 元素的实际背景色自动选择高对比度文本颜色
 *
 * @param elementRef 目标元素的 ref
 * @param options 配置选项
 */
export function useContrastColor(
  elementRef: RefObject<HTMLElement>,
  options: {
    /** 采样间隔 (毫秒, 默认: 500) */
    interval?: number;
    /** 亮色主题的文本颜色 (默认: 使用主题的 textPrimary) */
    lightColor?: string;
    /** 暗色主题的文本颜色 (默认: 使用主题的 textPrimary) */
    darkColor?: string;
    /** 是否使用主题文本颜色 (默认: true) */
    useThemeText?: boolean;
  } = {}
) {
  const {
    interval = 500,
    lightColor = '#ffffff',
    darkColor = '#000000',
    useThemeText = true,
  } = options;

  const [textColor, setTextColor] = useState<'light' | 'dark'>('light');
  const [actualColor, setActualColor] = useState<string>(lightColor);

  useEffect(() => {
    const element = elementRef.current;
    if (!element) return;

    const updateColor = () => {
      // 获取元素的背景色
      const computedStyle = window.getComputedStyle(element);
      const bgColor = computedStyle.backgroundColor;

      // 解析 RGB 值
      const match = bgColor.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
      if (!match) return;

      const [, r, g, b] = match.map(Number);

      // 计算最佳文本颜色
      const bestColor = getBestTextColor({ r, g, b });
      setTextColor(bestColor);

      // ✅ 如果使用主题文本颜色,从 CSS 变量获取
      if (useThemeText) {
        const root = document.documentElement;
        const themeTextColor = getComputedStyle(root).getPropertyValue('--ovelis-text-primary').trim();
        
        // 如果主题文本颜色存在,使用主题色
        if (themeTextColor) {
          setActualColor(themeTextColor);
        } else {
          // 后备方案:使用传入的 lightColor/darkColor
          setActualColor(bestColor === 'light' ? lightColor : darkColor);
        }
      } else {
        // 不使用主题文本颜色,使用传入的颜色
        setActualColor(bestColor === 'light' ? lightColor : darkColor);
      }
    };

    // 初始更新
    updateColor();

    // 定期更新
    const timer = setInterval(updateColor, interval);

    return () => clearInterval(timer);
  }, [elementRef, interval, lightColor, darkColor, useThemeText]);

  return { textColor, actualColor };
}

/**
 * 导出辅助函数供外部使用
 */
export { getLuminance, getContrastRatio, getBestTextColor };