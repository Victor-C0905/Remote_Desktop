# 内容区溢出与重叠修复实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复 Settings 和 FileManager 内容区在窗口缩小时的溢出与重叠问题，实现最小宽度 + 溢出隐藏 + 省略号显示

**Architecture:** 通过修改 CSS 文件，为内容区设置最小宽度，添加溢出隐藏和省略号显示，遵循 GNOME HIG 设计规范

**Tech Stack:** CSS (min-width, overflow, text-overflow, white-space, minmax)

---

## 文件结构

**修改文件：**
1. `src/apps/Settings.css` - Settings 内容区样式（4 个修改）
2. `src/apps/FileManager.css` - FileManager 文件列表样式（5 个修改）

---

## Task 1: 修改 Settings.css - `.st-main` 设置最小宽度

**Files:**
- Modify: `src/apps/Settings.css:125-131`

- [ ] **Step 1: 修改 `.st-main` 的 `min-width` 属性**

打开 `src/apps/Settings.css`，找到第 125-131 行的 `.st-main` 样式，修改：

```css
.st-main {
  flex: 1;
  overflow-y: auto;
  overflow-x: hidden;
  padding: 16px 24px;
  min-width: 400px; /* ✅ 修改：从 0 改为 400px（最小可读宽度） */
}
```

- [ ] **Step 2: 验证修改**

启动应用，打开 Settings，缩小窗口宽度，验证：
- 窗口 > 600px：内容正常显示
- 窗口 400-600px：内容适度收缩
- 窗口 < 400px：内容保持最小宽度

- [ ] **Step 3: 提交修改**

```bash
git add src/apps/Settings.css
git commit -m "fix: Settings 内容区设置最小宽度 400px

- 修改 .st-main 的 min-width 从 0 改为 400px
- 确保内容区至少有 400px 宽度，保证可读性
- 防止内容过度压缩导致重叠"
```

---

## Task 2: 修改 Settings.css - `.st-card` 设置最小宽度

**Files:**
- Modify: `src/apps/Settings.css:146-154`

- [ ] **Step 1: 修改 `.st-card` 的 `min-width` 属性**

打开 `src/apps/Settings.css`，找到第 146-154 行的 `.st-card` 样式，修改：

```css
.st-card {
  background: var(--view-bg);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  padding: 16px;
  margin-bottom: 16px;
  overflow: hidden;
  min-width: 300px; /* ✅ 修改：从 0 改为 300px（卡片最小宽度） */
}
```

- [ ] **Step 2: 验证修改**

启动应用，打开 Settings，缩小窗口宽度，验证：
- 卡片保持最小宽度 300px
- 卡片内容不会过度压缩

- [ ] **Step 3: 提交修改**

```bash
git add src/apps/Settings.css
git commit -m "fix: Settings 卡片设置最小宽度 300px

- 修改 .st-card 的 min-width 从 0 改为 300px
- 确保卡片至少有 300px 宽度
- 防止卡片过度压缩导致内容重叠"
```

---

## Task 3: 修改 Settings.css - `.st-section-title` 添加防重叠保护

**Files:**
- Modify: `src/apps/Settings.css:138-143`

- [ ] **Step 1: 为 `.st-section-title` 添加防重叠属性**

打开 `src/apps/Settings.css`，找到第 138-143 行的 `.st-section-title` 样式，添加：

```css
.st-section-title {
  font-size: 16pt;
  font-weight: 700;
  color: var(--text-primary);
  margin-bottom: 16px;
  white-space: nowrap; /* ✅ 新增：禁止换行 */
  overflow: hidden; /* ✅ 新增：隐藏溢出 */
  text-overflow: ellipsis; /* ✅ 新增：显示省略号 */
}
```

- [ ] **Step 2: 验证修改**

启动应用，打开 Settings，缩小窗口宽度，验证：
- 标题不换行
- 标题超出部分显示省略号（...）

- [ ] **Step 3: 提交修改**

