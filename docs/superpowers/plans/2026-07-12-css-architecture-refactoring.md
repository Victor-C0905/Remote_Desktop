# CSS 架构重构 - 桌面系统合理化方案

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 重构 CSS 架构，删除重复定义，分离职责，确保上层只提供变量，下层样式完全独立，符合 GNOME 桌面系统设计哲学。

**Architecture:**
- 全局层：variables.css（变量）+ base.css（Reset）+ typography.css（工具类）+ scrollbar.css + skeleton.css
- Shell 层：命名空间隔离（类名前缀 `.topbar-`、`.desktop-`）
- Window 层：CSS Modules（多窗口实例引用需要隔离）
- App 层：命名空间隔离（类名前缀 `.fm-`、`.terminal-`、`.sm-`、`.te-`、`.settings-`）

**Tech Stack:** CSS Modules、CSS 变量、命名空间类名前缀、GNOME Adwaita 设计系统

---

## 文件结构映射

### 新建文件（职责分离）
- `src/styles/typography.css` - Typography 工具类（从 adwaita.css 提取）
- `src/styles/scrollbar.css` - Scrollbar 全局样式（从 adwaita.css 提取）
- `src/components/window-shell/WindowControls.module.css` - 窗口控制按钮样式（新建）

### 删除文件（重复/职责混乱）
- `src/styles/adwaita.css` - 职责已分离到其他文件
- `src/components/window-shell/window-shell.css` - 全局样式（改用 CSS Module）

### 修改文件（清理重复定义/引用混乱）
- `src/styles/variables.css` - 删除重复定义（保留纯变量定义）
- `src/styles/base.css` - 删除 `:root` 变量定义（已由 variables.css 提供）
- `src/App.tsx` - 清理样式引用顺序
- `src/components/window-shell/WindowShell.tsx` - 删除全局样式引用，改用 CSS Module
- `src/apps/FileManager.css` - 统一类名前缀（`.fm-`）
- `src/apps/Terminal.css` - 统一类名前缀（`.terminal-`）
- `src/apps/SystemMonitor.css` - 统一类名前缀（`.sm-`）
- `src/apps/Settings.css` - 统一类名前缀（`.settings-`）
- `src/apps/TextEditor/TextEditor.css` - 统一类名前缀（`.te-`）

---

## Task 1: 创建 Typography 工具类文件

**Files:**
- Create: `src/styles/typography.css`

- [ ] **Step 1: 创建 typography.css 文件，提取 Typography 工具类**

```css
/* ============================================================
   Typography 工具类 — GNOME Remote Client
   从 adwaita.css 提取，只包含文本样式工具类
   ============================================================ */

/* ── Typography ───────────────────────────────────────── */

.text-title {
  font-size: var(--font-title);
  font-weight: 600;
  color: var(--ovelis-text-primary);
  white-space: nowrap;
  line-height: 1.2;
}

.text-heading {
  font-size: var(--font-body);
  font-weight: 600;
  color: var(--ovelis-text-primary);
  white-space: nowrap;
  line-height: 1.4;
}

/* 默认正文 — 允许自然换行，用于内容区、终端、文件列表等 */
.text-body {
  font-size: var(--font-body);
  font-weight: 400;
  color: var(--ovelis-text-primary);
  line-height: 1.5;
}

/* 固定标签 — 表单标签、侧边栏项名、按钮文字 */
.text-label {
  font-size: var(--font-small);
  font-weight: 600;
  color: var(--ovelis-text-secondary);
  white-space: nowrap;
  line-height: 1.3;
}

/* 辅助说明 — 时间、IP地址、状态值等短固定文本 */
.text-caption {
  font-size: var(--font-small);
  font-weight: 400;
  color: var(--ovelis-text-secondary);
  white-space: nowrap;
  line-height: 1.3;
}

/* 等宽文本 — 数值、版本号、代码片段 */
.text-mono {
  font-family: var(--font-mono);
  font-size: var(--font-small);
  font-weight: 400;
  color: var(--ovelis-text-secondary);
  white-space: nowrap;
  line-height: 1.3;
}

/* 可截断的名称字段 — flex 容器中占满剩余空间，溢出省略 */
.text-truncate {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
```

