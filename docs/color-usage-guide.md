# 颜色使用指南

> 版本: v2.0 | 日期: 2026-06-24
>
> 本指南定义了 GNOME Remote Client 的颜色系统和使用规范，确保整个项目的颜色统一管理。

---

## 一、主题系统架构

### 1.1 系统概述

项目采用**多主题系统**，支持用户在设置中切换不同主题。每个主题包含完整的亮色和暗色模式配置。

**核心文件：**
- `src/config/themes.ts` - 主题配置文件（数据结构定义）
- `src/hooks/useTheme.ts` - 主题切换 Hook（应用逻辑）
- `src/stores/settingsStore.ts` - 主题状态存储（持久化）
- `src/apps/Settings.tsx` - 主题选择界面（UI）
- `src/styles/adwaita.css` - CSS 变量基础定义

### 1.2 当前可用主题

| 主题 ID | 名称 | 特点 | 默认强调色 | 可选强调色 |
|---------|------|------|-----------|-----------|
| `paper` | 纸张 | 温暖的纸张色调，层次分明 | #15aa70 (绿) | 暖蓝、绿 |
| `neutral` | 中性色调（GNOME 标准） | 符合 GNOME Adwaita 标准 | #3584e4 (蓝) | 无 |
| `dark` | 暗色 | 高对比度深色主题 | #62a0ea (蓝) | 无 |

### 1.3 数据结构

#### ThemeId - 主题标识符

```typescript
export type ThemeId = 'paper' | 'neutral' | 'dark';
// 新增主题时在此处添加新的 ID
```

#### AccentColorId - 强调色标识符

```typescript
export type AccentColorId = 'warmBlue' | 'paperAccent' | 'orange';
// 新增强调色时在此处添加新的 ID
```

#### ThemeColors - 颜色配置接口

```typescript
export interface ThemeColors {
  windowBg: string;       // 主窗口背景
  viewBg: string;         // 内容区/视图背景（最浅）
  cardBg: string;         // 卡片背景
  cardHover: string;      // 卡片悬停状态
  headerbarBg: string;    // HeaderBar 背景色
  sidebarBg: string;      // 侧边栏背景色
  sidebarBorder: string;  // 边框颜色
  accentBg: string;       // 主要强调色
  accentHover: string;    // 强调色悬停
  accentActive: string;   // 强调色活动状态
  sliderTrackBg: string;  // 滑条轨道颜色
  sliderThumbBg: string;  // 滑块颜色
  sliderActiveBg: string; // 滑块活动状态颜色
}
```

#### Theme - 主题接口

```typescript
export interface Theme {
  id: ThemeId;                    // 唯一标识符
  name: string;                   // 显示名称
  description: string;            // 描述文字
  lightColors: ThemeColors;       // 亮色模式配置
  darkColors: ThemeColors;        // 暗色模式配置
  accentColorOptions?: AccentColorId[];  // 可选强调色列表
}
```

---

## 二、如何新增主题

### 2.1 完整步骤

**步骤 1：在 ThemeId 类型中添加新 ID**

```typescript
// src/config/themes.ts
export type ThemeId = 'paper' | 'neutral' | 'dark' | 'myNewTheme';
```

**步骤 2：在 themes 对象中添加完整配置**

```typescript
// src/config/themes.ts
myNewTheme: {
  id: 'myNewTheme',
  name: '我的新主题',
  description: '主题描述',
  lightColors: {
    windowBg: '#ffffff',      // 主窗口背景
    viewBg: '#f5f5f5',        // 内容区背景（最浅）
    cardBg: '#f5f5f5',        // 卡片背景
    cardHover: '#e8e8e8',     // 卡片悬停状态
    headerbarBg: '#e8e8e8',   // HeaderBar 背景
    sidebarBg: '#e8e8e8',     // 侧边栏背景
    sidebarBorder: '#d0d0d0', // 边框颜色
    accentBg: '#3584e4',      // 强调色
    accentHover: '#1f75d1',   // 强调色悬停
    accentActive: '#1a5fb4',  // 强调色活动状态
    sliderTrackBg: '#d0d0d0',// 滑条轨道
    sliderThumbBg: '#3584e4', // 滑块
    sliderActiveBg: '#1f75d1',// 滑块活动状态
  },
  darkColors: {
    // ... 同上结构，使用暗色调
    windowBg: '#242424',
    viewBg: '#1e1e1e',
    // ... 其他暗色配置
  },
  accentColorOptions: ['warmBlue'], // 可选：允许用户选择强调色
},
```

**步骤 3：（可选）添加新的可选强调色**

```typescript
// 在 AccentColorId 中添加
export type AccentColorId = 'warmBlue' | 'paperAccent' | 'orange' | 'myNewColor';

// 在 accentColors 中添加配置
myNewColor: {
  light: '#ff6b6b',   // 亮色模式
  dark: '#ff8787',    // 暗色模式
},
```

**步骤 4：（可选）更新默认主题**

```typescript
// src/stores/settingsStore.ts
const DEFAULT_SETTINGS: SettingsState = {
  themeId: "myNewTheme",  // 设为默认主题
  accentColorId: null,     // 或指定默认强调色
};
```