```bash
git add src/apps/Settings.css
git commit -m "fix: Settings 标题添加防重叠保护

- 新增 white-space: nowrap 禁止换行
- 新增 overflow: hidden 隐藏溢出
- 新增 text-overflow: ellipsis 显示省略号
- 防止标题过度压缩或换行"
```

---

## Task 4: 修改 Settings.css - `.st-sb-label` 添加防重叠保护

**Files:**
- Modify: `src/apps/Settings.css:118-122`

- [ ] **Step 1: 为 `.st-sb-label` 添加防重叠属性**

打开 `src/apps/Settings.css`，找到第 118-122 行的 `.st-sb-label` 样式，添加：

```css
.st-sb-label {
  font-size: var(--font-body);
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden; /* ✅ 新增：隐藏溢出 */
  text-overflow: ellipsis; /* ✅ 新增：显示省略号 */
}
```

- [ ] **Step 2: 验证修改**

启动应用，打开 Settings，缩小窗口宽度，验证：
- 侧边栏标签不换行
- 标签超出部分显示省略号

- [ ] **Step 3: 提交修改**

```bash
git add src/apps/Settings.css
git commit -m "fix: Settings 侧边栏标签添加防重叠保护

- 新增 overflow: hidden 隐藏溢出
- 新增 text-overflow: ellipsis 显示省略号
- 防止侧边栏标签过度压缩"
```

---

## Task 5: 修改 FileManager.css - `.fm-main` 设置最小宽度

**Files:**
- Modify: `src/apps/FileManager.css:247-253`

- [ ] **Step 1: 修改 `.fm-main` 的 `min-width` 属性**

打开 `src/apps/FileManager.css`，找到第 247-253 行的 `.fm-main` 样式，修改：

```css
.fm-main {
  flex: 1;
  overflow-y: auto;
  overflow-x: hidden;
  padding: 8px;
  min-width: 500px; /* ✅ 修改：从 0 改为 500px（最小可读宽度） */
}
```

- [ ] **Step 2: 验证修改**

启动应用，打开 FileManager，缩小窗口宽度，验证：
- 窗口 > 600px：文件列表正常显示
- 窗口 500-600px：文件列表适度收缩
- 窗口 < 500px：文件列表保持最小宽度

- [ ] **Step 3: 提交修改**

```bash
git add src/apps/FileManager.css
git commit -m "fix: FileManager 文件列表设置最小宽度 500px

- 修改 .fm-main 的 min-width 从 0 改为 500px
- 确保文件列表至少有 500px 宽度
- 保证文件名列有足够空间显示"
```

---

## Task 6: 修改 FileManager.css - `.fm-list-header` 设置最小列宽

**Files:**
- Modify: `src/apps/FileManager.css:260-274`

- [ ] **Step 1: 修改 `.fm-list-header` 的 `grid-template-columns`**

打开 `src/apps/FileManager.css`，找到第 260-274 行的 `.fm-list .fm-list-header` 样式，修改：

```css
.fm-list .fm-list-header {
  display: grid;
  grid-template-columns: minmax(200px, 1fr) 80px 120px 100px; /* ✅ 修改：第一列使用 minmax */
  gap: 8px;
  font-size: var(--font-small);
  font-weight: 600;
  color: var(--text-secondary);
  position: sticky;
  top: -8px;
  margin: -8px -8px 0 -8px;
  padding: 12px 20px;
  background: var(--window-bg);
  z-index: 5;
}
```

- [ ] **Step 2: 验证修改**

启动应用，打开 FileManager，缩小窗口宽度，验证：
- 文件名列保持最小宽度 200px
- 其他列保持固定宽度

- [ ] **Step 3: 提交修改**

```bash
git add src/apps/FileManager.css
git commit -m "fix: FileManager 文件列表标题设置最小列宽

- 修改 grid-template-columns 第一列使用 minmax(200px, 1fr)
- 确保文件名列至少有 200px 宽度
- 防止文件名列过度压缩导致标题重叠"
```

---

## Task 7: 修改 FileManager.css - `.fm-list-row` 设置最小列宽

**Files:**
- Modify: `src/apps/FileManager.css:276-285`

