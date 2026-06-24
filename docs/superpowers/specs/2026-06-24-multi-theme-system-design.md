# 多主题色系统设计文档

> 版本: v1.0 | 日期: 2026-06-24
>
> 目标：实现多主题色系统，用户可以在外观设置中切换不同的颜色方案，包括原版暖色调、增强对比度暖色调、混合色调和中性色调。

---

## 一、设计背景

### 1.1 当前问题

- 单一颜色方案：用户无法选择不同的颜色风格
- 暖色调可能不适合所有用户：部分用户可能更喜欢中性色调或冷色调
- 缺乏个性化：无法根据个人喜好调整颜色方案
- 风格不一致：亮色模式使用暖色调，暗色模式使用冷色调，风格不统一

### 1.2 设计目标

- **多主题支持**：提供多种颜色方案供用户选择
- **实时切换**：无需重新加载，实时切换主题
- **易于扩展**：可以轻松添加新主题
- **持久化存储**：主题选择应保存到本地存储
- **暗色模式兼容**：每个主题都应有对应的暗色模式

---

## 二、主题方案定义

### 2.1 主题 0：暖色调（原版）

**亮色模式：**

| 变量名 | 颜色值 | RGB | 用途 |
|--------|--------|-----|------|
| `--window-bg` | `#fafafa` | (250,250,250) | 主窗口背景 |
| `--view-bg` | `#f5f0e6` | (245,240,230) | 内容区背景（暖灰） |
| `--card-bg` | `#f5f0e6` | (245,240,230) | 卡片背景（暖灰） |
| `--card-hover` | `#ebe5d9` | (235,229,217) | 悬停状态（中暖灰） |
| `--headerbar-bg` | `#ebe5d9` | (235,229,217) | HeaderBar 背景（中暖灰） |
| `--sidebar-bg` | `#ebe5d9` | (235,229,217) | 侧边栏背景（中暖灰） |
| `--sidebar-border` | `#d5d0c4` | (213,208,196) | 边框、分隔线（深暖灰） |
| `--accent-bg` | `#3584e4` | (53,132,228) | 强调色背景（GNOME Blue） |

**暗色模式：**

保持原有暗色模式颜色（冷色调），风格对比。

### 2.2 主题 1：暖色调（增强对比）

**亮色模式：**

| 变量名 | 颜色值 | RGB | 用途 |
|--------|--------|-----|------|
| `--window-bg` | `#fafafa` | (250,250,250) | 主窗口背景 |
| `--view-bg` | `#f8f3e9` | (248,243,233) | 内容区背景（浅暖灰） |
| `--card-bg` | `#f8f3e9` | (248,243,233) | 卡片背景（浅暖灰） |
| `--card-hover` | `#e0d5c9` | (224,213,201) | 悬停状态（深暖灰） |
| `--headerbar-bg` | `#e0d5c9` | (224,213,201) | HeaderBar 背景（深暖灰） |
| `--sidebar-bg` | `#e0d5c9` | (224,213,201) | 侧边栏背景（深暖灰） |
| `--sidebar-border` | `#c5bfb4` | (197,191,180) | 边框、分隔线（更深暖灰） |
| `--accent-bg` | `#4a90e2` 或 `#e66100` | - | 强调色（暖蓝或橙色） |

**暗色模式：**

使用暖色调暗色方案：

| 变量名 | 颜色值 | 用途 |
|--------|--------|------|
| `--window-bg` | `#2a2520` | 主窗口背景（暖黑） |
| `--view-bg` | `#1e1e1e` | 内容区背景 |
| `--card-bg` | `#302b28` | 卡片背景（暖灰） |
| `--card-hover` | `#3a3530` | 悬停状态（暖灰） |
| `--headerbar-bg` | `#353030` | HeaderBar 背景（暖灰） |
| `--sidebar-bg` | `#353030` | 侧边栏背景（暖灰） |
| `--sidebar-border` | `#454040` | 边框、分隔线（暖灰） |
| `--accent-bg` | `#d4873a` 或 `#e66100` | 强调色（暖橙） |

### 2.3 主题 2：混合色调（冷暖平衡）

**亮色模式：**

| 变量名 | 颜色值 | RGB | 用途 |
|--------|--------|-----|------|
| `--window-bg` | `#fafafa` | (250,250,250) | 主窗口背景 |
| `--view-bg` | `#f5f5f5` | (245,245,245) | 内容区背景（中性灰） |
| `--card-bg` | `#f5f5f5` | (245,245,245) | 卡片背景（中性灰） |
| `--card-hover` | `#e8e8e8` | (232,232,232) | 悬停状态（中性灰） |
| `--headerbar-bg` | `#e8e8e8` | (232,232,232) | HeaderBar 背景（中性灰） |
| `--sidebar-bg` | `#e8e8e8` | (232,232,232) | 侧边栏背景（中性灰） |
| `--sidebar-border` | `#d0d0d0` | (208,208,208) | 边框、分隔线（中性灰） |
| `--accent-bg` | `#3584e4` | (53,132,228) | 强调色（GNOME Blue） |

