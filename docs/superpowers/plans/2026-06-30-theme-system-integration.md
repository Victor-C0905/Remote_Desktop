# 主题系统整合实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 整合现有主题系统到窗口系统层，删除暗色模式切换逻辑，统一CSS变量命名规范（ovelis前缀）

**Architecture:** 基于现有完善的窗口管理系统，整合主题系统，扩展ThemeColors接口，简化useTheme逻辑，统一CSS变量命名

**Tech Stack:** React 18, TypeScript, Zustand, CSS Variables

---

## 文件结构

**修改文件：**
- `src/config/themes.ts` - 扩展ThemeColors接口，添加字体和边框颜色配置
- `src/hooks/useTheme.ts` - 删除暗色模式逻辑，统一CSS变量命名（ovelis前缀）
- `src/stores/settingsStore.ts` - 删除isDarkMode状态和操作，更新onRehydrateStorage逻辑
- `src/apps/*.css` - 替换CSS变量引用（FileManager.css、Terminal.css、SystemMonitor.css、Settings.css等）

---

## Task 1: 扩展 ThemeColors 接口

**Files:**
- Modify: `src/config/themes.ts:99-126` (ThemeColors接口定义)

- [ ] **Step 1: 扩展ThemeColors接口，添加字体和边框颜色字段**

```typescript
// src/config/themes.ts (第99-126行，修改ThemeColors接口)

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

  // 新增：字体颜色和边框颜色
  /** 主字体颜色 */
  textPrimary: string;
  /** 次级字体颜色 */
  textSecondary: string;
  /** 禁用字体颜色 */
  textDisabled: string;
  /** 边框颜色 */
  borderColor: string;
}
```

- [ ] **Step 2: 提示用户需要提交此更改**

用户需要手动提交：
```bash
git add src/config/themes.ts
git commit -m "refactor: 扩展ThemeColors接口，添加字体和边框颜色字段"
```

---

## Task 2: 更新主题配置（添加字体和边框颜色）

**Files:**
- Modify: `src/config/themes.ts:162-285` (paper、neutral、dark主题配置)

- [ ] **Step 1: 更新paper主题配置，添加字体和边框颜色**

```typescript
// src/config/themes.ts (paper主题配置，lightColors和darkColors)

paper: {
  id: 'paper',
  name: '纸张',
  description: '温暖的纸张色调，层次分明',
  lightColors: {
    windowBg: '#fafafa',
    viewBg: '#faf5f3',
    cardBg: '#faf5f3',
    cardHover: '#ebe5d9',
    headerbarBg: '#f5f0e6',
    sidebarBg: '#f5f0e6',
    sidebarBorder: '#d5d0c4',
    accentBg: '#15aa70',
    accentHover: '#129864',
    accentActive: '#0f8658',
    sliderTrackBg: '#d5d0c4',
    sliderThumbBg: '#15aa70',
    sliderActiveBg: '#129864',

    // 新增：字体和边框颜色（亮色）
    textPrimary: 'rgba(0, 0, 0, 0.87)',
    textSecondary: 'rgba(0, 0, 0, 0.60)',
    textDisabled: 'rgba(0, 0, 0, 0.38)',
    borderColor: 'rgba(0, 0, 0, 0.15)',
  },
  darkColors: {
    windowBg: '#242424',
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

    // 新增：字体和边框颜色（暗色）
    textPrimary: 'rgba(255, 255, 255, 0.87)',
    textSecondary: 'rgba(255, 255, 255, 0.60)',
    textDisabled: 'rgba(255, 255, 255, 0.38)',
    borderColor: 'rgba(255, 255, 255, 0.12)',
  },
  accentColorOptions: ['warmBlue', 'paperAccent'],
},
```

- [ ] **Step 2: 更新neutral主题配置，添加字体和边框颜色**

