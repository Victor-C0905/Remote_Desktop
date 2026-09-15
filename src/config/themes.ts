/**
 * 主题系统配置文件
 *
 * 数据结构说明：
 * ================
 *
 * 1. ThemeId - 主题标识符类型
 *    - 新增主题时需要在此处添加新的主题 ID
 *    - 格式：'themeName'（小写驼峰）
 *
 * 2. AccentColorId - 强调色标识符类型
 *    - 新增强调色时需要在此处添加新的强调色 ID
 *    - 格式：'colorName'（小写驼峰）
 *
 * 3. ThemeColors - 颜色配置接口
 *    - 定义了所有主题必须包含的颜色变量
 *    - 所有颜色使用十六进制格式：'#RRGGBB'
 *
 * 4. Theme - 主题接口
 *    - id: 唯一标识符（与 ThemeId 对应）
 *    - name: 显示名称
 *    - description: 描述文字
 *    - lightColors: 亮色模式颜色配置
 *    - darkColors: 暗色模式颜色配置
 *    - accentColorOptions?: 可选的强调色列表（不定义则使用默认强调色）
 *
 * 5. themes - 主题对象
 *    - 使用 Record<ThemeId, Theme> 类型确保类型安全
 *    - 每个主题必须有完整的 lightColors 和 darkColors 配置
 *
 * 6. accentColors - 强调色对象
 *    - 使用 Record<AccentColorId, {light, dark}> 类型
 *    - 每个强调色需要提供亮色和暗色两种版本
 *
 * ===================
 * 如何新增主题：
 * ===================
 *
 * 步骤 1：在 ThemeId 类型中添加新 ID
 *   export type ThemeId = 'paper' | 'neutral' | 'dark' | 'myNewTheme';
 *
 * 步骤 2：在 themes 对象中添加新主题配置
 *   myNewTheme: {
 *     id: 'myNewTheme',
 *     name: '我的新主题',
 *     description: '主题描述',
 *     lightColors: {
 *       windowBg: '#ffffff',      // 主窗口背景
 *       viewBg: '#f5f5f5',        // 内容区背景（最浅）
 *       cardBg: '#f5f5f5',        // 卡片背景
 *       cardHover: '#e8e8e8',     // 卡片悬停状态
 *       headerbarBg: '#e8e8e8',   // HeaderBar 背景
 *       sidebarBg: '#e8e8e8',     // 侧边栏背景
 *       sidebarBorder: '#d0d0d0', // 边框颜色
 *       accentBg: '#3584e4',      // 强调色
 *       accentHover: '#1f75d1',   // 强调色悬停
 *       accentActive: '#1a5fb4',  // 强调色活动状态
 *       sliderTrackBg: '#d0d0d0',// 滑条轨道
 *       sliderThumbBg: '#3584e4', // 滑块
 *       sliderActiveBg: '#1f75d1',// 滑块活动状态
 *     },
 *     darkColors: {
 *       // ... 同上结构，使用暗色调
 *     },
 *     accentColorOptions: ['warmBlue'], // 可选：允许用户选择强调色
 *   },
 *
 * 步骤 3：（可选）如果主题支持可选强调色，在 AccentColorId 中添加新选项
 *   export type AccentColorId = 'warmBlue' | 'paperAccent' | 'orange' | 'myColor';
 *
 * 步骤 4：（可选）在 accentColors 中添加新强调色的亮色和暗色版本
 *   myColor: {
 *     light: '#ff6b6b',
 *     dark: '#ff8787',
 *   },
 *
 * 步骤 5：更新 settingsStore.ts 的 DEFAULT_SETTINGS（如需设为默认主题）
 *   themeId: "myNewTheme",
 *
 * 注意事项：
 * - 颜色层次应遵循：viewBg(最浅) → cardBg → cardHover → headerbarBg/sidebarBg(中等) → sidebarBorder(最深)
 * - 暗色模式的对比度要足够高，确保文字清晰可见
 * - 功能性颜色（状态、警告等）不应随主题变化
 */

