# 文件传输状态栏设计

> 版本: v1.0 | 日期: 2026-07-21

## 一、设计目标

将文件传输进度通知从独立的弹窗改为集成到文件管理器窗口底部状态栏，提供：
- 非干扰性的传输状态展示
- 快速访问传输控制功能
- 与 GNOME Files 风格一致的 UI 设计

## 二、UI 架构

### 2.1 整体布局

```
文件管理器窗口
┌────────────────────────────────────────────────────────────┐
│ HeaderBar                                                  │
├────────────────────────────────────────────────────────────┤
│ 侧边栏    │  主内容区（文件列表）                            │
│           │                                                 │
│           │                                                 │
├────────────────────────────────────────────────────────────┤
│ 底部状态栏                                                 │
│ 3 个文件夹, 12 个文件  总大小: 256 MB    [↓ 3] ├─ 45% ─┤ ▼ │
│ ←─────── 原有信息（左）───────────────→ ←─── 传输状态（右）───→│
└────────────────────────────────────────────────────────────┘
```

### 2.2 状态栏位置

- **位置**：文件管理器窗口底部，与窗口边界对齐
- **布局**：
  - **左侧**：原有的文件数量、总大小等信息（保持不变）
  - **右侧**：传输状态（新功能）
- **层级**：z-index 高于主内容区，低于弹窗
- **宽度**：100% 窗口宽度
- **显示逻辑**：
  - 无传输任务时：仅显示左侧原有信息
  - 有传输任务时：右侧显示传输状态

## 三、收起状态（默认）

### 3.1 尺寸

- **高度**：32px（与现有状态栏一致）
- **内边距**：左右 12px，上下 6px

### 3.2 布局结构

```
┌────────────────────────────────────────────────────────────┐
│ 3 个文件夹, 12 个文件  总大小: 256 MB    [↓ 3] ├─ 45% ─┤ ▼ │
│ ←───────── 左侧原有信息 ────────────────→ ←── 右侧传输状态 ──→│
└────────────────────────────────────────────────────────────┘
```

**左侧区域（自适应宽度）：**
- 原有的文件数量、总大小等信息（保持不变）

**右侧区域（200px，仅在有传输任务时显示）：**
- 传输方向图标（SVG，16x16）：`go-down-symbolic.svg` 或 `go-up-symbolic.svg`
- 任务数量徽章（如 "3"）
- 当前最活跃任务的迷你进度条（高度 4px，宽度 80px）
- 总进度百分比（如 "45%"）
- 展开/收起按钮（SVG，12x12）：`pan-down-symbolic.svg`

### 3.3 最活跃任务选择逻辑

优先级排序：
1. 正在传输的任务（状态为 `active`）
2. 排队中的任务（状态为 `queued`）
3. 最新的任务（按创建时间）

## 四、展开状态

### 4.1 尺寸

- **高度**：动态调整
  - 状态栏高度：32px（保持不变）
  - 展开的任务列表面板：
    - 每个任务卡片：40px
    - 最大高度：限制为窗口高度的 40%（防止遮挡主内容区）
    - 超过最大高度：显示滚动条

### 4.2 布局结构

展开后的任务列表面板作为独立的浮层显示在状态栏上方：

```
┌────────────────────────────────────────────────────────────┐
│              展开的任务列表面板（浮层）                      │
│ ┌────────────────────────────────────────────────────────┐ │
│ │ ↓ nginx.tar.gz    ├██████████░░░░░░░░░░░░░░░░░░  65%  │ │
│ │ ↓ backup.zip      ├██████░░░░░░░░░░░░░░░░░░░░░░  30%  │ │
│ │ ↓ logs.tar        ├░░░░░░░░░░░░░░░░░░░░░░░░░░░░   0%  │ │
│ │                                     [全部取消] [关闭]  │ │
│ └────────────────────────────────────────────────────────┘ │
├────────────────────────────────────────────────────────────┤
│ 3 个文件夹, 12 个文件  总大小: 256 MB    [↓ 3] ├─ 45% ─┤ ▲ │
│ ←───────── 左侧原有信息 ────────────────→ ←── 右侧传输状态 ──→│
└────────────────────────────────────────────────────────────┘
```

**状态栏（底部，32px，保持不变）：**
- 左侧：原有的文件数量、总大小等信息
- 右侧：传输状态（展开按钮变为收起图标 ▲）

**任务列表面板（浮层，显示在状态栏上方）：**
- 位置：状态栏正上方，右对齐（与传输状态区域对齐）
- 宽度：300px（固定宽度，不占满窗口）
- 每个任务卡片高度 40px
- 最多显示 5 个任务（超过显示滚动条）
- 任务排序：活动 → 排队 → 完成 → 失败

**面板底部操作栏（32px）：**
- 左侧：任务统计（如 "3 个任务，总计 1.2 GB"）
- 右侧：
  - "全部取消" 按钮（仅在有活动任务时显示）
  - "关闭" 按钮（清空所有已完成/失败任务）

### 4.3 展开/收起动画

