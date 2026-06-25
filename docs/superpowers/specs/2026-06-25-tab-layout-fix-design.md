# 标签组件布局修复设计文档

> **日期**: 2026-06-25
> **状态**: 待审核
> **作者**: AI Assistant
> **目标**: 修复标签组件在窗口变化时的收缩和间距问题

---

## 一、问题概述

### 1.1 问题描述

当前项目中存在多个标签组件，在窗口尺寸变化时出现以下问题：

1. **标签收缩问题**：标签应该保持固定尺寸，但在窗口变小时会缩小
2. **间距不足问题**：不同标签之间间距太小（2px 或 4px），视觉上不够清晰
3. **换行问题**：标签在容器不足时可能换行，破坏布局

### 1.2 影响范围

经过分析，发现以下 **3 个页面** 存在问题：

| 页面 | 组件 | 位置 | CSS 文件 | 问题 |
|------|------|------|----------|------|
| Terminal.tsx | 标签栏 `.terminal-tab-bar` | 第 659-673 行 | Terminal.css 第 208-225 行 | `flex-shrink: 1`，`gap: 2px` |
| SystemMonitor.tsx | 标签栏 `.sm-tabs` | 第 299-323 行 | SystemMonitor.css 第 29-39 行 | 缺少 `flex-shrink: 0`，`gap: 2px` |
| NotificationCenter.tsx | 过滤器 `.nc-filters` | 第 164-189 行 | NotificationCenter.css 第 129-138 行 | 缺少 `flex-shrink: 0`，`gap: 4px` |

---

## 二、设计目标

### 2.1 核心目标

1. **固定尺寸**：标签保持固定最小宽度，不随窗口收缩
2. **合理间距**：标签之间保持 6px 间距（符合 GNOME HIG）
3. **溢出处理**：标签过多或窗口太小时，显示横向滚动条
4. **一致性**：所有标签组件遵循统一规范

### 2.2 GNOME HIG 参考

根据 GNOME Human Interface Guidelines：
- 标签按钮应保持固定尺寸，不应动态收缩
- 标签间距推荐 6px（标准按钮间距）
- 横向滚动是处理溢出的标准方式（GNOME Terminal 采用此方案）

---

## 三、解决方案

### 3.1 方案选择

经过对比分析，选择 **方案 A：最小化修改**

**理由：**
- 只修改 CSS 文件，不改动组件结构
- 风险最小，实施最快
- 符合 GNOME Terminal 的设计模式（已有滚动条支持）
- 保持现有代码逻辑不变

### 3.2 实施策略

#### 策略 1：禁止标签收缩

为所有标签按钮添加：
```css
flex-shrink: 0;  /* 禁止 flex 收缩 */
min-width: <value>;  /* 设置固定最小宽度 */
```

#### 策略 2：增加标签间距

将标签容器的 `gap` 从 `2px` 或 `4px` 改为 `6px`

#### 策略 3：添加滚动支持

为标签容器添加横向滚动支持：
```css
overflow-x: auto;  /* 横向滚动 */
overflow-y: hidden;  /* 禁止纵向滚动 */
scrollbar-width: thin;  /* 细滚动条（Firefox） */
scrollbar-color: var(--border-color) transparent;  /* 滚动条颜色 */
```

---

## 四、具体修改

### 4.1 Terminal 标签栏

**文件**: `src/apps/Terminal.css`

**修改位置**: 第 173-303 行

**修改内容**:

```css
/* ── Tab Bar ─────────────────────────────────────────────── */
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
  overflow-x: auto;  /* ✅ 保持：已有滚动支持 */
  overflow-y: hidden;
  scrollbar-width: thin;
  scrollbar-color: var(--border-color) transparent;
}

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

**修改说明**:
- `gap: 6px` - 标签间距从 2px 改为 6px
- `min-width: 100px` - 最小宽度从 60px 改为 100px（更合理的最小宽度）
- `flex-shrink: 0` - 禁止收缩（从 1 改为 0）

---

### 4.2 SystemMonitor 标签栏

**文件**: `src/apps/SystemMonitor.css`

**修改位置**: 第 24-49 行

**修改内容**:

```css
.sm-tabs {
  display: flex;
  gap: 6px;  /* ✅ 修改：从 2px 改为 6px */
  overflow-x: auto;  /* ✅ 新增：横向滚动支持 */
  overflow-y: hidden;  /* ✅ 新增：禁止纵向滚动 */
  scrollbar-width: thin;  /* ✅ 新增：细滚动条 */
  scrollbar-color: var(--border-color) transparent;  /* ✅ 新增：滚动条颜色 */
}

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

