# CSS 架构规范 - GNOME Remote Client

> 版本: v2.0 | 日期: 2026-07-12
>
> 核心原则：保留所有原有样式，只做架构迁移，确保显示效果不变

---

## 一、三层分离架构

### 1.1 架构分层

```
Layer 1: 全局变量（variables.css）
  ↓ 提供CSS变量定义
Layer 2: CSS Modules（组件级样式）
  ↓ 使用变量 + 组件独立样式
Layer 3: 内联样式（动态样式）
  ↓ 仅用于动态计算的样式
```

### 1.2 各层职责

| 层级 | 文件 | 职责 | 示例 |
|------|------|------|------|
| Layer 1 | variables.css | 定义全局CSS变量 | --ovelis-accent-bg: #15aa70; |
| Layer 2 | *.module.css | 组件独立样式 | .topBar { background: var(--ovelis-headerbar-bg); } |
| Layer 3 | 内联style | 动态样式 | style={{ width: 300 }} |

---

## 二、重构原则（关键）

### 2.1 保留所有原有样式 ⚠️

**必须遵守：**
- ✅ 保留所有`background`属性
- ✅ 保留所有颜色值
- ✅ 保留所有间距、字体、圆角等样式
- ✅ 只改变样式的组织方式（从全局CSS迁移到CSS Modules）

**禁止操作：**
- ❌ 删除任何样式属性
- ❌ 修改样式值
- ❌ 注释掉样式（除非是冗余的重复定义）

### 2.2 验证策略

每重构一个组件后，必须验证：
1. 组件显示效果是否不变
2. 背景、颜色、间距是否一致
3. 交互效果是否正常

---

## 三、文件组织规范

### 3.1 目录结构

```
src/
├── styles/
│   ├── variables.css        # Layer 1: 全局变量
│   └── base.css             # 全局重置样式（最小化）
│
├── shell/                   # Shell 层组件
│   ├── Desktop.tsx
│   ├── Desktop.css          # 保留原有全局样式
│   └── TopBar/              # 新增：独立组件
│       ├── TopBar.tsx
│       └── TopBar.module.css # CSS Modules（保留所有原有样式）
│
├── components/              # 通用组件
│   └── window-shell/
│       ├── WindowShell.tsx
│       ├── window-shell.css  # 保留原有全局样式
│       └── WindowShell.module.css # CSS Modules（保留所有原有样式）
│
└── apps/                    # 应用组件
    ├── FileManager.tsx
    ├── FileManager.css       # 保留原有全局样式
    └── FileManager.module.css # CSS Modules（保留所有原有样式）
```

### 3.2 文件命名规范

- **全局CSS**: `Component.css`（保留）
- **CSS Modules**: `Component.module.css`（新建，从全局CSS迁移样式）

---

## 四、重构步骤

### 步骤 1：创建基础架构

**创建 variables.css**
```css
/* 只定义CSS变量，不包含任何样式规则 */
:root {
  /* Adwaita 颜色变量 */
  --ovelis-accent-bg: #15aa70;
  --ovelis-window-bg: #fafafa;
  --ovelis-headerbar-bg: #f5f0e6;
  /* ... 其他变量 */
}
```

**创建 base.css**
```css
/* 最小化全局重置 */
*, *::before, *::after {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

html, body, #root {
  width: 100%;
  height: 100%;
  overflow: hidden;
}
```

### 步骤 2：重构 Shell 层（TopBar）

**从 Desktop.css 提取 TopBar 样式 → TopBar.module.css**

```css
/* TopBar.module.css - 保留所有原有样式 */
.topBar {
  height: var(--topbar-height);
  background: var(--ovelis-headerbar-bg);  /* ✅ 保留 */
  border-bottom: 1px solid var(--ovelis-border-color);
  /* ... 其他样式全部保留 */
}
```

### 步骤 3：重构 Window 层

**从 window-shell.css 提取 → WindowShell.module.css**

```css
/* WindowShell.module.css - 保留所有原有样式 */
.windowShell {
  position: absolute;
  background: var(--ovelis-window-bg);  /* ✅ 保留 */
  /* ... 其他样式全部保留 */
}

.windowHeaderBar {
  height: var(--headerbar-height);
  background: var(--ovelis-headerbar-bg);  /* ✅ 保留 */
  /* ... 其他样式全部保留 */
}
```

### 步骤 4：重构 App 层

**从各App的CSS提取 → *.module.css**

```css
/* FileManager.module.css - 保留所有原有样式 */
.fmApp {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  background: var(--ovelis-window-bg);  /* ✅ 保留 */
}

.fmToolbar {
  height: 40px;
  background: var(--ovelis-view-bg);  /* ✅ 保留 */
  /* ... 其他样式全部保留 */
}
```

---

## 五、迁移检查清单

### 5.1 样式迁移检查

- [ ] 所有 `background` 属性已保留
- [ ] 所有颜色值已保留
- [ ] 所有间距、字体、圆角已保留
- [ ] 所有状态样式（:hover, :active等）已保留

### 5.2 组件集成检查

- [ ] CSS Modules 导入正确
- [ ] className 使用正确（styles.className）
- [ ] 显示效果与原来一致

---

## 六、常见问题

### Q1: 为什么要保留原有的全局CSS文件？

A: 为了安全起见，保留原有CSS作为备份。如果CSS Modules出现问题，可以快速回退。

### Q2: CSS Modules和全局CSS可以共存吗？

A: 可以。组件可以同时导入两种CSS：
```typescript
import "./FileManager.css";        // 保留：其他样式
import styles from "./FileManager.module.css"; // 新增：核心容器样式
```

### Q3: 如何确保显示效果不变？

A: 遵循"保留所有原有样式"原则。只改变样式的组织方式，不修改样式值。

---

## 七、禁止操作清单

**永远不要：**
- ❌ 删除 `background` 属性
- ❌ 注释样式属性（除非确认是冗余的）
- ❌ 修改样式值
- ❌ 在没有验证的情况下批量修改样式

---

## 八、成功标准

重构成功的标准：
1. ✅ 所有组件显示效果与原来完全一致
2. ✅ CSS架构清晰，组件样式独立
3. ✅ 父级CSS不会覆盖子组件
4. ✅ 保留了所有原有样式定义

---

## 附录：重构进度跟踪

| 层级 | 组件 | 状态 | 验证 |
|------|------|------|------|
| Shell | TopBar | 待开始 | - |
| Window | WindowShell | 待开始 | - |
| App | FileManager | 待开始 | - |
| App | Terminal | 待开始 | - |
| App | SystemMonitor | 待开始 | - |
| App | Settings | 待开始 | - |
| App | TextEditor | 待开始 | - |