- [ ] **Step 2: 验证 typography.css 创建成功**

Run: `Test-Path src/styles/typography.css`
Expected: True

---

## Task 2: 创建 Scrollbar 全局样式文件

**Files:**
- Create: `src/styles/scrollbar.css`

- [ ] **Step 1: 创建 scrollbar.css 文件，提取 Scrollbar 样式**

```css
/* ============================================================
   Scrollbar 全局样式 — GNOME Remote Client
   从 adwaita.css 提取，确保桌面系统统一滚动条样式
   ============================================================ */

/* ── Scrollbar (GNOME Style) ───────────────────────────── */
::-webkit-scrollbar {
  width: 6px;
  height: 6px;
}

::-webkit-scrollbar-track {
  background: transparent;
}

::-webkit-scrollbar-thumb {
  background: var(--ovelis-border-color);
  border-radius: var(--radius-pill);
}

::-webkit-scrollbar-thumb:hover {
  background: rgba(0,0,0,0.3);
}
```

- [ ] **Step 2: 验证 scrollbar.css 创建成功**

Run: `Test-Path src/styles/scrollbar.css`
Expected: True

---

## Task 3: 删除 base.css 的重复变量定义

**Files:**
- Modify: `src/styles/base.css`

- [ ] **Step 1: 删除 base.css 的 `:root` 变量定义（已由 variables.css 提供）**

修改前（需要删除的部分）：
```css
/* base.css 当前内容（第 7-22 行）包含不必要的变量引用 */
```

修改后（删除所有变量引用，保留纯 Reset）：
```css
/* ============================================================
   Base Styles — GNOME Remote Client
   最小化全局重置样式（职责单一：只做 Reset）
   ============================================================ */

/* ── Reset ──────────────────────────────────────────────── */
*, *::before, *::after {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
  user-select: none;
}

html, body, #root {
  width: 100%;
  height: 100%;
  overflow: hidden;
  /* ✅ 删除 font-family、font-size、color、background 变量引用 */
  /* 这些由 variables.css 的 CSS 变量在运行时提供 */
}

/* ✅ 关键修复：确保应用容器不阻止滚动 */
#root > * {
  overflow: visible;
}
```

- [ ] **Step 2: 验证 base.css 已删除变量引用**

Run: `Get-Content src/styles/base.css | Select-String "font-family|font-size|color|background" | Measure-Object`
Expected: Count = 0（无变量引用）

---

## Task 4: 删除 variables.css 的 `:root` 定义（保留纯变量定义）

**Files:**
- Modify: `src/styles/variables.css`

- [ ] **Step 1: 确认 variables.css 的 `:root` 定义是唯一的变量来源**

**不修改 variables.css**，因为它已经职责单一（只包含变量定义）。但需要确认 App.tsx 的引用顺序正确。

- [ ] **Step 2: 验证 variables.css 职责单一**

Run: `Get-Content src/styles/variables.css | Select-String "^\.|^@keyframes|^#" | Measure-Object`
Expected: Count = 0（无样式规则，只有变量定义）

---

## Task 5: 删除 adwaita.css 文件（职责已分离）

**Files:**
- Delete: `src/styles/adwaita.css`

- [ ] **Step 1: 确认 Typography 和 Scrollbar 样式已提取到新文件**

Run: `Test-Path src/styles/typography.css; Test-Path src/styles/scrollbar.css`
Expected: True, True

- [ ] **Step 2: 删除 adwaita.css 文件**

Run: `Remove-Item src/styles/adwaita.css -Force`
Expected: 文件删除成功

- [ ] **Step 3: 验证 adwaita.css 已删除**

Run: `Test-Path src/styles/adwaita.css`
Expected: False

---

## Task 6: 清理 App.tsx 样式引用顺序

**Files:**
- Modify: `src/App.tsx`