**修改说明**:
- `gap: 6px` - 标签间距从 2px 改为 6px
- `overflow-x: auto` - 新增横向滚动支持
- `flex-shrink: 0` - 新增禁止收缩
- `min-width: 80px` - 新增固定最小宽度
- `white-space: nowrap` - 新增禁止换行

**滚动条样式**（新增）:
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

---

### 4.3 NotificationCenter 过滤器

**文件**: `src/shell/NotificationCenter.css`

**修改位置**: 第 120-148 行

**修改内容**:

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

**修改说明**:
- `gap: 6px` - 标签间距从 4px 改为 6px
- `overflow-x: auto` - 新增横向滚动支持
- `flex-shrink: 0` - 新增禁止收缩
- `min-width: 60px` - 新增固定最小宽度
- `white-space: nowrap` - 新增禁止换行

**滚动条样式**（新增）:
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

---

## 五、验证方案

### 5.1 测试场景

修复后需要验证以下场景：

1. **正常窗口尺寸**
   - 标签显示正常，间距清晰
   - 无滚动条显示

2. **窗口缩小**
   - 标签保持固定尺寸，不收缩
   - 标签间距保持 6px
   - 出现横向滚动条

3. **标签过多**
   - 标签保持固定尺寸
   - 出现横向滚动条
   - 可以通过滚动访问所有标签

4. **极端情况**
   - 窗口极小（如 300px 宽）
   - 标签仍然保持最小宽度
   - 滚动条正常工作

### 5.2 测试页面

需要在以下页面测试：

1. **Terminal.tsx**
   - 打开多个终端标签（5-10 个）
   - 缩小窗口宽度
   - 验证标签不收缩，滚动条出现

2. **SystemMonitor.tsx**
   - 切换不同标签（进程、资源、文件系统）
   - 缩小窗口宽度
   - 验证标签不收缩，滚动条出现

3. **NotificationCenter.tsx**
   - 切换不同过滤器（全部、紧急、普通、低）
   - 缩小窗口宽度
   - 验证标签不收缩，滚动条出现

---

## 六、风险评估

### 6.1 低风险

- **CSS 修改**：只修改 CSS 文件，不改动组件逻辑
- **向后兼容**：不影响现有功能
- **易于回滚**：如有问题，可以快速恢复

### 6.2 需要注意

- **滚动条样式**：需要确保滚动条在所有浏览器中正常显示
- **最小宽度**：需要根据实际内容调整最小宽度值
- **性能影响**：横向滚动可能影响用户体验，需要测试

---

## 七、后续优化

### 7.1 短期优化

1. **统一组件**：考虑创建统一的 `TabBar` 组件
2. **响应式设计**：根据窗口大小动态调整标签宽度
3. **键盘导航**：支持键盘快捷键切换标签

### 7.2 长期优化

1. **标签管理**：支持标签拖拽排序、关闭等操作
2. **标签分组**：支持标签分组显示
3. **标签搜索**：支持标签搜索功能

---

## 八、实施步骤

### 8.1 实施顺序

1. 修改 `Terminal.css`（已有滚动支持，最简单）
2. 修改 `SystemMonitor.css`（需要新增滚动支持）
3. 修改 `NotificationCenter.css`（需要新增滚动支持）
4. 测试验证所有页面

### 8.2 验证步骤

1. 启动应用
2. 打开 Terminal、SystemMonitor、NotificationCenter
3. 缩小窗口宽度
4. 验证标签行为

---

## 九、总结

本设计文档详细描述了标签组件布局问题的修复方案。通过最小化修改 CSS 文件，实现：

1. **固定尺寸**：标签保持固定最小宽度，不随窗口收缩
2. **合理间距**：标签间距统一为 6px
3. **溢出处理**：横向滚动条处理溢出

方案风险低，易于实施和验证，符合 GNOME HIG 设计规范。

---

**下一步**: 用户审核设计文档，然后调用 `writing-plans` skill 创建实施计划。