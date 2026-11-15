# 开发治理体系 v2 — 设计文档

- 日期：2026-11-15
- 状态：设计已获用户批准（2026-11-15 会话逐节确认）；Rust 侧强化项（clippy /
  cargo audit / quirel-rust-idioms）经用户确认折入；回归防线四点经会话提出后按
  既定模式默认折入，待审阅时一并确认
- 分支：`feature/dev-governance`（自 dev `6803fa0` 拉出）
- 前置分析：同日「AGENTS.md 在 AI 辅助开发（vibe coding）下的缺口分析」会话

## 1. 背景

对 AGENTS.md 与仓库工程化现状的分析发现三类结构性缺口：

1. **验证缺口**：现有规则只约束「怎么提交 / 怎么发版」，未约束「怎么证明改对了」。
   仓库有 350+ 测试函数（前端 61 个 vitest 文件、src-tauri 77 个、agent 169+67 个、
   quirel-protocol 37 个），但无 CI、无测试门禁；husky 仅拦敏感日志（R1–R3）与
   提交信息格式。docs/testing/ 已积累 13 份手写黑盒测试报告（round3–9），人工回归
   负担持续膨胀。
2. **安全缺口**：本项目为远程服务器控制客户端（SSH / PAM / 凭据 / SOCKS5），
   AGENTS.md 却无安全条款。2026-11-01 审计曾发现私钥/passphrase 明文写入 client.log
   （e343318，P0）；`docs/data-storage.md` 仍写 servers.bin 凭据「明文 JSON」，与
   `src-tauri/src/credentials.rs` 的 keyring 加密实现不符（文档误导源，AI 高度信任文档）。
3. **产物缺口**：AI 会话产物无入库纪律——`.superpowers/`、`.trae/documents/*.md`、
   `e2e/`（仅剩 msedgedriver 驱动残留，真套件在 `feature/e2e-smoke`）均未跟踪；根目录
   散落 debug-*.md ×3、`vitest_output.txt`、`color-analysis.html`、`test-run-script.sh`、
   `quirel-client-plan.md`、游离的 `test_private_key.rs`。「来历不明改动 → 停止报告」
   条款信噪比持续下降。

次级缺口：红线触发依赖文件路径清单与「必读」自觉；`.trae/rules/项目规范.md`
（1524 行历史方案）与 AGENTS.md 无优先级声明；README.md 为 8 行模板原文；任务分支
rebase 语义、归档分支重名、「破坏性协议变更」判定标准均未定义。

**Rust 工程化核查（2026-11-15）**：三个 crate 均无 `[lints]` 配置、无 rustfmt /
clippy 配置文件、无 clippy 执行脚本；错误处理边界未成文（agent 实际统一 anyhow、
thiserror 仅声明未启用；src-tauri 命令 AppError 为主但有 String 混用，AI 改代码会
随机选风格）；tracing 级别惯例与 tokio 关停模式（孤儿进程为第九轮实测踩坑）未沉淀；
无 RustSec 依赖公告扫描——对远程控制客户端是实打实的风险面。

**回归防线核查（2026-11-15）**：规则未强制「修 bug 必须留复现测试」，回归防护完全
依赖人工多轮黑盒测试；原子性契约测试仅 `WindowManager.session.test.ts` 一个，凭据
加密链路与 store 版本迁移（EXPECTED_STORE_VERSIONS）无契约测试；摘要模板无
「影响面 → 受影响现有测试」映射要求。

## 2. 目标

- AI（与人）在任何会话中：启动即自检、完成必有证据、红线必有机检入口、
  修过的 bug 不再复发。
- AGENTS.md 瘦身为常驻宪法层（≤130 行），操作细节按需加载（skills），
  深度文档按指针读取（docs/）。
- 全部治理资产随仓库版本化；其他 AI 工具（Trae 等）可读取同一套规则。

## 3. 架构选型（三选一，采纳 A）

| 方案 | 结构 | 取舍 |
| --- | --- | --- |
| **A. 三层治理（采纳）** | L1 宪法：AGENTS.md ≤130 行常驻；L2 操作：`.agents/skills/` 触发式技能；L3 背景：docs/ 深度文档 | 核心约束每次必见、细节不被上下文截断、红线可机检、跨工具 |
| B. 单文件加重 | 全部规则继续内联 AGENTS.md（~250 行） | 简单；尾部红线在长会话中最易被截断、无机检 |
| C. 极简 + 全 skills | AGENTS.md ~60 行，细节全进 skills | 权责 / 发版等必须常驻的约束也变成按需加载，有漏看风险 |