- **动画时长**：200ms
- **动画曲线**：`ease-out`（GNOME 标准动画）
- **动画属性**：`height` + `opacity`（任务列表淡入淡出）

## 五、单个任务卡片

### 5.1 尺寸

- **高度**：40px
- **宽度**：300px（面板宽度）
- **内边距**：左右 12px，上下 8px

### 5.2 布局结构（单行）

```
┌────────────────────────────────────────────────┐
│ ↓ nginx.tar.gz    ├██████████░░░░░░░░  65% ⏸ ✕ │
└────────────────────────────────────────────────┘
```

**左侧区域（160px）：**
- 传输方向图标（SVG，16x16）
- 文件名（超长截断，显示省略号）

**中间区域（自适应宽度）：**
- 进度条（高度 4px，圆角 2px）
- 进度百分比（右侧显示）

**右侧区域（50px，悬停显示）：**
- 控制按钮（见 5.3 节）

### 5.3 控制按钮（悬停显示）

**设计原则：**
- 默认隐藏，鼠标悬停时淡入显示（200ms）
- 符合 GNOME HIG 的"无干扰"原则
- 按钮间距：4px

**活动任务（status = `active`）：**
- 暂停按钮：`media-playback-pause-symbolic.svg`
- 取消按钮：`window-close-symbolic.svg`

**已暂停任务（status = `paused`）：**
- 继续按钮：`media-playback-start-symbolic.svg`
- 取消按钮：`window-close-symbolic.svg`

**已完成任务（status = `completed`）：**
- 打开文件按钮：`folder-open-symbolic.svg`
- 关闭按钮：`window-close-symbolic.svg`

**失败任务（status = `error`）：**
- 重试按钮：`view-refresh-symbolic.svg`
- 关闭按钮：`window-close-symbolic.svg`

### 5.4 进度条样式

- **高度**：4px
- **圆角**：2px
- **背景色**：`var(--ovelis-border-color)`
- **填充色**：
  - 活动任务：`var(--ovelis-accent-bg)`（蓝色）
  - 已完成任务：`var(--ovelis-accent-bg)`
  - 失败任务：`var(--ovelis-error-color)`（红色）
- **动画**：活动任务的进度条显示微妙的脉动动画（opacity 0.8 → 1.0，2s 循环）

## 六、图标系统

### 6.1 图标来源

使用 GNOME Adwaita Symbolic Icons（LGPL 许可）：
- 图标尺寸：16x16（控制按钮 12x12）
- 格式：SVG，纯色填充
- 路径：`/assets/adwaita/symbolic/`

### 6.2 图标映射表

| 用途 | 图标文件 | SVG 路径 |
|------|---------|---------|
| 下载方向 | `go-down-symbolic.svg` | `/assets/adwaita/go-down-symbolic.svg` |
| 上传方向 | `go-up-symbolic.svg` | `/assets/adwaita/go-up-symbolic.svg` |
| 暂停 | `media-playback-pause-symbolic.svg` | `/assets/adwaita/media-playback-pause-symbolic.svg` |
| 继续 | `media-playback-start-symbolic.svg` | `/assets/adwaita/media-playback-start-symbolic.svg` |
| 取消/关闭 | `window-close-symbolic.svg` | `/assets/adwaita/window-close-symbolic.svg` |
| 打开文件 | `folder-open-symbolic.svg` | `/assets/adwaita/folder-open-symbolic.svg` |
| 重试 | `view-refresh-symbolic.svg` | `/assets/adwaita/view-refresh-symbolic.svg` |
| 展开 | `pan-up-symbolic.svg` | `/assets/adwaita/pan-up-symbolic.svg` |
| 收起 | `pan-down-symbolic.svg` | `/assets/adwaita/pan-down-symbolic.svg` |

### 6.3 图标颜色

- **默认颜色**：`fill="var(--ovelis-text-secondary)"`
- **悬停颜色**：`fill="var(--ovelis-text-primary)"`
- **状态颜色**：
  - 成功图标：`fill="var(--ovelis-success-color)"`
  - 错误图标：`fill="var(--ovelis-error-color)"`

## 七、视觉风格

### 7.1 颜色系统

使用项目的 `ovelis` 主题变量：

```css
/* 状态栏背景 */
--transfer-bar-bg: var(--ovelis-card-bg);

/* 进度条 */
--progress-bar-bg: var(--ovelis-border-color);
--progress-bar-fill: var(--ovelis-accent-bg);

/* 文本 */
--text-primary: var(--ovelis-text-primary);
--text-secondary: var(--ovelis-text-secondary);

/* 状态颜色 */
--color-success: var(--ovelis-success-color);
--color-error: var(--ovelis-error-color);
```

### 7.2 间距系统

遵循 GNOME HIG 间距规范：

- **状态栏内边距**：左右 12px，上下 6px
- **任务卡片内边距**：左右 12px，上下 8px
- **元素间距**：图标与文本 6px，按钮之间 4px

### 7.3 圆角系统

- **任务卡片**：8px
- **进度条**：2px
- **控制按钮**：4px（悬停背景）

## 八、交互行为

### 8.1 展开/收起

