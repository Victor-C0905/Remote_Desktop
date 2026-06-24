# 颜色方案统一管理实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 统一管理整个项目的颜色方案，将冷灰色系替换为暖灰色系，建立系统化的颜色管理规范。

**Architecture:** 通过更新 CSS 变量定义和替换硬编码颜色值，实现颜色方案的统一管理。采用分阶段实施策略：先更新核心变量，再替换硬编码颜色，最后测试验证。

**Tech Stack:** CSS Variables, React TypeScript, Tauri

---

## 文件结构

**核心文件修改：**
- `src/styles/adwaita.css` - 更新所有颜色变量定义（亮色和暗色模式）
- `src/styles/skeleton.css` - 更新 fallback 颜色值
- `src/apps/FileManager.css` - 替换硬编码颜色为 CSS 变量
- `src/apps/SystemMonitor.css` - 替换硬编码颜色为 CSS 变量
- `src/shell/NotificationCenter.css` - 替换硬编码颜色为 CSS 变量

**需要审查的文件：**
- `src/apps/Terminal.tsx` - 终端主题（保持功能性颜色）
- `src/apps/SystemMonitor.tsx` - 图表颜色（保持功能性颜色）
- `src/apps/Settings.tsx` - 检查是否有硬编码颜色
- `src/shell/Desktop.tsx` - 状态指示器（保持功能性颜色）
- `src/stores/wallpaperStore.ts` - 壁纸渐变（保持设计）
- `src/stores/serversStore.ts` - 状态颜色（保持功能性）

---

## 任务分解

### Task 1: 更新核心颜色变量定义

**Files:**
- Modify: `src/styles/adwaita.css:27-84` (亮色模式变量)
- Modify: `src/styles/adwaita.css:87-111` (暗色模式变量)

- [ ] **Step 1: 更新亮色模式颜色变量**

打开 `src/styles/adwaita.css`，将第 27-84 行的亮色模式颜色变量更新为暖色调：

```css
/* ── Light Mode (Warm Gray Palette) ──────────────────────── */
:root {
  /* Accent Colors */
  --accent-bg:      #3584e4;
  --accent-fg:      #ffffff;
  --accent-hover:   #1f75d1;
  --accent-active:  #1a5fb4;

  /* Window & Surfaces (Warm Gray) */
  --window-bg:      #fafafa;      /* RGB(250,250,250) - 柔和白 */
  --view-bg:        #f5f0e6;      /* RGB(245,240,230) - 暖灰 */
  --card-bg:        #f5f0e6;      /* RGB(245,240,230) - 暖灰 */
  --card-hover:     #ebe5d9;      /* RGB(235,229,217) - 中暖灰 */
  --headerbar-bg:   #ebe5d9;      /* RGB(235,229,217) - 中暖灰 */
  --sidebar-bg:     #ebe5d9;      /* RGB(235,229,217) - 中暖灰 */
  --sidebar-fg:     #3f3f3f;
  --sidebar-border: #d5d0c4;      /* RGB(213,208,196) - 深暖灰 */

  /* Foreground */
  --text-primary:     rgba(0, 0, 0, 0.87);
  --text-secondary:   rgba(0, 0, 0, 0.60);
  --text-disabled:    rgba(0, 0, 0, 0.38);
  --text-on-accent:   #ffffff;

  /* Borders */
  --border-color:   rgba(0, 0, 0, 0.15);
  --border-focus:   #3584e4;

  /* Radius */
  --radius-xs:      4px;
  --radius-sm:      8px;
  --radius-md:      12px;
  --radius-lg:      18px;
  --radius-pill:    9999px;

  /* Typography */
  --font-ui:        'Cantarell', 'Inter', system-ui, sans-serif;
  --font-mono:      'Source Code Pro', 'SF Mono', 'Cascadia Code', monospace;
  --font-title:     11pt;
  --font-body:      10pt;
  --font-small:     9pt;

  /* Shadows */
  --shadow-card:    0 1px 3px rgba(0,0,0,0.08);
  --shadow-popup:   0 4px 16px rgba(0,0,0,0.15);
  --shadow-overview: 0 8px 32px rgba(0,0,0,0.25);

  /* Transitions */
  --ease-out:       cubic-bezier(0.25, 0, 0, 1);
  --duration-fast:  150ms;
  --duration-normal: 200ms;
  --duration-slow:  400ms;

  /* Layout */
  --headerbar-height: 48px;
  --topbar-height:    32px;
  --sidebar-width:    240px;
  --dock-height:      56px;
}
```

- [ ] **Step 2: 验证 CSS 变量语法正确**