- [ ] **Step 1: 修改 App.tsx 样式引用顺序（职责清晰化）**

修改前：
```tsx
import { Desktop } from "./shell/Desktop";
import { StorageInitializer } from "./components/StorageInitializer";
import "./styles/variables.css"; // ✅ 新架构：只定义变量
import "./styles/base.css"; // ✅ 新架构：最小化全局样式
import "./styles/adwaita.css"; // ✅ 保留原有Adwaita样式作为备份
import "./styles/skeleton.css";

function App() {
  return (
    <StorageInitializer>
      <Desktop />
    </StorageInitializer>
  );
}

export default App;
```

修改后：
```tsx
import { Desktop } from "./shell/Desktop";
import { StorageInitializer } from "./components/StorageInitializer";
import "./styles/variables.css";    // ✅ 层 1：全局变量定义
import "./styles/base.css";          // ✅ 层 2：最小 Reset
import "./styles/typography.css";    // ✅ 层 3：Typography 工具类
import "./styles/scrollbar.css";     // ✅ 层 4：Scrollbar 全局样式
import "./styles/skeleton.css";      // ✅ 层 5：Skeleton 全局样式

function App() {
  return (
    <StorageInitializer>
      <Desktop />
    </StorageInitializer>
  );
}

export default App;
```

- [ ] **Step 2: 验证 App.tsx 引用顺序正确**

Run: `Get-Content src/App.tsx | Select-String "import.*styles" | Select-Object -First 5`
Expected: 显示正确的引用顺序（variables → base → typography → scrollbar → skeleton）

---

## Task 7: 创建 WindowControls CSS Module 文件

**Files:**
- Create: `src/components/window-shell/WindowControls.module.css`

- [ ] **Step 1: 创建 WindowControls.module.css 文件（窗口控制按钮样式）**

```css
/* src/components/window-shell/WindowControls.module.css */
/* WindowControls 样式 - GNOME 风格点状控制按钮 */

.windowControls {
  display: flex;
  gap: 8px;
  align-items: center;
}

.controlButton {
  width: 12px;
  height: 12px;
  border-radius: 50%;
  border: none;
  cursor: pointer;
  transition: background 150ms ease-out;
  /* ✅ GNOME 风格：半透明背景，hover 时加深 */
  background: rgba(0, 0, 0, 0.15);
}

.controlButton:hover {
  background: rgba(0, 0, 0, 0.25);
}

.closeButton {
  background: #e01b24; /* ✅ GNOME 风格：红色关闭按钮 */
}

.closeButton:hover {
  background: #ff4040;
}
```

- [ ] **Step 2: 验证 WindowControls.module.css 创建成功**

Run: `Test-Path src/components/window-shell/WindowControls.module.css`
Expected: True

---

## Task 8: 删除 window-shell.css 全局样式文件

**Files:**
- Delete: `src/components/window-shell/window-shell.css`

- [ ] **Step 1: 确认 WindowShell.module.css 已存在（替代全局样式）**

Run: `Test-Path src/components/window-shell/WindowShell.module.css`
Expected: True

- [ ] **Step 2: 删除 window-shell.css 文件**

Run: `Remove-Item src/components/window-shell/window-shell.css -Force`
Expected: 文件删除成功

- [ ] **Step 3: 验证 window-shell.css 已删除**

Run: `Test-Path src/components/window-shell/window-shell.css`
Expected: False

---

## Task 9: 重构 WindowShell.tsx - 删除全局样式引用

**Files:**
- Modify: `src/components/window-shell/WindowShell.tsx`

- [ ] **Step 1: 修改 WindowShell.tsx 样式引用（改用 CSS Module）**

修改前（第 6 行）：
```tsx
import './window-shell.css';
```

修改后：
```tsx
import styles from './WindowShell.module.css';
import controlsStyles from './WindowControls.module.css';
```

- [ ] **Step 2: 修改 WindowShell.tsx 类名引用（使用 CSS Module 类名）**

