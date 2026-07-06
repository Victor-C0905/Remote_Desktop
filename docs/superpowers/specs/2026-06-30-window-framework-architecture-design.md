# 窗口框架架构设计文档

> 日期: 2026-06-30
> 状态: 待批准
> 作者: AI Assistant

---

## 一、设计目标

### 1.1 核心需求

基于Web UI开发场景，整合现有架构，实现：

- **窗口管理系统**：完整的窗口生命周期、层级管理、交互控制
- **主题系统整合**：删除暗色模式切换，统一主题色变量命名规范
- **应用完全独立**：应用自由使用第三方组件，自主决定主题色使用
- **插件式扩展**：新应用遵循AppDefinition接口规范接入

### 1.2 现有架构基础

项目已具备完善的窗口管理系统：

- **WindowManager**：窗口管理器核心（生命周期、层级、持久化）
- **WindowRegistry**：应用注册表（AppDefinition接口）
- **WindowShell**：窗口交互层（拖拽、resize、聚焦）
- **ThemeSystem**：主题配置系统（themes.ts、settingsStore.ts、useTheme.ts）

**整合目标：** 基于现有架构，整合主题系统到窗口系统层，统一CSS变量命名规范。

---

## 二、架构设计

### 2.1 整合架构图

```
┌─ 窗口系统层（整合现有架构）─────────────────────────────┐
│                                                         │
│  ┌─ WindowManager（已完善，无需修改）─────────────┐  │
│  │  - 窗口生命周期管理                              │  │
│  │  - 窗口层级管理                                  │  │
│  │  - 窗口状态持久化                                │  │
│  │  - AppDefinition接口（无需添加主题配置）         │  │
│  └──────────────────────────────────────────────────┘  │
│                                                         │
│  ┌─ ThemeManager（整合现有主题系统）──────────────┐  │
│  │  - 整合 themes.ts（主题配置数据源）              │  │
│  │  - 整合 settingsStore.ts（主题状态管理）         │  │
│  │  - 整合 useTheme.ts（主题应用逻辑）              │  │
│  │  - 统一CSS变量命名（ovelis前缀）                 │  │
│  │  - 删除暗色模式切换逻辑                          │  │
│  └──────────────────────────────────────────────────┘  │
│                                                         │
│  ┌─ InteractionManager（已完善：WindowShell）─────┐  │
│  │  - WindowShell（拖拽、resize、聚焦）             │  │
│  │  - WindowControls（窗口控制按钮）                │  │
│  └──────────────────────────────────────────────────┘  │
│                                                         │
│  ┌─ CommunicationManager（整合现有）───────────────┐  │
│  │  - WindowChannel（窗口通信）                     │  │
│  │  - WindowEventBus（窗口事件总线）                │  │
│  │  - Tauri Event System                            │  │
│  └──────────────────────────────────────────────────┘  │
│                                                         │
└─────────────────────────────────────────────────────────┘

┌─ 应用层（完全独立）─────────────────────────────────────┐
│                                                         │
│  AppDefinition接口（无需修改）                          │
│    - id: string                                        │
│    - title: string                                     │
│    - icon: string                                      │
│    - defaultSize: { width, height }                    │
│    - minSize: { width, height }                        │
│    - allowMultipleInstances: boolean                   │
│    - lifecycle?: WindowLifecycle                       │
│    - component: React.ComponentType                    │
│                                                         │
│  应用自由：                                             │
│    - 完全自定义布局                                     │
│    - 自由集成第三方组件（xterm.js、Recharts）          │
│    - 自主决定使用主题色变量                             │
│    - 使用窗口通信API                                    │
│                                                         │
└─────────────────────────────────────────────────────────┘
```

---

## 三、主题系统整合设计

### 3.1 删除暗色模式切换

**删除内容：**
- ❌ `isDarkMode` 参数（useTheme.ts、settingsStore.ts）
- ❌ `useDarkColors` 逻辑（不再判断暗色模式）
- ❌ `data-theme` 属性设置（不需要暗色模式标记）
- ❌ 字体颜色的动态计算逻辑

**保留内容：**
- ✅ 主题选择（paper/neutral/dark）
- ✅ `dark` 主题作为其中一个主题选择（而非模式切换）

### 3.2 统一CSS变量命名规范

**命名规范：** 所有主题系统颜色变量使用 `ovelis` 前缀。

**CSS变量列表：**

```css
/* 窗口/视图背景 */
--ovelis-window-bg
--ovelis-view-bg

/* 卡片 */
--ovelis-card-bg
--ovelis-card-hover

/* HeaderBar/侧边栏 */
--ovelis-headerbar-bg
--ovelis-sidebar-bg
--ovelis-sidebar-border

/* 强调色 */
--ovelis-accent-bg
--ovelis-accent-hover
--ovelis-accent-active

/* 滑块 */
--ovelis-slider-track-bg
--ovelis-slider-thumb-bg
--ovelis-slider-active-bg

/* 字体颜色 */
--ovelis-text-primary
--ovelis-text-secondary
--ovelis-text-disabled

/* 边框颜色 */
--ovelis-border-color
```