选 A 的理由：vibe coding 的两类典型失败——常驻规则太长被截断、操作细节太远被
忽略——分别由 L1 的短与 L2 的触发式加载解决。

## 4. 设计明细

### 4.1 AGENTS.md v2 章节结构

| 节 | 状态 | 内容要点 |
| --- | --- | --- |
| §0 指令源优先级 | 新增 | AGENTS.md > `.agents/skills/` > docs/（与实现不符先修文档）> `.trae/rules/项目规范.md`（标注历史方案）；同时给该文件顶部加历史方案声明 |
| §1 会话启动协议 | 新增 | 任务开始前主动报告 `git status` + 当前分支；确认任务分支与改动归属后才动手 |
| §2 Git 提交规范 | 保留 | 现 §1 原样 |
| §3 分支开发规范 | 保留 + 补 | 补三句：任务分支允许 rebase 同步 dev（禁止改写已推送历史）；拉分支前先同步本地 dev；归档分支名永久占用、新任务禁止重名 |
| §4 版本与发版 | 合并瘦身 | 现 §3/§4/§5 三章合并，细节指向 `scripts/release.mjs` |
| §5 红线总表 | 新增（核心） | 六条红线一张表：红线一句话 / 触碰范围 / 机检或回归命令 / 详情文档 |
| §6 验证规范 | 新增 | 改动面 → 必跑命令矩阵 + 回归防线（修 bug 必留复现测试） + 完成摘要四件套 + 引用已装 superpowers 技能 |
| §7 安全红线 | 新增 | 见 4.4 |
| §8 AI 产物纪律 | 新增 | 见 4.5 |
| §9 磁盘与构建缓存 | 保留 | 现 §5.2 原样 |
| §10 窗口坏帧 | 缩为指针 | 红线表一行 + `docs/window-flash-rules.md` |

现 §5.1（浏览器会话所有权）全文外链到新文件 `docs/browser-session-rules.md`。

**红线总表（六行）**：

| # | 红线 | 触碰范围 | 机检 / 回归 | 详情 |
| --- | --- | --- | --- | --- |
| 1 | 浏览器会话销毁唯一入口（React effect/cleanup 禁止销毁会话） | browser 会话生命周期 / 浮层 | 契约测试 `src/window-system/core/WindowManager.session.test.ts` | docs/browser-session-rules.md |
| 2 | 窗口坏帧防闪三件套（WM_ERASEBKGND 填色 / 子窗口链不擦 / transparent(false)） | lib.rs 窗口段 / fullscreen.rs / 壁纸 / webview bounds / 升级 tauri | dupcap 探针（window-flash-rules §7） | docs/window-flash-rules.md |
| 3 | 版本号唯一入口 release.mjs、三组件 lockstep | 版本文件 / tauri.conf.json | verify 内置 lockstep 一致性检查（package.json 与三个 Cargo.toml 同版本、tauri.conf.json 无 version 字段） | AGENTS.md §4 |
| 4 | 协议破坏性变更须双端同发并升次版本（0.x 阶段） | quirel-protocol | `quirel-protocol/tests/wire_compat.rs` | docs/IPC_PROTOCOL.md + quirel-protocol-change 技能 |
| 5 | 日志 / 测试 fixture 禁含密钥、token、真实路径 | credentials.rs / agent/src/auth/ / path_guard.rs / socks5 | `npm run check:sensitive-logs` | docs/audits/ + quirel-safety 技能 |
| 6 | 构建缓存只经 cargo-sweep，禁手写 rm 与 --time | target/ 维护 | 纪律条款（无自动机检） | AGENTS.md §9 |

**§6 验证规范命令矩阵**：

| 改动面 | 必跑 |
| --- | --- |
| 前端 src/ | `npm run verify:fe` |
| Rust 任一 crate | `npm run verify:rust` |
| 触碰红线 | 红线表对应回归命令 |
| 完成声明 / 请求合并前 | `npm run verify:full` |

**回归防线（防「修 A 坏 B」）**：bug 修复黄金流程 = 先写复现测试（红）→ 根因修复
（绿）→ `verify:full` → 复现测试永久入库；完成摘要的「测试证据」必须含该复现测试，
「影响面」必须列出受影响的现有测试模块。新功能按 TDD（引用
`test-driven-development` 技能）。改动范围不得超过任务所需，禁止顺手重写无关代码
（对准 AI「过度修复」倾向）。Rust / TS 共同手段：状态机用 enum / newtype 表达，
让非法状态不可表示，把编译器当第一道回归防线。

