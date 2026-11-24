# AGENTS.md

Quirel — 远程 Linux 服务器控制客户端。三个自研组件统一版本号（lockstep）：

- `src-tauri/`：桌面客户端 `quirel`（Tauri 2 + 前端）
- `agent/`：远程服务端守护进程 `quireld`
- `quirel-protocol/`：双端共享的线上协议 crate（协议单一真相源）

工作流总览：`feature/` / `fix/` 分支开发 → 用户 squash 合并进 dev → 定期 dev 合入 main 发版。

## 0. 指令源优先级

- 本文件 > `.agents/skills/`（触发式操作手册）> `docs/`（事实性文档）> `.trae/rules/项目规范.md`（历史方案存档，冲突一律以本文件为准）。
- docs/ 与实现不符时先报告并修文档，再继续任务；禁止按过时文档开发。
- 红线深度文档：`docs/window-flash-rules.md`（窗口坏帧）、`docs/browser-session-rules.md`（浏览器会话）、`docs/IPC_PROTOCOL.md`（协议）、`docs/audits/`（安全审计）。

## 1. 会话启动协议

- AI 开始任何任务前：报告 `git status` 与当前分支 → 确认位于用户指定的任务分支、改动归属明确 → 才动手。
- 发现不在任务分支上、或存在来历不明的未提交改动：立即停止并报告，不得自行处置。

## 2. Git 提交规范

- 提交信息格式：`type(scope): 中文简述`，正文可选。
- type 仅允许：`feat` `fix` `docs` `refactor` `perf` `test` `build` `chore` `revert` `wip`；`wip` 仅限任务分支检查点，禁止出现在 dev / main。
- scope 用模块名：`ui` `core` `agent` `protocol` `i18n` 等。
- 一次提交只做一件事，禁止把不相关修改混入同一提交。
- 破坏性改动：type 后加 `!`，正文写 `BREAKING CHANGE: 说明`。
- 禁止 "Squashed commit of the following" 之类无信息标题；squash 合并必须重写为一行规范的 `type(scope):` 描述。

## 3. 分支开发规范（权责划分）

- 分支一律从 dev 拉出（拉出前先同步本地 dev）；一分支一任务；命名 `feature/<短名>` 或 `fix/<短名>`；禁止分支嵌套；归档分支名永久占用，新任务禁止重名。
- 任务分支允许 rebase 同步 dev；禁止改写已推送远端的历史。
- squash 合并进 dev 后分支归档为 `archive/<原名>`（受限操作，须先征得用户同意）。归档分支是冻结快照：禁止开发、rebase 或恢复使用；AI 不得将其视为工作分支。
- AI 权限边界：
  - 只在用户指定的分支上工作；未经用户指示，不得创建、切换、删除分支。
  - 任务分支内可自由提交检查点（允许 wip）。
  - 完成任务后输出摘要（§6 四件套），等待用户决定是否合并。
  - 涉及 dev/main 的操作（squash 合并、push、删除/归档分支、发版）执行前必须明确询问用户并取得同意；用户同意后由 AI 执行并回报结果。
  - 发现自己不在任务分支上、或工作区有来历不明的未提交改动：立即停止并报告，不得自行处置。
- main、dev 上禁止直接开发；squash 合并、push、删分支、归档重命名必须先征得用户同意；提交信息由 AI 起草并在征求同意时一并给出，用户可要求修改。
- dev 上只允许三类提交：squash 合并产生的规范提交、发版提交 `chore(release)`、用户亲自完成的微小规范提交；AI 产出一律走分支。

## 4. 版本与发版

- 三组件统一版本号（lockstep）：`quirel`（src-tauri/）、`quireld`（agent/）、`quirel-protocol`（quirel-protocol/）；一致性机检见 §5 红线表第 3 行。
- 版本号唯一修改入口：`npm run release <版本号>`（main 分支操作，须先征得用户同意，AI 可代执行），脚本同步三个 Cargo.toml 与 package.json 并刷新两个 lock 文件；禁止绕过脚本手改版本文件或手动 `git tag`；发版必须由用户明确指示并提供版本号，AI 不得自主发起。
- 发版流程：dev 合入 main → main 上执行 release 脚本（改版本 → 提交 `chore(release)` → tag `vX.Y.Z` 打在该提交上）→ `npm run tauri build`（构建命令零版本参数）。
- 发版收尾：release 脚本打 tag 后自动将 main 合回 dev（fast-forward 更新 dev 引用，不切分支、不产生合并提交；分叉时仅提示手动合并）。
- 版本号：语义化版本，可选预发布后缀（`0.3.0-beta.1`）；feature 升次版本、fix 升修订号、破坏性升次版本（0.x 阶段）。beta 按需发布、序号递增 `beta.N`；两类发版均在 main 上执行，时机由用户决定；已发布的版本号与 tag 永不复用。
- `quirel-protocol` 破坏性协议变更：该版本必须客户端与 quireld 同发，且 0.x 阶段至少升次版本号——保证「同版本号 = 协议兼容」；判定标准见 `quirel-protocol-change` 技能。
- 版本硬编码禁令：`src-tauri/tauri.conf.json` 不设 `version` 字段；客户端 Rust 侧用 `env!("CARGO_PKG_VERSION")`；前端展示用 `getVersion()`；quireld 包名由 `agent/build.sh` 动态读取。
- 开发/测试构建不改任何版本字段、不产生 release 提交与 tag：debug 构建展示版本自动追加 `-dev` 后缀（启动日志 / `--version` / 关于页与 quireld 一致）；非发版安装包用 `npm run build:dev`（按 `git describe` 命名 `<版本>-dev.N-g<sha>[-dirty]`）；开发环境升级以 commit / 产物文件名为准，不做版本号比较。

