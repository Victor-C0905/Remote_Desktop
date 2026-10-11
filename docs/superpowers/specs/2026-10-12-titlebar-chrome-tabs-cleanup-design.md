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

## 实施修订（浏览器实测发现，均已验证）

实施后用真实浏览器逐项验证时，发现设计中的两处假设与库的真实行为不符，按
实测修正：

### 修订 1："+"按钮改走库官方 `pinnedRight` 插槽

原设计的"flex 兄弟按钮"方案基于"库条面收窄至页签总宽"的假设——实测为假：
库的模型是把条面可用宽度**均分给各页签**（Chrome 真实行为，内容宽钳制
60~240，`tabContentWidths` 分配逻辑），条面并不随页签收窄。

同时实测发现库的 content 盒记账宽度恒定比最后一个页签的渲染右缘**短
`2×TAB_CONTENT_MARGIN − 8 = 10px`**（`TAB_CONTENT_MARGIN=9`、
`getTabsWidth() = Σ把手宽 + 8 − (N−1)`），因此旧 WIP 的两种"+"方案（portal
按 content 右缘定位、兄弟节点按塌缩条面定位）都存在约 4~10px 的重叠——
迁移中途两个"+"按钮恰好叠在同一位置，视觉上被掩盖，测试却因多元素命中而
失败。

修正："+"按钮放进库官方 `pinnedRight` 插槽（`chrome-tabs-toolbar-right`，
流内跟在 content 之后），配 `margin-left: 14px`（10px 清空 + 4px 间隙），
实测 N=1/2/3 页签均无重叠。零 portal、零 ResizeObserver 追踪。

### 修订 2：删除 vendor CSS，改用包内官方 CSS

`chrome-tabs-vendor.css` 的前提"库包内未发布 CSS"已过时——实测
`@sinm/react-chrome-tabs` 包内发布 `css/chrome-tabs.css`，且与 vendor 文件
**零差异**。改为官方导入
`import '@sinm/react-chrome-tabs/css/chrome-tabs.css'`，vendor 文件删除。

### 修订 3：条面从 `flex: 0 1 auto` 改为 `flex: 1 1 auto`

条面收缩会让库把极小的可用宽度均分给页签，页签被压到最小宽 78px、标题
只剩 4px 不可见。占满可用宽度后页签恢复 Chrome 行为（单页签 258px、标题
180px 完整可见）。

### 修订 4：隐藏库自带的 chrome-tabs-bottom-bar 白条

用户反馈标题栏底部（页签右侧"+"下方）有一条横线。实测定位：不是任何
border，而是库自带的 `.chrome-tabs-bottom-bar`——4px 高、全宽、纯白、
z-index:10 的横条（库 demo 中作"页签下方工具栏表面"），其下边缘与奶油色
顶栏相接形成硬边，还盖住页签底部 4px。库 JS 对该元素零依赖。项目顶栏
自己承担工具栏表面角色，故 `display: none` 隐藏；活动页签经 SVG 下探
2px 与顶栏直接融合（seam=0），即 Chrome 原版"页签条与工具栏连续表面"
形态。用户设想的"tab 下移"无需——页签本就贴条底。

### 修订 5：恢复迁移时丢失的底缝遮盖规则（修订 4 的补全）

用户复查指出：隐藏白条后页签底部与顶栏之间仍有缝隙、圆角不圆滑、且有
分割线。根因：修订 4 只处理了白条，而 @sinm 迁移重写 ChromeTabStrip.css
时，把 c770f4b 已验证的两条修缝规则弄丢了——① 条面 `::after` 全宽同色
覆盖层（下探 2px 盖亚像素缝隙行）；② 页签背景 SVG `height: calc(100%+2px)`
纵向下探。本修订原样恢复两条。

像素级验证：衔接区逐行扫描零暗线（页签正下方缝隙行最小 RGB
(242,238,229)，无蓝色缝隙露出），页签两脚弧线落在同色覆盖层上与顶栏
无缝融合，边界仅剩约 4 行毛玻璃→顶栏的柔和颜色过渡。

### 已知瞬态（接受）

库的布局是异步链（rAF + mouseEnter 延迟布局），开窗/增删页签后 content
宽度与"+"位置在约 1s 内有一次自愈式重排（实测瞬时偏差 −6px → 稳态 +4px）。
属库内部时序，不值得为消除它引入追踪机制。

## 验证标准

1. `vitest run src/apps/TextEditor/TextEditor.test.tsx` 全绿（含此前因重复
   按钮失败的 2 例）。✅ 实测 5/5
2. `tsc --noEmit` 通过；全量测试无回归。✅ 实测 297/297
3. 视觉验收：dev server 截图标题栏——单个"+"按钮紧随页签（间隙 4px，无
   重叠）、页签几何与主题令牌配色正常、活动页签与顶栏融合。✅ 实测通过