### 3.3 扩展主题配置

**扩展 `ThemeColors` 接口：**

```typescript
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
  sliderTrackBg: string;
  sliderThumbBg: string;
  sliderActiveBg: string;

  // 新增：字体颜色和边框颜色
  textPrimary: string;
  textSecondary: string;
  textDisabled: string;
  borderColor: string;
}
```

**更新主题配置：**

```typescript
export const themes: Record<ThemeId, Theme> = {
  paper: {
    lightColors: {
      // ... 原有颜色
      textPrimary: 'rgba(0, 0, 0, 0.87)',
      textSecondary: 'rgba(0, 0, 0, 0.60)',
      textDisabled: 'rgba(0, 0, 0, 0.38)',
      borderColor: 'rgba(0, 0, 0, 0.15)',
    },
    darkColors: {
      // ... 原有颜色
      textPrimary: 'rgba(255, 255, 255, 0.87)',
      textSecondary: 'rgba(255, 255, 255, 0.60)',
      textDisabled: 'rgba(255, 255, 255, 0.38)',
      borderColor: 'rgba(255, 255, 255, 0.12)',
    },
  },
  neutral: {
    // ... 同样结构
  },
  dark: {
    lightColors: {
      // ... 暗色配置（始终使用暗色）
      textPrimary: 'rgba(255, 255, 255, 0.87)',
      // ...
    },
    darkColors: {
      // ... 与 lightColors 相同
    },
  },
};
```

### 3.4 简化 `useTheme.ts`

**删除暗色模式逻辑：**

```typescript
export function useTheme(
  themeId: ThemeId,
  accentColorId?: AccentColorId | null
) {
  useEffect(() => {
    const theme = themes[themeId] || themes['paper'];
    const colors = theme.lightColors; // 直接使用 lightColors
    const root = document.documentElement;

    // 所有颜色变量加 ovelis 前缀
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

    // 强调色
    if (theme.accentColorOptions && accentColorId) {
      const accentColor = accentColors[accentColorId];
      const accent = accentColor.light; // 直接使用 light 版本
      root.style.setProperty('--ovelis-accent-bg', accent);
      root.style.setProperty('--ovelis-accent-hover', adjustBrightness(accent, -10));
      root.style.setProperty('--ovelis-accent-active', adjustBrightness(accent, -20));
    } else {
      root.style.setProperty('--ovelis-accent-bg', colors.accentBg);
      root.style.setProperty('--ovelis-accent-hover', colors.accentHover);
      root.style.setProperty('--ovelis-accent-active', colors.accentActive);
    }

    // 字体颜色和边框颜色（加前缀）
    root.style.setProperty('--ovelis-text-primary', colors.textPrimary);
    root.style.setProperty('--ovelis-text-secondary', colors.textSecondary);
    root.style.setProperty('--ovelis-text-disabled', colors.textDisabled);
    root.style.setProperty('--ovelis-border-color', colors.borderColor);
  }, [themeId, accentColorId]);
}
```

---

## 四、应用层设计

### 4.1 AppDefinition接口（无需修改）

应用注册接口保持不变，无需添加主题配置字段。

```typescript
export interface AppDefinition {
  id: string;
  title: string;
  icon: string;
  defaultSize: { width: number; height: number };
  minSize: { width: number; height: number };
  allowMultipleInstances: boolean;
  lifecycle?: WindowLifecycle;
  component: React.ComponentType<{ windowId: string; preloadData?: any }>;
}
```

### 4.2 应用使用主题色变量

应用自主决定使用哪些主题色变量：

```tsx
// Terminal应用示例
function TerminalInstance() {
  const terminal = new Terminal({
    theme: {
      background: 'var(--ovelis-window-bg)', // 使用主题色
      foreground: '#ffffff', // 使用自己的颜色
      cursor: 'var(--ovelis-accent-bg)', // 使用主题强调色
    },
  });
}

// FileManager应用示例
function FileManager() {
  return (
    <div style={{ background: 'var(--ovelis-headerbar-bg)' }}>
      <button style={{ background: 'var(--ovelis-accent-bg)' }}>
        新建
      </button>
      <button style={{ background: '#ff6b6b' }}>删除</button>
    </div>
  );
}
```

---

## 五、实施计划

### 5.1 第一阶段：主题系统整合

**任务列表：**

1. **扩展 `ThemeColors` 接口**
   - 添加 `textPrimary`、`textSecondary`、`textDisabled`、`borderColor` 字段
   - 文件：`src/config/themes.ts`

2. **更新主题配置**
   - 为 paper/neutral/dark 主题添加字体和边框颜色
   - 文件：`src/config/themes.ts`