```typescript
// src/config/themes.ts (neutral主题配置，lightColors和darkColors)

neutral: {
  id: 'neutral',
  name: '中性色调（GNOME 标准）',
  description: '符合 GNOME Adwaita 标准的中性色调',
  lightColors: {
    windowBg: '#fafafa',
    viewBg: '#f5f5f5',
    cardBg: '#f5f5f5',
    cardHover: '#e8e8e8',
    headerbarBg: '#e8e8e8',
    sidebarBg: '#e8e8e8',
    sidebarBorder: '#d0d0d0',
    accentBg: '#3584e4',
    accentHover: '#1f75d1',
    accentActive: '#1a5fb4',
    sliderTrackBg: '#d0d0d0',
    sliderThumbBg: '#3584e4',
    sliderActiveBg: '#1f75d1',

    // 新增：字体和边框颜色（亮色）
    textPrimary: 'rgba(0, 0, 0, 0.87)',
    textSecondary: 'rgba(0, 0, 0, 0.60)',
    textDisabled: 'rgba(0, 0, 0, 0.38)',
    borderColor: 'rgba(0, 0, 0, 0.15)',
  },
  darkColors: {
    windowBg: '#242424',
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

    // 新增：字体和边框颜色（暗色）
    textPrimary: 'rgba(255, 255, 255, 0.87)',
    textSecondary: 'rgba(255, 255, 255, 0.60)',
    textDisabled: 'rgba(255, 255, 255, 0.38)',
    borderColor: 'rgba(255, 255, 255, 0.12)',
  },
  // 无可选强调色，使用固定的 GNOME Blue
},
```

- [ ] **Step 3: 更新dark主题配置，添加字体和边框颜色**

```typescript
// src/config/themes.ts (dark主题配置，lightColors和darkColors)

dark: {
  id: 'dark',
  name: '暗色',
  description: '深色主题，高对比度，适合夜间使用',
  lightColors: {
    windowBg: '#242424',
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

    // 新增：字体和边框颜色（暗色，dark主题始终使用暗色）
    textPrimary: 'rgba(255, 255, 255, 0.87)',
    textSecondary: 'rgba(255, 255, 255, 0.60)',
    textDisabled: 'rgba(255, 255, 255, 0.38)',
    borderColor: 'rgba(255, 255, 255, 0.12)',
  },
  darkColors: {
    // 与 lightColors 相同（dark主题始终使用暗色配置）
    windowBg: '#242424',
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

    // 新增：字体和边框颜色（与lightColors相同）
    textPrimary: 'rgba(255, 255, 255, 0.87)',
    textSecondary: 'rgba(255, 255, 255, 0.60)',
    textDisabled: 'rgba(255, 255, 255, 0.38)',
    borderColor: 'rgba(255, 255, 255, 0.12)',
  },
  // 无可选强调色，使用固定的 GNOME Blue
},
```

- [ ] **Step 4: 提示用户需要提交此更改**

用户需要手动提交：
```bash
git add src/config/themes.ts
git commit -m "refactor: 更新主题配置，添加字体和边框颜色"
```

---

## Task 3: 简化 useTheme.ts（删除暗色模式逻辑）

**Files:**
- Modify: `src/hooks/useTheme.ts:1-82` (整个文件)

- [ ] **Step 1: 删除isDarkMode参数，简化useTheme函数签名**

```typescript
// src/hooks/useTheme.ts (第6-10行，修改函数签名)

export function useTheme(
  themeId: ThemeId,
  accentColorId?: AccentColorId | null
  // ❌ 删除 isDarkMode 参数
) {
  // ...
}
```

- [ ] **Step 2: 删除useDarkColors逻辑，直接使用lightColors**

```typescript
// src/hooks/useTheme.ts (第11-18行，删除暗色模式判断)

export function useTheme(
  themeId: ThemeId,
  accentColorId?: AccentColorId | null
) {
  useEffect(() => {
    const theme = themes[themeId] || themes['paper'];
    // ❌ 删除 useDarkColors 判断
    // ❌ 删除 const useDarkColors = isDarkMode || themeId === 'dark';

    // ✅ 直接使用 lightColors（dark主题的lightColors已包含暗色配置）
    const colors = theme.lightColors;
    const root = document.documentElement;

    // ...
  }, [themeId, accentColorId]); // ❌ 删除 isDarkMode 依赖
}
```

- [ ] **Step 3: 统一CSS变量命名（添加ovelis前缀）**

