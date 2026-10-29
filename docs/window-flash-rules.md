# 窗口坏帧体系：根因、修复决策与生命周期红线

> 面向所有会触碰**窗口创建 / resize / 壁纸 / 全屏 / webview bounds** 相关代码的人
> 与 AI 会话。帧级实验证据与探针数据见根目录 `debug-maximize-black-flash.md`；
> 本文回答「为什么是现在这套方案」以及「改什么必回归」。
> 定稿提交：`2862b6b`（实验 G，fix/20261026）。

---

## 1. 一句话结论

最大化/缩放时，窗口新增暴露区在 WebView2 新帧栅格化完成前**必然存在坏帧窗口**
（13~55ms，与刷新率无关）；修复不是消除它（做不到），而是**用当前壁纸平均色
填充它**，让坏帧从「闪黑」退化为与壁纸几乎不可分的色差。

## 2. 根因链条（六步）

```
最大化（几何一次性大跳变）
  → ① 窗口背衬面（DWM 重定向位图）随之放大，新增区域无任何像素内容
  → ② 防闪三件套（实验 F）：主窗口拦截 WM_ERASEBKGND「声称已擦不擦」
      + 类背景刷置 NULL → 新暴露区没有任何机制填色
  → ③ 未初始化背衬面 → 显示为纯黑
  → ④ WebView2（windowed 宿主）按新尺寸重栅格化需 13~55ms（Chromium 固有）
  → ⑤ 这段时间内暴露区持续为黑
  → ⑥ DWM 按显示器刷新率逐帧采样：165Hz 横跨 2~5 帧（必见），
      60Hz 横跨 0~1 帧（常被最大化动画掩蔽）
```

**刷新率是放大器，不是故障源。**「调 60Hz 就不闪了」是掩蔽效应，不是修复。

两个关键实测事实（探针 = DXGI Desktop Duplication 帧级抓屏）：

- 坏帧时长 13~55ms 在 60Hz 与 165Hz 下**一致**（同画面同时序）；
- 黑脉冲与最大化动画**无因果**——它是「栅格化 vs 下一个 vsync」的竞速，
  随机出现（同条件有的 run 有、有的没有），`--noanim` 下不消除反而略长。

## 3. 实验演进与每步的原因

| 实验 | 内容 | 结论 / 教训 |
|---|---|---|
| A2/A3/F' | resize 渲染闸门、延迟显示、强制合成器提交 | 治标，进了 beta.2 的 workaround 集合 |
| D | 有边框 + 非透明 | 钉死「透明是 resize 透明闪根源」→ 放弃透明 |
| E | **无边框 + 非透明（定稿形态）** | TopBar 即标题栏；NCHITTEST 子类补 resize 手柄；DWM 系统圆角 |
| F | 拦截 WM_ERASEBKGND「不擦」+ NULL 类刷 + 子窗口链全拦 | 治好拖拽缩放擦除闪；**副作用**：最大化暴露区无人填色 → 黑 |
| **G** | **主窗口暴露区以壁纸平均色填充 + WebView2 DefaultBackgroundColor + 前端动态同步** | 根治最大化黑帧，探针 4/4 零黑帧 |

G 的三层结构（缺一不可）：

1. **主窗口层**（`no_erase_proc` data=1 分支）：WM_ERASEBKGND 时
   `FillRect(WINDOW_BG_COLORREF)`——覆盖「背衬面已放大、子窗口未跟上」阶段。
   FillRect 受 DC 裁剪区约束只画无效像素，稳态零开销。
2. **WebView2 层**（`set_background_color` → DefaultBackgroundColor）：
   覆盖「子窗口已盖住、新帧未栅格化」阶段。
3. **同步链**（WallpaperProvider effect → `set_window_background_color` 命令）：
   四张内置壁纸平均色差异大（dusk #a3578a / dawn #e3bd84 / aurora #0e3f51 /
   forest #6d8372），静态单色只能匹配一张——颜色必须随壁纸运行时同步。
   Rust 侧静态初值 = 默认壁纸 dusk 的平均色，兜住前端就绪前的窗口期。
4. **浏览器页面 webview 层**（2026-10-27 补齐）：内置浏览器的页面 webview 是
   webview2-com 直建的独立 Controller（webview_host.rs create_controller），
   不经 wry——主窗口的 set_background_color 触达不到它，其 DefaultBackgroundColor
   默认白底，最大化/缩放 bounds 变更后新帧栅格化前闪白。修复 = 同一壁纸
   平均色：`apply_default_background`（Controller2::SetDefaultBackgroundColor，
   创建时应用 + HostCommand::SyncBackgroundColor 对存活 webview 补发，
   set_window_background_color 命令联动 proxy::sync_browser_background_colors）。
   **新增 webview 创建路径必须带此调用**；色值与主窗口同源（WINDOW_BG_COLORREF），
   勿另设颜色源。

## 4. 已否定的方案（勿重走）

| 方案 | 否定原因 | 证据 |
|---|---|---|
| 强制/建议用户调低刷新率 | 不可控用户环境；且只是掩蔽 | 用户原始观察 |
| 消除栅格化延迟（等 WebView2 更快） | Chromium 内部行为，不可控；13~55ms 已是稳态极限 | run5~9 |
| 禁用最大化动画（DWMWA_TRANSITIONS_FORCEDISABLED） | 黑脉冲与动画无因果，是竞速随机 | run9 vs run7/8 |
| 改回 tao 的 `set_background_color` 机制 | 会被我们的 no_erase_proc 子类短路（子类在 tao proc 之前返回） | 代码分层事实 |
| 子窗口链也做填色 | WebView2 宿主背景由 Chromium 自管，填充会在内容上叠色（实验 F 场景回归） | 设计约束 |
| 等升级 Tauri 自动修复 | 我们的子类工作在上游处理之下，上游行为变化触达不到暴露区填充 | 同上 |
| noRedirectionBitmap（tauri 2.12 新 API） | 针对透明窗口的重定向位图路径；主窗口已定稿非透明，收益路径不同 | lib.rs 注释保留待评估 |

