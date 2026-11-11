# Quirel 客户端第二轮可视化测试报告 — 窗口交互与瞬态视觉专项

- 日期：2026-11-11 01:30 ～ 02:40
- 被测版本：`fix/visual-test-issues` 分支（含第一轮 11 项修复）
- 测试服务器：泰国服务器 `mydoorctl.top:9443`（QUIC，全程已连接）
- 测试方法：真实客户端 + **连续帧捕获**（CDP screencast 帧序列 + 系统级 14fps 连拍）
  + Win32 确定性窗口操作（MoveWindow/ShowWindow）+ CDP DOM 驱动
- 本轮目标：第一轮截图法覆盖不到的**交互过程/瞬态视觉**问题（拖拽、缩放、最大化、
  z 序、Dock 交互）
- 证据目录：`docs/testing/evidence-2026-11-10/`（T1*/T2*/T3*/T6* 前缀 + 帧序列子目录）

> **复核状态（2026-11-11，`fix/visual-test-issues` 分支）**
> - **F2-A 撤回**：判定为四分之一吸附 + 静态按钮文案误判的测试伪影，非最大化
>   丢失（证据见 §六）。同症状的「刷新后最大化丢失」真实缺陷已另行修复。
> - **F2-B 保留待量化**：隐藏链路无逻辑错误，先按 §五.5 用 60fps 录屏计时，
>   再决定是否改时序（本轮未改）。
> - **F2-C / F2-D** 维持记录；测试可诊断性已补 `data-maximized` /
>   `data-snap-zone`，不再依赖静态按钮 title。

## 一、本轮结论

**窗口系统的核心交互链路整体稳健**：拖动/缩放/最大化/最小化恢复/z 序提升/
Dock 三态循环实测全部工作正常；主窗口 OS 级连续缩放 57 帧零坏帧。

发现 **1 个 P1**（浏览器窗口最大化状态在主窗口缩放后丢失且几何持续漂移）、
**1 个 P2**（拖动中 chrome/webview 瞬时分离帧）；另有若干重要排查结论
（多个第一轮疑点判定为测试伪影）。

## 二、问题清单（供后续 agent 修复）

### F2-A（P1）浏览器窗口最大化状态在主窗口缩放后丢失，且几何持续漂移

- **复现**：
  1. 打开浏览器窗口（TabBook，Google 页签），最大化
  2. 对主窗口做 OS 级连续缩放（MoveWindow 1100×880 ↔ 1180×910 六次，间隔 120ms）
  3. 再缩放主窗口至 1450×950
- **实测**：浏览器窗口脱离最大化态（按钮恢复「最大化」文案），几何先后读数为
  `718×455` → `583×435`——**两次只读观察之间仍在自行变化**；既不是工作区
  铺满尺寸，也不是其还原尺寸（1000×700）
- **预期**：最大化窗口应随工作区变化重新铺满；至少不应出现几何漂移
- **证据**：`T2-resize-os/`（57 帧）、`T3-terminal-shaken.png`（可见浏览器窗口
  被文件管理器覆盖时右侧露出桌面）、DOM 读数（本记录第二节）
- **代码指针**：
  - `src/window-system/core/WindowManager.ts`（maximize/unmaximize 状态与
    workspace 重算时机）
  - `src/components/window-shell/WindowShell.tsx`（「开窗动画归位后的 resize
    补偿」段，isOpening/transition 与窗口 resize 的竞态）
  - `src/apps/BrowserApp.tsx`（bounds 上报 effect 的触发条件与节流）
- **备注**：浏览器窗口此前还经历了一次主窗口 1024→1136→1436 的 resize 与
  最大化切换；最小触发路径需二分定位（怀疑：主窗口 resize 事件触发的
  un-maximize 用了过期的 workspace/窗口状态快照）

### F2-B（P2）拖动过程中 chrome 与 webview 瞬时分离帧

- **现象**：窗口拖动进行中的连拍帧（`T1a3-drag-os/o0016.png`）显示 DOM chrome
  （页签条/地址栏所在行）与 webview 内容（Google 页面）相对错位