// ============================================================
// 主题标识符 - 新增主题时在此处添加
// ============================================================
export type ThemeId = 'paper' | 'neutral' | 'dark';

// ============================================================
// 强调色标识符 - 新增强调色时在此处添加
// ============================================================
export type AccentColorId = 'warmBlue' | 'paperAccent' | 'orange';

// ============================================================
// 颜色配置接口 - 所有主题必须包含这些颜色变量
// ============================================================
export interface ThemeColors {
  /** 主窗口背景 */
  windowBg: string;
  /** 内容区/视图背景（通常是最浅的背景色） */
  viewBg: string;
  /** 卡片背景 */
  cardBg: string;
  /** 卡片悬停状态背景 */
  cardHover: string;
  /** HeaderBar 背景色 */
  headerbarBg: string;
  /** 侧边栏背景色 */
  sidebarBg: string;
  /** 侧边栏/卡片边框色 */
  sidebarBorder: string;
  /** 主要强调色（按钮、链接等） */
  accentBg: string;
  /** 强调色悬停状态 */
  accentHover: string;
  /** 强调色活动/按下状态 */
  accentActive: string;
  /** 滑条轨道背景色 */
  sliderTrackBg: string;
  /** 滑块填充色 */
  sliderThumbBg: string;
  /** 滑块活动/拖动状态色 */
  sliderActiveBg: string;

  // 字体颜色和边框颜色
  /** 主字体颜色 */
  textPrimary: string;
  /** 次级字体颜色 */
  textSecondary: string;
  /** 禁用字体颜色 */
  textDisabled: string;
  /** 边框颜色 */
  borderColor: string;

  // 通知严重度圆点色板（emoji 观感球体：hi 高光 / mid 主色 / lo 边缘暗色）
  /** 紧急（红）圆点：高光色 */
  dotCriticalHi: string;
  /** 紧急（红）圆点：主色 */
  dotCriticalMid: string;
  /** 紧急（红）圆点：边缘暗色 */
  dotCriticalLo: string;
  /** 普通（黄）圆点：高光色 */
  dotNormalHi: string;
  /** 普通（黄）圆点：主色 */
  dotNormalMid: string;
  /** 普通（黄）圆点：边缘暗色 */
  dotNormalLo: string;
  /** 低（绿）圆点：高光色 */
  dotLowHi: string;
  /** 低（绿）圆点：主色 */
  dotLowMid: string;
  /** 低（绿）圆点：边缘暗色 */
  dotLowLo: string;
}

// ============================================================
// 主题接口 - 定义主题的完整结构
// ============================================================
export interface Theme {
  /** 主题唯一标识符 */
  id: ThemeId;
  /** 显示名称 */
  name: string;
  /** 描述文字 */
  description: string;
  /** 亮色模式颜色配置 */
  lightColors: ThemeColors;
  /** 暗色模式颜色配置 */
  darkColors: ThemeColors;
  /**
   * 可选的用户可选择强调色列表
   * - 不设置或为空数组：使用 accentBg 作为固定强调色
   * - 设置后：用户可在这些强调色中选择
   * - 选择的值会覆盖 accentBg/accentHover/accentActive
   */
  accentColorOptions?: AccentColorId[];
}