## 5. 不变量红线（改动即回归）

1. `no_erase_proc` **主窗口分支（data=1）必须以 `WINDOW_BG_COLORREF` 填充
   暴露区**。改回「纯返回 1 不擦」→ 黑帧回归；改回默认擦除 → 白闪回归
   （实验 F 之前的状态）。
2. **子窗口链（data=0）必须维持「不擦」**。给 WebView2 宿主填色会在
   Chromium 内容上叠色。
3. **壁纸 ↔ 窗口背景色同步链不可断**：`wallpaperStore.wallpaperImageUrl`
   覆盖所有壁纸来源 → `WallpaperProvider` effect 64×64 平均色 → invoke
   `set_window_background_color` → `WINDOW_BG_COLORREF` + DefaultBackgroundColor。
   **新增壁纸来源类型时必须保证 `wallpaperImageUrl` 返回正确 URL**，否则新
   壁纸下坏帧退化为旧壁纸色（可见色差闪）。
4. **改默认壁纸必须同步改 Rust 侧静态初值**（`lib.rs` WINDOW_BG_COLORREF
   与 setup 里的 `set_background_color` 初值，当前 = dusk rgb(163,87,138)）。
5. **主窗口必须 `transparent(false)`**。透明窗口的坏帧形态完全不同（整窗
   透明闪，实验 D 的根源），且 transparent 分支里 wry 会把 DefaultBackgroundColor
   置 (0,0,0,0)。
6. no_erase_proc（SUBCLASS_ID 0x5152_4c）与 resize_hit_proc（0x5152_4d）是
   两个独立子类，靠 data 参数区分主/子窗口——**新增窗口类型时确认其在
   EnumChildWindows 链中的擦除语义**。
7. **浏览器页面 webview 的 DefaultBackgroundColor 必须保持壁纸平均色同源**
   （webview_host.rs apply_default_background ← WINDOW_BG_COLORREF）。改回
   默认白底 → 最大化/缩放闪白回归；另设颜色源 → 壁纸切换后与主窗口坏帧
   色差分叉（可见）。
8. **最大化路径对暴露区的任何新处理**（如未来引入窗口动画、自定义 DWM
   属性）必须重跑 §7 探针回归。

## 6. 生命周期触发点（何时必读本文件）

触碰以下任何一处，先读本文 §5 红线，改完跑 §7 回归：

- `src-tauri/src/lib.rs`：窗口创建段（builder 参数）、防闪三件套、
  NCHITTEST/DWM 圆角子类、Resized 事件段
- `src-tauri/src/fullscreen.rs`：全屏切换（最大化/全屏共享暴露区路径）
- 壁纸系统：`wallpaperStore.ts` / `WallpaperContext.tsx` / 新壁纸来源
- browser webview bounds：`webview_host.rs`（set_bounds/region）、
  `browser_window.rs`（sync_bounds 全屏铺满）。**注意**：不要在 runtime 端
  按「客户区−边距」外推直通——浏览器卡片是自有几何的浮动窗口（CSS 过渡
  动画、边距不恒定），直通必然错位（2026-10-27 实测回退，见 PROGRESS §进行中）
- **升级 tauri / wry / tao**（子类共存、ERASEBKGND 语义、DefaultBackgroundColor
  行为都可能变）
- 混合显卡 / 高刷屏环境验收（放大器效应最强处）

## 7. 回归验证方法

**帧级（金标准）**——探针在 `%TEMP%\flicker_probe\dupcap`（独立 Rust 工程，
项目同款 windows crate 0.62；Temp 可能被清理，重建要点见 debug 文档 §Technical
Notes：Duplication 必须指向实际拥有扫描输出的适配器，混合显卡下 dxcam 不可用）：

```bash
# 启动 release 包（必须 --features tauri/custom-protocol，否则嵌不进前端）
src-tauri/target/release/quirel.exe &
# 热身一轮（首最大化含一次性着色器编译，测的是冷态不是稳态）
dupcap.exe <out_dir> "Quirel"          # 弃
# 测量两轮以上（坏帧是竞速随机，单轮通过不算数）
dupcap.exe <out_dir> "Quirel"
# 判定：frames.csv 中 MAXIMIZE/RESTORE 前后窗口 h0（近黑像素）无 >55000 尖峰
```

**人工快速验收**：四张内置壁纸各最大化/还原一次 + 拖拽缩放目验（实验 F 场景，
erase 从「不擦」变「壁纸色填充」，理论上无擦除闪）。dev 模式（vite，无 CSP/无
release 合成路径）**不能替代安装包验收**。

## 8. 关联材料

- `debug-maximize-black-flash.md`：探针数据、逐帧图像分析、踩坑记录
- 提交：`2862b6b`（实验 G 修复）、`f1bb4f5`（无边框定稿 + resize 子类）、
  `6617adc`（渐变 CSP 发黑——同类「坏帧显形」问题的另一分支）
- PROGRESS.md「Agent 必读的技术坑」§7 / AGENTS.md §6 指向本文
