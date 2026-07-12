# 毛玻璃效果设计方案

> 版本: v1.0 | 日期: 2026-07-12
>
> 为TopBar和WindowShell.HeaderBar添加毛玻璃视觉效果，支持动态主题色和参数配置。

---

## 一、需求概述

**核心需求：**
- 为TopBar（顶部状态栏）和WindowShell.HeaderBar（窗口标题栏）添加毛玻璃效果
- RGB颜色从项目主题色`--ovelis-headerbar-bg`动态获取
- 所有参数支持配置，方便后续整合到外观设置

**视觉参数：**
- 模糊度：30px
- 饱和度：1.2
- 透明度：0.3
- 背景色：--ovelis-headerbar-bg (#f5f0e6) 的半透明版本

---

## 二、架构设计

### 2.1 整体架构

```
App.tsx
  └─ 导入 utilities.css（毛玻璃工具类）
      └─ .frosted-glass 类定义
          ├─ backdrop-filter: blur(var(--frosted-blur))
          ├─ saturate(var(--frosted-saturate))
          └─ background: rgba(R, G, B, var(--frosted-opacity))

组件使用：
  TopBar.tsx
    └─ <div className={`${styles.topBar} frosted-glass`}>

  WindowShell.tsx
    └─ <div className={`${styles.windowHeaderBar} frosted-glass`}>
```

### 2.2 文件结构

```
src/styles/
├── variables.css           # 扩展：添加RGB分离变量和毛玻璃参数
├── utilities.css           # 新增：毛玻璃效果工具类
└── ...

src/shell/TopBar/
├── TopBar.tsx              # 修改：应用毛玻璃效果
└── TopBar.module.css       # 修改：移除background属性

src/components/window-shell/
├── WindowShell.tsx         # 修改：HeaderBar应用毛玻璃效果
└── WindowShell.module.css  # 修改：移除.header-bar的background属性
```

---

## 三、详细设计

### 3.1 CSS变量设计（variables.css）

```css
/* src/styles/variables.css */

:root {
  /* 原有主题色 */
  --ovelis-headerbar-bg: #f5f0e6;

  /* ✅ 新增：RGB分离变量（为毛玻璃效果准备） */
  --ovelis-headerbar-bg-r: 245;
  --ovelis-headerbar-bg-g: 240;
  --ovelis-headerbar-bg-b: 230;

  /* ✅ 新增：毛玻璃效果可配置参数 */
  --frosted-blur: 30px;           /* 模糊度 */
  --frosted-saturate: 1.2;         /* 饱和度 */
  --frosted-opacity: 0.3;          /* 透明度 */
}
```

**设计说明：**
- 保持原有的`--ovelis-headerbar-bg`不变（兼容性）
- 新增RGB分离变量（`-r`, `-g`, `-b`后缀）
- 新增毛玻璃效果参数变量，方便后续配置

### 3.2 毛玻璃工具类（utilities.css）

```css
/* src/styles/utilities.css */

/* ── 毛玻璃效果工具类 ── */
.frosted-glass {
  /* 使用CSS变量实现动态配置 */
  backdrop-filter:
    blur(var(--frosted-blur))
    saturate(var(--frosted-saturate));
  -webkit-backdrop-filter:
    blur(var(--frosted-blur))
    saturate(var(--frosted-saturate));

  /* 动态背景色 - RGB从主题色提取 */
  background: rgba(
    var(--ovelis-headerbar-bg-r),
    var(--ovelis-headerbar-bg-g),
    var(--ovelis-headerbar-bg-b),
    var(--frosted-opacity)
  );

  /* 性能优化 */
  will-change: backdrop-filter;
  isolation: isolate;
}

/* 可选：不同强度的变体 */
.frosted-glass-light {
  --frosted-blur: 20px;
  --frosted-saturate: 1.1;
  --frosted-opacity: 0.2;
}

.frosted-glass-strong {
  --frosted-blur: 50px;
  --frosted-saturate: 1.5;
  --frosted-opacity: 0.4;
}
```

**技术要点：**
- 使用CSS变量实现动态配置
- 添加`-webkit-backdrop-filter`确保Safari兼容性
- 使用`isolation: isolate`创建独立的堆叠上下文
- 提供3种强度变体供后续使用

### 3.3 TopBar组件集成

**TopBar.module.css修改：**
```css
/* 移除 .topBar 的 background 属性 */
.topBar {
  height: var(--topbar-height);
  /* background: var(--ovelis-headerbar-bg); ← 删除这行 */
  border-bottom: 1px solid var(--ovelis-border-color);
  /* ... 其他样式保持不变 */
}
```

**TopBar.tsx修改：**
```typescript
export const TopBar = memo(function TopBar({ ... }: TopBarProps) {
  return (
    <div className={`${styles.topBar} frosted-glass`} data-tauri-drag-region>
      {/* 组件内容保持不变 */}
    </div>
  );
});
```

### 3.4 WindowShell组件集成

**WindowShell.module.css修改：**
```css
/* 移除 .windowHeaderBar 的 background 属性 */
.windowHeaderBar {
  height: var(--headerbar-height);
  /* background: var(--ovelis-headerbar-bg); ← 删除这行 */
  border-bottom: 1px solid var(--ovelis-border-color);
  /* ... 其他样式保持不变 */
}
```

**WindowShell.tsx修改：**
```typescript
export function WindowShell({ ... }: WindowShellProps) {
  return (
    <div className={styles.windowShell}>
      {/* HeaderBar 应用毛玻璃效果 */}
      <div className={`${styles.windowHeaderBar} frosted-glass`}>
        {/* 标题栏内容保持不变 */}
      </div>

      {/* 窗口内容保持不变 */}
      <div className={styles.windowContentFrame}>
        {children}
      </div>
    </div>
  );
}
```

### 3.5 App.tsx导入

```typescript
// src/App.tsx

import { Desktop } from "./shell/Desktop";
import { StorageInitializer } from "./components/StorageInitializer";
import "./styles/variables.css";
import "./styles/base.css";
import "./styles/adwaita.css";
import "./styles/skeleton.css";
import "./styles/utilities.css"; // ✅ 新增：毛玻璃工具类

function App() {
  return (
    <StorageInitializer>
      <Desktop />
    </StorageInitializer>
  );
}

export default App;
```

---

## 四、后续扩展

### 4.1 外观设置集成

**未来配置接口示例：**
```typescript
// 当用户在外观设置中调整参数时
function updateFrostedGlassSettings(blur: number, opacity: number) {
  const root = document.documentElement;
  root.style.setProperty('--frosted-blur', `${blur}px`);
  root.style.setProperty('--frosted-opacity', opacity.toString());
}
```

### 4.2 其他组件应用

毛玻璃效果可以通过添加`frosted-glass`类应用到任何组件：
- FileManager的Toolbar和StatusBar
- Terminal的HeaderBar和TabBar
- 自定义对话框和面板

---

## 五、实施步骤

1. **创建utilities.css** - 定义毛玻璃工具类
2. **扩展variables.css** - 添加RGB分离变量和参数变量
3. **修改TopBar** - 移除background，添加frosted-glass类
4. **修改WindowShell** - HeaderBar移除background，添加frosted-glass类
5. **更新App.tsx** - 导入utilities.css
6. **测试验证** - 确保视觉效果正确

---

## 六、验收标准

- ✅ TopBar和WindowShell.HeaderBar显示毛玻璃效果
- ✅ 颜色从--ovelis-headerbar-bg动态获取
- ✅ 参数可通过CSS变量配置
- ✅ 性能良好，无明显卡顿
- ✅ Safari浏览器兼容性良好