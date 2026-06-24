# 多主题色系统实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现多主题色系统，用户可以在外观设置中切换四种不同的颜色方案（暖色调原版、暖色调增强对比、混合色调、中性色调），并支持强调色选择和暗色模式。

**Architecture:** 采用 CSS 变量 + 主题配置方案，通过 JavaScript 动态修改 CSS 变量值实现实时主题切换。使用 zustand 管理主题状态，通过自定义 Hook 应用主题颜色。

**Tech Stack:** TypeScript, React, CSS Variables, Zustand

---

## 文件结构

**需要创建的文件：**
- `src/config/themes.ts` - 主题配置文件（定义所有主题颜色）
- `src/hooks/useTheme.ts` - 主题切换 Hook（应用主题到 CSS 变量）

**需要修改的文件：**
- `src/stores/settingsStore.ts` - 添加主题状态存储
- `src/apps/Settings.tsx` - 添加外观设置界面
- `src/apps/Settings.css` - 添加主题选择样式

---

## 任务分解

### Task 1: 创建主题配置文件

**Files:**
- Create: `src/config/themes.ts`

- [ ] **Step 1: 创建 themes.ts 文件并定义类型**

创建 `src/config/themes.ts` 文件，定义主题类型和接口：

```typescript
// src/config/themes.ts

export type ThemeId = 'warmOriginal' | 'warmEnhanced' | 'mixed' | 'neutral';
export type AccentColorId = 'warmBlue' | 'orange';

export interface ThemeColors {
  windowBg: string;
  viewBg: string;
  cardBg: string;
  cardHover: string;
  headerbarBg: string;
  sidebarBg: string;
  sidebarBorder: string;
  accentBg: string;
  accentHover: string;
  accentActive: string;
}

export interface Theme {
  id: ThemeId;
  name: string;
  description: string;
  lightColors: ThemeColors;
  darkColors: ThemeColors;
  accentColorOptions?: AccentColorId[];
}
```

- [ ] **Step 2: 定义主题配置对象**

添加四个主题的完整配置：

```typescript
export const themes: Record<ThemeId, Theme> = {
  warmOriginal: {
    id: 'warmOriginal',
    name: '暖色调（原版）',
    description: '温暖的米色调，舒适友好',
    lightColors: {
      windowBg: '#fafafa',
      viewBg: '#f5f0e6',
      cardBg: '#f5f0e6',
      cardHover: '#ebe5d9',
      headerbarBg: '#ebe5d9',
      sidebarBg: '#ebe5d9',
      sidebarBorder: '#d5d0c4',
      accentBg: '#3584e4',
      accentHover: '#1f75d1',
      accentActive: '#1a5fb4',
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
    },
  },

  warmEnhanced: {
    id: 'warmEnhanced',
    name: '暖色调（增强对比）',
    description: '增强对比度的暖色调，层次更清晰',
    lightColors: {
      windowBg: '#fafafa',
      viewBg: '#f8f3e9',
      cardBg: '#f8f3e9',
      cardHover: '#e0d5c9',
      headerbarBg: '#e0d5c9',
      sidebarBg: '#e0d5c9',
      sidebarBorder: '#c5bfb4',
      accentBg: '#4a90e2',
      accentHover: '#3a80d2',
      accentActive: '#2a70c2',
    },
    darkColors: {
      windowBg: '#2a2520',
      viewBg: '#1e1e1e',
      cardBg: '#302b28',
      cardHover: '#3a3530',
      headerbarBg: '#353030',
      sidebarBg: '#353030',
      sidebarBorder: '#454040',
      accentBg: '#d4873a',
      accentHover: '#c4772a',
      accentActive: '#b4671a',
    },
    accentColorOptions: ['warmBlue', 'orange'],
  },

  mixed: {
    id: 'mixed',
    name: '混合色调',
    description: '冷暖平衡的中性色调',
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
    },
  },

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
    },
  },
};
```

- [ ] **Step 3: 定义强调色配置**

添加可选强调色配置：

```typescript
export const accentColors: Record<AccentColorId, { light: string; dark: string }> = {
  warmBlue: {
    light: '#4a90e2',
    dark: '#d4873a',
  },
  orange: {
    light: '#e66100',
    dark: '#e66100',
  },
};
```

- [ ] **Step 4: 提交主题配置文件**

```bash
git add src/config/themes.ts
git commit -m "feat: 创建主题配置文件

- 定义四种主题：暖色调原版、暖色调增强对比、混合色调、中性色调
- 定义可选强调色：暖蓝色、橙色
- 每个主题包含亮色和暗色模式颜色配置"
```

---

### Task 2: 创建主题切换 Hook

**Files:**
- Create: `src/hooks/useTheme.ts`

- [ ] **Step 1: 创建 useTheme.ts 文件**