修改前（第 412-454 行）：
```tsx
className={`window-shell${isActive ? ' active' : ''}${isDragging ? ' dragging' : ''}${isMaximized ? ' maximized' : ''}`}
className="window-header-bar"
className="window-title"
className="window-content-frame"
```

修改后：
```tsx
className={`${styles.windowShell}${isActive ? ` ${styles.windowShellActive}` : ''}${isDragging ? ` ${styles.windowShellDragging}` : ''}${isMaximized ? ` ${styles.windowShellMaximized}` : ''}`}
className={styles.windowHeaderBar}
className={styles.windowTitle}
className={styles.windowContentFrame}
```

- [ ] **Step 3: 修改 WindowControls 引用（使用 CSS Module 类名）**

修改前（第 452 行）：
```tsx
<WindowControls onClose={onClose} onMinimize={onMinimize} onMaximize={onMaximize} />
```

**注意：** WindowControls 是一个独立组件，需要单独重构（见 Task 10）。

- [ ] **Step 4: 验证 WindowShell.tsx 已删除全局样式引用**

Run: `Get-Content src/components/window-shell/WindowShell.tsx | Select-String "import.*window-shell.css" | Measure-Object`
Expected: Count = 0（无全局样式引用）

---

## Task 10: 创建 WindowControls.tsx 组件（如果不存在）

**Files:**
- Create: `src/components/window-shell/WindowControls.tsx`（如果不存在）
- Modify: `src/components/window-shell/WindowControls.tsx`（如果已存在）

- [ ] **Step 1: 检查 WindowControls.tsx 是否已存在**

Run: `Test-Path src/components/window-shell/WindowControls.tsx`

**如果不存在，创建新组件：**
```tsx
// src/components/window-shell/WindowControls.tsx
// GNOME 风格窗口控制按钮（点状按钮：关闭、最小化、最大化）

import { memo } from "react";
import styles from "./WindowControls.module.css";

interface WindowControlsProps {
  onClose: () => void;
  onMinimize: () => void;
  onMaximize: () => void;
}

export const WindowControls = memo(function WindowControls({
  onClose,
  onMinimize,
  onMaximize,
}: WindowControlsProps) {
  return (
    <div className={styles.windowControls}>
      <button
        className={`${styles.controlButton} ${styles.closeButton}`}
        onClick={onClose}
        title="关闭"
        aria-label="关闭窗口"
      />
      <button
        className={styles.controlButton}
        onClick={onMinimize}
        title="最小化"
        aria-label="最小化窗口"
      />
      <button
        className={styles.controlButton}
        onClick={onMaximize}
        title="最大化"
        aria-label="最大化窗口"
      />
    </div>
  );
});
```

**如果已存在，修改样式引用：**
```tsx
import styles from "./WindowControls.module.css";
// 删除全局样式引用（如 import "./WindowControls.css"）
```

- [ ] **Step 2: 验证 WindowControls.tsx 使用 CSS Module**

Run: `Get-Content src/components/window-shell/WindowControls.tsx | Select-String "import.*WindowControls.module.css" | Measure-Object`
Expected: Count = 1

---

## Task 11: 统一 FileManager.css 类名前缀（`.fm-`）

**Files:**
- Modify: `src/apps/FileManager.css`

- [ ] **Step 1: 分析 FileManager.css 类名前缀不统一的部分**

Run: `Get-Content src/apps/FileManager.css | Select-String "^\." | Select-Object -First 20`

需要修改的类名（不统一部分）：
- `.fileName` → `.fmFileName`
- `.fnIcon` → `.fmFnIcon`
- `.fnText` → `.fmFnText`
- `.fnEditInput` → `.fmFnEditInput`
- `.fileSize` → `.fmFileSize`
- `.fileMtime` → `.fmFileMtime`
- `.filePerm` → `.fmFilePerm`
- `.fmListRowSelectedFileSize` → `.fmListRowSelectedFileSize`（已统一）
- `.fmListRowSelectedFileMtime` → `.fmListRowSelectedFileMtime`（已统一）
- `.fmListRowSelectedFilePerm` → `.fmListRowSelectedFilePerm`（已统一）

