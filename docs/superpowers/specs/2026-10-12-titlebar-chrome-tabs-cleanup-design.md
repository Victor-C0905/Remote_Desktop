# 编辑器标题栏 chrome-tabs 收敛设计（推倒重建）

日期：2026-10-12　分支：feature/dev_by_zcode

## 背景与问题

标题栏页签（Chrome 形态）经过多轮迭代（自绘几何 → 接入 vanilla `chrome-tabs`
→ 迁移 `@sinm/react-chrome-tabs`），工作区里积累了多套并存的实现：

1. **两个"+"新建页签按钮**：`ChromeTabStrip.tsx` portal 进库条内一个（由
   `ResizeObserver` + `--tabs-end` 变量追踪页签右缘定位），`TextEditor.tsx`
   又渲染一个 flex 兄弟节点（strip 收窄后自然紧随页签）。两份 DOM、两套定位
   CSS 同时生效，测试 `getByTitle('新建页签')` 命中多元素，2 个用例失败。
2. **三份"+"按钮样式**：`TextEditor.css`（`.te-tab-new`）、
   `ChromeTabStrip.css`（`.te-tabstrip .te-tab-new` 静态版 + `.te-chrome-tabs
   .te-tab-new` 绝对定位版）。
3. **自绘页签死代码**：接入库之前的经典 Chrome 几何自绘实现（`.te-tab`、
   `.te-tab-icon/name/dot/close`、`.te-tab-corner*` 连接弧，约 93 行）在 TSX
   中已零引用，纯死 CSS。
4. **双依赖**：`chrome-tabs`（vanilla）与 `@sinm/react-chrome-tabs` 曾经并存
   （WIP 已移除前者，待提交确认）。

## 目标

- 单一实现：页签条只由一个第三方库渲染，项目内只保留必要的桥接与主题覆盖。
- 库选型：**`@sinm/react-chrome-tabs@2.6.3`**（用户确认）。

### 选库依据（2026-10 调研）

| 库 | npm 周下载 | GitHub Stars | 最新 release | 维护 |
|---|---|---|---|---|
| adamschwartz/chrome-tabs（vanilla 原版） | 1,251 | 1,791 | 5.4.0（2019-05） | 停更约 3 年 |
| **@sinm/react-chrome-tabs（选定）** | **1,391** | 36 | 2.6.3（2026-03-04） | 活跃 |
| react-chrome-tabs（whlshy） | 35 | 极少 | — | 无人用 |

`@sinm/react-chrome-tabs` 是 Adam Schwartz 原版引擎的 React 移植（几何/拖拽
复用原版实现），提供声明式 props；npm 实际安装量最高，维护活跃，有正式
release。选它同时消除了原版方案所需的 ~450 行命令式 React 桥接。

## 拆除清单

| # | 位置 | 动作 |
|---|---|---|
| 1 | `ChromeTabStrip.tsx` | 删 portal"+"按钮、`--tabs-end` ResizeObserver 追踪、`onAdd` prop（`+` 由上层渲染，桥接不再关心） |
| 2 | `ChromeTabStrip.css` | 删两套 `.te-tab-new` 规则（按钮样式归位 `TextEditor.css`） |
| 3 | `TextEditor.css` | 删自绘页签死代码 `.te-tab` 系列 + `.te-tab-corner*`（~93 行）；`.te-tab-new` 收敛为唯一规则集（采用现生效的 24px 版数值） |
| 4 | `TextEditor.tsx` | 保留 flex 兄弟"+"按钮（唯一来源）；桥接调用去掉 `onAdd` |
| 5 | `package.json` | 维持单一依赖 `@sinm/react-chrome-tabs@^2.6.3`（vanilla 已移除，锁文件已同步） |

## 最终架构（重建后）

```
WindowShell HeaderBar
└─ [data-titlebar-slot] 槽位（空白区域拖拽窗口）
   └─ TextEditor portal → .te-tabstrip（flex 行）
      ├─ ChromeTabStrip（纯桥接：tabs/onTabActive/onTabClose/onTabReorder）
      │  └─ @sinm/react-chrome-tabs <Tabs>（库渲染页签 DOM/SVG 几何）
      └─ .te-tab-new"+"按钮（flex 兄弟，strip 收窄后自然紧随页签）
```

- `ChromeTabStrip.tsx`：唯一职责是 React 状态 ↔ 库事件的声明式对接
  （脏标记借 `faviconClass` 渲染圆点；移除由上层状态驱动）。
- `ChromeTabStrip.css`：唯一职责是库主题覆盖（`--quirel-*` 令牌替换
  Chrome 原版灰）。
- `TextEditor.css`：标签条行布局（菜单栏分隔线、"+"按钮）。
- vendor CSS（`chrome-tabs-vendor.css`）：库包内未发布 CSS，从官方 tarball
  vendor，保留。

## 验证标准

1. `vitest run src/apps/TextEditor/TextEditor.test.tsx` 全绿（含此前因重复
   按钮失败的 2 例）。
2. `tsc --noEmit` 通过；全量测试无回归。
3. 视觉验收：dev server 截图标题栏——单个"+"按钮紧随页签、页签几何与主题
   令牌配色正常、活动页签与顶栏融合。