**暗色模式：**

保持原有暗色模式颜色（冷色调），形成风格对比。

### 2.4 主题 3：中性色调（GNOME 标准）

**亮色模式：**

| 变量名 | 颜色值 | RGB | 用途 |
|--------|--------|-----|------|
| `--window-bg` | `#fafafa` | (250,250,250) | 主窗口背景 |
| `--view-bg` | `#f5f5f5` | (245,245,245) | 内容区背景（中性灰） |
| `--card-bg` | `#f5f5f5` | (245,245,245) | 卡片背景（中性灰） |
| `--card-hover` | `#e8e8e8` | (232,232,232) | 悬停状态（中性灰） |
| `--headerbar-bg` | `#e8e8e8` | (232,232,232) | HeaderBar 背景（中性灰） |
| `--sidebar-bg` | `#e8e8e8` | (232,232,232) | 侧边栏背景（中性灰） |
| `--sidebar-border` | `#d0d0d0` | (208,208,208) | 边框、分隔线（中性灰） |
| `--accent-bg` | `#3584e4` | (53,132,228) | 强调色（GNOME Blue） |

**暗色模式：**

保持原有暗色模式颜色（冷色调），符合 GNOME 标准。

---

## 三、技术实现方案

### 3.1 方案选择

**采用方案 C：CSS 变量 + 主题配置**

理由：
- 最灵活 - 可以实时切换，无需重新加载
- 符合现代 CSS 最佳实践
- 易于扩展 - 可以轻松添加新主题
- 性能最优 - 无需额外文件加载

### 3.2 架构设计

```
主题系统架构：
├─ src/styles/adwaita.css (基础变量定义)
├─ src/config/themes.ts (主题配置文件)
├─ src/hooks/useTheme.ts (主题切换 Hook)
├─ src/context/ThemeContext.tsx (主题 Context)
├─ src/apps/Settings.tsx (主题选择界面)
└─ src/stores/settingsStore.ts (主题状态存储)
```

### 3.3 核心组件设计

#### 3.3.1 主题配置文件 (themes.ts)

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
  accentColorOptions?: AccentColorId[]; // 可选强调色
}

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
      accentBg: '#4a90e2', // 默认暖蓝，可切换为橙色
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
      accentBg: '#d4873a', // 默认暖橙，可切换为暖蓝
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

#### 3.3.2 主题 Hook (useTheme.ts)

```typescript
// src/hooks/useTheme.ts

import { useEffect } from 'react';
import { ThemeId, AccentColorId, themes, accentColors } from '../config/themes';

export function useTheme(
  themeId: ThemeId,
  accentColorId?: AccentColorId,
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
      // 计算悬停和激活色（略微深一点）
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

#### 3.3.3 主题状态存储 (settingsStore.ts)

```typescript
// src/stores/settingsStore.ts

import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import { ThemeId, AccentColorId } from '../config/themes';

interface SettingsState {
  // 主题设置
  themeId: ThemeId;
  accentColorId: AccentColorId | null;
  isDarkMode: boolean;

  // 主题操作
  setTheme: (themeId: ThemeId) => void;
  setAccentColor: (accentColorId: AccentColorId) => void;
  toggleDarkMode: () => void;
}

export const useSettingsStore = create<SettingsState>()(
  persist(
    (set) => ({
      themeId: 'warmOriginal', // 默认主题
      accentColorId: null, // 默认无自定义强调色
      isDarkMode: false, // 默认亮色模式

      setTheme: (themeId) => set({ themeId }),
      setAccentColor: (accentColorId) => set({ accentColorId }),
      toggleDarkMode: () => set((state) => ({ isDarkMode: !state.isDarkMode })),
    }),
    {
      name: 'settings-storage', // localStorage key
    }
  )
);
```

---

## 四、用户界面设计

### 4.1 Settings 应用主题选择界面

在 Settings 应用中添加"外观"部分：

```tsx
// src/apps/Settings.tsx (新增外观部分)