完成摘要四件套：变更摘要（改了什么 / 为什么）/ 影响面（含受影响现有测试）/
测试证据（命令 + 结果，修 bug 时含复现测试）/ 建议 squash 提交信息。

通用纪律显式引用已装用户级技能：`verification-before-completion`（先证据后断言）、
`systematic-debugging`（bug 先根因后修复）、`test-driven-development`、
`requesting-code-review`（任务完成后）。

### 4.2 验证门禁 scripts/verify.mjs

| 命令 | 内容 | 用途 |
| --- | --- | --- |
| `npm run verify`（quick，默认） | `vitest run` + 三 crate `cargo check` + `check:sensitive-logs` + lockstep 一致性检查 | 检查点后快速自证 |
| `npm run verify:fe` | 敏感日志 + vitest run | 纯前端改动 |
| `npm run verify:rust` | src-tauri / agent / quirel-protocol 各自 `cargo test` | Rust 改动 |
| `npm run verify:full` | quick + 全部 cargo test + per-crate `cargo clippy` | 完成声明前必跑 |

- 统一输出 PASS/FAIL 摘要，供「测试证据」直接粘贴。
- 不新增 git hook（AI 不执行 push，pre-push 拦不到 AI；机检点 = 完成声明必须附
  verify 证据）。pre-commit 维持现状。
- quick / full 的具体步骤划分在实施时按实际运行时长微调，原则：quick 控制在
  2 分钟量级（增量编译下）。
- **clippy 严格度实施时评估**：先全量跑一次 clippy 评估存量警告——存量干净则
  `cargo clippy -- -D warnings` 直接进门禁；存量多则先以非阻塞清单输出、分批清理
  后再收紧。
- **契约测试盘点**：实施时评估凭据加密链路（credentials.rs）与 store 版本迁移
  （EXPECTED_STORE_VERSIONS）是否补原子性契约测试，结论写入实施报告。

### 4.3 项目级 skills（`.agents/skills/`，随仓库版本化）

6 个触发式技能（各自 SKILL.md，frontmatter 描述含触发词）：

| 技能 | 触发时机 | 承载 |
| --- | --- | --- |
| quirel-redlines-window | 触碰 lib.rs 窗口段 / fullscreen.rs / 壁纸 / webview bounds / 升级 tauri | 必读 window-flash-rules.md + §7 dupcap 探针回归步骤 |
| quirel-redlines-browser | 触碰浏览器会话 / webview 生命周期 / 浮层 | 会话所有权红线全文 + WindowManager.session.test.ts 回归 |
| quirel-protocol-change | 改 quirel-protocol | 破坏性判定 checklist（删枚举 variant / 删字段 / 类型变更 = 破坏性，须双端同发并升次版本；新增 `#[serde(default)]` 可选字段 = 兼容）+ wire_compat.rs 必更 + 双端同步自查 |
| quirel-verify | 任务完成声明前 | 按改动面选 verify 组合 + 证据模板 + bug 修复黄金流程（复现测试先行） + 按改动文件定位相关现有测试的指引 |
| quirel-safety | 触碰 credentials.rs / agent/src/auth/ / path_guard.rs / socks5 | 安全红线全文 + 审计文档指针 |
| quirel-rust-idioms | 新增 / 修改错误类型、日志语句、async 任务关停路径 | thiserror/anyhow 边界（协议与库层用 thiserror 类型化错误、main/命令层用 anyhow）+ tracing 级别惯例（呼应敏感日志门禁）+ tokio 关停模式（孤儿进程为第九轮实测踩坑）+ 状态机用 enum / newtype 表达（非法状态不可表示） |

发版流程留在 AGENTS.md 宪法层（低频但必须全见，不做 skill）。

选 `.agents/skills/` 而非 `.zcode/skills/` 的原因：`.gitignore:205` 忽略整个
`.zcode/`，`.agents/` 未被忽略、可随仓库版本化，且是跨 AI 工具的通用发现路径。

### 4.4 安全红线章 + 文档修正

- 红线条款：触碰 `credentials.rs` / `agent/src/auth/` / `path_guard.rs` / socks5
  前必读 docs/audits/ 对应审计文档；日志与测试 fixture 禁止出现密钥、token、真实
  路径；凭据加密链路只许加强不许降级；文档与实现不符时先报告再改。
- **依赖安全**：`cargo audit`（RustSec 公告扫描）在依赖升级后与周期性（如月度）
  执行；工具需 `cargo install cargo-audit`；不进 verify 门禁（需联网，不适合本地
  快速门禁）。