### 2.2 颜色设计原则

**层次关系（从浅到深）：**

```
viewBg (最浅)
  ↓
cardBg
  ↓
cardHover
  ↓
headerbarBg / sidebarBg (中等)
  ↓
sidebarBorder (最深)
```

**注意事项：**
- 颜色对比度要足够高，确保 WCAG AA 标准
- 暗色模式的文字必须清晰可见（使用白色系）
- 功能性颜色（状态、警告）不应随主题变化
- 保持色彩倾向的一致性（暖色/冷色/中性）

---

## 三、当前主题详细配置

### 3.1 纸张主题 (paper)

**特点：** 温暖的纸张色调，带有米色倾向，适合阅读和长时间使用

**亮色模式：**

| 变量 | 颜色值 | RGB | 用途 |
|------|--------|-----|------|
| windowBg | `#fafafa` | (250,250,250) | 主窗口背景（柔和白） |
| viewBg | `#faf5f3` | (250,245,243) | 内容区背景（最浅） |
| cardBg | `#faf5f3` | (250,245,243) | 卡片背景 |
| cardHover | `#ebe5d9` | (235,229,217) | 悬停状态（中暖灰） |
| headerbarBg | `#f5f0e6` | (245,240,230) | HeaderBar（暖灰） |
| sidebarBg | `#f5f0e6` | (245,240,230) | 侧边栏（暖灰） |
| sidebarBorder | `#d5d0c4` | (213,208,196) | 边框（深暖灰） |
| accentBg | `#15aa70` | (21,170,112) | 默认强调色（绿色） |

**可选强调色：**
- `warmBlue`: #4a90e2 (暖蓝)
- `paperAccent`: #15aa70 (绿)

### 3.2 中性色调主题 (neutral)

**特点：** 符合 GNOME Adwaita 标准的中性灰色系，无色彩倾向

**亮色模式：**

| 变量 | 颜色值 | RGB | 用途 |
|------|--------|-----|------|
| windowBg | `#fafafa` | (250,250,250) | 主窗口背景 |
| viewBg | `#f5f5f5` | (245,245,245) | 内容区背景（中性浅灰） |
| cardBg | `#f5f5f5` | (245,245,245) | 卡片背景 |
| cardHover | `#e8e8e8` | (232,232,232) | 悬停状态（中性灰） |
| headerbarBg | `#e8e8e8` | (232,232,232) | HeaderBar（中性灰） |
| sidebarBg | `#e8e8e8` | (232,232,232) | 侧边栏（中性灰） |
| sidebarBorder | `#d0d0d0` | (208,208,208) | 边框（中性深灰） |
| accentBg | `#3584e4` | (53,132,228) | 强调色（GNOME Blue） |

**无可选强调色。**

### 3.3 暗色主题 (dark)

**特点：** 高对比度深色主题，始终使用暗色配置（lightColors === darkColors）

**颜色配置：**

| 变量 | 颜色值 | 用途 |
|------|--------|------|
| windowBg | `#242424` | 主窗口背景 |
| viewBg | `#1e1e1e` | 内容区背景（最深） |
| cardBg | `#2d2d2d` | 卡片背景 |
| cardHover | `#353535` | 悬停状态 |
| headerbarBg | `#303030` | HeaderBar |
| sidebarBg | `#303030` | 侧边栏 |
| sidebarBorder | `#3d3d3d` | 边框 |
| accentBg | `#62a0ea` | 强调色（GNOME Blue 暗色版） |

**字体颜色（由 useTheme 自动应用）：**
- text-primary: rgba(255, 255, 255, 0.87)
- text-secondary: rgba(255, 255, 255, 0.60)
- text-disabled: rgba(255, 255, 255, 0.38)

**无可选强调色。**

---

## 四、使用原则

### 4.1 核心原则

1. **优先使用 CSS 变量** - 所有装饰性颜色必须通过 CSS 变量引用
2. **禁止硬编码** - 除非是功能性颜色（状态指示、图表、终端主题等）
3. **遵循层次** - 按照背景 → 卡片 → 边框的层次使用颜色
4. **保持语义** - 变量名反映用途，不要随意使用变量
5. **支持多主题** - 所有颜色必须能随主题切换而变化

### 4.2 例外情况（可硬编码）

以下功能性颜色可以硬编码：

- **状态指示颜色**：连接状态（绿色、黄色、红色）
- **图表颜色**：系统监控图表（蓝色、绿色、黄色、红色）
- **终端主题**：终端背景和前景色
- **通知徽章**：紧急通知红色徽章
- **壁纸渐变**：壁纸设计颜色

---

## 五、常见用途映射

### 5.1 UI 元素颜色映射

