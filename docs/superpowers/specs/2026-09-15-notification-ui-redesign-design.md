# 通知 UI 与连接失败画面重设计 · 设计规格

- **日期**：2026-09-15
- **状态**：待审阅
- **范围**：纯前端（TSX/CSS），不改协议、不改 Rust、不改通知数据模型
- **前置依赖**：登录通知回应系统（2026-09-15 已实现）：`src/types/errors.ts` 错误映射表、`src/stores/notificationStore.ts`、ServerManager 通知派发

## 1. 背景与目标

登录通知回应系统已接通结构化错误数据（`AuthErrorCode` → `ERROR_MAP`），但呈现层仍是初版：

- NotificationCenter 使用 emoji 图标（🔴🟡🟢💡📭）、大量硬编码颜色（#e01b24/#99c1f1 等），暗色主题不适配
- 正文为 `\n` 拼接纯文本，标题/消息/建议/对比提示混排无层次
- 新通知到达无任何即时反馈（仅 TopBar 徽标计数变化）
- 应用断连后仅显示「📡 未连接」占位，结构化错误信息（标题/建议/重试）不进入应用主画面，用户必须去 Settings 或通知中心才能看到失败原因

**目标**：

1. 通知中心视觉与信息层次升级，对齐项目 Adwaita 设计语言（Symbolic SVG 图标、`--quirel-*` 令牌、亮暗主题）
2. 新通知到达时桌面顶部短暂预览，不打断操作
3. 应用画面内直接呈现连接失败错误态（含建议与重试入口），全应用统一组件

## 2. 决策记录

| 决策点 | 选择 | 备选（已否决） |
|---|---|---|
| 通知中心布局 | A 增强右侧面板（现有布局精修） | B GNOME Shell 式（顶部横幅+按来源分组折叠，过度设计）；C Toast+通知中心（Win11 式，双机制维护成本高） |
| 新通知到达预览 | 桌面顶部预览条（TopBar 下方居中，全局可见） | 应用内 toast（依赖聚焦窗口）；无预览（反馈弱） |
| 失败画面布局 | A 居中错误态覆盖层（骨架屏背景） | B 顶部横幅（详情展示受限）；C 全屏错误页（连接失败非致命场景，过重） |
| 重连中呈现 | 轻量重连态（spinner+进度提示），最终失败才显示完整错误态 | 直接完整错误态；维持现状骨架 |
| 图标体系 | Adwaita Symbolic 风格内联 SVG | 保留 emoji（与项目风格不统一）；混合方案 |

## 3. 图标与设计令牌基础

### 3.1 Symbolic SVG 组件（新建 `src/components/symbolic.tsx`）

内联 SVG 图标集，16px 网格，`fill="currentColor"`，用法 `<SymbolicIcon name="network-offline" size={16} />`：

| name | 用途 |
|---|---|
| `dialog-error` | 紧急严重度、错误态 |
| `dialog-warning` | 普通严重度 |
| `dialog-information` | 低严重度、对比提示前缀 |
| `view-refresh` | 重连中（旋转动画）、重试按钮 |
| `network-offline` | 断开/错误态主图标 |
| `mail-unread` / `mailbox` | 通知列表 / 空态 |
| `emblem-ok` | 全部已读 |
| `user-trash` | 清除/删除 |
| `window-close` | 关闭按钮 |
| `bell` / `bell-outline` | TopBar 通知入口（有/无未读） |

要求：单文件集中管理；每个图标为纯 path 的函数组件；不引入外部依赖。

### 3.2 颜色令牌（`src/styles/variables.css` 增补，`--quirel-` 前缀）

```css
/* 语义严重度（亮色值示例，暗色主题同结构覆盖） */
--quirel-danger-fg / --quirel-danger-bg / --quirel-danger-border
--quirel-warning-fg / --quirel-warning-bg / --quirel-warning-border
--quirel-success-fg / --quirel-success-bg / --quirel-success-border
```

- NotificationCenter.css、Settings.css 中所有硬编码语义色（#e01b24、#99c1f1、rgba(224,27,36,.1) 等）替换为上述令牌
- 令牌值需在亮/暗两套主题下各自定义（跟随现有主题切换机制）
- `serversStore.ts` 的 `getStatusColor`/`getStatusIcon` 中硬编码色与 emoji 同步替换