- [ ] **Step 1: 修改 `.fm-list-row` 的 `grid-template-columns`**

打开 `src/apps/FileManager.css`，找到第 276-285 行的 `.fm-list .fm-list-row` 样式，修改：

```css
.fm-list .fm-list-row {
  display: grid;
  grid-template-columns: minmax(200px, 1fr) 80px 120px 100px; /* ✅ 修改：第一列使用 minmax */
  gap: 8px;
  padding: 6px 12px;
  border-radius: var(--radius-xs);
  cursor: pointer;
  align-items: center;
  transition: background var(--duration-fast) var(--ease-out);
}
```

- [ ] **Step 2: 验证修改**

启动应用，打开 FileManager，缩小窗口宽度，验证：
- 文件名保持最小宽度 200px
- 文件名不会过度压缩

- [ ] **Step 3: 提交修改**

```bash
git add src/apps/FileManager.css
git commit -m "fix: FileManager 文件列表行设置最小列宽

- 修改 grid-template-columns 第一列使用 minmax(200px, 1fr)
- 确保文件名至少有 200px 宽度
- 防止文件名过度压缩导致内容重叠"
```

---

## Task 8: 修改 FileManager.css - `.fm-list-row .file-name` 添加防重叠保护

**Files:**
- Modify: `src/apps/FileManager.css:296-302`

- [ ] **Step 1: 修改 `.fm-list-row .file-name` 的 `min-width` 属性**

打开 `src/apps/FileManager.css`，找到第 296-302 行的 `.fm-list .fm-list-row .file-name` 样式，修改：

```css
.fm-list .fm-list-row .file-name {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 200px; /* ✅ 修改：从 0 改为 200px */
  overflow: hidden;
}
```

- [ ] **Step 2: 验证修改**

启动应用，打开 FileManager，缩小窗口宽度，验证：
- 文件名容器保持最小宽度 200px

- [ ] **Step 3: 提交修改**

```bash
git add src/apps/FileManager.css
git commit -m "fix: FileManager 文件名容器设置最小宽度

- 修改 .file-name 的 min-width 从 0 改为 200px
- 确保文件名容器最小宽度
- 防止文件名过度压缩"
```

---

## Task 9: 修改 FileManager.css - `.fm-list-row .file-name .fn-text` 添加防重叠保护

**Files:**
- Modify: `src/apps/FileManager.css:309-315`

- [ ] **Step 1: 为 `.fm-list-row .file-name .fn-text` 添加防重叠属性**

打开 `src/apps/FileManager.css`，找到第 309-315 行的 `.fm-list .fm-list-row .file-name .fn-text` 样式，添加：

```css
.fm-list .fm-list-row .file-name .fn-text {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap; /* ✅ 新增：禁止换行 */
}
```

- [ ] **Step 2: 验证修改**

启动应用，打开 FileManager，缩小窗口宽度，验证：
- 文件名不换行
- 文件名超出部分显示省略号

- [ ] **Step 3: 提交修改**

```bash
git add src/apps/FileManager.css
git commit -m "fix: FileManager 文件名文本添加防重叠保护

- 新增 white-space: nowrap 禁止换行
- 保持 text-overflow: ellipsis 显示省略号
- 防止文件名过度压缩或换行"
```

---

## Task 10: 修改 FileManager.css - `.fm-breadcrumb` 设置最小宽度

**Files:**
- Modify: `src/apps/FileManager.css:51-63`

- [ ] **Step 1: 修改 `.fm-breadcrumb` 的 `min-width` 属性**

打开 `src/apps/FileManager.css`，找到第 51-63 行的 `.fm-breadcrumb` 样式，修改：

```css
.fm-breadcrumb {
  display: flex;
  align-items: center;
  gap: 2px;
  flex: 1;
  min-width: 200px; /* ✅ 修改：从 0 改为 200px */
  padding: 0 8px;
  height: 32px;
  background: var(--view-bg);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  overflow: hidden;
}
```

- [ ] **Step 2: 验证修改**