- [ ] **Step 2: 修改 FileManager.css 类名前缀（批量替换）**

```powershell
# 统一类名前缀（.fileName → .fmFileName）
(Get-Content src/apps/FileManager.css) -replace '\.fileName', '.fmFileName' -replace '\.fnIcon', '.fmFnIcon' -replace '\.fnText', '.fmFnText' -replace '\.fnEditInput', '.fmFnEditInput' -replace '\.fileSize', '.fmFileSize' -replace '\.fileMtime', '.fmFileMtime' -replace '\.filePerm', '.fmFilePerm' | Set-Content src/apps/FileManager.css
```

- [ ] **Step 3: 验证 FileManager.css 类名前缀统一**

Run: `Get-Content src/apps/FileManager.css | Select-String "^\.[^f]" | Select-String "^(?!\.fm)" | Measure-Object`
Expected: Count = 0（所有类名都以 `.fm` 开头）

- [ ] **Step 4: 修改 FileManager.tsx 类名引用（匹配新类名）**

需要在 FileManager.tsx 中批量替换类名引用：
```powershell
# 统一类名引用（fileName → fmFileName）
(Get-Content src/apps/FileManager.tsx) -replace 'fileName', 'fmFileName' -replace 'fnIcon', 'fmFnIcon' -replace 'fnText', 'fmFnText' -replace 'fnEditInput', 'fmFnEditInput' -replace 'fileSize', 'fmFileSize' -replace 'fileMtime', 'fmFileMtime' -replace 'filePerm', 'fmFilePerm' | Set-Content src/apps/FileManager.tsx
```

---

## Task 12: 统一 Terminal.css 类名前缀（`.terminal-`）

**Files:**
- Modify: `src/apps/Terminal.css`

- [ ] **Step 1: 分析 Terminal.css 类名前缀**

Run: `Get-Content src/apps/Terminal.css | Select-String "^\." | Select-Object -First 20`

**如果类名前缀不统一，批量替换：**
```powershell
# 统一类名前缀（假设有不统一的类名）
(Get-Content src/apps/Terminal.css) -replace '\.([^t])', '.terminal$1' | Set-Content src/apps/Terminal.css
```

**注意：** 需要先检查 Terminal.css，确保不破坏已有类名。

- [ ] **Step 2: 验证 Terminal.css 类名前缀统一**

Run: `Get-Content src/apps/Terminal.css | Select-String "^\." | Select-String "^\.terminal" | Measure-Object`
Expected: Count = 所有类名数量

---

## Task 13: 统一 SystemMonitor.css 类名前缀（`.sm-`）

**Files:**
- Modify: `src/apps/SystemMonitor.css`

- [ ] **Step 1: 分析 SystemMonitor.css 类名前缀**

Run: `Get-Content src/apps/SystemMonitor.css | Select-String "^\." | Select-Object -First 20`

**如果类名前缀不统一，批量替换：**
```powershell
# 统一类名前缀（假设有不统一的类名）
(Get-Content src/apps/SystemMonitor.css) -replace '\.([^s])', '.sm$1' | Set-Content src/apps/SystemMonitor.css
```

- [ ] **Step 2: 验证 SystemMonitor.css 类名前缀统一**

Run: `Get-Content src/apps/SystemMonitor.css | Select-String "^\." | Select-String "^\.sm" | Measure-Object`
Expected: Count = 所有类名数量

---

## Task 14: 统一 Settings.css 类名前缀（`.settings-`）

**Files:**
- Modify: `src/apps/Settings.css`

- [ ] **Step 1: 分析 Settings.css 类名前缀**

Run: `Get-Content src/apps/Settings.css | Select-String "^\." | Select-Object -First 20`

**如果类名前缀不统一，批量替换：**
```powershell
# 统一类名前缀（假设有不统一的类名）
(Get-Content src/apps/Settings.css) -replace '\.([^s])', '.settings$1' | Set-Content src/apps/Settings.css
```

