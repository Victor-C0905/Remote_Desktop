# Debug Session: maximize-black-flash

**Session ID:** `maximize-black-flash`
**Status:** [FIXED-VERIFIED]（探针帧级实证 + 修复实现；安装包级人工复核待用户）
**Created:** 2026-10-26 → 2026-10-27
**沉淀文档:** 根因决策链与生命周期红线见 `docs/window-flash-rules.md`（本文是证据与实验过程记录）

## Problem Description

**Symptoms:**
1. 165Hz 笔记本上窗口最大化时明显闪黑；75Hz 台式机无感。
2. 把笔记本刷新率调到 60Hz 后闪黑消失（用户的原始观察）。

**用户核心问题：** 不能强制用户调低刷新率，怎么根治？

## 根因模型（探针实测钉死）

**刷新率不是故障源，是「放大器」。**

最大化是窗口几何的一次性大跳变。窗口背衬面随之放大，新增暴露区在
WebView2 新帧栅格化完成前是坏帧；防闪三件套（实验 F+G）的「不擦背景 +
NULL 类刷」让坏帧呈现为**纯黑**。

坏帧窗口时长 **13~55ms，与刷新率无关**（60Hz 与 165Hz 实测一致）；
刷新率只决定它被 DWM 采样成几个合成帧：

- 165Hz：2~5 帧连续黑 → 必然可见
- 60Hz：1 帧 → 常被最大化动画掩蔽

### 探针测量（DXGI Desktop Duplication，帧级抓屏）

探针：`%TEMP%/flicker_probe/dupcap`（独立 Rust 工具，项目同款 windows
crate 0.62；dxcam/windows-capture 在混合显卡上不可用，必须 Duplication 指向
实际拥有扫描输出的 AMD 适配器）。流程：置前台 → 1s → SW_MAXIMIZE →
2.5s → SW_RESTORE，逐合成帧统计 h0（近黑像素数）+ 异常帧留档。

| run | 条件 | 结果 |
|---|---|---|
| run2 | 旧构建（直通修复前）165Hz | 坏帧窗口 ≈300ms（130ms 全黑 + 170ms 渐进填充） |
| run4 | 新构建首最大化（冷实例） | 2.3s 黑屏冻结（Chromium 首帧着色器编译一次性成本，非稳态行为） |
| run5 | 新构建热实例 165Hz | 黑脉冲 2 帧 ≈13ms |
| run6 | 同实例 60Hz | 黑脉冲同样存在（h0=85502 与 165Hz 的 85479 同画面），时序一致 |
| run7/8 | 热实例 165Hz（带动画） | **无脉冲** |
| run9 | 同实例 `--noanim`（DWMWA_TRANSITIONS_FORCEDISABLED） | 黑脉冲 5 帧 ≈42ms |

**结论修正（run7~9 鉴别实验）：** 黑脉冲与最大化动画**无因果**——它是
「栅格化 vs 下一个 vsync」的竞速，随机出现（run5 有、run7/8 无）；关动画
不消除反而可能更长（n=1）。**掩蔽比消除更可行。**

### 修复前关键帧证据（run2，t=1290）

窗口已达最大化尺寸，但 WebView2 内容仍停在旧 bounds（左上角一小块），
其余 ~65% 屏幕纯黑——连主 UI 壁纸 DOM 都没画出来。

## 修复（实验 G 定稿：坏帧着色，mask 而非消除）

主窗口暴露区与 WebView2 未栅格化区域以**当前壁纸平均色**填充：

1. `lib.rs` `no_erase_proc`：主窗口分支（`SetWindowSubclass` data=1 区分）
   WM_ERASEBKGND 改为 `FillRect` 壁纸平均色（`WINDOW_BG_COLORREF` 静态，
   初值 = 默认壁纸 dusk 平均色）；子窗口链维持「不擦」（WebView2 背景由
   Chromium 自管，填充会在内容上叠色）。
2. WebView2 层：`main_window.set_background_color`（builder 后同步 +
   运行时命令），DefaultBackgroundColor 兜底「子窗口已覆盖但新帧未就位」
   的阶段。
3. 前端 `WallpaperProvider` effect：当前壁纸 64×64 canvas 平均色 → 新命令
   `set_window_background_color(r,g,b)`。四张内置壁纸平均色差异大
   （dusk #a3578a / dawn #e3bd84 / aurora #0e3f51 / forest #6d8372），
   静态单色只能匹配一张，故运行时同步。asset 协议图污染 canvas 时
   （SecurityError）保持上次颜色。

## Verification

- [x] `cargo check` / `tsc --noEmit` / vitest 383 干净
- [x] 探针复测（run11~14，修复后同条件 4/4）：最大化/还原全程零黑帧
      （修复前同条件稳态脉冲出现率 1/3，run5/run9 实锤）
- [x] 「点击闪屏」同因性实测（2026-10-27，run18）：最大化后窗口中心连点
      5 次，全程零像素变化、零黑帧——点击闪屏与最大化黑帧**同族**（点击
      强制重合成，暴露最大化大重栅格后仍未完成的瓦片），实验 G 的壁纸色
      掩蔽已将其一并覆盖；浏览器页面 webview 的对应缺口（webview2-com
      直建、DefaultBackgroundColor 默认白底）由 ab5f630 补齐第四层掩蔽
      （创建时 + SyncBackgroundColor 壁纸变更补发）
- [ ] 浏览器会话级人工验收（需连接服务器）：最大化/还原确认页面 webview
      不再闪白、壁纸切换后色值跟随
- [ ] 安装包级人工验收（换四张壁纸各最大化一次）
- [ ] 交互式拖拽缩放无回归（实验 F 的场景：erase 从「不擦」变「壁纸色填充」，
      理论上无擦除闪——填充色与内容近似；需人工目验）

## Technical Notes

1. **混合显卡抓屏**：DXGI Duplication 必须在实际拥有扫描输出的适配器上
   （本机 AMD iGPU）；dxcam 只枚举到 NVIDIA 独显，windows-capture 走
   Graphics Capture API 可绕开但不稳。项目同款 windows crate 直接写
   Duplication 最可控。
2. **裸 cargo 编 release**：必须 `--features tauri/custom-protocol`，否则
   dist 不嵌入、WebView2 加载 devUrl 拒绝连接（tauri CLI 会代加）。
3. **稳态 vs 冷态**：首最大化有一次性 2.3s 冻结（着色器/缓存冷启动），
   探针实验必须先热身一轮再测稳态，否则测到的是冷启动行为。
4. 桌面复制抓的是真实桌面合成——目标窗口被其他窗口遮挡时抓到的是遮挡者
   （run1 教训：Quirel 被 MobaXterm 全屏挡住，测了半天零变化）。
