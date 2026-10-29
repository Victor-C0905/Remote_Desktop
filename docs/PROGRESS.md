# PROGRESS — 项目进度与交接记录

> 面向 AI/Agent 工具的活文档：每次重要迭代后更新，新条目加在最前。
> 规则见根目录 `AGENTS.md`；图标体系设计详见 `docs/icon-system-proposals.md`。
> 最后更新：2026-10-29（会话：失焦标题栏透明两连修——漏底收敛令牌 + 关闭态降档实色）

---

## fix/20261026（2026-10-29 已 squash 合入 dev）

主窗口无边框定稿（实验 E）及全部配套，已完成并过验证基线
（cargo check / tsc / vitest 383 / build / CSP 哨兵全绿；安装包级验收待用户）：

1. **无边框窗口**：`decorations(false)` + 非透明（实验 D 结论：透明才是 resize
   闪屏根源）；WM_NCHITTEST 子类把边缘 8 逻辑 px（含四角）改判边框命中，
   补回 tao 不再提供的 NC resize 手柄（与 no_erase_proc 子类共存）；
   DWMWA_WINDOW_CORNER_PREFERENCE=ROUND 取 Win11 系统圆角（DWM 阴影随附）。
2. **TopBar 窗口控制菜单**：`window_minimize` / `window_toggle_maximize` /
   `window_close` 三命令；最大化态经 `app-maximize-changed`（Resized 段探测，
   与最小化闸门同路径去重）驱动图标文案切换；全屏项接既有 `fullscreen://changed`
   事件（此前前端无监听）随态切换进入/退出；ToolbarMenu 新增 icon / danger 项
   支持 + trigger 点击不再「原地点不收」；symbolic 新增 5 图标
   （window-minimize/maximize/restore、view-fullscreen/view-restore）。
3. **网页全屏联动（Fullscreen API）**：WebView2 ContainsFullScreenElementChanged
   → EvFullscreen → apply_page_fullscreen **异步派发**主线程 ensure_fullscreen
   （runtime STA 同步等待会经 SetWindowPos 跨线程 SendMessage 互等死锁）→
   ResyncBounds 回流重铺 + 焦点二次归还网页（不归还则 Esc 退不出网页全屏）；
   全屏期前端 bounds 上报屏蔽；set_bounds 应用统一收敛到 sync_bounds 入口
   （防失活重显等路径把 webview 拉回卡片矩形）；set_fullscreen_bounds 清圆角
   region（child HWND 不受 DOM 圆角裁剪，四角会露透明缺口）；关窗路径异步退出全屏。
4. **resize 跟随（直通已回退）**：浏览器 webview 的窗口 resize 跟随唯一真源是
   前端 ResizeObserver 直测上报（不套 rAF，省一帧）。曾实现 runtime 端
   「客户区−边距」外推直通（lib.rs Resized 直通），**实测回退**——浏览器卡片
   是桌面内自有几何的浮动窗口，带 ~400ms CSS 过渡动画（最大化时底边距 ~7px、
   浮动态 ~175px，10-26 日志实锤），「边距恒定」假设不成立：直通把 webview
   跳到错误终态（与卡片可差 ~170px），动画中途记录的边距还会产生负坐标外推。
   webview 的正确行为是跟随卡片动画逐帧走，而非跳到几何终态。
5. **最大化黑帧根治（实验 G 定稿）**：DXGI Duplication 抓屏探针帧级实证
   （工具与全程数据见 `debug-maximize-black-flash.md`）——最大化时窗口背衬面
   新增暴露区在 WebView2 新帧栅格化完成前是坏帧，「不擦+NULL 刷」下呈纯黑；
   坏帧时长 13~55ms 与刷新率无关，刷新率只决定被采成几帧（165Hz 必闪、
   60Hz 被动画掩蔽，这就是「调 60Hz 就好了」的真相）；与最大化动画无因果
   （--noanim 对照）。修复：暴露区以**当前壁纸平均色**填充——`no_erase_proc`
   主窗口分支（data=1 区分）FillRect + WebView2 DefaultBackgroundColor 兜底，
   前端 WallpaperProvider 随壁纸 64×64 canvas 平均色经新命令
   `set_window_background_color` 运行时同步（四张内置壁纸平均色差异大，
   静态单色只匹配一张）。探针复测 4/4 零黑帧。
