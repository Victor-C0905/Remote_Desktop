# 标签组件布局修复实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复标签组件在窗口变化时的收缩和间距问题，实现固定尺寸 + 6px 间距 + 滚动条

**Architecture:** 通过修改 CSS 文件，为标签添加 `flex-shrink: 0`、`min-width` 和横向滚动支持，遵循 GNOME HIG 设计规范

**Tech Stack:** CSS (flexbox, scrollbar styling)

---

## 文件结构

**修改文件：**
1. `src/apps/Terminal.css` - 终端标签栏样式（已有滚动支持，最简单）
2. `src/apps/SystemMonitor.css` - 系统监控标签栏样式（需新增滚动支持）
3. `src/shell/NotificationCenter.css` - 通知中心过滤器样式（需新增滚动支持）

---

## Task 1: 修改 Terminal 标签栏样式

**Files:**
- Modify: `src/apps/Terminal.css:173-225`

- [ ] **Step 1: 修改 `.terminal-tab-bar` 的 `gap` 属性**

打开 `src/apps/Terminal.css`，找到第 181 行的 `gap: 2px;`，修改为：

```css
.terminal-tab-bar {
  display: flex;
  align-items: center;
  height: 36px;
  background: var(--headerbar-bg);
  border-bottom: 1px solid var(--border-color);
  padding: 0 4px;
  gap: 6px;  /* ✅ 修改：从 2px 改为 6px */
  flex-shrink: 0;
  z-index: 10;
  position: relative;
  overflow-x: auto;
  overflow-y: hidden;
  scrollbar-width: thin;
  scrollbar-color: var(--border-color) transparent;
}
```

- [ ] **Step 2: 修改 `.terminal-tab` 的 `flex-shrink` 属性**

找到第 223 行的 `flex-shrink: 1;`，修改为：

```css
.terminal-tab {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 4px 8px;
  height: 28px;
  font-size: 12px;
  color: var(--text-secondary);
  border-radius: var(--radius-xs) var(--radius-xs) 0 0;
  cursor: pointer;
  border: none;
  background: transparent;
  max-width: 180px;
  min-width: 100px;  /* ✅ 修改：从 60px 改为 100px */
  white-space: nowrap;
  flex-shrink: 0;  /* ✅ 修改：从 1 改为 0 */
  flex-grow: 0;
}
```

- [ ] **Step 3: 验证修改**

启动应用，打开 Terminal，创建 5-10 个标签，缩小窗口宽度，验证：
- 标签保持固定尺寸，不收缩
- 标签间距为 6px
- 出现横向滚动条

- [ ] **Step 4: 提交修改**

```bash
git add src/apps/Terminal.css
git commit -m "fix: Terminal 标签栏固定尺寸和间距优化

- 修改标签间距从 2px 到 6px（符合 GNOME HIG）
- 禁止标签收缩（flex-shrink: 0）
- 增加最小宽度从 60px 到 100px
- 保持横向滚动支持"
```

---

## Task 2: 修改 SystemMonitor 标签栏样式

**Files:**
- Modify: `src/apps/SystemMonitor.css:24-49`

- [ ] **Step 1: 修改 `.sm-tabs` 的 `gap` 属性**

打开 `src/apps/SystemMonitor.css`，找到第 26 行的 `gap: 2px;`，修改为：

```css
.sm-tabs {
  display: flex;
  gap: 6px;  /* ✅ 修改：从 2px 改为 6px */
  overflow-x: auto;  /* ✅ 新增：横向滚动支持 */
  overflow-y: hidden;  /* ✅ 新增：禁止纵向滚动 */
  scrollbar-width: thin;  /* ✅ 新增：细滚动条 */
  scrollbar-color: var(--border-color) transparent;  /* ✅ 新增：滚动条颜色 */
}
```

- [ ] **Step 2: 为 `.sm-tab` 添加固定尺寸属性**