- **设计**：WindowShell 拖拽时广播 `WINDOW_DRAG_EVENT`，BrowserApp 收到后隐藏
  webview（防止「画面与窗口分离」——代码注释明确此设计）
- **实测**：干净拖动确认广播正常 fire（`dragging:true→false` 成对出现）；
  但连拍仍捕捉到分离帧 → 隐藏生效前/恢复竞态窗口期（约百毫秒级）内
  webview 仍以旧 bounds 可见
- **复核方法**：正常用户速度拖动 + 60fps 录屏（OBS/WinUI Recorder），统计
  分离帧持续时长；>100ms 即用户可感知
- **代码指针**：`BrowserApp.tsx` 可见性裁决（visibleStateRef/串行恢复流程）+
  `webview_host.rs set_bounds`

### F2-C（P3，记录）OS 级合成拖拽会命中主窗口顶栏——测试污染源

- 页面坐标 (300,90) 等看似「桌面空白区」的点，实际位于主窗口顶栏拖拽区
- 本轮多组 OS 拖拽实际移动了主窗口（净位移累计），导致：
  - 主窗口部分出屏 → 后续所有「元素被屏幕边缘裁切」的观察
    （红绿灯/Gmail/AI 模式缺失）均为**出屏裁切伪影**，非应用缺陷
  - 主窗口归位后复测（`T1f-clean-final.png`）：页签标题、红绿灯、Google
    内容全部正确对齐 ✓
- **后续测试规范**：交互测试前先用 Win32 枚举确认主窗口位置，或全部用
  CDP/MoveWindow 驱动，避免 OS 级合成拖拽误触顶栏

### F2-D（P3，未复现）第一轮疑点「Dock 点击已运行应用未提升」

- 本轮实测三态循环全部正常：未聚焦→点击提升 ✓ / 已聚焦→点击最小化 ✓ /
  最小化→点击恢复 ✓。第一轮观察疑似受当时设置窗口最大化状态干扰，撤回。

## 三、通过项（本轮）

| 项 | 覆盖点 | 结果 |
|---|---|---|
| 浏览器窗口拖动 | chrome-tabs 空白区拖动=窗口移动（位移精确跟随），拖页签=页签手势（不误触窗口拖拽），两者互斥正确 | ✅ |
| 拖拽广播 | WINDOW_DRAG_EVENT 成对 fire（dragging:true/false），按 windowId 过滤 | ✅ |
| 主窗口 OS 缩放 | MoveWindow 连续 6 次变尺寸 + 两次定型缩放：57 帧 0 坏帧（14fps 采样），内容自适应重排，终端/浏览器同步跟随 | ✅（13~55ms 级坏帧仍需按 `docs/window-flash-rules.md` §7 探针终判） |
| 多窗口 z 序 | 点击下层窗口可见区提升 ✓；Dock 提升 ✓ | ✅ |
| Dock 三态 | 提升 → 最小化 → 恢复 循环 | ✅ |
| 终端随主窗口缩放 | shake 后 xterm fit 正常（111×26）、渲染无破损 | ✅ |
| 窗口出屏恢复 | 主窗口被拖出屏后拖回，webview 内容即时贴合 | ✅ |

## 四、测试基建（供后续 agent 复用）