6. **图标磁贴透明 P0 根治（15fd0c8 → 357f2d9 → 8bbd8a0 三连）**：用户报「classic
   档侧栏图标渲染成 paper 视觉」，前两轮 DOM 层修复（纸形变量补齐、iconTheme
   →DOM 镜像）均无效——CDP（debug 构建注入 --remote-debugging-port）实锤
   **DOM 与像素全面脱节**：getComputedStyle 全 classic（ink 白/stop 饱和），
   画出来的磁贴却透明。真因：每实例内嵌重复 defs → url(#icg-*) 按树序解析到
   文档第一个同名 defs，其宿主恰为最小化「文件」窗口（_windowShell_
   display:none），Chromium 无法从隐藏子树绘制 paint server → fill 等效 none，
   磁贴露出底下背景色被误读为 paper；字面 fill=#ff0000 立即上色、改指 body 直挂
   sprite 立即变蓝，两点定案。修复：普通实例 defs 上移 #icon-defs-sprite
   （body 直挂常驻，全文档唯一非后缀 icg-* defs），预览卡 forced 后缀机制不变；
   测试补三条防线（sprite 在位/普通实例无内嵌 defs/引用按档可达）。

7. **失焦标题栏透明两连修（2314798 → 890d626）**：多窗口并开时，失焦窗口
   标题栏直接透出壁纸（活动窗口正常）。CDP（debug 构建 9222 口，live 取证：
   `Page.captureScreenshot` + `getComputedStyle`）一次定位——
   `.windowShellNotActive .windowHeaderBar` 写死 `rgba(headerbar-rgb, 0.55)`，
   该形态依赖毛玻璃 blur 兜底；`data-frosted=off`（默认）时 blur=none，
   半透明底色即「漏底」而非「薄玻璃」。修复分两步：
   - ① 底色收敛为令牌 `--quirel-headerbar-inactive-bg`（:root 半透明 0.55
     供 frosted=on；关闭态经 `html:not([data-frosted="on"])` 覆盖为不透明），
     组件不再写散值；
   - ② 关闭态覆盖改为 `color-mix(in srgb, headerbar-bg 88%, window-bg)`
     不透明降档实色——焦点层级与开启态「厚玻璃→薄玻璃」同构（GNOME
     Adwaita 关透明同款思路），混色朝 window-bg 退一档，亮色主题变浅、
     暗色主题变沉，方向随主题明暗自动正确。
   红线沉淀：**任何半透明 UI 形态必须声明其 blur 兜底前提**；毛玻璃开关
   覆盖块（variables.css）是全站半透明形态的唯一收敛点，消费方只引令牌。
   失焦窗口其余焦点表达（标题退次级色、阴影降档）不变。

---

## 当前状态速览（2026-10-26）

| 项 | 状态 |
|---|---|
| 最新发布版 | **v0.3.0-beta.2**（tag 在 main `514c2f9`），安装包 `src-tauri/target/release/bundle/nsis/Quirel_0.3.0-beta.2_x64-setup.exe` |
| main | `514c2f9` chore(release): 0.3.0-beta.2 —— 含 0.3.0-beta.1 之后全部 67 项迭代 |
| dev | `8f45ae0` —— 领先 main 1 个提交（终端错误提示 CSP 修复，未进包） |
| push | **未推送**（main/dev/tag 均只在本地，由用户执行） |
| 待归档分支 | feature/new_ui、fix/tauri-plugin-version-alignment、fix/build-info-tags、fix/cli-version-flag、fix/gradient-stop-csp、fix/terminal-init-failed-style（均已 squash 合入，待用户改名 archive/） |
| 测试 | `vitest run` 383 用例全过；`tsc --noEmit` 干净 |

## 本轮（0.3.0-beta.1 → beta.2）完成内容

### 功能与修复（全部已在 beta.2 包内）

1. **桌面 UI 体系翻新**（65 项，squash 提交 `fd2bcd2`）：设计令牌/排版/CJK 字体栈、壳层与窗口自适应、
   resize 闪屏 workaround、Tauri 2.12.5、内置四主题壁纸、凭据 keyring 信封加密体系、
   apply_diff 双端协同修复、本地数据清除体系。详单见 `git log v0.3.0-beta.1..v0.3.0-beta.2`。
2. **图标三层体系双档落地**（方案 C+B，见 `docs/icon-system-proposals.md`）：
   - `AppIcon`（24 视图内容图标）：按 `settingsStore.iconTheme` 双档构建（`makeIcons` 缓存，
     组件新增可选 `iconTheme` 覆盖参数）。classic = 渐变磁贴+白图形+双高光弧；
     paper「纸墨」= 平涂淡彩磁贴+本色实边+深墨图形，文件类型为着色纸形（无磁贴），
     文件夹为双色调全出血轮廓。
   - `AppBadge`（48 视图桌面徽章）：classic 底色从 10 个硬编码 hex 改为 `--ic-*` 令牌
     （双高光弧光影层保留，是 classic 档签名细节，不得移除）。
   - `SymbolicIcon`（16 视图单色）：`bell-outline` 改真描边形态（TopBar 无未读态区分）。
   - 色相再平衡去蓝过载：music/file-audio→紫、videos/file-video→橙、documents→灰。
   - `FILE_ICONS` 补全约 20 个扩展名映射（.pdf/文档/压缩/音视频/镜像等）。
3. **关键 bug 修复**（每个都有浏览器/构建级实证，勿回退）：
   - `icg-folder`/`icg-paper` 渐变死引用（`AppIcon.tsx` DEFS 曾未定义 → fill:none，
     文件夹只剩页签、纸身透明，此前三次"隐形"修复无效的根因）。
   - `hex` 图标路径坐标损坏重绘；`variables.css` paper 组重复 folder 行清理。
   - **CSP 拦截渐变发黑**（beta.2 首发实锤）：Tauri release 向 `style-src` 追加注入样式
     nonce/hash → 按 CSP 规范 `'unsafe-inline'` 失效 → innerHTML 动态创建的
     `style="stop-color:var(…)"` 被拦 → stop-color 回落黑色。修复：`G()` 改用
     `stop-color="var(…)"` presentation attribute（不受 style-src 管）。已用模拟 Tauri
     CSP 的对照实验验证（style 写法 rgb(0,0,0)，属性写法正常琥珀）。
4. **开发体验**：`build.rs`（客户端+quireld）describe 加 `--tags`（轻量 tag 参与基点匹配）；
   客户端支持 `--version`/`-V`（AttachConsole 后输出版本与构建信息，实测
   `quirel 0.3.0-beta.2 (main@v0.3.0-beta.2)`）。
5. **发版过程波折**（已解决）：首发构建被 Tauri 插件版本对齐检查拦下（npm 侧
   plugin-fs/opener 落后 Rust 侧两个 minor）→ `fix/tauri-plugin-version-alignment`
   钉死 npm 精确版本 2.7.1/2.7.2。beta.2 tag 曾因该失败本地撤回重取一次
   （tag 从未推送，不复用已发布版本号的规则不受影响）。

### 已合入 dev、尚未进包（下次发版自动带入）

- `8f45ae0` 终端初始化失败提示 style→`.terminal-init-failed` 样式类（同类 CSP 问题，
  beta.2 包内该提示仍会退化无样式，罕见错误路径仅外观）。

## 遗留事项 / 待办

- [ ] **用户**：push（`git push origin main dev v0.3.0-beta.2`，gitee 远端按需）+ 6 个分支改名归档。
- [ ] **用户**：安装新 beta.2 包，验收设置 → 图标主题 classic 卡观感（琥珀文件夹/彩色磁贴/白纸身）。
- [x] ~~色相再平衡第二阶段~~（2026-10-29 已落地：database→teal 新色组、
      search/about 归灰、spreadsheet green；与 §3 的其余差集此前已合入）。
- [x] ~~`AppIcon` DEFS 提为全局单例~~（2026-10-28 已落地：#icon-defs-sprite，
      顺带根治磁贴透明 P0，见「进行中」第 6 条）。
- [x] ~~新增 spreadsheet 图标~~（2026-10-29 已落地：green 网格纸形，.xlsx/.csv 已改映射）。
- [ ] `tauri.conf.json` 的 bundle identifier `com.quirel.app` 以 `.app` 结尾，构建有警告（macOS 语义冲突）。
- [x] ~~Rust 侧 9 个编译警告~~（2026-10-29 已清零：3 处机械修正（BOOL 显式
      忽略 ×2、函数指针 `as *const ()` ×1）；6 处浏览器 tab/诊断预留代码按
      reserved-features 惯例 `#[allow(dead_code)]` + 注释标注，未删除）。

## Agent 必读的技术坑（本会话实证）

1. **CSP 与动态内联样式**：release 经 `tauri://` 伺服且 CSP 被 Tauri 改写（注入 nonce/hash），
   一切运行时 `innerHTML` 里的 `style="…"` 都会被拦（拦后回落 CSS 初始值，如 stop-color=黑）。
   SVG 一律用 presentation attribute（`fill=`/`stroke=`/`stop-color="var(--x)"`）；
   React 的 style prop 走 CSSOM 不受影响。dev（vite，无 CSP 头）不复现——**不要以 dev 观感替代安装包验收**。
2. **SVG 属性不支持 `color-mix()`**（解析失败按黑处理）；需要混色时在 JS 里算好 hex
   （参考可视化稿的做法）或走外部样式表。
3. **轻量 tag 与 describe**：release 脚本打的是轻量 tag，`git describe` 必须带 `--tags`
   （两个 build.rs 已加）；tag 只打在 main。
4. **url(#id) 引用必须有定义，且宿主必须常驻渲染**：引用不存在 id 按 fill:none
   渲染（透明非黑）；解析目标是**树序第一个同名 id**——宿主处于 display:none
   子树（如最小化合成窗口）时 Chromium 取不出 paint server，全文档同名引用
   一起透明。非后缀 `icg-*` defs 现全局仅 `#icon-defs-sprite` 一处（body 直挂
   常驻），图标组件勿再内嵌 defs。
5. **版本号**：唯一入口 `npm run release <版本号>`（仅 main，工作区须干净）；代码内一律
   `env!("CARGO_PKG_VERSION")` / 前端 `getVersion()`；dev 构建靠 QUIREL_BUILD_INFO
   （分支@describe）区分，不动版本号。
6. **分支纪律**：AI 产出一律走 `feature/|fix/` 分支 → squash 进 dev（信息须规范重写）→
   发版时 dev 合 main；push/归档改名由用户执行。
7. **窗口坏帧体系**：最大化/缩放坏帧的根因链、实验演进、已否定方案与代码红线
   见 `docs/window-flash-rules.md`（AGENTS.md §6 同步索引）——改窗口/壁纸/全屏
   相关代码或升级 tauri/wry/tao 前必读，改完用探针回归（方法在文档 §7）。

## 验证基线（改动图标/前端后建议过一遍）

```bash
npx tsc --noEmit && npx vitest run        # 类型 + 399 用例
npm run build                              # 前端产物
grep -c 'style="stop-color' dist/assets/*.js   # 应为 0（CSP 回归哨兵）
npm run tauri build                        # 安装包（仅发版时）
```