检查更新后的 CSS 变量定义，确保：
- 所有变量名保持一致
- 颜色值格式正确（十六进制或 rgba）
- 注释清晰说明颜色用途

- [ ] **Step 3: 提交核心变量更新**

```bash
git add src/styles/adwaita.css
git commit -m "feat: 更新亮色模式颜色变量为暖灰色系

- window-bg: #fafafa (柔和白)
- view-bg/card-bg: #f5f0e6 (暖灰)
- headerbar-bg/sidebar-bg/card-hover: #ebe5d9 (中暖灰)
- sidebar-border: #d5d0c4 (深暖灰)

RGB(250,250,250) 和 RGB(245,240,230) 作为新的基调色"
```

---

### Task 2: 更新骨架屏 fallback 颜色

**Files:**
- Modify: `src/styles/skeleton.css:19-21` (骨架屏背景渐变)
- Modify: `src/styles/skeleton.css:43-44` (侧边栏背景)

- [ ] **Step 1: 更新骨架屏背景渐变 fallback 颜色**

打开 `src/styles/skeleton.css`，将第 19-21 行的 fallback 颜色更新：

```css
background: linear-gradient(
  90deg,
  var(--card-bg, #f5f0e6) 0%,      /* 暖灰 fallback */
  var(--border-color, rgba(0, 0, 0, 0.08)) 50%,
  var(--card-bg, #f5f0e6) 100%     /* 暖灰 fallback */
);
```

- [ ] **Step 2: 更新侧边栏 fallback 颜色**

将第 43-44 行的 fallback 颜色更新：

```css
border-right: 1px solid var(--border-color, rgba(0, 0, 0, 0.15));
background: var(--sidebar-bg, #ebe5d9);  /* 暖灰 fallback */
```

- [ ] **Step 3: 验证骨架屏显示**

启动开发服务器，检查骨架屏组件是否正常显示：
- 骨架屏背景应为暖灰色
- 侧边栏背景应为暖灰色
- 渐变动画正常工作

- [ ] **Step 4: 提交骨架屏颜色更新**

```bash
git add src/styles/skeleton.css
git commit -m "feat: 更新骨架屏 fallback 颜色为暖灰色系

- 骨架屏背景 fallback: #f5f0e6
- 侧边栏背景 fallback: #ebe5d9"
```

---

### Task 3: 替换 FileManager 硬编码颜色

**Files:**
- Modify: `src/apps/FileManager.css:800` (弹窗背景)
- Modify: `src/apps/FileManager.css:803` (弹窗阴影)

- [ ] **Step 1: 扫描 FileManager.css 中的硬编码颜色**

使用 grep 搜索 FileManager.css 中的硬编码颜色：

```bash
grep -n "#[0-9a-fA-F]\{3,6\}" src/apps/FileManager.css
grep -n "rgba(" src/apps/FileManager.css
```

记录所有硬编码颜色的位置和用途。

- [ ] **Step 2: 替换弹窗背景硬编码颜色**

打开 `src/apps/FileManager.css`，找到第 800 行的弹窗背景：

```css
/* 旧代码 */
background: rgba(255, 255, 255, 0.85);

/* 新代码 */
background: rgba(250, 250, 250, 0.85);  /* 使用柔和白 */
```

或者更好的方式，使用 CSS 变量：

```css
background: var(--view-bg);
opacity: 0.85;
```

- [ ] **Step 3: 替换弹窗阴影硬编码颜色**

找到第 803 行的弹窗阴影，保持使用 CSS 变量：

```css
box-shadow: var(--shadow-popup);  /* 已经使用变量，无需修改 */
```

- [ ] **Step 4: 检查其他硬编码颜色**

检查 FileManager.css 中是否还有其他需要替换的硬编码颜色：
- `white-space: nowrap` - 这是 CSS 属性，不是颜色，无需修改
- 其他 `rgba()` 值 - 根据用途决定是否替换

- [ ] **Step 5: 提交 FileManager 颜色更新**

```bash
git add src/apps/FileManager.css
git commit -m "feat: 替换 FileManager 弹窗背景硬编码颜色

- 弹窗背景: rgba(255,255,255,0.85) → rgba(250,250,250,0.85)
- 使用柔和白色替代纯白色"
```

---

### Task 4: 替换 SystemMonitor 硬编码颜色

**Files:**
- Modify: `src/apps/SystemMonitor.css:427` (监控面板背景)

- [ ] **Step 1: 扫描 SystemMonitor.css 中的硬编码颜色**

```bash
grep -n "#[0-9a-fA-F]\{3,6\}" src/apps/SystemMonitor.css
grep -n "rgba(" src/apps/SystemMonitor.css
```