- 修正 `docs/data-storage.md`：实施时先核对 credentials.rs 的 keyring 加密现状，
  改正「明文 JSON」过时描述。
- 处理游离的 `test_private_key.rs`：查看内容后移入对应 crate 测试目录或删除
  （执行前向用户确认处置方式）。

### 4.5 AI 产物纪律 + 根目录清整

规则：AI 产物只写白名单路径（`docs/superpowers/specs|plans/`、`docs/testing/`）；
临时实验文件放已忽略目录（`.zcode/` 等）；**禁止 `git add .`**，一律显式指定文件。

清整清单（受限操作，逐项经用户确认后执行）：

| 项 | 处置 |
| --- | --- |
| debug-maximize-black-flash.md / debug-quic-connection-drop.md / debug-settings-not-working.md | 逐个阅读：含根因链且未过时的并入 docs/audits/，否则删除；处置结果向用户报告 |
| vitest_output.txt / color-analysis.html / test-run-script.sh | 删除 |
| quirel-client-plan.md | 历史计划移 docs/superpowers/plans/ |
| e2e/ 驱动残留（.tools/msedgedriver、reports） | 删除；AGENTS.md 注明套件在 feature/e2e-smoke、暂未启用 |
| .trae/documents/filemanager-crud-consistency.md、notification-server-isolation-plan.md | 移 docs/superpowers/plans/ 入库（沿现有计划文档惯例） |
| .gitignore | 增补 `.superpowers/`（.screenshots / .workbuddy / .zcode-visual / .zcode 已被忽略） |
| test_private_key.rs | 见 4.4 |

### 4.6 README.md 重写

三组件简介、开发环境要求（Node / Rust / cargo-sweep / cargo-audit）、常用命令表
（`dev` / `test` / `verify` 系列 / `build:dev` / `release`）、文档索引
（AGENTS.md、window-flash-rules.md、IPC_PROTOCOL.md、audits/）、AI 工具入口
（指向 AGENTS.md）。

## 5. 实施顺序与提交划分（同一 feature/dev-governance 分支）

1. `docs`: AGENTS.md v2 重写（§0–§10，§6 含回归防线，§7 安全红线含 cargo audit 条款）
2. `docs`: 新建 docs/browser-session-rules.md（承接现 §5.1 全文）
3. `build`: scripts/verify.mjs（含 lockstep 检查与 clippy 调用）+ package.json
   四个 verify 命令；跑一次 clippy 评估存量警告并定严格度；盘点原子性契约测试
   缺口（凭据链路 / store 版本迁移）并给出结论
4. `docs`: data-storage.md 修正 + .trae/rules 历史方案声明
5. `chore`: 根目录清整 + .gitignore 增补（逐项确认后执行）
6. `docs`: README 重写
7. `feat`: .agents/skills/ 六技能（含 quirel-rust-idioms）

每步独立规范提交（`type(scope): 中文简述`）；完成后输出变更摘要 + 建议 squash
信息，等待用户决定合并。

## 6. 验收标准

- AGENTS.md ≤130 行，含红线总表六行且每行有机检 / 回归入口；§6 含修 bug 黄金流程。
- `npm run verify` / `verify:fe` / `verify:rust` / `verify:full` 全部可执行并输出
  PASS/FAIL 摘要。
- `.agents/skills/` 六技能可被发现（工作区扫描路径），SKILL.md 含明确触发词；
  quirel-verify 含证据模板与复现测试指引。
- data-storage.md 与 credentials.rs 实现一致。
- 根目录清整清单逐项执行完毕（或有明确保留理由）。
- README 覆盖命令表与文档索引。
- `npm run verify:full` 全绿（含 clippy，严格度按实施时评估结果执行）。
- 契约测试盘点有书面结论（补或不补、理由）。

## 7. 明确不做（Non-goals）

- 不引入 CI 流水线（无托管流水线诉求；verify 脚本日后可直接复用为 CI 步骤）。
- 不新增 git hook（pre-commit 维持现状）。
- e2e 套件正式落地不在本轮（仅删残留 + 注明状态；后续独立任务）。
- 不重写 `.trae/rules` 正文（仅顶部加历史方案声明）。
- 不改动任何版本号文件（与发版规范无关）。
- 不实施 cargo-nextest / cargo-machete / rustfmt 统一格式化 / proptest（协议
  serde roundtrip 模糊测试）/ cargo-mutants（变异测试）——均为可选后续，
  经用户同意再纳入。