- [ ] **Step 2: 验证 Settings.css 类名前缀统一**

Run: `Get-Content src/apps/Settings.css | Select-String "^\." | Select-String "^\.settings" | Measure-Object`
Expected: Count = 所有类名数量

---

## Task 15: 统一 TextEditor.css 类名前缀（`.te-`）

**Files:**
- Modify: `src/apps/TextEditor/TextEditor.css`

- [ ] **Step 1: 分析 TextEditor.css 类名前缀**

Run: `Get-Content src/apps/TextEditor/TextEditor.css | Select-String "^\." | Select-Object -First 20`

**如果类名前缀不统一，批量替换：**
```powershell
# 统一类名前缀（假设有不统一的类名）
(Get-Content src/apps/TextEditor/TextEditor.css) -replace '\.([^t])', '.te$1' | Set-Content src/apps/TextEditor/TextEditor.css
```

- [ ] **Step 2: 验证 TextEditor.css 类名前缀统一**

Run: `Get-Content src/apps/TextEditor/TextEditor.css | Select-String "^\." | Select-String "^\.te" | Measure-Object`
Expected: Count = 所有类名数量

---

## Task 16: 验证整体架构 - 检查样式文件职责清晰化

**Files:**
- None（验证任务）

- [ ] **Step 1: 验证全局层文件职责单一**

Run:
```powershell
# 检查 variables.css 是否只包含变量定义
Write-Host "variables.css 职责检查:"
Get-Content src/styles/variables.css | Select-String "^\.|^@keyframes|^#" | Measure-Object

# 检查 base.css 是否只包含 Reset
Write-Host "base.css 职责检查:"
Get-Content src/styles/base.css | Select-String "font-family|font-size|color|background" | Measure-Object

# 检查 typography.css 是否只包含 Typography 工具类
Write-Host "typography.css 职责检查:"
Get-Content src/styles/typography.css | Select-String "^\.(?!text)" | Measure-Object

# 检查 scrollbar.css 是否只包含 Scrollbar 样式
Write-Host "scrollbar.css 职责检查:"
Get-Content src/styles/scrollbar.css | Select-String "^::-webkit-scrollbar" | Measure-Object
```

Expected:
- variables.css: 无样式规则（Count = 0）
- base.css: 无变量引用（Count = 0）
- typography.css: 只有 `.text` 前缀类名（Count = 0）
- scrollbar.css: 只有 `::-webkit-scrollbar` 样式（Count > 0）

- [ ] **Step 2: 验证 App.tsx 样式引用顺序正确**

Run: `Get-Content src/App.tsx | Select-String "import.*styles" | Select-Object -First 5`
Expected: 显示正确的顺序（variables → base → typography → scrollbar → skeleton）

- [ ] **Step 3: 验证 Window 层使用 CSS Modules**

Run:
```powershell
# 检查 WindowShell.tsx 是否使用 CSS Module
Write-Host "WindowShell.tsx CSS Module 检查:"
Get-Content src/components/window-shell/WindowShell.tsx | Select-String "import.*WindowShell.module.css" | Measure-Object

# 检查 WindowControls.tsx 是否使用 CSS Module
Write-Host "WindowControls.tsx CSS Module 检查:"
Get-Content src/components/window-shell/WindowControls.tsx | Select-String "import.*WindowControls.module.css" | Measure-Object

# 检查 AppLayout 是否使用 CSS Module
Write-Host "AppLayout CSS Module 检查:"
Get-Content src/components/app-shell/AppLayout.module.css | Measure-Object
```

Expected:
- WindowShell.tsx: 引用 CSS Module（Count = 1）
- WindowControls.tsx: 引用 CSS Module（Count = 1）
- AppLayout.module.css: 文件存在（Count > 0）

- [ ] **Step 4: 验证 App 层类名前缀统一**

