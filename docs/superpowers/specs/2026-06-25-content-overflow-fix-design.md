# 内容区溢出与重叠修复设计文档

> **日期**: 2026-06-25
> **状态**: 待审核
> **作者**: AI Assistant
> **目标**: 修复 Settings 和 FileManager 内容区在窗口缩小时的溢出与重叠问题

---

## 一、问题概述

### 1.1 问题描述

用户反馈：Settings 内容区和 FileManager 内容区的小标题在窗口缩小时会出现重叠或压缩问题。

**具体表现：**
- Settings 内容区：标题和卡片内容在窗口缩小时过度压缩，导致不可读或重叠
- FileManager 文件列表：文件名列在窗口缩小时过度压缩，标题和内容重叠

### 1.2 根本原因分析

| 应用 | 问题组件 | CSS 属性 | 导致问题 |
|------|----------|----------|----------|
| **Settings** | `.st-main` | `min-width: 0` | 允许内容区完全收缩 |
| **Settings** | `.st-card` | `min-width: 0` | 允许卡片完全收缩 |
| **Settings** | `.st-section-title` | 无防重叠保护 | 标题可能换行或压缩 |
| **FileManager** | `.fm-main` | `min-width: 0` | 允许文件列表完全收缩 |
| **FileManager** | `.fm-list-header` | `grid-template-columns: 1fr ...` | 第一列可能收缩到 0 |
| **FileManager** | `.fm-list-row` | `grid-template-columns: 1fr ...` | 第一列可能收缩到 0 |

**核心问题：**
- `min-width: 0` 允许 flex/grid 子项完全收缩
- 没有设置合理的最小宽度，导致内容过度压缩
- 缺少文本溢出保护（`overflow: hidden` + `text-overflow: ellipsis`）

---

## 二、设计目标

### 2.1 核心目标

遵循 GNOME HIG 设计规范，采用"内容优先"原则：

1. **最小宽度策略**：根据内容的实际可读宽度设置最小宽度
2. **溢出隐藏**：超过最小宽度后，使用 `overflow: hidden` + `text-overflow: ellipsis` 隐藏不可见部分
3. **不使用滚动条**：避免复杂的横向滚动逻辑，保持界面简洁
4. **自适应布局**：内容可以适度收缩，但不会过度压缩导致重叠

### 2.2 GNOME HIG 参考

根据 GNOME Human Interface Guidelines：
- 内容应保持最小可读宽度（通常 400-600px）
- 文本溢出应使用省略号（ellipsis）而非换行或压缩
- 横向滚动应避免，除非必要（如大型表格）

---

## 三、解决方案

### 3.1 方案选择

经过对比分析，选择 **方案 A：基于内容的自适应最小宽度**

**理由：**
- 符合 GNOME 设计理念（内容优先）
- 符合用户需求："设置最小压缩距离, 继续压缩就是会隐藏不可见"
- 实现简单，只需要修改 CSS
- 性能好，不需要监听窗口尺寸变化
- 易于维护，每个组件有明确的最小宽度

### 3.2 CSS 属性组合标准

```css
/* 标准的防重叠组合 */
min-width: <value>;           /* 最小宽度 */
overflow: hidden;             /* 隐藏溢出 */
text-overflow: ellipsis;      /* 文本溢出显示省略号 */
white-space: nowrap;          /* 禁止换行 */
```

---

## 四、具体修改

### 4.1 Settings 内容区修复

**文件**: `src/apps/Settings.css`

#### 修改 1：`.st-main` 设置最小宽度

**位置**: 第 125-131 行

**修改内容**:

```css
.st-main {
  flex: 1;
  overflow-y: auto;
  overflow-x: hidden;
  padding: 16px 24px;
  min-width: 400px; /* ✅ 修改：从 0 改为 400px（最小可读宽度） */
}
```

**修改说明**:
- `min-width: 400px` - 确保内容区至少有 400px 宽度，保证可读性
- 超过最小宽度后，内容区不再收缩，而是保持固定宽度

---

#### 修改 2：`.st-card` 设置最小宽度

**位置**: 第 146-154 行

**修改内容**:

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

**修改说明**:
- `min-width: 300px` - 确保卡片至少有 300px 宽度
- 防止卡片过度压缩导致内容重叠

---

#### 修改 3：`.st-section-title` 添加防重叠保护

**位置**: 第 138-143 行

**修改内容**:

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

**修改说明**:
- `white-space: nowrap` - 标题不换行
- `overflow: hidden` - 隐藏超出部分
- `text-overflow: ellipsis` - 超出部分显示省略号（...）

---

#### 修改 4：`.st-sb-label` 添加防重叠保护

**位置**: 第 118-122 行

**修改内容**:

```css
.st-sb-label {
  font-size: var(--font-body);
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden; /* ✅ 新增：隐藏溢出 */
  text-overflow: ellipsis; /* ✅ 新增：显示省略号 */
}
```

**修改说明**:
- `overflow: hidden` - 隐藏超出部分
- `text-overflow: ellipsis` - 超出部分显示省略号

---

### 4.2 FileManager 文件列表修复

**文件**: `src/apps/FileManager.css`

#### 修改 1：`.fm-main` 设置最小宽度

**位置**: 第 247-253 行

**修改内容**:

```css
.fm-main {
  flex: 1;
  overflow-y: auto;
  overflow-x: hidden;
  padding: 8px;
  min-width: 500px; /* ✅ 修改：从 0 改为 500px（最小可读宽度） */
}
```

**修改说明**:
- `min-width: 500px` - 确保文件列表至少有 500px 宽度
- 保证文件名列有足够空间显示

---

#### 修改 2：`.fm-list-header` 设置最小列宽

**位置**: 第 260-274 行

**修改内容**:

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

**修改说明**:
- `minmax(200px, 1fr)` - 文件名列最小 200px，最大可扩展
- 确保文件名列至少有 200px 宽度，防止过度压缩
- 其他列保持固定宽度（80px、120px、100px）

---

#### 修改 3：`.fm-list-row` 设置最小列宽

**位置**: 第 276-285 行

**修改内容**:

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

**修改说明**:
- `minmax(200px, 1fr)` - 文件名列最小 200px，最大可扩展
- 确保文件名列至少有 200px 宽度

---

#### 修改 4：`.fm-list-row .file-name` 添加防重叠保护

**位置**: 第 296-302 行

**修改内容**:

```css
.fm-list .fm-list-row .file-name {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 200px; /* ✅ 修改：从 0 改为 200px */
  overflow: hidden;
}
```

**修改说明**:
- `min-width: 200px` - 文件名容器最小宽度

---

#### 修改 5：`.fm-list-row .file-name .fn-text` 添加防重叠保护

**位置**: 第 309-315 行（需要查看完整代码）

**修改内容**:

```css
.fm-list .fm-list-row .file-name .fn-text {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap; /* ✅ 新增：禁止换行 */
}
```

**修改说明**:
- `white-space: nowrap` - 文件名不换行
- `text-overflow: ellipsis` - 超出部分显示省略号

---

### 4.3 其他组件的统一规范

#### 修改 1：FileManager 面包屑

**文件**: `src/apps/FileManager.css`

**位置**: 第 51-63 行

**检查内容**:
- `.fm-breadcrumb` 已有 `overflow: hidden` ✅
- `.fm-breadcrumb .crumb` 已有 `white-space: nowrap` ✅
- 需要确保面包屑容器有最小宽度（已有 `flex: 1` 和 `min-width: 0`，需要改为 `min-width: 200px`）

**修改内容**:

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

---

## 五、验证方案

### 5.1 测试场景

修复后需要验证以下场景：

1. **Settings 内容区**
   - 正常窗口尺寸（> 600px）
   - 中等窗口尺寸（400-600px）
   - 小窗口尺寸（< 400px）

2. **FileManager 文件列表**
   - 正常窗口尺寸（> 600px）
   - 中等窗口尺寸（500-600px）
   - 小窗口尺寸（< 500px）

### 5.2 验证标准

**Settings 内容区：**
- ✅ 窗口 > 600px：内容正常显示，无溢出
- ✅ 窗口 400-600px：内容适度收缩，标题显示省略号
- ✅ 窗口 < 400px：内容保持最小宽度，标题显示省略号

**FileManager 文件列表：**
- ✅ 窗口 > 600px：文件名列正常显示，无溢出
- ✅ 窗口 500-600px：文件名列适度收缩，长文件名显示省略号
- ✅ 窗口 < 500px：文件名列保持最小宽度，长文件名显示省略号

---

## 六、风险评估

### 6.1 低风险

- **CSS 修改**：只修改 CSS 文件，不改动组件逻辑
- **向后兼容**：不影响现有功能
- **易于回滚**：如有问题，可以快速恢复

### 6.2 需要注意

- **最小宽度值**：需要根据实际内容调整最小宽度值
- **用户体验**：小窗口时用户可能看不到完整内容
- **性能影响**：无性能影响，纯 CSS 修改

---

## 七、后续优化

### 7.1 短期优化

1. **响应式设计**：根据窗口大小动态调整最小宽度
2. **用户提示**：小窗口时提示用户可以放大窗口查看完整内容
3. **自适应布局**：根据内容长度动态调整布局

### 7.2 长期优化

1. **统一组件库**：创建统一的防重叠组件
2. **设计规范文档**：编写 CSS 防重叠规范文档
3. **自动化检测**：使用工具检测潜在的溢出问题

---

## 八、实施步骤

### 8.1 实施顺序

1. 修改 `Settings.css`（4 个修改）
2. 修改 `FileManager.css`（5 个修改）
3. 测试验证所有场景
4. 根据测试结果调整最小宽度值

### 8.2 验证步骤

1. 启动应用
2. 打开 Settings 和 FileManager
3. 缩小窗口宽度
4. 验证内容区行为

---

## 九、总结

本设计文档详细描述了 Settings 和 FileManager 内容区溢出与重叠问题的修复方案。通过设置最小宽度 + 溢出隐藏 + 省略号显示，实现：

1. **最小宽度保护**：内容保持最小可读宽度
2. **溢出隐藏**：超出部分隐藏，不显示滚动条
3. **省略号显示**：文本溢出显示省略号，提示用户内容被截断

方案符合 GNOME HIG 设计规范，实现简单，易于维护。

---

**下一步**: 用户审核设计文档，然后调用 `writing-plans` skill 创建实施计划。