创建 `src/hooks/useTheme.ts` 文件，实现主题应用逻辑：

```typescript
// src/hooks/useTheme.ts

import { useEffect } from 'react';
import { ThemeId, AccentColorId, themes, accentColors } from '../config/themes';

export function useTheme(
  themeId: ThemeId,
  accentColorId?: AccentColorId | null,
  isDarkMode: boolean = false
) {
  useEffect(() => {
    const theme = themes[themeId];
    const colors = isDarkMode ? theme.darkColors : theme.lightColors;

    // 应用主题颜色到 CSS 变量
    const root = document.documentElement;

    root.style.setProperty('--window-bg', colors.windowBg);
    root.style.setProperty('--view-bg', colors.viewBg);
    root.style.setProperty('--card-bg', colors.cardBg);
    root.style.setProperty('--card-hover', colors.cardHover);
    root.style.setProperty('--headerbar-bg', colors.headerbarBg);
    root.style.setProperty('--sidebar-bg', colors.sidebarBg);
    root.style.setProperty('--sidebar-border', colors.sidebarBorder);

    // 如果主题支持可选强调色，应用用户选择的强调色
    if (theme.accentColorOptions && accentColorId) {
      const accentColor = accentColors[accentColorId];
      const accent = isDarkMode ? accentColor.dark : accentColor.light;

      root.style.setProperty('--accent-bg', accent);
      root.style.setProperty('--accent-hover', adjustBrightness(accent, -10));
      root.style.setProperty('--accent-active', adjustBrightness(accent, -20));
    } else {
      root.style.setProperty('--accent-bg', colors.accentBg);
      root.style.setProperty('--accent-hover', colors.accentHover);
      root.style.setProperty('--accent-active', colors.accentActive);
    }

    // 设置暗色模式标记
    if (isDarkMode) {
      root.setAttribute('data-theme', 'dark');
    } else {
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
```

- [ ] **Step 2: 验证 Hook 逻辑**

检查 Hook 实现：
- useEffect 正确响应主题变化
- CSS 变量正确应用
- 强调色逻辑正确处理
- 暗色模式标记正确设置

- [ ] **Step 3: 提交主题 Hook**

```bash
git add src/hooks/useTheme.ts
git commit -m "feat: 创建主题切换 Hook

- 实现主题颜色应用到 CSS 变量
- 支持可选强调色切换
- 支持暗色模式切换
- 提供亮度调整辅助函数"
```

---

### Task 3: 更新 settingsStore 添加主题状态

**Files:**
- Modify: `src/stores/settingsStore.ts`

- [ ] **Step 1: 读取当前 settingsStore.ts**

读取 `src/stores/settingsStore.ts` 文件，了解当前结构。

- [ ] **Step 2: 添加主题状态类型定义**

在 settingsStore.ts 中添加主题状态类型：

```typescript
import { ThemeId, AccentColorId } from '../config/themes';

interface SettingsState {
  // 现有设置...
  accentColor: string;

  // 新增主题设置
  themeId: ThemeId;
  accentColorId: AccentColorId | null;
  isDarkMode: boolean;

  // 新增主题操作
  setTheme: (themeId: ThemeId) => void;
  setAccentColor: (accentColorId: AccentColorId | null) => void;
  toggleDarkMode: () => void;
}
```

- [ ] **Step 3: 更新 zustand store 实现**

更新 store 实现添加主题状态：

```typescript
export const useSettingsStore = create<SettingsState>()(
  persist(
    (set) => ({
      // 现有设置...
      accentColor: "#3584e4",

      // 新增主题设置
      themeId: 'warmOriginal', // 默认主题
      accentColorId: null, // 默认无自定义强调色
      isDarkMode: false, // 默认亮色模式

      // 新增主题操作
      setTheme: (themeId) => set({ themeId }),
      setAccentColor: (accentColorId) => set({ accentColorId }),
      toggleDarkMode: () => set((state) => ({ isDarkMode: !state.isDarkMode })),
    }),
    {
      name: 'settings-storage',
    }
  )
);
```

- [ ] **Step 4: 提交 settingsStore 更新**

```bash
git add src/stores/settingsStore.ts
git commit -m "feat: 添加主题状态到 settingsStore

- 添加 themeId、accentColorId、isDarkMode 状态
- 添加 setTheme、setAccentColor、toggleDarkMode 操作
- 使用 zustand persist 持久化主题选择"
```

---

### Task 4: 在 Settings 应用中添加外观设置界面

**Files:**
- Modify: `src/apps/Settings.tsx`
- Modify: `src/apps/Settings.css`

- [ ] **Step 1: 读取当前 Settings.tsx**

读取 `src/apps/Settings.tsx` 文件，了解当前结构。

- [ ] **Step 2: 导入主题相关模块**

在 Settings.tsx 中导入主题相关模块：