// ============================================================
// 主题配置 - 在此处添加新主题
// ============================================================
export const themes: Record<ThemeId, Theme> = {

  // --------------------------------------------------------
  // 纸张主题 - 温暖的纸张色调
  // 特点：暖灰色系，带有米色倾向，适合阅读和长时间使用
  // 默认强调色：#15aa70 (绿色)
  // 可选强调色：warmBlue (暖蓝)、paperAccent (绿)
  // --------------------------------------------------------
  paper: {
    id: 'paper',
    name: '纸张',
    description: '温暖的纸张色调，层次分明',
    lightColors: {
      windowBg: '#fafafa',           // 柔和白
      viewBg: '#faf5f3',             // 内容区（最浅）- RGB(250,245,243)
      cardBg: '#faf5f3',             // 卡片背景（最浅）
      cardHover: '#ebe5d9',          // 卡片悬停（较深）- 中暖灰 RGB(235,229,217)
      headerbarBg: '#f5f0e6',        // HeaderBar（中等）- 暖灰 RGB(245,240,230)
      sidebarBg: '#f5f0e6',          // 侧边栏（中等）- 暖灰 RGB(245,240,230)
      sidebarBorder: '#d5d0c4',      // 边框（最深）- 深暖灰 RGB(213,208,196)
      accentBg: '#15aa70',           // 默认强调色 - 绿色 RGB(21,170,112)
      accentHover: '#129864',        // 悬停状态
      accentActive: '#0f8658',       // 活动状态
      sliderTrackBg: '#d5d0c4',      // 深暖灰滑条轨道
      sliderThumbBg: '#15aa70',      // 绿色滑块
      sliderActiveBg: '#129864',     // 活动状态
      textPrimary: 'rgba(0, 0, 0, 0.87)',
      textSecondary: 'rgba(0, 0, 0, 0.60)',
      textDisabled: 'rgba(0, 0, 0, 0.38)',
      borderColor: 'rgba(0, 0, 0, 0.15)',
      // 圆点色板（暖纸张）：与米色暖背景和谐的暖调三色
      dotCriticalHi: '#ff9c9c',        // 高光
      dotCriticalMid: '#e01b24',       // 主色
      dotCriticalLo: '#a5121a',        // 边缘暗色
      dotNormalHi: '#ffe066',
      dotNormalMid: '#f5c211',
      dotNormalLo: '#b3870a',
      dotLowHi: '#7ce3a0',
      dotLowMid: '#26a269',
      dotLowLo: '#15764b',
    },
    darkColors: {
      windowBg: '#242424',           // 中性暗色
      viewBg: '#1e1e1e',             // 最深背景
      cardBg: '#2d2d2d',             // 暗色卡片
      cardHover: '#353535',          // 悬停状态
      headerbarBg: '#303030',        // HeaderBar
      sidebarBg: '#303030',          // 侧边栏
      sidebarBorder: '#3d3d3d',      // 边框
      accentBg: '#62a0ea',           // Adwaita Blue
      accentHover: '#7ab2f0',
      accentActive: '#8ec1ff',
      sliderTrackBg: '#454545',      // 深灰滑条轨道
      sliderThumbBg: '#62a0ea',      // Adwaita Blue 滑块
      sliderActiveBg: '#7ab2f0',     // 活动状态
      textPrimary: 'rgba(255, 255, 255, 0.87)',
      textSecondary: 'rgba(255, 255, 255, 0.60)',
      textDisabled: 'rgba(255, 255, 255, 0.38)',
      borderColor: 'rgba(255, 255, 255, 0.12)',
      // 圆点色板（暗色）：GNOME 暗色调色板，暗背景上更亮、有发光感
      dotCriticalHi: '#ffb3ab',
      dotCriticalMid: '#f66151',
      dotCriticalLo: '#c0272d',
      dotNormalHi: '#fff29a',
      dotNormalMid: '#f8e45c',
      dotNormalLo: '#c5a10e',
      dotLowHi: '#a5f0c3',
      dotLowMid: '#57e389',
      dotLowLo: '#24a360',
    },
    accentColorOptions: ['warmBlue', 'paperAccent'],
  },

  // --------------------------------------------------------
  // 中性色调主题 - 符合 Adwaita 标准
  // 特点：纯中性灰色系，无色彩倾向，最符合 Adwaita 设计规范
  // 默认强调色：#3584e4 (Adwaita Blue)
  // 无可选强调色
  // --------------------------------------------------------
  neutral: {
    id: 'neutral',
    name: '中性色调（Adwaita 标准）',
    description: '符合 Adwaita 标准的中性色调',
    lightColors: {
      windowBg: '#fafafa',           // 柔和白
      viewBg: '#f5f5f5',             // 内容区 - 中性浅灰
      cardBg: '#f5f5f5',             // 卡片 - 中性浅灰
      cardHover: '#e8e8e8',          // 悬停 - 中性灰
      headerbarBg: '#e8e8e8',        // HeaderBar - 中性灰
      sidebarBg: '#e8e8e8',          // 侧边栏 - 中性灰
      sidebarBorder: '#d0d0d0',      // 边框 - 中性深灰
      accentBg: '#3584e4',           // Adwaita Blue
      accentHover: '#1f75d1',
      accentActive: '#1a5fb4',
      sliderTrackBg: '#d0d0d0',      // 中性灰滑条轨道
      sliderThumbBg: '#3584e4',      // Adwaita Blue 滑块
      sliderActiveBg: '#1f75d1',     // 活动状态
      textPrimary: 'rgba(0, 0, 0, 0.87)',
      textSecondary: 'rgba(0, 0, 0, 0.60)',
      textDisabled: 'rgba(0, 0, 0, 0.38)',
      borderColor: 'rgba(0, 0, 0, 0.15)',
      // 圆点色板（中性）：Adwaita 官方三色，与中性灰背景和谐
      dotCriticalHi: '#ff9c9c',
      dotCriticalMid: '#e01b24',
      dotCriticalLo: '#a5121a',
      dotNormalHi: '#ffe26b',
      dotNormalMid: '#f5c211',
      dotNormalLo: '#b3870a',
      dotLowHi: '#8ff0b4',
      dotLowMid: '#33d17a',
      dotLowLo: '#1c7a4a',
    },
    darkColors: {
      windowBg: '#242424',           // 中性暗色
      viewBg: '#1e1e1e',             // 最深背景
      cardBg: '#2d2d2d',             // 暗色卡片
      cardHover: '#353535',          // 悬停状态
      headerbarBg: '#303030',        // HeaderBar
      sidebarBg: '#303030',          // 侧边栏
      sidebarBorder: '#3d3d3d',      // 边框
      accentBg: '#62a0ea',           // Adwaita Blue (暗色版)
      accentHover: '#7ab2f0',
      accentActive: '#8ec1ff',
      sliderTrackBg: '#454545',      // 深灰滑条轨道
      sliderThumbBg: '#62a0ea',      // Adwaita Blue 滑块
      sliderActiveBg: '#7ab2f0',     // 活动状态
      textPrimary: 'rgba(255, 255, 255, 0.87)',
      textSecondary: 'rgba(255, 255, 255, 0.60)',
      textDisabled: 'rgba(255, 255, 255, 0.38)',
      borderColor: 'rgba(255, 255, 255, 0.12)',
      // 圆点色板（暗色）：GNOME 暗色调色板，暗背景上更亮、有发光感
      dotCriticalHi: '#ffb3ab',
      dotCriticalMid: '#f66151',
      dotCriticalLo: '#c0272d',
      dotNormalHi: '#fff29a',
      dotNormalMid: '#f8e45c',
      dotNormalLo: '#c5a10e',
      dotLowHi: '#a5f0c3',
      dotLowMid: '#57e389',
      dotLowLo: '#24a360',
    },
    // 无可选强调色，使用固定的 Adwaita Blue
  },

  // --------------------------------------------------------
  // 暗色主题 - 高对比度深色主题
  // 特点：深色背景 + 白色文字，适合夜间使用
  // 默认强调色：#62a0ea (Adwaita Blue 暗色版)
  // 注意：此主题始终使用暗色配置（lightColors === darkColors）
  // 无可选强调色
  // --------------------------------------------------------
  dark: {
    id: 'dark',
    name: '暗色',
    description: '深色主题，高对比度，适合夜间使用',
    lightColors: {
      windowBg: '#242424',           // 中性暗色
      viewBg: '#1e1e1e',             // 最深背景
      cardBg: '#2d2d2d',             // 暗色卡片
      cardHover: '#353535',          // 悬停状态
      headerbarBg: '#303030',        // HeaderBar
      sidebarBg: '#303030',          // 侧边栏
      sidebarBorder: '#3d3d3d',      // 边框
      accentBg: '#62a0ea',           // Adwaita Blue (暗色版)
      accentHover: '#7ab2f0',
      accentActive: '#8ec1ff',
      sliderTrackBg: '#454545',      // 深灰滑条轨道
      sliderThumbBg: '#62a0ea',      // Adwaita Blue 滑块
      sliderActiveBg: '#7ab2f0',     // 活动状态
      textPrimary: 'rgba(255, 255, 255, 0.87)',
      textSecondary: 'rgba(255, 255, 255, 0.60)',
      textDisabled: 'rgba(255, 255, 255, 0.38)',
      borderColor: 'rgba(255, 255, 255, 0.12)',
      // 圆点色板（暗色）：GNOME 暗色调色板，暗背景上更亮、有发光感
      dotCriticalHi: '#ffb3ab',
      dotCriticalMid: '#f66151',
      dotCriticalLo: '#c0272d',
      dotNormalHi: '#fff29a',
      dotNormalMid: '#f8e45c',
      dotNormalLo: '#c5a10e',
      dotLowHi: '#a5f0c3',
      dotLowMid: '#57e389',
      dotLowLo: '#24a360',
    },
    darkColors: {
      windowBg: '#242424',           // 与 lightColors 相同
      viewBg: '#1e1e1e',
      cardBg: '#2d2d2d',
      cardHover: '#353535',
      headerbarBg: '#303030',
      sidebarBg: '#303030',
      sidebarBorder: '#3d3d3d',
      accentBg: '#62a0ea',
      accentHover: '#7ab2f0',
      accentActive: '#8ec1ff',
      sliderTrackBg: '#454545',
      sliderThumbBg: '#62a0ea',
      sliderActiveBg: '#7ab2f0',
      textPrimary: 'rgba(255, 255, 255, 0.87)',
      textSecondary: 'rgba(255, 255, 255, 0.60)',
      textDisabled: 'rgba(255, 255, 255, 0.38)',
      borderColor: 'rgba(255, 255, 255, 0.12)',
      // 圆点色板（暗色）：与 lightColors 相同
      dotCriticalHi: '#ffb3ab',
      dotCriticalMid: '#f66151',
      dotCriticalLo: '#c0272d',
      dotNormalHi: '#fff29a',
      dotNormalMid: '#f8e45c',
      dotNormalLo: '#c5a10e',
      dotLowHi: '#a5f0c3',
      dotLowMid: '#57e389',
      dotLowLo: '#24a360',
    },
    // 无可选强调色，使用固定的 Adwaita Blue
  },
};

// ============================================================
// 强调色配置 - 在此处添加新强调色
// ============================================================
export const accentColors: Record<AccentColorId, { light: string; dark: string }> = {

  // 暖蓝色 - 比 Adwaita Blue 更暖的蓝色
  warmBlue: {
    light: '#4a90e2',               // 亮色模式
    dark: '#62a0ea',                // 暗色模式
  },

  // 纸张主题默认强调色 - 温暖的绿色
  paperAccent: {
    light: '#15aa70',               // RGB(21,170,112) - 亮色模式
    dark: '#4ec9b0',                // 暗色模式
  },

  // 橙色 - Adwaita Orange
  orange: {
    light: '#e66100',               // 亮色模式
    dark: '#e66100',                // 暗色模式（橙色在暗色模式下保持不变）
  },
};