找到第 29-39 行的 `.sm-tab` 样式，添加：

```css
.sm-tab {
  padding: 6px 16px;
  font-size: var(--font-body);
  font-weight: 600;
  color: var(--text-secondary);
  background: none;
  border: none;
  border-radius: var(--radius-xs);
  cursor: pointer;
  transition: all var(--duration-fast) var(--ease-out);
  flex-shrink: 0;  /* ✅ 新增：禁止收缩 */
  min-width: 80px;  /* ✅ 新增：固定最小宽度 */
  white-space: nowrap;  /* ✅ 新增：禁止换行 */
}
```

- [ ] **Step 3: 添加滚动条样式（WebKit 浏览器）**

在 `.sm-tab` 样式后添加：

```css
.sm-tabs::-webkit-scrollbar {
  height: 4px;
}

.sm-tabs::-webkit-scrollbar-track {
  background: transparent;
}

.sm-tabs::-webkit-scrollbar-thumb {
  background: var(--border-color);
  border-radius: var(--radius-pill);
}

.sm-tabs::-webkit-scrollbar-thumb:hover {
  background: rgba(0,0,0,0.2);
}
```

- [ ] **Step 4: 验证修改**

启动应用，打开 SystemMonitor，切换不同标签（进程、资源、文件系统），缩小窗口宽度，验证：
- 标签保持固定尺寸，不收缩
- 标签间距为 6px
- 出现横向滚动条

- [ ] **Step 5: 提交修改**

```bash
git add src/apps/SystemMonitor.css
git commit -m "fix: SystemMonitor 标签栏固定尺寸和间距优化

- 修改标签间距从 2px 到 6px（符合 GNOME HIG）
- 新增横向滚动支持（overflow-x: auto）
- 禁止标签收缩（flex-shrink: 0）
- 新增固定最小宽度 80px
- 新增 WebKit 滚动条样式"
```

---

## Task 3: 修改 NotificationCenter 过滤器样式

**Files:**
- Modify: `src/shell/NotificationCenter.css:120-148`

- [ ] **Step 1: 修改 `.nc-filters` 的 `gap` 属性**

打开 `src/shell/NotificationCenter.css`，找到第 123 行的 `gap: 4px;`，修改为：

```css
.nc-filters {
  display: flex;
  gap: 6px;  /* ✅ 修改：从 4px 改为 6px */
  padding: 8px 16px;
  border-bottom: 1px solid var(--border-color);
  flex-shrink: 0;
  overflow-x: auto;  /* ✅ 新增：横向滚动支持 */
  overflow-y: hidden;  /* ✅ 新增：禁止纵向滚动 */
  scrollbar-width: thin;  /* ✅ 新增：细滚动条 */
  scrollbar-color: var(--border-color) transparent;  /* ✅ 新增：滚动条颜色 */
}
```

- [ ] **Step 2: 为 `.nc-filter-btn` 添加固定尺寸属性**

找到第 129-138 行的 `.nc-filter-btn` 样式，添加：

```css
.nc-filter-btn {
  font-size: var(--font-small);
  color: var(--text-secondary);
  background: var(--card-bg);
  border: none;
  border-radius: var(--radius-xs);
  padding: 4px 10px;
  cursor: pointer;
  transition: all var(--duration-fast) var(--ease-out);
  flex-shrink: 0;  /* ✅ 新增：禁止收缩 */
  min-width: 60px;  /* ✅ 新增：固定最小宽度 */
  white-space: nowrap;  /* ✅ 新增：禁止换行 */
}
```

- [ ] **Step 3: 添加滚动条样式（WebKit 浏览器）**

在 `.nc-filter-btn` 样式后添加：

```css
.nc-filters::-webkit-scrollbar {
  height: 4px;
}

.nc-filters::-webkit-scrollbar-track {
  background: transparent;
}

.nc-filters::-webkit-scrollbar-thumb {
  background: var(--border-color);
  border-radius: var(--radius-pill);
}

.nc-filters::-webkit-scrollbar-thumb:hover {
  background: rgba(0,0,0,0.2);
}
```

