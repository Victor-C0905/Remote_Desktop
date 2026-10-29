# AGENTS.md

Quirel — 远程 Linux 服务器控制客户端。仓库含三个自研组件，统一版本号（lockstep）：

- `src-tauri/`：桌面客户端 `quirel`（Tauri 2 + 前端）
- `agent/`：远程服务端守护进程 `quireld`
- `quirel-protocol/`：双端共享的线上协议 crate（协议单一真相源）

工作流总览：`feature/` / `fix/` 分支开发 → 用户 squash 合并进 dev → 定期 dev 合入 main 发版。

## 1. Git 提交规范

- 提交信息格式：`type(scope): 中文简述`，正文可选。
- type 仅允许：`feat` `fix` `docs` `refactor` `perf` `test` `build` `chore` `revert` `wip`。
  - `wip` 仅限任务分支上的检查点提交，禁止出现在 dev / main 上。
- scope 用模块名：`ui` `core` `agent` `protocol` `i18n` 等。
- 一次提交只做一件事，禁止把不相关修改混入同一提交。
- 破坏性改动：type 后加 `!`，并在正文写 `BREAKING CHANGE: 说明`。
- 禁止使用 "Squashed commit of the following" 之类无信息标题；
  squash 合并时必须重写为一行规范的 type(scope): 描述。

## 2. 分支开发规范（权责划分）

- 分支一律从 dev 拉出；一分支一任务；命名 `feature/<短名>` 或 `fix/<短名>`；
  禁止分支嵌套。
- 分支 squash 合并进 dev 后不删除，由用户重命名 `archive/<原名>` 归档保留；
  归档分支是冻结快照：禁止在其上继续开发、rebase 或恢复使用，相关任务一律
  从当前 dev 重新拉新分支；AI 不得将归档分支视为工作分支。
- AI 权限边界：
  - 只在用户指定的分支上工作；未经用户指示，不得创建、切换、删除分支。
  - 任务分支内可自由提交检查点（允许 wip），禁止 push 到远程。
  - 完成任务后输出变更摘要（改了什么 / 为什么 / 影响面）并停手；
    禁止自行合并，禁止代写合并提交。
  - 发现自己不在任务分支上、或工作区有来历不明的未提交改动：
    立即停止并报告，不得自行处置。
- main、dev 上禁止直接开发。squash 合并、提交信息撰写、push、删分支
  均由用户执行；AI 可在摘要中给出建议的提交信息供用户采用。
- dev 上只允许三类提交：squash 合并产生的规范提交、发版提交 `chore(release)`、
  用户亲自完成的微小规范提交；AI 产出一律走分支。

## 3. 版本号与发版规范

- 三个组件统一使用同一版本号（lockstep）：
  `quirel`（src-tauri/）、`quireld`（agent/）、`quirel-protocol`（quirel-protocol/）。
- 版本号唯一修改入口：用户在 main 分支执行 `npm run release <版本号>`，
  由脚本同步三个 Cargo.toml 与 package.json，并刷新两个 lock 文件。
- 发版必须由用户明确指示并提供版本号；AI 可代为执行 release 脚本与
  构建命令，但必须经脚本执行，禁止绕过脚本手改版本文件或手动 `git tag`；
  未经用户指示，AI 不得自主发起发版。
- 发版流程：dev 合入 main → main 上执行 release 脚本
  （改版本 → 提交 `chore(release)` → tag `vX.Y.Z` 打在该提交上）
  → `npm run tauri build`（构建命令零版本参数）。
- 发版收尾：release 脚本打 tag 后自动将 main 合回 dev（fast-forward
  更新 dev 引用，不切分支、不产生合并提交；dev 与 main 分叉时脚本不
  做自动处理，仅提示手动合并），让版本提交与 tag 回流；此后 dev 的
  git describe 基点即对齐最近发布，开发构建的 -dev.N 计数以此为准。
- 版本号格式：语义化版本，可选预发布后缀（`0.3.0-beta` / `-beta.1`）。
  升版基准：feature 合入升次版本；fix 合入升修订号；破坏性改动升次版本（0.x 阶段）。
- 发版节奏：beta 测试版按需发布（预发布序号递增 `beta.N`），正式版在累计
  多个特性并验证后发布；两类发版都在 main 上执行，时机由用户决定。
- beta 是发布流水线上的正式候选版（有版本号、tag、安装包），用于按真实
  安装方式验收；beta 发现问题发 `0.3.0-beta.1`，禁止重打同名 tag。
- `quirel-protocol` 发生破坏性协议变更时，该版本必须客户端与 quireld 同发，
  且 0.x 阶段至少升次版本号——保证「同版本号 = 协议兼容」。
- 版本号与 tag 由 release 脚本原子创建，已发布的版本号与 tag 永不复用。

## 4. 版本号硬编码禁令

- `src-tauri/tauri.conf.json` 不设 `version` 字段（Tauri 自动回落读 Cargo.toml）。
- 代码与脚本中禁止硬编码版本号字面量：
  - 客户端 Rust 侧用 `env!("CARGO_PKG_VERSION")` 编译期读取；
  - 前端展示版本用 `@tauri-apps/api/app` 的 `getVersion()` 运行时读取；
  - quireld 包名由 `agent/build.sh` 从 Cargo.toml 动态读取，保持动态读取，勿写死。

## 5. 开发/测试环境构建的版本规则

- 版本号只在 release 时变更。dev 分支部署到开发环境做测试时，
  不修改任何版本字段，不产生 release 提交和 tag。
- 开发构建通过构建信息区分：构建时自动注入 `git describe`
  （分支 + commit + dirty 标记），在 `--version` 与启动日志中展示；
  禁止为了区分构建而手改版本号。
- debug 构建（`cargo tauri dev` / 未开 release 的 cargo 构建）的展示版本
  编译期自动追加 `-dev` 后缀（客户端启动日志 / `--version` / 关于页与
  quireld `--version` 一致生效）；仅展示层注入，不改任何版本文件。
- 非发版安装包用 `npm run build:dev` 构建：脚本按 `git describe`
  计算后缀，经 `tauri build --config` 临时覆盖产物命名为
  `<版本>-dev.N-g<sha>[-dirty]`，不写任何版本文件；HEAD 精确等于
  tag 时不加后缀（等同正式构建）。
- 开发环境升级 quirel / quireld 以 commit / 产物文件名为准，不做版本号比较。

## 6. 窗口坏帧红线（最大化/缩放闪黑根治体系）

最大化/缩放时窗口暴露区在 WebView2 新帧就位前必然存在坏帧（13~55ms，与刷新率
无关），现方案为「以当前壁纸平均色填充暴露区」（实验 G 定稿）。触碰以下任何
一处，改动前必读 `docs/window-flash-rules.md`（根因链、已否定方案、回归方法
均在其中），改完按其 §7 用探针回归：

- `lib.rs` 窗口创建段 / 防闪三件套（no_erase_proc）/ NCHITTEST 子类 / Resized 段；
- `fullscreen.rs` 全屏切换；壁纸系统（wallpaperStore / WallpaperContext）；
- browser webview bounds（webview_host.rs / browser_window.rs）；
- 升级 tauri / wry / tao 版本。

硬红线（细节见上述文档 §5）：主窗口 WM_ERASEBKGND 必须以 `WINDOW_BG_COLORREF`
填色、子窗口链必须维持「不擦」、壁纸变更必须经 `set_window_background_color`
同步双层背景色、主窗口必须 `transparent(false)`。