import { useSettingsStore } from '../stores/settingsStore';
import { useTheme } from '../hooks/useTheme';
import { themes } from '../config/themes';

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
      <div className="settings-theme-selector">
        <label>主题</label>
        <div className="theme-options">
          {Object.entries(themes).map(([id, theme]) => (
            <div
              key={id}
              className={`theme-option ${themeId === id ? 'selected' : ''}`}
              onClick={() => setTheme(id as ThemeId)}
            >
              <div className="theme-preview" style={{ background: theme.lightColors.viewBg }}>
                {/* 主题预览 */}
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
        <div className="settings-accent-selector">
          <label>强调色</label>
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
      <div className="settings-dark-mode">
        <label>暗色模式</label>
        <button onClick={toggleDarkMode}>
          {isDarkMode ? '开启' : '关闭'}
        </button>
      </div>
    </div>
  );
}
```

### 4.2 界面布局

```
设置页面：
├─ 连接服务器配置
├─ 外观（新增）
│   ├─ 主题选择
│   │   ├─ 暖色调（原版） - 预览卡片
│   │   ├─ 暖色调（增强对比） - 预览卡片
│   │   ├─ 混合色调 - 预览卡片
│   │   └─ 中性色调（GNOME 标准） - 预览卡片
│   ├─ 强调色选择（仅暖色调增强）
│   │   ├─ 暖蓝色 - 预览圆点
│   │   └─ 橙色 - 预览圆点
│   └─ 暗色模式开关
├─ 快捷键
└─ 其他设置
```

---

## 五、实施步骤

### 5.1 文件创建和修改

**需要创建的文件：**
1. `src/config/themes.ts` - 主题配置文件
2. `src/hooks/useTheme.ts` - 主题切换 Hook

**需要修改的文件：**
1. `src/styles/adwaita.css` - 移除硬编码颜色，保留基础变量定义
2. `src/stores/settingsStore.ts` - 添加主题状态存储
3. `src/apps/Settings.tsx` - 添加外观设置界面
4. `src/apps/Settings.css` - 添加主题选择样式

### 5.2 实施顺序

**阶段 1：基础架构**
1. 创建主题配置文件 `themes.ts`
2. 创建主题 Hook `useTheme.ts`
3. 更新 `settingsStore.ts` 添加主题状态

**阶段 2：UI 实现**
4. 在 Settings 应用中添加外观部分
5. 实现主题选择界面
6. 实现强调色选择界面
7. 实现暗色模式开关

**阶段 3：测试和优化**
8. 测试主题切换功能
9. 测试暗色模式切换
10. 测试强调色切换
11. 优化用户体验

---

## 六、测试验证

### 6.1 功能测试

**主题切换测试：**
- 切换到暖色调（原版） - 验证颜色正确应用
- 切换到暖色调（增强对比） - 验证对比度增强
- 切换到混合色调 - 验证中性色调
- 切换到中性色调 - 验证 GNOME 标准颜色

**强调色测试：**
- 暖色调增强主题下切换暖蓝色 - 验证强调色正确
- 暖色调增强主题下切换橙色 - 验证强调色正确

**暗色模式测试：**
- 每个主题切换暗色模式 - 验证暗色颜色正确
- 亮暗模式切换平滑 - 验证动画和过渡

**持久化测试：**
- 保存主题选择 - 验证重启后主题保持
- 保存强调色选择 - 验证重启后强调色保持
- 保存暗色模式状态 - 验证重启后模式保持

### 6.2 视觉测试

**对比度测试：**
- 每个主题的文字对比度符合 WCAG AA 标准
- 强调色对比度符合 WCAG AA 标准

**一致性测试：**
- 所有组件颜色一致
- 亮色和暗色模式风格统一（针对暖色调增强）

---

## 七、后续扩展

### 7.1 可扩展性

**添加新主题：**
1. 在 `themes.ts` 中添加新主题配置
2. 在 Settings 界面中添加新主题选项
3. 无需修改其他代码

**添加新强调色：**
1. 在 `accentColors` 中添加新强调色配置
2. 在主题配置中添加 `accentColorOptions`
3. 在 Settings 界面中自动显示

### 7.2 未来功能

**自定义主题：**
- 允许用户自定义颜色值
- 保存自定义主题到本地
- 导入/导出主题配置

**主题预览：**
- 在选择前提供实时预览
- 显示不同组件的颜色效果

---

## 八、风险评估

### 8.1 潜在风险

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| CSS 变量兼容性问题 | 部分旧浏览器不支持 | 使用 fallback 颜色值 |
| 主题切换延迟 | 用户体验下降 | 优化 Hook 性能，使用 useEffect |
| 持久化失败 | 重启后主题丢失 | 使用 zustand persist，添加错误处理 |
| 暗色模式不一致 | 视觉体验差 | 为每个主题设计对应的暗色方案 |

### 8.2 回退方案

如果多主题系统出现问题：
- 保留原版暖色调主题作为默认
- 可以快速禁用其他主题
- 提供重置到默认主题的功能

---

## 九、成功标准

### 9.1 完成标准

- ✅ 四个主题全部实现并可切换
- ✅ 强调色选择功能正常工作
- ✅ 暗色模式切换平滑
- ✅ 主题选择持久化存储
- ✅ 所有主题颜色符合 WCAG AA 标准
- ✅ Settings 界面提供清晰的主题选择

### 9.2 验收标准

- 用户可以在 Settings 中轻松切换主题
- 主题切换实时生效，无需重新加载
- 所有组件颜色一致协调
- 暗色模式正常工作
- 重启后主题选择保持

---

**设计文档完成日期：** 2026-06-24