**urgency → 语义令牌映射**（全 UI 统一）：`critical → danger`、`normal → warning`、`low → success`

## 4. 通知中心面板重设计（`src/shell/NotificationCenter.tsx` + `.css`）

**数据模型与交互逻辑不动**（notificationStore、过滤、已读、清除均保持），纯呈现层重写。

### 4.1 条目卡片结构

```
┌──────────────────────────────────────┐
│ ▌ [sev-icon] 标题            [来源chip] ×│
│   正文多行（pre-line，含对比提示）          │
│   [建议行动 ghost-chip]           时间    │
└──────────────────────────────────────┘
```

- 左侧 3px 严重度色条（`--quirel-danger/warning/success` 按 urgency）
- 严重度 Symbolic 图标随 urgency 着色
- 标题 15px medium；正文 13px secondary 色 `pre-line`；建议 chip 12px ghost（中性背景+边框，纯展示不可点击）；时间 12px muted
- 来源 chip：`server.name || server.host`，12px，muted 背景
- 未读条目：浅色 tint 背景 + 左边框强调色；已读：中性背景
- 删除按钮：hover 条目时显示（替代常驻 ×，降低视觉噪音）
- 点击条目 = 标记已读（维持现状）

### 4.2 面板结构

- 头部：标题「通知」+ 未读徽标 + 操作（全部已读 / 清除 / 关闭，均为 Symbolic 图标按钮）
- 筛选行：caption 级 chips（全部/紧急/普通/低 + 计数），active 用主题强调色
- 空态：`mailbox` 图标 + 「没有通知」
- 页脚：单行未读计数（保留现有三种文案逻辑）
- 面板宽度维持 360px、右侧滑出交互不变

## 5. 到达预览条（新建 `src/shell/ArrivalToast.tsx`，Desktop 挂载）

### 5.1 结构与位置

- Desktop 层渲染，`position: fixed`，TopBar 下方 12px 水平居中，z-index 高于所有窗口
- 尺寸：宽 `min(480px, 90%)`，高 44px，`--quirel-window-bg` 背景、主题边框、radius-card 圆角
- 内容：严重度 Symbolic 图标（随 urgency 着色）+ 标题（14px medium，溢出省略）+ 来源 chip（12px）+ 「+N」累计徽标（brand 色，有积压时）+ 关闭按钮（仅 hover 显示）

### 5.2 交互规格

| 状态 | 行为 |
|---|---|
| 进入 | 订阅 notificationStore；`pushNotification` 触发，内容为最新一条；translateY 下滑 + 淡入 200ms ease-out |
| 停留 | 3.5s 计时；hover 暂停计时并显示关闭按钮 |
| 退出 | 超时上滑淡出 200ms；点击任意区域 → 打开通知中心（不标记已读）；点 × → 直接关闭 |
| 连发 | 最新一条替换内容并重置计时；期间错过的条目累计进「+N」 |
| 降级 | `prefers-reduced-motion: reduce` 时无位移动画，直接显隐 |

### 5.3 实现

- 组件内部管理单条显示状态 + 计时器（`setTimeout` 清理于 unmount）；不新增 store 字段
- 通过 prop 回调 `onOpen` 与 Desktop 的 `setNotificationOpen(true)` 联动

## 6. 应用错误态/重连态组件（新建 `src/components/ConnectionState.tsx`）

### 6.1 `ConnectionErrorState`

props：`{ title: string; message: string; hint?: string; action?: string; onRetry: () => void; onOpenSettings?: () => void }`

结构（居中卡片，宽约 420px）：

1. `network-offline` Symbolic 图标 48px，danger 色
2. 标题：16px semibold（`ERROR_MAP.title`）
3. 消息：14px secondary
4. 建议：ghost chip（12px，中性背景边框）
5. 对比提示（可选 hint）：12px muted + `dialog-information` 小图标
6. 按钮组：`重试`（强调色填充，高 36px，`view-refresh` 图标+文字，触发重连）｜`查看服务器`（ghost，打开 Settings 应用，复用现有应用打开机制）

动效：卡片淡入上浮 180ms ease-out；背景骨架屏透明度降至 40%。

### 6.2 `ReconnectingState`

props：`{ serverName?: string; attempt: number; maxAttempts: number }`

- 居中轻量卡片：旋转 `view-refresh` 图标（1s 线性循环）+ 「正在重新连接…（第 n/3 次）」14px
- 无按钮；背景骨架屏同样降透明
- reduced-motion 时 spinner 改为静态图标 + 文字