| UI 元素 | 应使用的变量 | 示例 |
|---------|-------------|------|
| 窗口主背景 | `var(--window-bg)` | 主窗口、桌面背景 |
| 内容区背景 | `var(--view-bg)` | 文件列表、内容区域 |
| 卡片背景 | `var(--card-bg)` | 卡片、面板、弹窗背景 |
| 卡片悬停 | `var(--card-hover)` | 卡片悬停状态 |
| HeaderBar | `var(--headerbar-bg)` | 顶部标题栏 |
| 侧边栏 | `var(--sidebar-bg)` | 左侧导航栏 |
| 边框 | `var(--border-color)` 或 `var(--sidebar-border)` | 分隔线、边框 |
| 主文字 | `var(--text-primary)` | 主要内容文字 |
| 次文字 | `var(--text-secondary)` | 辅助说明文字 |
| 禁用文字 | `var(--text-disabled)` | 禁用状态文字 |
| 强调按钮 | `var(--accent-bg)` | 主要操作按钮 |
| 滑条轨道 | `var(--slider-track-bg)` | 滑块轨道背景 |
| 滑块填充 | `var(--slider-thumb-bg)` | 滑块颜色 |

### 5.2 CSS 示例

```css
/* ✅ 正确：使用 CSS 变量 */
.card {
  background: var(--card-bg);
  border: 1px solid var(--border-color);
  color: var(--text-primary);
}

.card:hover {
  background: var(--card-hover);
}

/* ❌ 错误：硬编码颜色 */
.card {
  background: #f0f0f0;  /* 禁止！ */
  border: 1px solid #d5d5d5;  /* 禁止！ */
  color: rgba(0,0,0,0.87);  /* 禁止！ */
}
```

### 5.3 React/TSX 示例

```tsx
// ✅ 正确：使用 CSS 变量
<div style={{ background: 'var(--card-bg)', color: 'var(--text-primary)' }}>
  内容
</div>

// ❌ 错误：硬编码颜色
<div style={{ background: '#f0f0f0', color: 'rgba(0,0,0,0.87)' }}>
  内容
</div>

// ✅ 正确：功能性颜色可以硬编码
<div style={{ background: '#33d17a' }}>  {/* 连接状态指示器 */}
  已连接
</div>
```

---

## 六、功能性颜色定义

### 6.1 状态指示颜色

| 状态 | 颜色值 | 用途 |
|------|--------|------|
| 已连接 | `#33d17a` | 连接状态指示器（绿色） |
| 连接中 | `#e8a416` | 连接状态指示器（黄色） |
| 已断开 | `#9a9996` | 连接状态指示器（灰色） |
| 错误 | `#e01b24` | 连接状态指示器（红色） |

### 6.2 图表颜色

| 用途 | 颜色值 | 说明 |
|------|--------|------|
| CPU 图表 | `#3584e4` | GNOME Blue |
| 内存图表 | `#33d17a` | 绿色 |
| 磁盘使用警告 | `#e8a416` | 黄色（>60%） |
| 磁盘使用危险 | `#e01b24` | 红色（>80%） |

### 6.3 终端主题颜色

| 用途 | 颜色值 | 说明 |
|------|--------|------|
| 背景 | `#1e1e1e` | 暗色背景 |
| 前景 | `#ffffff` | 白色文字 |
| 光标 | `#4ec9b0` | 青色光标 |
| 选择背景 | `#264f78` | 蓝色选择 |

### 6.4 通知徽章颜色

| 用途 | 颜色值 | 说明 |
|------|--------|------|
| 紧急通知徽章 | `#e01b24` | 红色徽章 |
| 徽章文字 | `#ffffff` | 白色文字 |

---

## 七、常见错误和修正

### 7.1 错误示例

```css
/* ❌ 错误：硬编码背景颜色 */
background: #ffffff;

/* ❌ 错误：硬编码边框颜色 */
border: 1px solid #d5d5d5;

/* ❌ 错误：硬编码文字颜色 */
color: rgba(0,0,0,0.87);

/* ❌ 错误：使用错误的变量 */
background: var(--sidebar-bg);  /* 应该用 var(--card-bg) */
```

### 7.2 修正示例

```css
/* ✅ 正确：使用 CSS 变量 */
background: var(--view-bg);

/* ✅ 正确：使用 CSS 变量 */
border: 1px solid var(--border-color);

/* ✅ 正确：使用 CSS 变量 */
color: var(--text-primary);

/* ✅ 正确：使用正确的变量 */
background: var(--card-bg);  /* 卡片背景使用 card-bg */
```

---

## 八、维护和更新

### 8.1 更新流程

如果需要更新或新增主题：

1. **修改 `src/config/themes.ts`**
   - 更新现有主题的颜色值
   - 或添加新的主题配置

2. **测试所有模式**
   - 测试亮色模式下的视觉效果
   - 测试暗色模式下的视觉效果
   - 测试主题切换功能
   - 测试可选强调色功能

3. **提交更改**
   - 记录变更内容
   - 更新本文档（如有必要）

### 8.2 变更日志

| 日期 | 版本 | 变更内容 |
|------|------|----------|
| 2026-06-24 | v1.0 | 初始版本，引入暖灰色系 |
| 2026-06-24 | v2.0 | 重构为多主题系统，支持用户切换主题 |

---

**文档完成日期：** 2026-06-24
**相关文件：** [themes.ts](../src/config/themes.ts) | [useTheme.ts](../src/hooks/useTheme.ts) | [adwaita.css](../src/styles/adwaita.css)