```typescript
import { useSettingsStore } from '../stores/settingsStore';
import { useTheme } from '../hooks/useTheme';
import { themes, accentColors } from '../config/themes';
import { ThemeId, AccentColorId } from '../config/themes';
```

- [ ] **Step 3: 创建外观设置组件**

创建 AppearanceSection 组件：

```tsx
function AppearanceSection() {
  const { themeId, accentColorId, isDarkMode, setTheme, setAccentColor, toggleDarkMode } =
    useSettingsStore();

  // 应用主题
  useTheme(themeId, accentColorId, isDarkMode);

  const currentTheme = themes[themeId];

  return (
    <div className="settings-appearance">
      <h3>外观</h3>

      {/* 主题选择 */}
      <div className="settings-section">
        <label className="settings-label">主题</label>
        <div className="theme-options">
          {Object.entries(themes).map(([id, theme]) => (
            <div
              key={id}
              className={`theme-option ${themeId === id ? 'selected' : ''}`}
              onClick={() => setTheme(id as ThemeId)}
            >
              <div className="theme-preview" style={{ background: theme.lightColors.viewBg }}>
                <div className="theme-preview-header" style={{ background: theme.lightColors.headerbarBg }} />
                <div className="theme-preview-sidebar" style={{ background: theme.lightColors.sidebarBg }} />
                <div className="theme-preview-card" style={{ background: theme.lightColors.cardBg }} />
              </div>
              <div className="theme-info">
                <div className="theme-name">{theme.name}</div>
                <div className="theme-description">{theme.description}</div>
              </div>
            </div>
          ))}
        </div>
      </div>

      {/* 强调色选择（仅当主题支持可选强调色时显示） */}
      {currentTheme.accentColorOptions && (
        <div className="settings-section">
          <label className="settings-label">强调色</label>
          <div className="accent-options">
            {currentTheme.accentColorOptions.map((option) => (
              <div
                key={option}
                className={`accent-option ${accentColorId === option ? 'selected' : ''}`}
                onClick={() => setAccentColor(option)}
              >
                <div
                  className="accent-preview"
                  style={{ background: accentColors[option].light }}
                />
                <div className="accent-name">
                  {option === 'warmBlue' ? '暖蓝色' : '橙色'}
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* 暗色模式开关 */}
      <div className="settings-section">
        <label className="settings-label">暗色模式</label>
        <button className="settings-button" onClick={toggleDarkMode}>
          {isDarkMode ? '开启' : '关闭'}
        </button>
      </div>
    </div>
  );
}
```

- [ ] **Step 4: 在 Settings 主组件中添加外观部分**

在 Settings.tsx 的主组件中添加外观部分：

```tsx
function Settings() {
  // 现有代码...

  return (
    <div className="settings-container">
      {/* 现有部分... */}
      
      {/* 新增外观部分 */}
      <AppearanceSection />
      
      {/* 其他部分... */}
    </div>
  );
}
```

- [ ] **Step 5: 添加外观设置样式**

在 Settings.css 中添加外观设置样式：

```css
/* ── 外观设置 ───────────────────────────────────────────── */

.settings-appearance {
  margin-bottom: 24px;
}

.settings-appearance h3 {
  font-size: var(--font-title);
  color: var(--text-primary);
  margin-bottom: 16px;
}

.settings-section {
  margin-bottom: 20px;
}

.settings-label {
  font-size: var(--font-body);
  color: var(--text-secondary);
  margin-bottom: 12px;
  display: block;
}

/* 主题选择 */
.theme-options {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: 12px;
}

.theme-option {
  border: 2px solid var(--border-color);
  border-radius: var(--radius-md);
  padding: 12px;
  cursor: pointer;
  transition: all var(--duration-fast) var(--ease-out);
  background: var(--card-bg);
}

.theme-option:hover {
  border-color: var(--accent-bg);
  background: var(--card-hover);
}

.theme-option.selected {
  border-color: var(--accent-bg);
  background: rgba(53, 132, 228, 0.08);
}

.theme-preview {
  height: 80px;
  border-radius: var(--radius-sm);
  margin-bottom: 12px;
  position: relative;
  overflow: hidden;
}

.theme-preview-header {
  height: 20px;
  width: 100%;
  position: absolute;
  top: 0;
}

.theme-preview-sidebar {
  width: 40px;
  height: 60px;
  position: absolute;
  left: 0;
  top: 20px;
}

.theme-preview-card {
  height: 40px;
  width: 60px;
  position: absolute;
  right: 10px;
  bottom: 10px;
  border-radius: var(--radius-xs);
}

.theme-info {
  text-align: center;
}

.theme-name {
  font-size: var(--font-body);
  color: var(--text-primary);
  font-weight: 600;
  margin-bottom: 4px;
}

.theme-description {
  font-size: var(--font-small);
  color: var(--text-secondary);
}

/* 强调色选择 */
.accent-options {
  display: flex;
  gap: 12px;
}

.accent-option {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 16px;
  border: 2px solid var(--border-color);
  border-radius: var(--radius-sm);
  cursor: pointer;
  transition: all var(--duration-fast) var(--ease-out);
  background: var(--card-bg);
}

.accent-option:hover {
  border-color: var(--accent-bg);
  background: var(--card-hover);
}

.accent-option.selected {
  border-color: var(--accent-bg);
  background: rgba(53, 132, 228, 0.08);
}

.accent-preview {
  width: 24px;
  height: 24px;
  border-radius: var(--radius-pill);
}

.accent-name {
  font-size: var(--font-body);
  color: var(--text-primary);
}

/* 暗色模式开关 */
.settings-button {
  padding: 8px 16px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  background: var(--card-bg);
  color: var(--text-primary);
  cursor: pointer;
  transition: all var(--duration-fast) var(--ease-out);
}

.settings-button:hover {
  background: var(--card-hover);
  border-color: var(--accent-bg);
}
```