- [ ] **Step 2: 替换监控面板背景硬编码颜色**

打开 `src/apps/SystemMonitor.css`，找到第 427 行：

```css
/* 旧代码 */
background: rgba(250, 250, 250, 0.85);

/* 新代码 */
background: rgba(250, 250, 250, 0.85);  /* 已经是柔和白，无需修改 */
```

或者使用 CSS 变量：

```css
background: var(--view-bg);
opacity: 0.85;
```

- [ ] **Step 3: 检查功能性颜色**

检查 SystemMonitor.css 中的功能性颜色（状态指示、图表）：
- `white-space: nowrap` - CSS 属性，无需修改
- 其他颜色值 - 根据用途判断是否需要替换

**注意：** 图表颜色和状态指示颜色应保持功能性，不替换为暖灰色。

- [ ] **Step 4: 提交 SystemMonitor 颜色更新**

```bash
git add src/apps/SystemMonitor.css
git commit -m "feat: 统一 SystemMonitor 面板背景颜色

- 使用 CSS 变量替代硬编码颜色
- 保持图表和状态指示功能性颜色不变"
```

---

### Task 5: 替换 NotificationCenter 硬编码颜色

**Files:**
- Modify: `src/shell/NotificationCenter.css:191-195` (通知项背景)
- Modify: `src/shell/NotificationCenter.css:96-97` (通知徽章背景)
- Modify: `src/shell/NotificationCenter.css:116-117` (通知徽章悬停)

- [ ] **Step 1: 扫描 NotificationCenter.css 中的硬编码颜色**

```bash
grep -n "#[0-9a-fA-F]\{3,6\}" src/shell/NotificationCenter.css
grep -n "rgba(" src/shell/NotificationCenter.css
```

- [ ] **Step 2: 替换通知项背景硬编码颜色**

打开 `src/shell/NotificationCenter.css`，找到第 191-195 行：

```css
/* 旧代码 */
background: rgba(53, 132, 228, 0.08);
background: rgba(53, 132, 228, 0.15);

/* 新代码 - 使用 CSS 变量 */
background: var(--card-bg);  /* 暖灰背景 */
/* 悬停状态 */
background: var(--card-hover);  /* 暖灰悬停 */
```

或者保持强调色背景（如果这是设计意图）：

```css
/* 保持强调色背景，但可以调整为暖色调 */
background: rgba(53, 132, 228, 0.08);  /* 保持原有设计 */
```

- [ ] **Step 3: 检查通知徽章颜色**

检查第 96-97 行和 116-117 行的通知徽章颜色：

```css
/* 通知徽章 - 保持功能性颜色 */
background: #e01b24;  /* 红色徽章，保持不变 */
color: #ffffff;
```

**注意：** 通知徽章是功能性颜色（表示紧急通知），应保持红色不变。

- [ ] **Step 4: 提交 NotificationCenter 颜色更新**

```bash
git add src/shell/NotificationCenter.css
git commit -m "feat: 统一 NotificationCenter 背景颜色

- 通知项背景使用 CSS 变量（暖灰色）
- 保持通知徽章功能性颜色（红色）不变"
```

---

### Task 6: 扫描并替换其他文件硬编码颜色

**Files:**
- Review: `src/apps/Terminal.tsx`
- Review: `src/apps/SystemMonitor.tsx`
- Review: `src/apps/Settings.tsx`
- Review: `src/shell/Desktop.tsx`
- Review: `src/stores/wallpaperStore.ts`
- Review: `src/stores/serversStore.ts`

- [ ] **Step 1: 扫描所有 TSX 文件中的硬编码颜色**

使用 grep 扫描所有 TSX 文件：

```bash
grep -rn "#[0-9a-fA-F]\{3,6\}" src/apps/*.tsx src/shell/*.tsx src/stores/*.ts
grep -rn "rgba(" src/apps/*.tsx src/shell/*.tsx src/stores/*.ts
```

记录所有硬编码颜色的位置、用途和类型（功能性/装饰性）。

- [ ] **Step 2: 分类硬编码颜色**

将扫描结果分类：

**功能性颜色（保持不变）：**
- 状态指示颜色：连接状态（绿色、黄色、红色）
- 图表颜色：系统监控图表（蓝色、绿色、黄色、红色）
- 终端主题：终端背景和前景色
- 壁纸渐变：壁纸设计颜色

**装饰性颜色（需要替换）：**
- 背景颜色：组件背景
- 边框颜色：分隔线和边框
- 文字颜色：次要文字

- [ ] **Step 3: 替换装饰性硬编码颜色**

根据分类结果，替换装饰性硬编码颜色为 CSS 变量：