启动应用，打开 FileManager，缩小窗口宽度，验证：
- 面包屑保持最小宽度 200px
- 面包屑不会过度压缩

- [ ] **Step 3: 提交修改**

```bash
git add src/apps/FileManager.css
git commit -m "fix: FileManager 面包屑设置最小宽度

- 修改 .fm-breadcrumb 的 min-width 从 0 改为 200px
- 确保面包屑至少有 200px 宽度
- 防止面包屑过度压缩"
```

---

## Task 11: 综合验证

**Files:**
- None (验证阶段)

- [ ] **Step 1: 启动应用并测试所有页面**

启动应用，依次测试：
1. Settings - 打开不同设置页面，缩小窗口
2. FileManager - 打开文件列表，缩小窗口

- [ ] **Step 2: 验证 Settings 内容区**

确认 Settings 内容区：
- 窗口 > 600px：内容正常显示，无溢出
- 窗口 400-600px：内容适度收缩，标题显示省略号
- 窗口 < 400px：内容保持最小宽度，标题显示省略号

- [ ] **Step 3: 验证 FileManager 文件列表**

确认 FileManager 文件列表：
- 窗口 > 600px：文件名列正常显示，无溢出
- 窗口 500-600px：文件名列适度收缩，长文件名显示省略号
- 窗口 < 500px：文件名列保持最小宽度，长文件名显示省略号

- [ ] **Step 4: 验证极端情况**

测试极端情况：
- 窗口极小（300px 宽）
- 内容仍然保持最小宽度
- 标题和文件名显示省略号

- [ ] **Step 5: 最终提交（如果需要）**

如果所有验证通过，无需额外提交。如果发现问题，修复后提交：

```bash
git add src/apps/Settings.css src/apps/FileManager.css
git commit -m "fix: 内容区溢出与重叠修复完成

- Settings 内容区设置最小宽度 400px
- FileManager 文件列表设置最小宽度 500px
- 所有文本添加防重叠保护（overflow + text-overflow + white-space）
- 符合 GNOME HIG 设计规范"
```

---

## Self-Review Checklist

**1. Spec coverage:**
- ✅ Settings `.st-main` 修改（Task 1）
- ✅ Settings `.st-card` 修改（Task 2）
- ✅ Settings `.st-section-title` 修改（Task 3）
- ✅ Settings `.st-sb-label` 修改（Task 4）
- ✅ FileManager `.fm-main` 修改（Task 5）
- ✅ FileManager `.fm-list-header` 修改（Task 6）
- ✅ FileManager `.fm-list-row` 修改（Task 7）
- ✅ FileManager `.fm-list-row .file-name` 修改（Task 8）
- ✅ FileManager `.fm-list-row .file-name .fn-text` 修改（Task 9）
- ✅ FileManager `.fm-breadcrumb` 修改（Task 10）
- ✅ 综合验证（Task 11）

**2. Placeholder scan:**
- ✅ 无 "TBD"、"TODO"、"implement later"
- ✅ 无模糊描述（如 "add appropriate error handling"）
- ✅ 所有步骤都有具体代码或命令
- ✅ 无引用未定义的类型或函数

**3. Type consistency:**
- ✅ CSS 类名一致（`.st-main`, `.fm-main`, `.fm-list-header` 等）
- ✅ CSS 属性名一致（`min-width`, `overflow`, `text-overflow`, `white-space`）
- ✅ 验证步骤一致（缩小窗口 → 验证最小宽度 → 验证省略号）

---

## 执行选项

计划已完成并保存到 `docs/superpowers/plans/2026-06-25-content-overflow-fix.md`。

**两种执行方式：**

**1. Subagent-Driven（推荐）** - 我为每个任务派发一个新的子代理，任务间进行审查，快速迭代

**2. Inline Execution** - 在此会话中使用 executing-plans 执行，批量执行并设置检查点进行审查

**您选择哪种方式？**

---

**注意：根据用户规则，我不会控制 git 版本，不回退，不提交代码，只提示您需要这么做，您来操作。**

因此，所有 git commit 步骤将改为提示您手动执行。