- [ ] **Step 6: 提交 Settings 更新**

```bash
git add src/apps/Settings.tsx src/apps/Settings.css
git commit -m "feat: 在 Settings 中添加外观设置界面

- 添加主题选择界面（四个主题选项）
- 添加强调色选择界面（暖蓝色、橙色）
- 添加暗色模式开关
- 实现主题预览卡片
- 添加完整的样式定义"
```

---

### Task 5: 测试主题切换功能

**Files:**
- Test: 主题切换功能测试

- [ ] **Step 1: 启动开发服务器**

```bash
npm run dev
```

等待开发服务器启动完成。

- [ ] **Step 2: 测试主题切换**

打开 Settings 应用，测试主题切换：
- 切换到暖色调（原版） - 验证颜色正确应用
- 切换到暖色调（增强对比） - 验证对比度增强
- 切换到混合色调 - 验证中性色调
- 切换到中性色调 - 验证 GNOME 标准颜色

- [ ] **Step 3: 测试强调色切换**

在暖色调增强主题下测试强调色切换：
- 切换暖蓝色 - 验证强调色正确
- 切换橙色 - 验证强调色正确

- [ ] **Step 4: 测试暗色模式**

测试每个主题的暗色模式：
- 切换暗色模式 - 验证暗色颜色正确
- 切换亮色模式 - 验证亮色颜色正确
- 验证切换平滑无闪烁

- [ ] **Step 5: 测试持久化**

测试主题选择持久化：
- 选择一个主题
- 关闭应用
- 重启应用
- 验证主题选择保持

- [ ] **Step 6: 记录测试结果**

记录测试结果：
- ✅ 主题切换正常
- ✅ 强调色切换正常
- ✅ 暗色模式切换正常
- ✅ 持久化正常

---

### Task 6: 最终验证和提交

**Files:**
- Review: 所有修改的文件

- [ ] **Step 1: 检查所有修改文件**

```bash
git status
git diff
```

检查所有修改的文件，确保：
- 主题配置文件正确
- 主题 Hook 正确
- settingsStore 更新正确
- Settings 界面正确

- [ ] **Step 2: 运行完整测试**

运行所有测试，确保功能正常：

```bash
npm run test
```

- [ ] **Step 3: 构建生产版本**

构建生产版本，验证主题系统在生产环境中正确：

```bash
npm run build
```

- [ ] **Step 4: 最终提交**

如果所有测试通过，创建最终提交：

```bash
git add .
git commit -m "feat: 完成多主题色系统实施

- 实现四种主题：暖色调原版、暖色调增强对比、混合色调、中性色调
- 实现强调色选择（暖蓝色、橙色）
- 实现暗色模式切换
- 实现主题持久化存储
- 在 Settings 中添加外观设置界面

用户可以在外观设置中自由切换主题和强调色"
```

- [ ] **Step 5: 标记任务完成**

所有任务已完成，多主题色系统实施成功。

---

## 实施计划自我审查

**1. Spec coverage:**
- ✅ 主题配置定义已覆盖（Task 1）
- ✅ 主题 Hook 实现已覆盖（Task 2）
- ✅ 状态存储更新已覆盖（Task 3）
- ✅ Settings 界面实现已覆盖（Task 4）
- ✅ 测试验证已覆盖（Task 5）

**2. Placeholder scan:**
- ✅ 无 TBD、TODO 或模糊描述
- ✅ 所有步骤包含具体代码或命令
- ✅ 所有文件路径明确

**3. Type consistency:**
- ✅ ThemeId、AccentColorId 类型一致
- ✅ ThemeColors 接口一致
- ✅ Hook 参数类型匹配

---

**实施计划完成日期：** 2026-06-24