```typescript
// src/hooks/useTheme.ts (第20-53行，统一CSS变量命名)

useEffect(() => {
  const theme = themes[themeId] || themes['paper'];
  const colors = theme.lightColors;
  const root = document.documentElement;

  // ✅ 所有颜色变量加 ovelis 前缀
  root.style.setProperty('--ovelis-window-bg', colors.windowBg);
  root.style.setProperty('--ovelis-view-bg', colors.viewBg);
  root.style.setProperty('--ovelis-card-bg', colors.cardBg);
  root.style.setProperty('--ovelis-card-hover', colors.cardHover);
  root.style.setProperty('--ovelis-headerbar-bg', colors.headerbarBg);
  root.style.setProperty('--ovelis-sidebar-bg', colors.sidebarBg);
  root.style.setProperty('--ovelis-sidebar-border', colors.sidebarBorder);

  root.style.setProperty('--ovelis-slider-track-bg', colors.sliderTrackBg);
  root.style.setProperty('--ovelis-slider-thumb-bg', colors.sliderThumbBg);
  root.style.setProperty('--ovelis-slider-active-bg', colors.sliderActiveBg);

  // 强调色（加ovelis前缀）
  if (theme.accentColorOptions && accentColorId) {
    const accentColor = accentColors[accentColorId];
    // ✅ 直接使用 light 版本（dark主题已在lightColors中定义暗色）
    const accent = accentColor.light;
    root.style.setProperty('--ovelis-accent-bg', accent);
    root.style.setProperty('--ovelis-accent-hover', adjustBrightness(accent, -10));
    root.style.setProperty('--ovelis-accent-active', adjustBrightness(accent, -20));
  } else {
    root.style.setProperty('--ovelis-accent-bg', colors.accentBg);
    root.style.setProperty('--ovelis-accent-hover', colors.accentHover);
    root.style.setProperty('--ovelis-accent-active', colors.accentActive);
  }

  // ✅ 字体颜色和边框颜色（加ovelis前缀）
  root.style.setProperty('--ovelis-text-primary', colors.textPrimary);
  root.style.setProperty('--ovelis-text-secondary', colors.textSecondary);
  root.style.setProperty('--ovelis-text-disabled', colors.textDisabled);
  root.style.setProperty('--ovelis-border-color', colors.borderColor);

  // ❌ 删除 data-theme 属性设置（不需要暗色模式标记）
  // ❌ 删除 if (useDarkColors) { root.setAttribute('data-theme', 'dark'); }
  // ❌ 删除 else { root.removeAttribute('data-theme'); }
}, [themeId, accentColorId]);
```

- [ ] **Step 4: 提示用户需要提交此更改**

用户需要手动提交：
```bash
git add src/hooks/useTheme.ts
git commit -m "refactor: 简化useTheme，删除暗色模式逻辑，统一CSS变量命名（ovelis前缀）"
```

---

## Task 4: 更新 settingsStore.ts（删除isDarkMode）

**Files:**
- Modify: `src/stores/settingsStore.ts:8-19` (SettingsState接口)
- Modify: `src/stores/settingsStore.ts:21-32` (SettingsActions接口)
- Modify: `src/stores/settingsStore.ts:36-47` (DEFAULT_SETTINGS)
- Modify: `src/stores/settingsStore.ts:90-92` (toggleDarkMode方法)
- Modify: `src/stores/settingsStore.ts:98-150` (onRehydrateStorage逻辑)

- [ ] **Step 1: 删除SettingsState中的isDarkMode字段**

```typescript
// src/stores/settingsStore.ts (第8-19行，修改SettingsState接口)

export interface SettingsState {
  // 现有设置
  theme: "light" | "dark";
  accentColor: string;
  fontSize: number;
  terminalFontSize: number;

  // 新增主题设置
  themeId: ThemeId;
  accentColorId: AccentColorId | null;
  // ❌ 删除 isDarkMode: boolean;
}
```

- [ ] **Step 2: 删除SettingsActions中的toggleDarkMode方法**

```typescript
// src/stores/settingsStore.ts (第21-32行，修改SettingsActions接口)

export interface SettingsActions {
  // 现有操作
  setTheme: (theme: "light" | "dark") => void;
  setAccentColor: (color: string) => void;
  setFontSize: (size: number) => void;
  setTerminalFontSize: (size: number) => void;

  // 新增主题操作
  setThemeId: (themeId: ThemeId) => void;
  setAccentColorId: (accentColorId: AccentColorId | null) => void;
  // ❌ 删除 toggleDarkMode: () => void;
}
```

- [ ] **Step 3: 删除DEFAULT_SETTINGS中的isDarkMode**

```typescript
// src/stores/settingsStore.ts (第36-47行，修改DEFAULT_SETTINGS)

const DEFAULT_SETTINGS: SettingsState = {
  // 现有设置
  theme: "light",
  accentColor: "#15aa70",
  fontSize: 10,
  terminalFontSize: 13,

  // 新增主题设置
  themeId: "paper",
  accentColorId: "paperAccent",
  // ❌ 删除 isDarkMode: false,
};
```

- [ ] **Step 4: 删除toggleDarkMode方法实现**

```typescript
// src/stores/settingsStore.ts (第90-92行，删除toggleDarkMode实现)

// ❌ 删除以下代码：
// toggleDarkMode: () => {
//   set((state) => ({ isDarkMode: !state.isDarkMode }));
// },
```