| 工具 | 位置 | 用途 |
|---|---|---|
| `quirel-qa.mjs` | `%TEMP%\quirel-qa\` | CDP 驱动：snapshot/click/dbl/drag(已修坐标解析)/type/typex/key/wheel/console 等 |
| `quirel-cast.mjs` | 同上 | CDP screencast 帧序列捕获（变化帧，约 3-5fps） |
| `osburst.ps1` | 同上 | 系统级全屏连拍（固定间隔，含子 webview HWND） |
| `mainwin.ps1` | 同上 | Win32 确定性主窗口操作：resize/shake/minmax（用 hwnd=133512，参数名已去前缀歧义） |
| `sendinput.ps1` | 同上 | OS 级真实键鼠（click/type/ctrla/drag）——注意会命中主窗口顶栏 |

**注意事项（写给下一个测试 agent）**：
1. OS 级合成拖拽会误触主窗口顶栏移动主窗口（F2-C）——每轮交互测试前先
   截屏核对主窗口位置，出屏内容一律先归位再判定。
2. 屏幕物理分辨率与 DPI 虚拟化坐标系（winrect/CopyFromScreen/CDP CSS）
   三者不能互相换算，跨坐标系定位一律以 DOM gBCR（CSS px）为准。
3. vite HMR 重载 window-system/Desktop 模块会导致 WindowManagerContext
   丢失崩溃（「界面发生错误」），location.reload() 即可恢复；属开发模式
   已知现象，非应用缺陷。
4. 帧序列分析程序（异常帧采样判定）见本报告执行记录；判定阈值：80 采样点
   中 ≥30 个纯白(>248)/纯黑(<8)。

## 五、遗留（本轮未覆盖，建议下一轮）

1. 终端洪流输出期间的窗口缩放（xterm 渲染压力下的掉帧/重排错乱）
2. 文件管理器列宽拖拽手柄的连续拖拽、虚拟化列表快速滚动中的闪烁
3. 编辑器 CodeMirror 在窗口缩放中的滚动位置保持
4. 浏览器页签快速开关与窗口缩放的并发（webview 创建/销毁竞态）
5. 60fps 录屏（OBS）复现 F2-B 的用户可感知时长

## 六、复核记录（2026-11-11）

### F2-A 撤回：四分之一吸附伪影，非最大化丢失

1. **代码路径不支持该复现**：`WindowManager.handleViewportResize()` 对最大化
   窗口只重铺工作区；能清除最大化状态的只有 `setMaximized(false)` 与
   `setSnap()`，后者仅在拖拽松手命中吸附区时触发。
2. **读数正好是四分之一吸附矩形**：`1450×950` 工作区的一半为 `725×459`，
   `1180×910` 的一半为 `590×439`；报告读数 `718×455` / `583×435` 与两组值
   恰好相差同一比例（×0.99，两轴一致），即主窗口两个尺寸下的吸附矩形。
3. **按钮文案不是状态判据**：`WindowControls` 的 `title` 恒为
   `windowShell.maximize`（静态文案），与当前是否最大化无关。
4. **T2 之前窗口已被拖成吸附态**：`T1a3-drag-os/o0067.png` 显示浏览器窗口
   处于右半屏吸附；按本报告 F2-C，OS 合成拖拽本身就污染现场。

结论：F2-A 是「OS 合成拖拽造成吸附 + 静态按钮文案误判」，撤回。

### 顺带修复的真实缺陷（同症状）

- 会话快照此前不保存 `maximized` / `snapZone` / `pre*`，刷新（HMR/崩溃重载）
  后最大化窗口会变成「普通窗口 + 快照时刻旧工作区尺寸」——与 F2-A 症状一致。
  已修 `c127317`：快照往返布局状态，恢复时按当前工作区归一化，旧快照兼容。
- 最大化几何曾在 render 时缓存，主窗口缩放后的 120ms 防抖窗口内点击最大化
  会取到过期工作区。已改为点击时现取 `cb67061`。
- `WindowShell` 根节点补 `data-maximized` / `data-snap-zone`（`cb67061`），
  后续自动化可直接读取状态。

### F2-B 保持待量化

隐藏链路为「4px 阈值 → 广播 → React state → effect → IPC → runtime
SetVisible」，与 webview bounds 上报是两条独立 IPC；分离帧是该链路的固有
天花板，不是逻辑缺陷。提前到 mousedown 广播会让「单击标题栏」也闪隐，
本轮不改；按 §五.5 用 60fps 录屏统计时长，>100ms 再考虑在事件监听器内
直接 invoke 隐藏等提速方案。

### 验证

- `npx tsc --noEmit` 干净。
- `NODE_OPTIONS=--experimental-require-module npx vitest run src/window-system
  src/components/window-shell` → 9 文件 / 65 用例全绿（含新增 3 条快照布局
  用例）。未加该变量时本地仍被 `@csstools/css-calc` ERR_REQUIRE_ESM 阻塞
  （环境问题，非本次改动引入）。
- 提交：`e4249a5`（补 hook 文件）、`2b5e39a`（忽略证据目录）、`c127317`、
  `cb67061`。