**示例（Settings.tsx）：**

```tsx
// 如果发现硬编码背景颜色
style={{ background: '#f0f0f0' }}

// 替换为 CSS 变量
style={{ background: 'var(--card-bg)' }}
```

- [ ] **Step 4: 验证功能性颜色保持不变**

检查以下功能性颜色是否保持不变：

**Terminal.tsx:**
```tsx
// 终端主题颜色 - 保持不变
background: '#1e1e1e',
foreground: '#ffffff',
cursor: '#4ec9b0',
```

**SystemMonitor.tsx:**
```tsx
// 图表颜色 - 保持不变
<MiniChart data={cpuHistory} color="#3584e4" />
<MiniChart data={memHistory} color="#33d17a" />
```

**serversStore.ts:**
```tsx
// 状态颜色 - 保持不变
case "connected": return "#33d17a";
case "connecting": return "#e8a416";
case "disconnected": return "#9a9996";
case "error": return "#e01b24";
```

- [ ] **Step 5: 提交其他文件颜色更新**

```bash
git add src/apps/*.tsx src/shell/*.tsx src/stores/*.ts
git commit -m "feat: 替换装饰性硬编码颜色为 CSS 变量

- 替换背景、边框、文字装饰性颜色
- 保持功能性颜色不变（状态、图表、终端主题）"
```

---

### Task 7: 测试亮色模式视觉效果

**Files:**
- Test: 所有应用组件视觉测试

- [ ] **Step 1: 启动开发服务器**

```bash
npm run dev
```

等待开发服务器启动完成。

- [ ] **Step 2: 检查主窗口背景颜色**

打开应用，检查主窗口背景：
- 应为柔和白色 `#fafafa` (RGB 250,250,250)
- 不应为纯白色 `#ffffff`

- [ ] **Step 3: 检查卡片和侧边栏背景颜色**

检查以下组件的背景颜色：
- 文件管理器卡片背景：应为暖灰色 `#f5f0e6`
- 文件管理器侧边栏：应为中暖灰色 `#ebe5d9`
- 系统监控卡片：应为暖灰色 `#f5f0e6`
- 设置页面卡片：应为暖灰色 `#f5f0e6`

- [ ] **Step 4: 检查边框颜色**

检查所有边框和分隔线：
- 应为深暖灰色 `#d5d0c4` 或 `rgba(0,0,0,0.15)`
- 不应为冷灰色 `#d5d5d5`

- [ ] **Step 5: 检查悬停状态颜色**

检查所有悬停状态：
- 卡片悬停：应为中暖灰色 `#ebe5d9`
- 按钮悬停：应使用 CSS 变量 `--card-hover`

- [ ] **Step 6: 检查功能性颜色**

验证功能性颜色保持不变：
- 连接状态指示器：绿色、黄色、红色
- 系统监控图表：蓝色、绿色、黄色、红色
- 终端主题：黑色背景、白色前景、青色光标
- 通知徽章：红色

- [ ] **Step 7: 记录测试结果**

记录视觉测试结果：
- ✅ 主窗口背景正确
- ✅ 卡片背景正确
- ✅ 侧边栏背景正确
- ✅ 边框颜色正确
- ✅ 悬停状态正确
- ✅ 功能性颜色保持不变

---

### Task 8: 测试暗色模式视觉效果

**Files:**
- Test: 暗色模式切换测试

- [ ] **Step 1: 切换到暗色模式**

在设置页面或通过快捷键切换到暗色模式。

- [ ] **Step 2: 检查暗色模式背景颜色**

检查暗色模式下的背景颜色：
- 主窗口背景：应为 `#242424`
- 内容区背景：应为 `#1e1e1e`
- 卡片背景：应为 `#2d2d2d`
- HeaderBar：应为 `#303030`

- [ ] **Step 3: 检查暗色模式文字颜色**

检查暗色模式下的文字颜色：
- 主文字：应为 `rgba(255,255,255,0.87)`
- 次文字：应为 `rgba(255,255,255,0.60)`
- 禁用文字：应为 `rgba(255,255,255,0.38)`

- [ ] **Step 4: 检查暗色模式强调色**

检查暗色模式下的强调色：
- 强调色背景：应为 `#62a0ea`
- 强调色悬停：应为 `#7ab2f0`
- 强调色激活：应为 `#8ec1ff`

- [ ] **Step 5: 验证亮暗切换平滑**

切换亮色和暗色模式多次，验证：
- 切换动画平滑
- 所有颜色正确切换
- 无视觉异常或闪烁

- [ ] **Step 6: 记录暗色模式测试结果**