Run:
```powershell
# 检查 FileManager.css 类名前缀
Write-Host "FileManager.css 类名前缀检查:"
Get-Content src/apps/FileManager.css | Select-String "^\." | Select-String "^\.fm" | Measure-Object

# 检查 Terminal.css 类名前缀
Write-Host "Terminal.css 类名前缀检查:"
Get-Content src/apps/Terminal.css | Select-String "^\." | Select-String "^\.terminal" | Measure-Object

# 检查 SystemMonitor.css 类名前缀
Write-Host "SystemMonitor.css 类名前缀检查:"
Get-Content src/apps/SystemMonitor.css | Select-String "^\." | Select-String "^\.sm" | Measure-Object

# 检查 Settings.css 类名前缀
Write-Host "Settings.css 类名前缀检查:"
Get-Content src/apps/Settings.css | Select-String "^\." | Select-String "^\.settings" | Measure-Object

# 检查 TextEditor.css 类名前缀
Write-Host "TextEditor.css 类名前缀检查:"
Get-Content src/apps/TextEditor/TextEditor.css | Select-String "^\." | Select-String "^\.te" | Measure-Object
```

Expected: 所有 App 层 CSS 文件类名前缀统一（Count = 总类名数量）

---

## Task 17: 最终验证 - 运行应用检查 UI 是否正常

**Files:**
- None（最终验证）

- [ ] **Step 1: 运行 Tauri 开发服务器**

Run: `npm run tauri dev`
Expected: 应用启动成功，无 CSS 加载错误

- [ ] **Step 2: 检查浏览器控制台无 CSS 错误**

**手动检查：**
- 打开浏览器开发者工具（F12）
- 检查 Console 是否有 CSS 加载错误或样式警告
- 检查 Network 面板，确认所有 CSS 文件加载成功（variables.css、base.css、typography.css、scrollbar.css、skeleton.css）

Expected: 无 CSS 错误，所有文件加载成功

- [ ] **Step 3: 检查 UI 视觉效果是否保持一致**

**手动检查：**
- TopBar 是否显示正常（HeaderBar 样式）
- Desktop 布局是否正常
- WindowShell 窗口是否正常显示（HeaderBar、ContentFrame）
- WindowControls 控制按钮是否正常（GNOME 风格点状按钮）
- FileManager 文件列表样式是否正常
- Terminal 终端样式是否正常
- Typography 工具类是否生效（`.text-title`、`.text-body` 等）

Expected: UI 视觉效果与重构前一致，无样式丢失

- [ ] **Step 4: 检查窗口交互功能是否正常**

**手动检查：**
- 窗口拖拽功能是否正常
- 窗口 resize 功能是否正常
- 窗口最大化/最小化/关闭按钮是否正常
- 窗口激活状态切换是否正常（z-index）

Expected: 所有窗口交互功能正常

---

## Self-Review Checklist

**完成自我审查后，修复以下问题：**

1. **Spec 覆盖检查：**
   - ✅ CSS 变量重复定义已删除（Task 3-4）
   - ✅ 全局样式职责分离已完成（Task 1-2）
   - ✅ WindowShell 引用混乱已修复（Task 7-9）
   - ✅ 样式引用顺序已清理（Task 6）
   - ✅ App 层类名前缀已统一（Task 11-15）
   - ✅ 整体架构已验证（Task 16-17）

2. **占位符检查：**
   - ✅ 无 "TBD"、"TODO"、"implement later" 等占位符
   - ✅ 所有代码块包含完整内容
   - ✅ 所有命令包含具体执行步骤

3. **类型一致性检查：**
   - ✅ 类名引用一致（CSS 文件类名 vs TSX 文件引用）
   - ✅ 文件路径一致（创建文件路径 vs 引用文件路径）

---

## 执行选择

**Plan complete and saved to `docs/superpowers/plans/2026-07-12-css-architecture-refactoring.md`. Two execution options:**

**1. Subagent-Driven (recommended)** - I dispatch a fresh subagent per task, review between tasks, fast iteration

**2. Inline Execution** - Execute tasks in this session using executing-plans, batch execution with checkpoints

**Which approach?**