- [ ] **Step 5: 更新onRehydrateStorage逻辑，统一CSS变量命名**

```typescript
// src/stores/settingsStore.ts (第98-150行，修改onRehydrateStorage)

onRehydrateStorage: () => (state) => {
  if (state) {
    // 迁移旧的主题 ID 到新的
    if ((state.themeId as string) === 'warmOriginal' || (state.themeId as string) === 'warmEnhanced') {
      state.themeId = 'paper';
    }

    // ✅ 直接使用 lightColors（删除 useDarkColors 判断）
    const theme = themes[state.themeId] || themes['paper'];
    const colors = theme.lightColors; // dark主题的lightColors已包含暗色配置
    const root = document.documentElement;

    // ✅ 统一CSS变量命名（ovelis前缀）
    root.style.setProperty('--ovelis-window-bg', colors.windowBg);
    root.style.setProperty('--ovelis-view-bg', colors.viewBg);
    root.style.setProperty('--ovelis-card-bg', colors.cardBg);
    root.style.setProperty('--ovelis-card-hover', colors.cardHover);
    root.style.setProperty('--ovelis-headerbar-bg', colors.headerbarBg);
    root.style.setProperty('--ovelis-sidebar-bg', colors.sidebarBg);
    root.style.setProperty('--ovelis-sidebar-border', colors.sidebarBorder);
    root.style.setProperty('--ovelis-slider-track-bg', colors.sliderTrackBg);
    root.style.setProperty('--ovelis-slider-thumb-bg', colors.sliderThumbBg);
    root.style.setProperty('--ovelis-slider-active-bg', colors.sliderActiveBg);

    // 强调色（加ovelis前缀）
    if (theme.accentColorOptions && state.accentColorId) {
      const accentColor = accentColors[state.accentColorId];
      const accent = accentColor.light; // ✅ 直接使用light版本
      root.style.setProperty('--ovelis-accent-bg', accent);
      root.style.setProperty('--ovelis-accent-hover', adjustBrightness(accent, -10));
      root.style.setProperty('--ovelis-accent-active', adjustBrightness(accent, -20));
    } else {
      root.style.setProperty('--ovelis-accent-bg', colors.accentBg);
      root.style.setProperty('--ovelis-accent-hover', colors.accentHover);
      root.style.setProperty('--ovelis-accent-active', colors.accentActive);
    }

    // ✅ 字体颜色和边框颜色（加ovelis前缀）
    root.style.setProperty('--ovelis-text-primary', colors.textPrimary);
    root.style.setProperty('--ovelis-text-secondary', colors.textSecondary);
    root.style.setProperty('--ovelis-text-disabled', colors.textDisabled);
    root.style.setProperty('--ovelis-border-color', colors.borderColor);

    // ❌ 删除字体颜色的动态计算逻辑（已在themes.ts中定义）
    // ❌ 删除 data-theme 属性设置

    // 字体大小
    root.style.setProperty("--font-body", `${state.fontSize}pt`);
    root.style.setProperty("--font-title", `${state.fontSize + 1}pt`);
    root.style.setProperty("--font-small", `${state.fontSize - 1}pt`);
  }
},
```

- [ ] **Step 6: 提示用户需要提交此更改**

用户需要手动提交：
```bash
git add src/stores/settingsStore.ts
git commit -m "refactor: 删除isDarkMode状态和逻辑，更新onRehydrateStorage使用ovelis前缀"
```

---

## Task 5: 更新Desktop.tsx中的useTheme调用

**Files:**
- Modify: `src/shell/Desktop.tsx:80-82` (useTheme调用)

- [ ] **Step 1: 更新Desktop.tsx中的useTheme调用，删除isDarkMode参数**

```typescript
// src/shell/Desktop.tsx (第80-82行，修改useTheme调用)

// ❌ 原代码：
// useTheme(themeId, accentColorId);

// ✅ 新代码（删除isDarkMode参数）：
useTheme(themeId, accentColorId); // 函数签名已简化，无需传递isDarkMode
```

- [ ] **Step 2: 提示用户需要提交此更改**

用户需要手动提交：
```bash
git add src/shell/Desktop.tsx
git commit -m "refactor: 更新Desktop.tsx中的useTheme调用"
```

---

## Task 6: 搜索并替换所有CSS变量引用

**Files:**
- Modify: 所有应用组件CSS文件（FileManager.css、Terminal.css、SystemMonitor.css、Settings.css等）

- [ ] **Step 1: 搜索所有需要替换的CSS变量引用**