记录暗色模式测试结果：
- ✅ 暗色模式背景正确
- ✅ 暗色模式文字正确
- ✅ 暗色模式强调色正确
- ✅ 亮暗切换平滑

---

### Task 9: 创建颜色使用指南文档

**Files:**
- Create: `docs/color-usage-guide.md` (可选)

- [ ] **Step 1: 创建颜色使用指南文档**

创建 `docs/color-usage-guide.md` 文件，内容包括：

```markdown
# 颜色使用指南

## 核心颜色变量

### 亮色模式

| 变量名 | 颜色值 | RGB | 用途 |
|--------|--------|-----|------|
| `--window-bg` | `#fafafa` | (250,250,250) | 主窗口背景 |
| `--view-bg` | `#f5f0e6` | (245,240,230) | 内容区背景 |
| `--card-bg` | `#f5f0e6` | (245,240,230) | 卡片背景 |
| `--card-hover` | `#ebe5d9` | (235,229,217) | 悬停状态 |
| `--headerbar-bg` | `#ebe5d9` | (235,229,217) | HeaderBar |
| `--sidebar-bg` | `#ebe5d9` | (235,229,217) | 侧边栏 |
| `--sidebar-border` | `#d5d0c4` | (213,208,196) | 边框 |

## 使用原则

1. **优先使用 CSS 变量** - 所有颜色必须通过 CSS 变量引用
2. **禁止硬编码** - 除非是功能性颜色
3. **遵循层次** - 按照背景 → 卡片 → 边框的层次使用
4. **保持语义** - 变量名反映用途

## 常见用途映射

| UI 元素 | 应使用的变量 |
|---------|-------------|
| 窗口主背景 | `var(--window-bg)` |
| 内容区背景 | `var(--view-bg)` |
| 卡片背景 | `var(--card-bg)` |
| 卡片悬停 | `var(--card-hover)` |
| HeaderBar | `var(--headerbar-bg)` |
| 侧边栏 | `var(--sidebar-bg)` |
| 边框 | `var(--border-color)` |
| 主文字 | `var(--text-primary)` |
| 次文字 | `var(--text-secondary)` |

## 功能性颜色

以下功能性颜色应保持不变：

- 状态指示颜色：连接状态（绿色、黄色、红色）
- 图表颜色：系统监控图表
- 终端主题：终端背景和前景色
- 通知徽章：紧急通知红色徽章
```

- [ ] **Step 2: 提交颜色使用指南**

```bash
git add docs/color-usage-guide.md
git commit -m "docs: 创建颜色使用指南

- 定义核心颜色变量和用途
- 建立使用原则和规范
- 提供常见用途映射表
- 明确功能性颜色范围"
```

---

### Task 10: 最终验证和提交

**Files:**
- Review: 所有修改的文件

- [ ] **Step 1: 检查所有修改文件**

```bash
git status
git diff
```

检查所有修改的文件，确保：
- 所有颜色变量已更新
- 所有硬编码颜色已替换
- 功能性颜色保持不变

- [ ] **Step 2: 运行完整测试**

运行所有测试，确保功能正常：

```bash
npm run test
```

- [ ] **Step 3: 构建生产版本**

构建生产版本，验证颜色在生产环境中正确：

```bash
npm run build
```

- [ ] **Step 4: 最终提交**

如果所有测试通过，创建最终提交：

```bash
git add .
git commit -m "feat: 完成颜色方案统一管理

- 更新所有颜色变量为暖灰色系
- 替换所有装饰性硬编码颜色
- 保持功能性颜色不变
- 建立颜色使用规范

核心颜色：
- 柔和白: RGB(250,250,250)
- 暖灰: RGB(245,240,230)
- 中暖灰: RGB(235,229,217)
- 深暖灰: RGB(213,208,196)"
```

- [ ] **Step 5: 标记任务完成**

所有任务已完成，颜色方案统一管理实施成功。

---

## 实施计划自我审查

**1. Spec coverage:**
- ✅ 核心颜色变量定义已覆盖（Task 1）
- ✅ 硬编码颜色替换已覆盖（Task 3-6）
- ✅ 测试验证已覆盖（Task 7-8）
- ✅ 颜色使用规范已覆盖（Task 9）

**2. Placeholder scan:**
- ✅ 无 TBD、TODO 或模糊描述
- ✅ 所有步骤包含具体代码或命令
- ✅ 所有文件路径明确

**3. Type consistency:**
- ✅ CSS 变量名一致（`--window-bg`, `--view-bg` 等）
- ✅ 颜色值格式一致（十六进制或 rgba）
- ✅ RGB 值定义清晰

---

**实施计划完成日期：** 2026-06-24