- **触发方式**：点击展开/收起按钮
- **动画**：200ms ease-out
- **状态保持**：展开状态在窗口刷新后重置为收起

### 8.2 悬停显示控制按钮

- **触发方式**：鼠标悬停到任务卡片
- **动画**：200ms 淡入
- **阻止消失**：鼠标移到按钮区域时保持显示

### 8.3 任务排序

按以下优先级排序：
1. **活动任务**（status = `active`）：按开始时间降序
2. **排队任务**（status = `queued`）：按创建时间升序
3. **已暂停任务**（status = `paused`）：按暂停时间降序
4. **已完成任务**（status = `completed`）：按完成时间降序
5. **失败任务**（status = `error`）：按失败时间降序

### 8.4 错误处理

- **失败任务保留**：保留在列表中，显示错误信息（工具提示）
- **重试功能**：点击重试按钮重新开始传输
- **关闭功能**：点击关闭按钮移除失败任务

### 8.5 批量操作

- **全部取消**：取消所有活动任务（不取消已完成的）
- **关闭**：清空所有已完成和失败任务（不影响活动任务）

## 九、响应式设计

### 9.1 窗口宽度适配

- **窄窗口（< 600px）**：
  - 左侧原有信息正常显示
  - 右侧传输状态仅显示图标和数量（如 "[↓ 3]"）
  - 点击后展开的任务列表面板宽度调整为 250px

- **正常窗口（≥ 600px）**：
  - 左侧原有信息正常显示
  - 右侧传输状态显示完整内容（图标 + 数量 + 进度条 + 百分比）
  - 展开的任务列表面板宽度 300px

### 9.2 高密度显示适配

- **高 DPI 屏幕**：SVG 图标自动缩放
- **缩放比例 > 150%**：减小内边距（左右 8px，上下 4px）

## 十、实现要点

### 10.1 技术栈

- **前端组件**：React + TypeScript
- **状态管理**：useTransferProgress Hook（复用现有）
- **样式**：CSS Modules + CSS 变量
- **图标**：内联 SVG（支持 CSS 填充）

### 10.2 性能优化

- **虚拟滚动**：任务超过 10 个时启用虚拟滚动
- **动画优化**：使用 `transform` 和 `opacity`（GPU 加速）
- **事件节流**：进度更新使用节流（最小间隔 100ms）

### 10.3 可访问性

- **键盘导航**：Tab 键在任务间切换，Enter/Space 触发按钮
- **屏幕阅读器**：ARIA 标签描述任务状态和进度
- **高对比度模式**：进度条使用实心填充，图标使用描边

## 十一、文件结构

```
src/
  ├── components/
  │   ├── TransferStatusBar/
  │   │   ├── TransferStatusBar.tsx       # 状态栏右侧传输状态组件
  │   │   ├── TransferStatusBar.css       # 样式
  │   │   ├── TransferPanel.tsx           # 展开的任务列表面板（浮层）
  │   │   ├── TransferPanel.css
  │   │   ├── TaskCard.tsx                # 单个任务卡片
  │   │   └── TaskCard.css
  │   └── TransferNotification.tsx        # 旧组件（将被替换）
  └── apps/
      └── FileManager.tsx                 # 在状态栏中集成 TransferStatusBar
```

## 十二、迁移计划

### 阶段 1：组件开发（1-2 天）
- 创建 TransferStatusBar 组件（状态栏右侧部分）
- 创建 TransferPanel 组件（展开的浮层面板）
- 实现 TaskCard 子组件
- 复用 useTransferProgress Hook

### 阶段 2：集成到文件管理器（0.5 天）
- 修改 FileManager.tsx 的状态栏布局
- 在状态栏右侧集成 TransferStatusBar 组件
- 保持左侧原有的文件数量、总大小信息不变
- 移除旧的 TransferNotification 组件

### 阶段 3：测试和优化（0.5 天）
- 测试多任务并发显示
- 测试展开/收起动画
- 测试控制按钮功能
- 测试响应式布局（窄窗口适配）
- 优化性能和动画流畅度

## 十三、验收标准

- ✅ 无传输任务时状态栏仅显示左侧原有信息
- ✅ 有传输任务时状态栏右侧显示传输状态
- ✅ 收起状态右侧显示图标、数量、进度条、百分比
- ✅ 展开的任务列表面板显示在状态栏上方（浮层，右对齐）
- ✅ 每个任务卡片高度 40px，悬停显示控制按钮
- ✅ 展开/收起动画流畅（200ms）
- ✅ 图标使用 Adwaita Symbolic SVG
- ✅ 响应式布局适配不同窗口尺寸
- ✅ 键盘导航和屏幕阅读器支持

## 十四、参考资料

- [GNOME Human Interface Guidelines](https://developer.gnome.org/hig/)
- [Adwaita Icon Theme](https://gitlab.gnome.org/GNOME/adwaita-icon-theme)
- [Chrome Download Bar Design](https://developer.chrome.com/docs/extensions/mv3/user_interface/#downloads)
- 项目现有文件传输功能：`docs/superpowers/specs/2026-07-19-file-transfer-design.md`