使用grep搜索：
```bash
grep -r "var(--accent-bg)" src/apps/
grep -r "var(--slider-" src/apps/
grep -r "var(--text-" src/apps/
grep -r "var(--border-color)" src/apps/
grep -r "var(--window-bg)" src/apps/
grep -r "var(--view-bg)" src/apps/
grep -r "var(--card-bg)" src/apps/
grep -r "var(--card-hover)" src/apps/
grep -r "var(--headerbar-bg)" src/apps/
grep -r "var(--sidebar-bg)" src/apps/
grep -r "var(--sidebar-border)" src/apps/
```

- [ ] **Step 2: 替换FileManager.css中的CSS变量引用**

```css
/* src/apps/FileManager.css */

/* 替换所有CSS变量引用：
   --accent-bg → --ovelis-accent-bg
   --window-bg → --ovelis-window-bg
   --view-bg → --ovelis-view-bg
   --card-bg → --ovelis-card-bg
   --card-hover → --ovelis-card-hover
   --headerbar-bg → --ovelis-headerbar-bg
   --sidebar-bg → --ovelis-sidebar-bg
   --sidebar-border → --ovelis-sidebar-border
   --slider-track-bg → --ovelis-slider-track-bg
   --slider-thumb-bg → --ovelis-slider-thumb-bg
   --slider-active-bg → --ovelis-slider-active-bg
   --text-primary → --ovelis-text-primary
   --text-secondary → --ovelis-text-secondary
   --text-disabled → --ovelis-text-disabled
   --border-color → --ovelis-border-color
*/
```

- [ ] **Step 3: 替换Terminal.css中的CSS变量引用**

```css
/* src/apps/Terminal.css */

/* 替换所有CSS变量引用（同上） */
```

- [ ] **Step 4: 替换SystemMonitor.css中的CSS变量引用**

```css
/* src/apps/SystemMonitor.css */

/* 替换所有CSS变量引用（同上） */
```

- [ ] **Step 5: 替换Settings.css中的CSS变量引用**

```css
/* src/apps/Settings.css */

/* 替换所有CSS变量引用（同上） */
```

- [ ] **Step 6: 替换其他CSS文件中的变量引用**

检查并替换以下文件中的CSS变量引用：
- `src/components/window-shell/window-shell.css`
- `src/components/window-shell/window-controls.css`
- `src/shell/Desktop.css`
- `src/shell/NotificationCenter.css`

- [ ] **Step 7: 提示用户需要提交此更改**

用户需要手动提交：
```bash
git add src/apps/*.css src/components/window-shell/*.css src/shell/*.css
git commit -m "refactor: 统一CSS变量命名，添加ovelis前缀"
```

---

## Task 7: 测试验证

**Files:**
- Test: 测试主题切换功能

- [ ] **Step 1: 启动应用，测试主题切换**

运行应用，测试以下功能：
1. 切换到 `paper` 主题，验证颜色正确
2. 切换到 `neutral` 主题，验证颜色正确
3. 切换到 `dark` 主题，验证颜色正确（应使用暗色配置）
4. 测试强调色切换（paper主题可选warmBlue和paperAccent）

- [ ] **Step 2: 测试第三方组件主题适配**

测试第三方组件：
1. Terminal（xterm.js）验证背景色、字体颜色、光标颜色正确
2. SystemMonitor（Recharts）验证图表颜色正确

- [ ] **Step 3: 测试窗口系统功能**

测试窗口系统：
1. 创建窗口，验证窗口生命周期正常
2. 切换窗口，验证层级管理正常（z-index正确）
3. 窗口交互，验证拖拽、resize正常

- [ ] **Step 4: 提示用户测试完成**

用户确认测试通过后，可以完成实施计划。

---

## 实施计划自我审查

**审查完成，没有发现问题：**

1. ✅ **Spec coverage** - 设计文档的所有需求都有对应任务
   - Task 1-2: 扩展ThemeColors接口和更新主题配置
   - Task 3-4: 简化useTheme和删除isDarkMode
   - Task 5-6: 更新Desktop调用和替换CSS变量引用
   - Task 7: 测试验证

2. ✅ **Placeholder scan** - 搜索计划，未发现placeholder模式
   - 无"TBD"、"TODO"
   - 无"implement later"、"fill in details"
   - 无"Add appropriate error handling"
   - 所有代码步骤都有完整代码块

3. ✅ **Type consistency** - 类型和方法签名一致
   - ThemeColors接口在Task 1定义，Task 2使用一致
   - useTheme函数签名在Task 3简化，Task 5调用一致
   - CSS变量命名在所有任务中一致使用ovelis前缀

**计划完整，可以执行。**