3. **简化 `useTheme.ts`**
   - 删除 `isDarkMode` 参数
   - 删除 `useDarkColors` 逻辑
   - 统一CSS变量命名（ovelis前缀）
   - 文件：`src/hooks/useTheme.ts`

4. **更新 `settingsStore.ts`**
   - 删除 `isDarkMode` 状态和操作
   - 删除 `toggleDarkMode` 方法
   - 更新 `onRehydrateStorage` 逻辑
   - 文件：`src/stores/settingsStore.ts`

5. **更新全局CSS变量引用**
   - 替换所有 `--accent-bg` → `--ovelis-accent-bg`
   - 替换所有 `--slider-*` → `--ovelis-slider-*`
   - 替换所有 `--text-*` → `--ovelis-text-*`
   - 替换所有 `--border-color` → `--ovelis-border-color`
   - 文件：所有应用组件CSS文件

### 5.2 第二阶段：测试验证

**测试内容：**

1. **主题切换测试**
   - 测试 paper/neutral/dark 主题切换
   - 验证CSS变量动态更新
   - 验证应用颜色正确响应

2. **第三方组件测试**
   - 测试 xterm.js 主题色适配
   - 测试 Recharts 主题色适配
   - 验证第三方组件颜色正确

3. **窗口系统测试**
   - 测试窗口生命周期
   - 测试窗口层级管理
   - 测试窗口交互

---

## 六、预期效果

### 6.1 架构清晰度

- ✅ 主题系统统一管理（ThemeManager）
- ✅ CSS变量命名规范（ovelis前缀）
- ✅ 删除冗余逻辑（暗色模式切换）
- ✅ 应用完全独立（无主题配置约束）

### 6.2 开发效率

- ✅ 新应用接入简单（遵循AppDefinition接口）
- ✅ 主题色使用自由（应用自主决定）
- ✅ 第三方组件集成简单（使用CSS变量）

### 6.3 维护性

- ✅ 主题配置集中管理（themes.ts）
- ✅ CSS变量命名统一（ovelis前缀）
- ✅ 逻辑简化（删除暗色模式切换）

---

## 七、风险与应对

### 7.1 CSS变量替换遗漏

**风险：** 应用组件中可能遗漏CSS变量替换。

**应对：**
- 使用 grep 搜索所有 `--accent-bg`、`--slider-*`、`--text-*`、`--border-color` 引用
- 逐一替换为新命名
- 测试验证所有组件颜色

### 7.2 第三方组件兼容性

**风险：** 第三方组件可能不支持CSS变量。

**应对：**
- 提供JavaScript API获取主题颜色值（不使用CSS变量）
- 在应用组件中动态传递颜色值给第三方组件

---

## 八、附录

### 8.1 CSS变量命名对照表

| 原命名 | 新命名 | 用途 |
|--------|--------|------|
| `--window-bg` | `--ovelis-window-bg` | 窗口背景 |
| `--view-bg` | `--ovelis-view-bg` | 内容区背景 |
| `--card-bg` | `--ovelis-card-bg` | 卡片背景 |
| `--card-hover` | `--ovelis-card-hover` | 卡片悬停 |
| `--headerbar-bg` | `--ovelis-headerbar-bg` | HeaderBar背景 |
| `--sidebar-bg` | `--ovelis-sidebar-bg` | 侧边栏背景 |
| `--sidebar-border` | `--ovelis-sidebar-border` | 侧边栏边框 |
| `--accent-bg` | `--ovelis-accent-bg` | 强调色 |
| `--accent-hover` | `--ovelis-accent-hover` | 强调色悬停 |
| `--accent-active` | `--ovelis-accent-active` | 强调色激活 |
| `--slider-track-bg` | `--ovelis-slider-track-bg` | 滑条轨道 |
| `--slider-thumb-bg` | `--ovelis-slider-thumb-bg` | 滑块填充 |
| `--slider-active-bg` | `--ovelis-slider-active-bg` | 滑块活动 |
| `--text-primary` | `--ovelis-text-primary` | 主字体颜色 |
| `--text-secondary` | `--ovelis-text-secondary` | 次级字体颜色 |
| `--text-disabled` | `--ovelis-text-disabled` | 禁用字体颜色 |
| `--border-color` | `--ovelis-border-color` | 边框颜色 |

### 8.2 删除内容清单

| 删除项 | 文件 | 说明 |
|--------|------|------|
| `isDarkMode` 参数 | `useTheme.ts` | 删除暗色模式参数 |
| `isDarkMode` 状态 | `settingsStore.ts` | 删除暗色模式状态 |
| `toggleDarkMode` 方法 | `settingsStore.ts` | 删除暗色模式切换方法 |
| `useDarkColors` 逻辑 | `useTheme.ts` | 删除暗色模式判断逻辑 |
| `data-theme` 属性 | `useTheme.ts` | 删除暗色模式标记 |