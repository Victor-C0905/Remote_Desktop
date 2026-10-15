# Chrome 式页签条（ChromeTabStrip）实现与复用指南

## 功能概述

窗口标题栏即标签条（Chrome 形态）：页签条 portal 进窗口标题栏槽位，
活动页签与页签条正下方的表面（顶栏/终端区）无缝融合，看起来"伸入"
下方内容。共享组件一套，Terminal（深色变体）与 TextEditor（浅色变体）
已在用，新应用接入只需 5 步（见下文），不必重复造轮子。

## 组件清单（复用什么）

| 层 | 文件 | 职责 |
| --- | --- | --- |
| 共享页签条 | `src/components/ChromeTabStrip/ChromeTabStrip.tsx` | 声明式对接 `@sinm/react-chrome-tabs`：页签增删/激活/拖拽排序/+ 新建按钮（pinnedRight 插槽）；行内附件（状态灯、⌄ 菜单）走 children |
| 主题覆盖 | `src/components/ChromeTabStrip/ChromeTabStrip.css` | 库配色全部改写为 `--quirel-*` 令牌；variant 决定活动页签填充色；SVG 纵向 +2px 下探衔接 |
| 标题栏槽位 | `src/components/window-shell/WindowShell.tsx` 的 `.windowHeaderSlot` | 应用把页签条 portal 进来；槽位有内容时默认标题自动隐藏（CSS `:has`）；空白区域保持拖拽/双击最大化 |
| 底缝桥接 | `src/components/window-shell/WindowShell.module.css` 的 `.windowHeaderBar::after` | 窗口级全宽 4px 遮盖层，盖掉毛玻璃底边伪影暗带；颜色按 variant 随动（圆滑的关键，见下） |
| 颜色令牌 | `src/styles/variables.css` | `--quirel-headerbar-bg`（顶栏米色）/ `--quirel-window-bg`（内容区浅色）/ `--quirel-terminal-bg`（终端黑） |
| 底层库 | `@sinm/react-chrome-tabs` | 页签几何 SVG、拖拽排序、宽度自适应；自带 bottom-bar/入场动画已被 CSS 关闭 |

## 页签底部圆角的渲染机制

库的页签形状是单个 SVG path（viewBox `0 0 214 36`）：

```
M17 0 h197 v36 H0 v-2 c4.5 0 9-3.5 9-8 V8 c0-4.5 3.5-8 8-8 z
```

底部两角是**外撇的凹圆角（flare）**：页签左下角先从底边收到 (0,34)，
再用 `c4.5 0 9-3.5 9-8` 一段凹弧向上收进侧边。凹弧缺口区域是透明的，
透出的是页签条背景（标题栏毛玻璃）。所以"底部圆角圆不圆滑"本质是
**凹弧缺口区和页签底缘在标题栏交界行露出什么颜色**。

两个关键事实（实测，终端窗口，页签底=标题栏底=终端区顶=118px 三层齐平）：

1. 页签 SVG 底缘最后一行是抗锯齿半透明行，**颜色 = 填充色 × 背后垫色**。
   垫色 = 底缝桥接层（画在页签 SVG 下层）。垫色与页签同色则该行不可见；
   不同色则交界处出现一条异色细带，切断凹弧与下方表面的连续。
2. 毛玻璃标题栏自身最底 1-2 行有伪影暗带（backdrop-filter 采样到更深的
   壁纸渐变，比两侧深 30-40 RGB），不遮盖时凹弧缺口区会露出这条暗带。

### 终端 tab 底部圆角不圆滑的根因（已修复）

终端桥接层曾设为 `transparent`（试图避免浅色带压进黑终端），结果 6 倍
放大逐行取色（修复前）：

- 页签黑体正下方交界行：`63,63,64`（黑填充 × 毛玻璃浅底的混色行）
- 凹弧缺口区：`187,188,192`（毛玻璃伪影暗带）

即一条贯穿的灰带把黑凹弧与黑终端隔断。修复 = 桥接色必须等于
**标题栏正下方的表面色**（终端 = `--quirel-terminal-bg` 黑）：

- 页签黑体下：黑混黑，交界行不可见
- 凹弧缺口区：伪影暗带被表面色遮盖，凹弧落地连续

修复后同位置取色全部为纯黑 `30,30,30`，圆滑。TextEditor 无此问题
正是因为它的桥接色（`--quirel-window-bg`）= 页签填充色 = 顶栏色，
同一机制恰好成立。

### variant 体系

`ChromeTabStrip` 的 `variant` 声明活动页签融合哪个表面，行 wrapper 上
标 `data-tabstrip-variant`，三处联动（改 variant 不需要动 WindowShell）：

| variant | 活动页签填充 | 底缝桥接（WindowShell `:has` 随动） | 适用 |
| --- | --- | --- | --- |
| 缺省 | `--quirel-headerbar-bg` | `--quirel-headerbar-bg` | 下方是应用内米色顶栏 |
| `light` | `--quirel-window-bg` | `--quirel-window-bg` | 下方是浅色内容区/顶栏（TextEditor） |
| `dark` | `--quirel-terminal-bg` | `--quirel-terminal-bg` | 下方是黑色终端区（Terminal），标题/关闭钮自动反白 |

## 新应用接入清单（5 步）

1. **渲染页签条行**：应用内 `<ChromeTabStrip tabs activeTabId onAdd
   onActivate onClose onReorder variant="...">`，行内附件（状态、菜单）
   作为 children 传入（参考 `Terminal.tsx` 的 `renderTabRow`）。
2. **portal 进标题栏**：`createPortal(renderTabRow(), titlebarSlot)`，
   槽位来自 `document.querySelector('[data-titlebar-slot="<windowId>"]')`；
   槽位缺失时回退渲染在应用内（Terminal 已有该回退分支）。
3. **选 variant**：看标题栏正下方第一层表面的颜色——浅色内容区 →
   `light`；黑色终端 → `dark`；米色顶栏 → 缺省。**并保证该表面真用
   这个颜色**（TextEditor 为此把 te-topbar 换成了 `--quirel-window-bg`）。
4. **状态灯/低频操作**：连接状态等用行内附件；低频开关收纳进
   ToolbarMenu（⌄），不要挤占条面。
5. **验证**：浏览器 `document.body.style.zoom='6'` 放大页签底部两角，
   逐行取色确认凹弧缺口区与交界行没有异色细带（见上文根因分析）。

## 注意事项

- variant 必须匹配"标题栏正下方第一层表面"。若应用在标题栏与内容之间
  插入浅色子栏（如终端的搜索栏 `--quirel-card-bg`），子栏显示期间桥接
  色与其不一致，属已知取舍；新应用应让紧贴标题栏的那层表面直接用
  variant 同色令牌。
- 非活动页签 hover 底缘会有轻微混色（hover 填充是顶栏暗调 ≠ 桥接色），
  瞬时状态，可接受。
- 库自带 `chrome-tabs-bottom-bar`（4px 白条）与"新页签入场动画"已在
  ChromeTabStrip.css 关闭，勿删。
- 页签背景 SVG 的 `height: calc(100% + 2px)` 下探与窗口级桥接层是
  两道互补的衔接（局部几何衔接 + 全宽伪影遮盖），都不可拆。