- [ ] **Step 4: 验证修改**

启动应用，打开 NotificationCenter，切换不同过滤器（全部、紧急、普通、低），缩小窗口宽度，验证：
- 标签保持固定尺寸，不收缩
- 标签间距为 6px
- 出现横向滚动条

- [ ] **Step 5: 提交修改**

```bash
git add src/shell/NotificationCenter.css
git commit -m "fix: NotificationCenter 过滤器固定尺寸和间距优化

- 修改标签间距从 4px 到 6px（符合 GNOME HIG）
- 新增横向滚动支持（overflow-x: auto）
- 禁止标签收缩（flex-shrink: 0）
- 新增固定最小宽度 60px
- 新增 WebKit 滚动条样式"
```

---

## Task 4: 综合验证

**Files:**
- None (验证阶段)

- [ ] **Step 1: 启动应用并测试所有页面**

启动应用，依次测试：
1. Terminal.tsx - 打开 5-10 个标签，缩小窗口
2. SystemMonitor.tsx - 切换标签，缩小窗口
3. NotificationCenter.tsx - 切换过滤器，缩小窗口

- [ ] **Step 2: 验证固定尺寸**

确认所有标签：
- 保持固定最小宽度，不随窗口收缩
- 标签间距统一为 6px
- 标签内容不换行

- [ ] **Step 3: 验证滚动条**

确认所有标签容器：
- 窗口缩小时出现横向滚动条
- 滚动条样式正常（细滚动条）
- 可以通过滚动访问所有标签

- [ ] **Step 4: 验证极端情况**

测试极端情况：
- 窗口极小（300px 宽）
- 标签仍然保持最小宽度
- 滚动条正常工作

- [ ] **Step 5: 最终提交（如果需要）**

如果所有验证通过，无需额外提交。如果发现问题，修复后提交：

```bash
git add src/apps/Terminal.css src/apps/SystemMonitor.css src/shell/NotificationCenter.css
git commit -m "fix: 标签组件布局修复完成

- 所有标签保持固定尺寸，不收缩
- 标签间距统一为 6px（符合 GNOME HIG）
- 添加横向滚动条处理溢出
- 符合 GNOME Terminal 设计规范"
```

---

## Self-Review Checklist

**1. Spec coverage:**
- ✅ Terminal 标签栏修改（Task 1）
- ✅ SystemMonitor 标签栏修改（Task 2）
- ✅ NotificationCenter 过滤器修改（Task 3）
- ✅ 综合验证（Task 4）

**2. Placeholder scan:**
- ✅ 无 "TBD"、"TODO"、"implement later"
- ✅ 无模糊描述（如 "add appropriate error handling"）
- ✅ 所有步骤都有具体代码或命令
- ✅ 无引用未定义的类型或函数

**3. Type consistency:**
- ✅ CSS 类名一致（`.terminal-tab-bar`, `.sm-tabs`, `.nc-filters`）
- ✅ CSS 属性名一致（`flex-shrink`, `min-width`, `gap`）
- ✅ 验证步骤一致（缩小窗口 → 验证固定尺寸 → 验证滚动条）

---

## 执行选项

计划已完成并保存到 `docs/superpowers/plans/2026-06-25-tab-layout-fix.md`。

**两种执行方式：**

**1. Subagent-Driven（推荐）** - 我为每个任务派发一个新的子代理，任务间进行审查，快速迭代

**2. Inline Execution** - 在此会话中使用 executing-plans 执行，批量执行并设置检查点进行审查

**您选择哪种方式？**

---

**注意：根据用户规则，我不会控制 git 版本，不回退，不提交代码，只提示您需要这么做，您来操作。**

因此，所有 git commit 步骤将改为提示您手动执行。