### 6.3 数据流改造（`src/stores/serversStore.ts`）

`ServerConfig.error` 从 `string` 扩展为结构化：

```ts
error?: { code: number; detail?: string } | string  // 过渡期兼容
```

- ServerManager 写入点（`setServerStatus(id, "error", ...)`）改传结构化对象（`parseConnectError` 结果）
- 新增辅助 `getServerErrorInfo(server): { title, message, action, hint... }`，内部查 `getErrorInfo` + 组装 hint（其他服务器对比提示）
- 现有消费方（Terminal/Settings 的 `buildConnectFailureText`）无需改动：其 `parseConnectError` 已识别数值 `code` 字段，结构化对象传入时自动走查表路径
- 展示组件通过 `getServerErrorInfo` 渲染，不再解析拼接文本

### 6.4 接入范围

| 应用/位置 | 现状 | 改造 |
|---|---|---|
| SystemMonitor.tsx:310 | `📡 未连接` overlay | error → `ConnectionErrorState`；reconnecting → `ReconnectingState`；**从未连接（无 activeServer 或从未收到数据）→ 维持轻量占位（仅换 Symbolic 图标+令牌色），不显示错误态** |
| FileManager.tsx:1711 | `offline-icon 📡` | 同上 |
| BrowserApp.tsx:144 | 「未连接服务器」占位 | error → 错误态；未连接维持现有占位 |
| Settings.tsx 连接卡片 | `.st-conn-error` 纯文本 | 结构升级：danger 图标 + 标题行 + 分行正文 + 建议 chip（同一套数据源）；`white-space: pre-line` 保留 |

接入原则：错误态仅在 `status === "error"` 时替换占位；各应用「从未连接」的空态语义不变。

## 7. TopBar 升级

- 通知铃铛：`bell`（有未读，brand 强调）/ `bell-outline`（无未读）替换现有图标
- 未读徽标样式沿用，颜色收敛令牌
- 服务器状态：维持色点方案（`getStatusColor` 收敛令牌），`getStatusIcon` 的 emoji 仅用于既有文本场景的逐步替换，TopBar 不引入新图标

## 8. 范围外（本次不做）

- 通知 action chip 的点击路由（纯展示）
- 通知免打扰/静音设置项
- 通知持久化（维持内存瞬态，符合「存储全走 Rust」约束）
- 通知按来源/日期分组（平铺按时间倒序）
- 协议与 Rust 侧任何改动

## 9. 测试与验证

- **vitest**：
  - `ArrivalToast` 计时逻辑（vi.useFakeTimers：3.5s 自动退出、hover 暂停、连发重置+计数）
  - `ConnectionState` 渲染（props → DOM 层级）
  - `serversStore` error 结构化写入/读取（getServerErrorInfo）
- **静态**：`npx tsc --noEmit` 零错误
- **构建**：`npm run build` 通过
- **手动冒烟**：断网自动重连（轻量态→错误态→恢复）、错误密码（错误态+不重试文案）、多服务器对比提示、通知到达预览条、亮暗主题切换

## 10. 文件清单

| 文件 | 操作 |
|---|---|
| `src/components/symbolic.tsx` | 新建（Symbolic SVG 图标集） |
| `src/components/ConnectionState.tsx` | 新建（错误态+重连态） |
| `src/shell/ArrivalToast.tsx` | 新建（到达预览条） |
| `src/shell/NotificationCenter.tsx` / `.css` | 重写呈现层 |
| `src/shell/Desktop.tsx` | 挂载 ArrivalToast、联动打开通知中心 |
| `src/styles/variables.css` | 增补语义令牌（亮暗两套） |
| `src/stores/serversStore.ts` | error 结构化 + getServerErrorInfo + 状态图标色收敛 |
| `src/context/ServerManager.tsx` | setServerStatus 写入结构化错误（`自动重连失败:` 前缀场景同样结构化） |
| `src/apps/SystemMonitor.tsx` / `FileManager.tsx` / `BrowserApp.tsx` | 错误态/重连态接入 |
| `src/apps/Settings.tsx` / `.css` | 错误行结构升级 + 令牌收敛 |
| `src/shell/TopBar/TopBar.tsx` | 铃铛图标替换 |
| 对应 `.test.tsx` | 新建/更新 |