## 5. 红线总表

| # | 红线 | 触碰范围 | 机检 / 回归 | 详情 |
|---|---|---|---|---|
| 1 | 浏览器会话销毁唯一入口（React effect/cleanup 禁止销毁会话） | browser 会话生命周期 / 浮层 | `src/window-system/core/WindowManager.session.test.ts` | docs/browser-session-rules.md |
| 2 | 窗口坏帧防闪三件套（WM_ERASEBKGND 填色 / 子窗口链不擦 / transparent(false)） | lib.rs 窗口段 / fullscreen.rs / 壁纸 / webview bounds / 升级 tauri | dupcap 探针（window-flash-rules §7） | docs/window-flash-rules.md |
| 3 | 版本号唯一入口 release.mjs、三组件 lockstep | 版本文件 / tauri.conf.json | `npm run verify`（内置 lockstep 检查） | scripts/release.mjs |
| 4 | 协议破坏性变更须双端同发并升次版本（0.x） | quirel-protocol | `quirel-protocol/tests/wire_compat.rs` | docs/IPC_PROTOCOL.md |
| 5 | 日志 / 测试 fixture 禁含密钥、token、真实路径 | credentials.rs / agent auth / path_guard / socks5 | `npm run check:sensitive-logs` | docs/audits/ |
| 6 | 构建缓存只经 cargo-sweep，禁手写 rm 与 --time | target/ 维护 | 纪律条款 | §9 |

触碰红线 1/2/4/5 前必读对应详情文档或 `.agents/skills/` 对应技能，改完必跑对应回归。

## 6. 验证规范

| 改动面 | 必跑 |
|---|---|
| 前端 src/ | `npm run verify:fe` |
| Rust 任一 crate | `npm run verify:rust` |
| 触碰红线 | 红线表对应回归命令 |
| 完成声明 / 请求合并前 | `npm run verify:full` |

- 回归防线：bug 修复 = 先写复现测试（红）→ 根因修复（绿）→ `verify:full` → 复现测试永久入库；新功能按 TDD；改动范围不得超过任务所需，禁止顺手重写无关代码；状态机用 enum / newtype 表达，让非法状态不可表示。
- 完成摘要四件套：变更摘要（改了什么 / 为什么）/ 影响面（含受影响现有测试）/ 测试证据（命令 + 结果，修 bug 时含复现测试）/ 建议 squash 提交信息。
- 通用纪律技能：`verification-before-completion`（先证据后断言）、`systematic-debugging`（bug 先根因）、`test-driven-development`、`requesting-code-review`（任务完成后）。
- e2e：套件在 `feature/e2e-smoke` 分支（WebdriverIO + tauri-driver），暂未并入 dev；当前 `npm test` 不含 e2e。

## 7. 安全红线

- 触碰 `src-tauri/src/credentials.rs`、`agent/src/auth/`、`path_guard.rs`、socks5 相关代码前：必读 `docs/audits/` 对应审计文档与 `quirel-safety` 技能。
- 日志与测试 fixture 禁止出现密钥、token、口令、真实用户路径；新日志语句遵循 `quirel-rust-idioms` 的 tracing 级别惯例。
- 凭据加密链路只许加强不许降级（现状：keyring 主密钥 + AES-GCM 信封加密，见 credentials.rs 模块注释）；文档与实现不符时先报告并修文档。
- 依赖安全：`cargo audit` 在依赖升级后与周期性执行（需 `cargo install cargo-audit`）。

## 8. AI 产物纪律

- AI 产物只写白名单路径：`docs/superpowers/specs|plans/`、`docs/testing/`；临时实验文件放已忽略目录（`.zcode/` 等），禁止留在仓库根目录。
- 禁止 `git add .`：一律显式指定文件路径后再提交。

## 9. 磁盘与构建缓存维护

target 体积无上限增长，清理统一走 cargo-sweep（全局安装，v0.8+），禁止手写 rm 脚本与 `--time` 模式（Windows atime 不可靠）：

- 依赖升级后（Cargo.lock 变更）：crate 目录内 `cargo sweep --stamp` 标定分界 → `cargo check` 重建新依赖产物 → `cargo sweep --file .` 清除分界之前的全部旧产物（顺序不可颠倒——先扫会把要保留的新产物一并清除）；
- 工具链升级后：`cargo sweep --toolchains <旧版本>` 点名清除；
- 纪律：构建/会话运行时不 sweep；共享 target 下一次 sweep 覆盖全部 crate。

## 10. 窗口坏帧红线

最大化/缩放坏帧现方案为「以当前壁纸平均色填充暴露区」（实验 G 定稿）。触碰 §5 红线表第 2 行所列范围，改动前必读 `docs/window-flash-rules.md`，改完按其 §7 用 dupcap 探针回归；硬红线清单见该文档 §5。

## 11. 跨项目发布约定（RELEASE-CONTRACT v1）

本节为本次通过的发布约定，与 §4 并用，不改变 §4 既有条款：

- 版本号与 tag 只能由用户在 main 上通过 `npm run release <版本号>` 产生；AI 与 CI 不得自行决定版本号、创建或删除 tag。
- 发布流水线只消费用户已产生的 tag，不生产 tag；不得由 CI bump 版本、squash 合并、合并 dev→main 或推送 main/dev。
- squash 合并、dev→main、push、生产发布审批仍归人工；生产发布须经人工审批。
- 跨项目制品的发布契约（职责边界、端点/字段/错误码、制品命名与目录、鉴权作用域、时序与回滚）以 `docs/superpowers/plans/2026-11-24-cross-project-release-automation-plan.md` 为唯一权威版本；变更须在该文档升版并同步 quirel_web 副本。
