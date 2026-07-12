# 毛玻璃效果实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为TopBar和WindowShell.HeaderBar添加毛玻璃视觉效果，支持动态主题色和参数配置。

**Architecture:** 使用CSS工具类方案，创建`.frosted-glass`全局类，通过CSS变量实现动态颜色和参数配置。组件只需添加className即可使用。

**Tech Stack:** React 18 + TypeScript + CSS Variables + backdrop-filter

---

## 文件结构

**创建文件：**
- `src/styles/utilities.css` - 毛玻璃效果工具类

**修改文件：**
- `src/styles/variables.css` - 添加RGB分离变量和毛玻璃参数
- `src/shell/TopBar/TopBar.module.css` - 移除.topBar的background
- `src/shell/TopBar/TopBar.tsx` - 添加frosted-glass类
- `src/components/window-shell/WindowShell.module.css` - 移除.windowHeaderBar的background
- `src/components/window-shell/WindowShell.tsx` - HeaderBar添加frosted-glass类
- `src/App.tsx` - 导入utilities.css

---

## Task 1: 扩展variables.css

**Files:**
- Modify: `src/styles/variables.css:31-83`

- [ ] **Step 1: 在variables.css中添加RGB分离变量**

在现有的`--ovelis-headerbar-bg`变量后添加：

```css
/* src/styles/variables.css */

:root {
  /* ... 原有变量 ... */

  /* Window & Surfaces (Warm Gray) */
  --ovelis-headerbar-bg:      #f5f0e6;      /* RGB(245,240,230) - HeaderBar（中等） */
  --ovelis-sidebar-bg:        #f5f0e6;      /* RGB(245,240,230) - 侧边栏（中等） */
  
  /* ✅ 新增：RGB分离变量（为毛玻璃效果准备） */
  --ovelis-headerbar-bg-r:    245;
  --ovelis-headerbar-bg-g:    240;
  --ovelis-headerbar-bg-b:    230;
  
  /* ... 其他原有变量 ... */
}
```

- [ ] **Step 2: 添加毛玻璃效果参数变量**

在变量定义末尾添加：

```css
  /* ... 其他变量 ... */

  /* Layout Constants */
  --topbar-height:    32px;
  --headerbar-height: 48px;
  --sidebar-width:    240px;
  --dock-height:      64px;
  
  /* ✅ 新增：毛玻璃效果可配置参数 */
  --frosted-blur:     30px;           /* 模糊度 */
  --frosted-saturate: 1.2;            /* 饱和度 */
  --frosted-opacity:  0.3;            /* 透明度 */
}
```

---

## Task 2: 创建utilities.css

**Files:**
- Create: `src/styles/utilities.css`

- [ ] **Step 1: 创建utilities.css文件**

创建新文件并添加毛玻璃效果工具类：

```css
/* ============================================================
   Utility Classes — GNOME Remote Client
   工具类样式，提供可复用的视觉效果
   ============================================================ */

/* ── 毛玻璃效果 ── */

/**
 * 毛玻璃效果工具类
 * 使用方法：在组件上添加 className="frosted-glass"
 * 
 * 技术要点：
 * - RGB颜色从 --ovelis-headerbar-bg-* 变量获取
 * - 所有参数可通过CSS变量配置
 * - 支持 Safari 浏览器（-webkit-backdrop-filter）
 */
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
  isolation: isolate; /* 创建新的堆叠上下文，避免与子元素冲突 */
}

/* ── 毛玻璃效果变体 ── */

/**
 * 轻度毛玻璃
 * 适用于需要更轻透明度的场景
 */
.frosted-glass-light {
  --frosted-blur: 20px;
  --frosted-saturate: 1.1;
  --frosted-opacity: 0.2;
}

/**
 * 重度毛玻璃
 * 适用于需要更明显模糊效果的场景
 */
.frosted-glass-strong {
  --frosted-blur: 50px;
  --frosted-saturate: 1.5;
  --frosted-opacity: 0.4;
}
```

---

## Task 3: 修改TopBar.module.css

**Files:**
- Modify: `src/shell/TopBar/TopBar.module.css:6-18`

- [ ] **Step 1: 移除.topBar的background属性**

修改前：
```css
.topBar {
  height: var(--topbar-height);
  background: var(--ovelis-headerbar-bg);  /* ← 删除这行 */
  border-bottom: 1px solid var(--ovelis-border-color);
  display: flex;
  align-items: center;
  padding: 0 12px;
  gap: 16px;
  flex-shrink: 0;
  z-index: 100;
  -webkit-app-region: drag;
  app-region: drag;
}
```

修改后：
```css
.topBar {
  height: var(--topbar-height);
  /* ✅ background 由毛玻璃效果提供，不再在这里设置 */
  border-bottom: 1px solid var(--ovelis-border-color);
  display: flex;
  align-items: center;
  padding: 0 12px;
  gap: 16px;
  flex-shrink: 0;
  z-index: 100;
  -webkit-app-region: drag;
  app-region: drag;
}
```

---

## Task 4: 修改TopBar.tsx

**Files:**
- Modify: `src/shell/TopBar/TopBar.tsx:40-42`

- [ ] **Step 1: TopBar组件添加frosted-glass类**

