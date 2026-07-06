// src/hooks/useTheme.ts

import { useLayoutEffect } from 'react';
import { ThemeId, AccentColorId } from '../config/themes';
import { applyThemeColors } from '../utils/themeUtils';

export function useTheme(
  themeId: ThemeId,
  accentColorId?: AccentColorId | null
) {
  useLayoutEffect(() => {
    // 应用主题颜色到 CSS 变量
    applyThemeColors(themeId, accentColorId);
  }, [themeId, accentColorId]);
}