修改前：
```typescript
export const TopBar = memo(function TopBar({
  // ...
}: TopBarProps) {
  return (
    <div className={styles.topBar} data-tauri-drag-region>
      {/* 组件内容 */}
    </div>
  );
});
```

修改后：
```typescript
export const TopBar = memo(function TopBar({
  // ...
}: TopBarProps) {
  return (
    <div className={`${styles.topBar} frosted-glass`} data-tauri-drag-region>
      {/* 组件内容 */}
    </div>
  );
});
```

---

## Task 5: 修改WindowShell.module.css

**Files:**
- Modify: `src/components/window-shell/WindowShell.module.css:33-41`

- [ ] **Step 1: 移除.windowHeaderBar的background属性**

修改前：
```css
.windowHeaderBar {
  height: var(--headerbar-height);
  background: var(--ovelis-headerbar-bg);  /* ← 删除这行 */
  border-bottom: 1px solid var(--ovelis-border-color);
  display: flex;
  align-items: center;
  padding: 0 12px;
  gap: 8px;
  flex-shrink: 0;
  cursor: move;
  user-select: none;
}
```

修改后：
```css
.windowHeaderBar {
  height: var(--headerbar-height);
  /* ✅ background 由毛玻璃效果提供，不再在这里设置 */
  border-bottom: 1px solid var(--ovelis-border-color);
  display: flex;
  align-items: center;
  padding: 0 12px;
  gap: 8px;
  flex-shrink: 0;
  cursor: move;
  user-select: none;
}
```

---

## Task 6: 修改WindowShell.tsx

**Files:**
- Modify: `src/components/window-shell/WindowShell.tsx:60-70`

- [ ] **Step 1: HeaderBar组件添加frosted-glass类**

修改前：
```typescript
export function WindowShell({
  // ...
}: WindowShellProps) {
  return (
    <div className={styles.windowShell}>
      <div className={styles.windowHeaderBar}>
        {/* 标题栏内容 */}
      </div>
      {/* ... */}
    </div>
  );
}
```

修改后：
```typescript
export function WindowShell({
  // ...
}: WindowShellProps) {
  return (
    <div className={styles.windowShell}>
      <div className={`${styles.windowHeaderBar} frosted-glass`}>
        {/* 标题栏内容 */}
      </div>
      {/* ... */}
    </div>
  );
}
```

---

## Task 7: 修改App.tsx

**Files:**
- Modify: `src/App.tsx:1-6`

- [ ] **Step 1: 导入utilities.css**

修改前：
```typescript
import { Desktop } from "./shell/Desktop";
import { StorageInitializer } from "./components/StorageInitializer";
import "./styles/variables.css";
import "./styles/base.css";
import "./styles/adwaita.css";
import "./styles/skeleton.css";
```

修改后：
```typescript
import { Desktop } from "./shell/Desktop";
import { StorageInitializer } from "./components/StorageInitializer";
import "./styles/variables.css";
import "./styles/base.css";
import "./styles/adwaita.css";
import "./styles/skeleton.css";
import "./styles/utilities.css"; // ✅ 新增：毛玻璃效果工具类
```

---

## Task 8: 测试验证

- [ ] **Step 1: 启动开发服务器**

Run: `npm run dev`
Expected: 应用正常启动，无编译错误

- [ ] **Step 2: 验证TopBar毛玻璃效果**

检查项：
- TopBar显示半透明背景
- 能看到背后的桌面壁纸模糊效果
- 颜色为暖橘色（#f5f0e6）的半透明版本

- [ ] **Step 3: 验证WindowShell.HeaderBar毛玻璃效果**

检查项：
- 打开任意应用窗口（如FileManager）
- 窗口标题栏显示半透明背景
- 能看到背后的内容模糊效果

- [ ] **Step 4: 验证浏览器兼容性**

Run: 在Chrome、Firefox、Safari中测试
Expected: 
- Chrome/Firefox: 毛玻璃效果正常
- Safari: 毛玻璃效果正常（使用-webkit-backdrop-filter）

---

## Task 9: 提交代码

- [ ] **Step 1: 提交所有修改**

```bash
git add src/styles/variables.css src/styles/utilities.css src/shell/TopBar/ src/components/window-shell/ src/App.tsx
git commit -m "feat: 添加毛玻璃效果到TopBar和WindowShell.HeaderBar

- 创建utilities.css提供毛玻璃工具类
- 扩展variables.css添加RGB分离变量和参数配置
- TopBar和WindowShell.HeaderBar应用毛玻璃效果
- 支持动态主题色和参数配置，方便后续整合到外观设置"
```

---

## 自我审查清单

**1. Spec覆盖检查：**
- ✅ RGB颜色从--ovelis-headerbar-bg动态获取
- ✅ 所有参数支持CSS变量配置
- ✅ 应用到TopBar和WindowShell.HeaderBar
- ✅ 保留扩展性（提供变体类）

**2. 占位符扫描：**
- ✅ 无"TBD"、"TODO"等占位符
- ✅ 所有步骤都有具体代码
- ✅ 所有文件路径准确

**3. 类型一致性：**
- ✅ CSS变量命名一致（--ovelis-headerbar-bg-*）
- ✅ className使用一致（frosted-glass）
- ✅ 文件路径一致