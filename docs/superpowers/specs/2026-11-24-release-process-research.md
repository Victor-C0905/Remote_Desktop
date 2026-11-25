# Quirel 发布流程与版本号机制研究报告

> 日期：2026-11-24
> 性质：只读研究。未修改代码、未提交、未打 tag、未发布任何产物。
> 研究时工作区：`feature/fm-sidebar-tree`（HEAD `e140783`），基于 `dev`（`b96ff66`）。
> 事实来源：当前工作区文件、`main` 分支文件、git 历史与 tag。凡属框架默认行为或推断，均在文中标注「推断」。

---

## 0. 结论摘要

1. 三组件（`quirel` 客户端、`quireld` 服务端、`quirel-protocol` 协议 crate）采用 **lockstep 统一版本号**，当前为 `0.3.3`，处于 0.x 阶段。
2. 版本号的唯一写入口是 `npm run release <版本号>`（[release.mjs](file:///e:/MyWork/Quirel/scripts/release.mjs)），它只负责「校验 → 改四个版本文件 → 刷新 lock → 提交 `chore(release)` → 打轻量 tag `vX.Y.Z` → 尝试把 main 快进回 dev」，**不构建、不推送、不发布产物**。
3. 正式发布是「人工编排的本地流程」：用户在 main 上执行 release 脚本，再执行 `npm run tauri build` 构建客户端 NSIS 安装包；quireld 另在 WSL 内用 `agent/build.sh` 构建双架构 tar.gz，再用 `publish:quireld` 生成官网静态目录并人工上传。仓库内没有 CI。
4. 客户端没有 Tauri updater / 自动更新配置；quireld 有独立的「官网 `latest.json` 优先 + GitHub Releases 兜底」更新通道，且客户端自动安装按客户端版本精确钉扎。
5. 存在两个需要优先处理的实际问题：
   - 本仓库有一个 **不在 main、也没有版本提升的 `v0.3.4` 注解 tag**（指向 `feature/ui-audit-fixes` 的 `c0477fb`），与 AGENTS.md「tag 只打在 main」的规则冲突。
   - 当前 dev 系的 `publish-quireld.mjs` 版本号校验正则丢失了反斜杠（`d+` 而非 `\d+`），按代码会直接中止；修复提交只存在于未合并的 `fix/publish-semver-regex` 分支。且该脚本在 `main` 上根本不存在。

---

## 1. 研究范围与仓库状态快照

### 1.1 组件与版本文件

| 组件 | 目录 | 版本字段位置 | 当前值 |
|---|---|---|---|
| 客户端 `quirel` | `src-tauri/`（前端 `src/`） | [src-tauri/Cargo.toml](file:///e:/MyWork/Quirel/src-tauri/Cargo.toml#L1-L4) | `0.3.3` |
| 服务端 `quireld` | `agent/` | [agent/Cargo.toml](file:///e:/MyWork/Quirel/agent/Cargo.toml#L1-L4) | `0.3.3` |
| 协议 crate | `quirel-protocol/` | [quirel-protocol/Cargo.toml](file:///e:/MyWork/Quirel/quirel-protocol/Cargo.toml#L1-L4) | `0.3.3` |
| npm 根包 | 仓库根 | [package.json](file:///e:/MyWork/Quirel/package.json#L1-L5) | `0.3.3` |

`AGENTS.md` §4 明确 lockstep 与唯一入口（[AGENTS.md](file:///e:/MyWork/Quirel/AGENTS.md#L45-L54)）：

- 三组件统一版本号，一致性机检见 §5 红线表第 3 行（[AGENTS.md](file:///e:/MyWork/Quirel/AGENTS.md#L62)）。
- 版本号唯一修改入口：`npm run release <版本号>`；禁止绕过脚本手改版本文件或手动 `git tag`。
- `src-tauri/tauri.conf.json` 不设 `version` 字段；客户端 Rust 侧用 `env!("CARGO_PKG_VERSION")`；前端展示用 `getVersion()`（实际实现见 §4.4）。

### 1.2 git 状态（研究时）

| 引用 | 提交 | 版本 | 说明 |
|---|---|---|---|
| `main` | `baf01e5` | `0.3.3` | 最后一次由 release 脚本产生的发版提交，带轻量 tag `v0.3.3` |
| `dev` | `b96ff66` | `0.3.3` | 本地领先 `origin/dev` 1 个提交；`origin/dev` 为 `0f4373f` |
| 当前分支 | `e140783` | `0.3.3` | `feature/fm-sidebar-tree`，基于 dev |
| `v0.3.4` tag | `c0477fb` | 文件仍为 `0.3.3` | 注解 tag，tagger `vic`，仅 `feature/ui-audit-fixes` 包含；未在 main/dev |

补充事实：

- `v0.3.3` 是轻量 tag（`git cat-file -t v0.3.3` → `commit`），符合 [release.mjs](file:///e:/MyWork/Quirel/scripts/release.mjs#L145-L147) 的 `git tag v...` 行为。
- `v0.3.4` 是注解 tag（`git cat-file -t v0.3.4` → `tag`），tag 消息为 `chore(release): 0.3.4`，但目标提交 `c0477fb` 的 `package.json` / `src-tauri/Cargo.toml` 仍是 `0.3.3`，也没有 `chore(release): 0.3.4` 提交。**它不是 release 脚本产物。**
- 尝试执行 `git ls-remote --tags origin v0.3.4` 时网络连接被重置，**无法确认该 tag 是否已推到远端**。

---

## 2. 一次正式发布的端到端流程

### 2.1 触发者与阶段划分

按 [AGENTS.md](file:///e:/MyWork/Quirel/AGENTS.md#L45-L54) 与脚本实际行为，正式发布分为四个阶段：

| 阶段 | 谁触发 | 做什么 | 产物 |
|---|---|---|---|
| A. 合流 | 用户 | `dev` 合入 `main`；确认 main 工作区干净 | main 上的待发版提交 |
| B. 冻结版本 | 用户明确指示并提供版本号；AI 可代跑脚本 | `npm run release <版本号>` | 版本提交 `chore(release): X`、轻量 tag `vX`、dev 快进 |
| C. 构建 | 用户/AI | 客户端 `npm run tauri build`；quireld 在 WSL 内 `bash agent/build.sh release <arch>` | NSIS 安装包、双架构 tar.gz、`SHA256SUMS` |
| D. 分发 | 用户 | 上传官网静态目录；可选上传 GitHub Releases；推送 main/dev/tag | 官网文件、GitHub 资产、远端引用 |

脚本明确不推送（[release.mjs](file:///e:/MyWork/Quirel/scripts/release.mjs#L166-L167)），因此「到哪一步结束」在自动化层面是 **B 阶段结束**；C/D 是人工编排。

### 2.2 客户端（quirel）发布

1. 在 main、工作区干净的前提下执行：

   ```powershell
   npm run release 0.4.0
   npm run tauri build
   ```

2. `npm run tauri build` 的版本参数为零。`tauri.conf.json` 没有 `version` 字段（[tauri.conf.json](file:///e:/MyWork/Quirel/src-tauri/tauri.conf.json#L1-L39)），Tauri 2 缺省使用 `src-tauri/Cargo.toml` 的 package version（框架默认行为，推断）。构建前会执行 `beforeBuildCommand: "npm run build"`，即 `tsc && vite build`（[tauri.conf.json](file:///e:/MyWork/Quirel/src-tauri/tauri.conf.json#L8-L8)）。
3. 打包目标只有 `nsis`（[tauri.conf.json](file:///e:/MyWork/Quirel/src-tauri/tauri.conf.json#L24-L24)），产物默认位于 `src-tauri/target/release/bundle/nsis/Quirel_<版本>_x64-setup.exe`（路径由 Tauri 约定得出；仓库文档中的历史实例见 [PROGRESS.md](file:///e:/MyWork/Quirel/docs/PROGRESS.md#L323) 与 [client-test-report-round4](file:///e:/MyWork/Quirel/docs/testing/client-test-report-round4-2026-11-11.md#L111)）。
4. 仓库内**没有客户端安装包的上传/分发脚本，也没有 CI 工作流**（根目录无 `.github/`；`scripts/` 仅有 `build-dev.mjs`、`catchup-dev.mjs`、`check-sensitive-logs.mjs`、`publish-quireld.mjs`、`release.mjs`、`verify.mjs` 与 `start-dev` 启动脚本）。客户端安装包如何交付给用户，仓库没有定义（事实上的缺口，不是推断）。
5. 未配置代码签名：`tauri.conf.json` 的 `bundle` 段没有 `windows` 签名配置，仓库中也未找到 `certificateThumbprint` / `signtool` / 证书相关配置。因此 NSIS 安装包默认未签名（Tauri 缺省行为，推断），会触发 SmartScreen 提示。
6. 未配置 Tauri updater：`plugins` 为空（[tauri.conf.json](file:///e:/MyWork/Quirel/src-tauri/tauri.conf.json#L38)），`package.json` 与 `src-tauri/Cargo.toml` 均无 `plugin-updater` / `tauri-plugin-updater` 依赖。**客户端没有内置自动更新通道，升级依赖重新下载并安装 NSIS 包**（基于「未找到实现」的事实判断）。

### 2.3 quireld 发布

quireld 的发布在客户端版本锁定之后执行，原因是客户端首装按自身版本精确取包，版本目录不存在会 404。

1. 在 WSL/Linux 内构建双架构产物：

   ```bash
   bash agent/build.sh release amd64
   bash agent/build.sh release arm64
   ```

   `build.sh` 从 `agent/Cargo.toml` 动态读取版本（[build.sh](file:///e:/MyWork/Quirel/agent/build.sh#L158-L158)），产物命名 `quireld-{version}-release-linux-{amd64|arm64}.tar.gz`（[build.sh](file:///e:/MyWork/Quirel/agent/build.sh#L238-L274)），并在每次构建后重建 `dist/SHA256SUMS`（[build.sh](file:///e:/MyWork/Quirel/agent/build.sh#L284-L298)）。

2. 生成官网静态目录：

   ```bash
   npm run publish:quireld
   # 可选：同时上传 GitHub Releases 兜底
   npm run publish:quireld -- --github
   ```

   `package.json` 中的入口是 [package.json](file:///e:/MyWork/Quirel/package.json#L16)，脚本为 [publish-quireld.mjs](file:///e:/MyWork/Quirel/scripts/publish-quireld.mjs)。它：

   - 从 `agent/Cargo.toml` 读版本（L82-L84）；
   - 校验本地 tag `v{version}` 存在（L94-L110）；
   - 校验 `agent/dist/` 下双架构 tar.gz 存在（L107-L113）；
   - 生成稳定别名 `quireld-linux-{arch}.tar.gz` 与 `SHA256SUMS`（L115-L131）；
   - 生成 `agent/dist/publish/`：`latest.json`、`install-quireld.sh`、根级稳定别名、版本目录 `{version}/`（L140-L185）；
   - `--github` 时通过 `gh` 创建或更新 GitHub Release（L190-L233）；
   - 打印人工上传顺序：版本目录 → 根文件 → **`latest.json` 最后**（L244-L258）。

3. 人工上传到官网服务器（`quirel_web` 的 `data/downloads/quireld/`），顺序敏感，具体命令见 [DEPLOYMENT.md](file:///e:/MyWork/Quirel/docs/DEPLOYMENT.md#L29-L53) 与脚本 L244-L258。两个域名 `quireld.cn` / `quireld.icu` 共享同一份文件。

4. 校验：两个域名都必须能取到 `{version}/SHA256SUMS` 与 `latest.json`（脚本 L254-L258）。

### 2.4 发布记录

| 记录形式 | 是否存在 | 说明 |
|---|---|---|
| `chore(release): X` 提交 | 是 | 由 release 脚本产生，包含 4 个版本文件 + 3 个 lock 文件；历史实例见 `baf01e5` |
| 轻量 tag `vX.Y.Z` | 是 | 脚本产物无附注，发布说明为空；`v0.3.4` 是唯一不符合脚本约定的注解 tag |
| CHANGELOG | **否** | 仓库无任何 `CHANGELOG*` 文件，git 历史中也未出现过 |
| GitHub Release（客户端） | **否** | 仓库内无客户端发布工作流/脚本 |
| GitHub Release（quireld） | 可选 | 仅 `publish:quireld -- --github` 时创建，标题 `Quireld {VERSION}`，notes 为固定通用文案（L226-L230） |
| 人工进度记录 | 有但过期 | [PROGRESS.md](file:///e:/MyWork/Quirel/docs/PROGRESS.md#L323) 仍写「最新发布版 v0.3.0-beta.2」，与当前 `v0.3.3` 不符 |

---

## 3. `npm run release <版本号>` 逐段剖析

### 3.1 入口与前置校验

文件：[scripts/release.mjs](file:///e:/MyWork/Quirel/scripts/release.mjs)；入口：[package.json](file:///e:/MyWork/Quirel/package.json#L15)。

| 步骤 | 代码位置 | 行为 | 失败后果 |
|---|---|---|---|
| 参数解析 | L54-L56 | 必须是合法 semver（允许可选 `v` 前缀与预发布后缀） | 打印用法并退出 1 |
| 分支校验 | L58-L65 | 必须 `git branch --show-current === "main"` | 中止 |
| 工作区校验 | L67-L73 | `git status --porcelain` 必须为空 | 中止 |
| 当前版本读取 | L75-L77 | 只从 `src-tauri/Cargo.toml` 读取 `version` | 中止 |
| 单调性校验 | L78-L79 | 新版本按 semver 排序必须 **大于** 当前版本 | 中止 |
| 预检报告 | L81-L99 | 打印分支、版本变化、上个 tag、自上个 tag 以来的提交 | 仅展示 |

注意：脚本**不校验增量类型**（feature 必须升 minor、fix 必须升 patch），只校验「更大」。因此 `0.3.3 → 1.0.0`、`0.3.3 → 0.3.4`、`0.3.3 → 0.4.0` 都会通过版本校验；其中 `0.3.4` 因已存在同名 tag，会在随后 `git tag` 阶段失败并留下未打 tag 的版本提交。

### 3.2 修改的文件（lockstep 的实际保证方式）

写入阶段（L101-L118）固定修改四个文件：

1. `src-tauri/Cargo.toml` 的 package `version`
2. `agent/Cargo.toml` 的 package `version`
3. `quirel-protocol/Cargo.toml` 的 package `version`
4. `package.json` 的 `version`

**不修改** `src-tauri/tauri.conf.json`，因为该文件被规定不允许有 `version` 字段；`npm run verify` 的 lockstep 检查会显式拒绝该字段（[verify.mjs](file:///e:/MyWork/Quirel/scripts/verify.mjs#L75-L97)）。

lock 文件刷新（L120-L141）：

- `npm install --package-lock-only` → 更新 `package-lock.json`
- `cargo update -p quirel -p quirel-protocol`（cwd `src-tauri`）→ 更新 `src-tauri/Cargo.lock`
- `cargo update -p quireld -p quirel-protocol`（cwd `agent`）→ 更新 `agent/Cargo.lock`

因此 lockstep 是「脚本统一写入 + verify 机检」双保险；脚本自身不读取 `package.json` / `agent/Cargo.toml` 的旧值，也不检查它们是否已经漂移，而是直接覆盖。

### 3.3 提交与打 tag

L143-L150：

```text
git add -A
git commit -m "chore(release): <版本>"
git tag v<版本>
```

- `git add -A` 比 AGENTS.md §8「禁止 `git add .`，一律显式指定文件」更宽（[AGENTS.md](file:///e:/MyWork/Quirel/AGENTS.md#L92-L93)）；由于工作区必须干净，实际只会纳入版本文件与 lock 文件的改动，但这仍是与仓库纪律不完全一致的实现。
- tag 在 commit 之后创建，二者不是原子操作。若 `git tag` 因 tag 已存在而失败，脚本会中止，但版本提交已经产生，留下「有 chore(release) 提交、无对应新 tag」的中间状态。
- 提交会经过 husky `commit-msg`（commitlint）与 `pre-commit`（`npm run check:sensitive-logs`）钩子（[.husky/commit-msg](file:///e:/MyWork/Quirel/.husky/commit-msg)、[.husky/pre-commit](file:///e:/MyWork/Quirel/.husky/pre-commit)）。`chore(release): X` 符合 conventional 规则。

### 3.4 发版收尾

L152-L161：

```text
git fetch . main:dev
```

- 当 dev 是 main 的祖先时，直接把本地 dev 快进到 main（不切分支、不产生合并提交）。
- 当 dev 与 main 分叉时，`fetch` 失败，脚本只打印警告并提示人工 `git checkout dev && git merge main`，**不回滚已完成的提交与 tag**。
- 这是「release 脚本打 tag 后自动将 main 合回 dev」的实现（AGENTS.md L50）。

### 3.5 脚本明确不做的事

- 不执行 `git push`（L166-L167）。
- 不执行构建（L166 仅提示下一步 `npm run tauri build`）。
- 不执行 `verify` / 测试 / clippy。
- 不生成 CHANGELOG / release notes。
- 不检查 tag 是否已存在、是否已推送、当前 main 是否落后于远端。
- 不发布 quireld 产物（由 `publish:quireld` 另行负责）。

---

## 4. 版本号如何确定

### 4.1 当前阶段

当前为 **0.x 预 1.0 阶段**，main 上最新正式版本为 `0.3.3`。仓库没有进入 1.x 的迹象，也没有 MAJOR 版本历史；另有一个未落在 main 的异常 tag `v0.3.4`（见 §8.1）。

### 4.2 规则（仓库明文）

[AGENTS.md](file:///e:/MyWork/Quirel/AGENTS.md#L51) 规定：

- 语义化版本，可选预发布后缀（示例 `0.3.0-beta.1`）。
- feature 升次版本、fix 升修订号、破坏性升次版本（0.x 阶段）。
- beta 按需发布，序号递增 `beta.N`。
- 两类发版都在 main 上执行，时机由用户决定。
- 已发布的版本号与 tag 永不复用。

协议破坏性变更另有硬约束（[AGENTS.md](file:///e:/MyWork/Quirel/AGENTS.md#L52)）：`quirel-protocol` 破坏性变更必须客户端与 quireld 同发，且 0.x 阶段至少升次版本号，保证「同版本号 = 协议兼容」；回归测试为 [wire_compat.rs](file:///e:/MyWork/Quirel/quirel-protocol/tests/wire_compat.rs#L1-L12)。

### 4.3 历史递增轨迹

按 tag 创建时间排序（`git for-each-ref --sort=creatordate`）：

| tag | 日期 | 类型 | 相对前一稳定版的递增 |
|---|---|---|---|
| `0.1.2` | 2026-09-15 | 注解（手工） | — |
| `0.1.3-bate` | 2026-09-15 | 注解（手工） | 拼写错误，非规范 tag |
| `0.1.3` | 2026-09-16 | 轻量（手工） | patch |
| `0.2.0-beta` | 2026-10-15 | 注解（手工） | 预发布 |
| `0.2.1-beta` | 2026-10-20 | 注解（手工） | 预发布 |
| `0.3.0-beta` | 2026-10-24 | 注解（手工） | 预发布 |
| `v0.3.0-beta.1` | 2026-10-24 | 轻量 | 预发布，开始使用脚本 |
| `v0.3.0-beta.2` | 2026-10-26 | 轻量 | 预发布序号 +1 |
| `v0.3.1-beta.1` | 2026-10-29 | 轻量 | 预发布 |
| `v0.3.1-beta.2` | 2026-10-29 | 轻量 | 预发布序号 +1 |
| `v0.3.2-beta.1` | 2026-11-05 | 轻量 | 预发布 |
| `v0.3.2-beta.2` | 2026-11-06 | 轻量 | 预发布序号 +1 |
| `v0.3.2-beta.3` | 2026-11-09 | 轻量 | 预发布序号 +1 |
| `v0.3.2` | 2026-11-11 | 轻量 | 稳定版 |
| `v0.3.3` | 2026-11-11 | 轻量 | patch（同日发布） |
| `v0.3.4` | 2026-11-23 | 注解（手工，异常） | **无对应版本提交，文件仍为 0.3.3** |

从 0.3.0-beta.1 起，tag 命名与轻量 tag 形式与脚本一致；在此之前是手工 tag（以注解 tag 为主，`0.1.3` 为轻量），且存在缺少 `v` 前缀、`-bate` 拼写错误、`-beta` 与 `-beta.N` 混用等历史不一致。

### 4.4 版本号的运行时来源

| 位置 | 实现 | 证据 |
|---|---|---|
| 客户端 Rust | `env!("CARGO_PKG_VERSION")`；debug 构建展示层追加 `-dev` | [lib.rs](file:///e:/MyWork/Quirel/src-tauri/src/lib.rs#L25-L30) |
| 客户端前端 | `invoke<string>("get_app_version")` 读取 Rust 常量，不硬编码 | [Settings.tsx](file:///e:/MyWork/Quirel/src/apps/Settings.tsx#L1101-L1106) |
| quireld 展示 | `CARGO_PKG_VERSION + QUIREL_BUILD_INFO`；debug 追加 `-dev` | [agent/src/main.rs](file:///e:/MyWork/Quirel/agent/src/main.rs#L13-L21) |
| 构建元数据 | `分支@git describe --tags --always --dirty`，与版本号双轨 | [src-tauri/build.rs](file:///e:/MyWork/Quirel/src-tauri/build.rs#L26-L43)、[agent/build.rs](file:///e:/MyWork/Quirel/agent/build.rs#L6-L23) |
| 客户端自动供应 | `TARGET_QUIRELD_VERSION = env!("CARGO_PKG_VERSION")`，即客户端版本就是期望的 quireld 版本 | [provision.rs](file:///e:/MyWork/Quirel/src-tauri/src/provision.rs#L37-L41) |

注意：`AGENTS.md` 写「前端展示用 `getVersion()`」，但当前实际代码是 `invoke("get_app_version")`；属于文档与实现的轻微不一致。

### 4.5 版本号由谁决定

- **人决定**：发版时机与目标版本号由用户给出；AI 不得自主发起（AGENTS.md L48）。
- **脚本只做守卫**：合法性、在 main、工作区干净、严格递增。
- **没有自动推导**：没有 changeset、commit 分析、CHANGELOG 或 conventional-release 自动决定版本号。

---

## 5. dev 测试部署与正式发布的差别

依据 [AGENTS.md](file:///e:/MyWork/Quirel/AGENTS.md#L54) 与 [build-dev.mjs](file:///e:/MyWork/Quirel/scripts/build-dev.mjs)：

| 维度 | 正式发布 | dev/测试构建 |
|---|---|---|
| 版本文件 | 由 `npm run release` 写入并提交 | **一律不写** |
| 版本号来源 | Cargo.toml 中的正式版本 | 正式版本 + `git describe` 后缀：`<版本>-dev.N-g<sha>[-dirty]` |
| 注入方式 | 无 | 临时 `--config` 文件覆盖 `version` 与 `identifier`（`com.quirel.app.dev`），不落盘版本字段（L87-L100） |
| release 提交 | 有 `chore(release): X` | 无 |
| tag | 有 `vX` | 无 |
| 数据隔离 | 正式 identifier `com.quirel.app` | dev identifier `com.quirel.app.dev`，隔离数据目录与 keyring |
| 产物命名 | 正式版本 | 带 `-dev.N-g<sha>` 后缀（示例见 L5-L7） |
| HEAD 恰好在 tag 上 | 正式构建 | 脚本判定为正式构建，零后缀（L58-L64） |

补充：

- debug 构建（`cargo tauri dev` / `cargo build` 未开 release）在展示层追加 `-dev`，但不改版本文件（[lib.rs](file:///e:/MyWork/Quirel/src-tauri/src/lib.rs#L25-L30)、[agent/src/main.rs](file:///e:/MyWork/Quirel/agent/src/main.rs#L13-L21)）。
- `build-dev.mjs` 还会用全仓库最新 tag 做「发版收尾」检查（L43-L46、L71-L76）；当前异常 `v0.3.4` 会让该检查在 main/其他分支上误报「最新发布 v0.3.4，Cargo.toml 仍为 0.3.3」。
- dev 测试部署不产生 release 提交与 tag，符合 AGENTS.md L54。

---

## 6. 分支与发版的关系（按 AGENTS.md 逐节引用）

### 6.1 §2 提交规范

[AGENTS.md](file:///e:/MyWork/Quirel/AGENTS.md#L22-L29)：

- 提交格式 `type(scope): 中文简述`；type 仅限 `feat/fix/docs/refactor/perf/test/build/chore/revert/wip`。
- `wip` 仅限任务分支检查点，禁止出现在 dev/main。
- 一次提交只做一件事。
- 破坏性改动 type 后加 `!`，正文写 `BREAKING CHANGE:`。

release 脚本产生的 `chore(release): X` 正好符合该格式，且会被 commitlint 校验。

### 6.2 §3 分支开发规范

[AGENTS.md](file:///e:/MyWork/Quirel/AGENTS.md#L33-L43)：

- 任务分支一律从 dev 拉出；一分支一任务；命名 `feature/<短名>` 或 `fix/<短名>`；禁止分支嵌套。
- 任务分支允许 rebase 同步 dev；禁止改写已推送远端的历史。
- squash 合并进 dev 后归档为 `archive/<原名>`；归档分支是冻结快照。
- AI 权限边界：只在用户指定分支工作；任务分支内可提交检查点；完成任务后输出摘要；涉及 dev/main 的操作（squash、push、删除/归档、发版）必须先征得用户同意。
- main、dev 禁止直接开发；dev 只允许三类提交：squash 合并提交、`chore(release)`、用户亲自完成的微小规范提交。

### 6.3 §4 版本与发版

[AGENTS.md](file:///e:/MyWork/Quirel/AGENTS.md#L45-L54)：

- 三组件 lockstep（L47）。
- 版本唯一入口 `npm run release`，main 分支操作，须先征得用户同意，AI 可代执行（L48）。
- 发版流程：dev 合入 main → release 脚本（改版本 → `chore(release)` 提交 → tag `vX.Y.Z` 打在该提交上）→ `npm run tauri build`（L49）。
- 发版收尾：脚本打 tag 后自动将 main 快进回 dev（L50）。
- 版本规则与 beta 规则（L51）。
- 协议破坏性变更双端同发且 0.x 升次版本（L52）。
- 版本硬编码禁令（L53）。
- 开发/测试构建不改版本字段、不产生 release 提交与 tag（L54）。

### 6.4 三者角色总结

| 分支 | 角色 | 与发布的关系 |
|---|---|---|
| `main` | 发布基线 | 唯一允许执行 release 脚本与打 tag 的分支；每次 release 提交都在这里 |
| `dev` | 集成分支 | 接收任务分支 squash；发版时合入 main；release 后由脚本快进对齐 main |
| `feature/*` / `fix/*` | 任务分支 | 开发与验证；不发版、不打 tag；完成后 squash 进 dev |

当前实际状态与规则的偏差见 §8.1（`v0.3.4` 异常 tag）。

---

## 7. 构建、签名与更新通道

### 7.1 构建位置

| 组件 | 构建方式 | 是否 CI | 证据 |
|---|---|---|---|
| 客户端 | 本地 Windows：`npm run tauri build`；非发版用 `npm run build:dev` | 否 | 根目录无 `.github/`；`package.json` scripts |
| quireld | WSL/Linux：`agent/build.sh release <arch>`；arm64 交叉编译需 rust target 与 `aarch64-linux-gnu-gcc` | 否 | [build.sh](file:///e:/MyWork/Quirel/agent/build.sh#L177-L187) |
| 协议 crate | 随两端编译；`cargo test` 校验 wire 兼容 | 否 | [wire_compat.rs](file:///e:/MyWork/Quirel/quirel-protocol/tests/wire_compat.rs) |

### 7.2 安装包格式

- 客户端：仅 NSIS（`targets: ["nsis"]`），Windows x64；无 MSI、无 arm64 Windows 包。
- quireld：tar.gz，内含二进制、`quireld.toml`、`install.sh`、`update.sh`、`uninstall.sh`、可选 systemd service（[build.sh](file:///e:/MyWork/Quirel/agent/build.sh#L238-L267)）。

### 7.3 签名

- 客户端：未配置代码签名（`tauri.conf.json` bundle 无 windows 签名配置；仓库无证书/签名命令）。推断为未签名安装包。
- quireld：分发包不做二进制签名；完整性依赖 `SHA256SUMS` 与官网/GitHub 传输源。
- GitHub 上传可选，官网为主源。

### 7.4 更新通道

#### 客户端

- **没有 Tauri updater**（无插件依赖、`plugins` 为空）。
- 未找到客户端自更新代码或脚本。结论：客户端升级靠用户重新安装新的 NSIS 包。

#### quireld

quireld 有两条更新链路：

1. **客户端自动供应/在线更新**（SSH + QUIC 协议）：
   - 目标版本固定为客户端版本：`TARGET_QUIRELD_VERSION = env!("CARGO_PKG_VERSION")`（[provision.rs](file:///e:/MyWork/Quirel/src-tauri/src/provision.rs#L37-L41)）。
   - 下载地址按版本精确拼接：`{base}/{version}/quireld-{version}-release-linux-{arch}.tar.gz` 与同目录 `SHA256SUMS`（[provision.rs](file:///e:/MyWork/Quirel/src-tauri/src/provision.rs#L823-L830)）。
   - 源顺序：官网主域 → 官网备用域 → GitHub Releases（[provision.rs](file:///e:/MyWork/Quirel/src-tauri/src/provision.rs#L909-L970)）。
   - SHA-256 不匹配立即中断，不换源；只有「源不可达」才回落（[provision.rs](file:///e:/MyWork/Quirel/src-tauri/src/provision.rs#L1062-L1074)）。
   - 本地按版本缓存，命中时复验旁挂哈希（L852-L907）。
   - 最新版本提示：官网 `latest.json` 优先，GitHub API 兜底（[agent_update.rs](file:///e:/MyWork/Quirel/src-tauri/src/agent_update.rs#L294-L339)）。
   - 是否提示更新以「客户端版本 > 服务端版本」为条件，服务端超前时不诱导降级（[agent_update.rs](file:///e:/MyWork/Quirel/src-tauri/src/agent_update.rs#L344-L348)）。
   - 服务端执行更新时校验声明版本与二进制 `--version` 探测值一致，失败回滚（[update_manager.rs](file:///e:/MyWork/Quirel/agent/src/manager/update_manager.rs#L167-L196)）。

2. **手工引导安装**：
   - `install-quireld.sh` 使用**稳定别名** `quireld-linux-{arch}.tar.gz`，不拼版本号（[install-quireld.sh](file:///e:/MyWork/Quirel/agent/deploy/install-quireld.sh#L92-L106)）。
   - 因此手工安装取到的是官网当前最新，而不是客户端锁定的版本；与客户端自动安装的「版本钉扎」语义不同（见 §8.2 风险）。

#### 更新发布顺序

[`publish-quireld.mjs`](file:///e:/MyWork/Quirel/scripts/publish-quireld.mjs#L238-L258) 与 [DEPLOYMENT.md](file:///e:/MyWork/Quirel/docs/DEPLOYMENT.md#L29-L53) 都要求：

1. 先传版本目录 `{version}/`
2. 再传根级稳定别名与清单
3. 最后传 `latest.json`

原因是 `latest.json` 是更新检查开关，提前发布会让客户端在产物就绪前发现新版本。

---

## 8. 现存问题与风险

### 8.1 高优先级

#### R1. `v0.3.4` 是游离在 main 之外的异常 tag

事实：

- 指向 `c0477fb`（`feature/ui-audit-fixes`），不是 `chore(release)` 提交。
- 是注解 tag，tagger `vic`，创建于 2026-11-23 08:34:47 +0800；而 release 脚本打的是轻量 tag。
- 目标提交的 `package.json` / `src-tauri/Cargo.toml` 仍为 `0.3.3`，`agent/Cargo.toml` 也是 `0.3.3`。
- 未包含于 `main` 或 `dev`；仅 `feature/ui-audit-fixes` 包含。

影响：

- 违反 AGENTS.md §4「版本唯一入口」与「tag 打在发版提交上」，也违反 [PROGRESS.md](file:///e:/MyWork/Quirel/docs/PROGRESS.md#L392-L392) 记录的「tag 只打在 main」。
- `build-dev.mjs` 用全仓库最新 tag 做收尾检查（L43-L46、L71-L76），会在 main/其他分支误报「最新发布 v0.3.4，Cargo.toml 仍为 0.3.3」。
- 在包含该 tag 的分支上，`git describe --tags` 的基点会变成 `v0.3.4`，与 Cargo.toml 的 `0.3.3` 不一致，触发 [build-dev.mjs](file:///e:/MyWork/Quirel/scripts/build-dev.mjs#L53-L57) 的警告。
- 若该 tag 已推到远端，后续 `git describe`/发布检查/用户认知都会把 0.3.4 当成已发布版本；但仓库没有 0.3.4 的版本提交与产物。
- 本次研究无法确认远端是否存在该 tag（`git ls-remote` 网络失败）。

建议（需用户决定）：先确认远端是否有 `v0.3.4`；若没有，删除本地异常 tag；若已推送，需要按实际发布策略处理（不能用同一版本号重新发布）。

#### R2. `publish:quireld` 在当前 dev 系分支上按代码必然失败

事实：

- [publish-quireld.mjs](file:///e:/MyWork/Quirel/scripts/publish-quireld.mjs#L82-L89) 的 semver 校验正则为 `/^d+.d+.d+(-[0-9A-Za-z.-]+)?$/`，其中 `d+` 是字面量 `d`，不是 `\d+`。对 `0.3.3` 这类版本号不匹配，脚本会执行 `die("版本号非合法 semver")`。
- 修复提交 `de107a0`（`fix(scripts): 修复 publish 版本号 semver 正则转义丢失`）只存在于 `fix/publish-semver-regex` 分支，未合入 dev/main。
- 该脚本在 `main` 上**不存在**（`git ls-tree main scripts/` 只有 `build-dev.mjs`、`catchup-dev.mjs`、`check-sensitive-logs.mjs`、`release.mjs` 和启动脚本）。

影响：

- 按当前 dev 分支执行 `npm run publish:quireld` 会直接中止，无法生成官网静态目录。
- [DEPLOYMENT.md](file:///e:/MyWork/Quirel/docs/DEPLOYMENT.md#L29-L53) 在 dev 分支上把它写成标准发布步骤，但 main 上既没有该脚本也没有该段文档；文档与 main 实际能力不一致。

建议：合并 `fix/publish-semver-regex`，并在下一次发版前确认 `publish:quireld` 已进入 main。

### 8.2 中优先级

#### R3. main 与 dev 的发布工具链存在漂移

事实（`git diff --stat main..HEAD`）：

- `main` 缺少 `scripts/verify.mjs`、`scripts/publish-quireld.mjs`，`package.json` 缺少 `publish:quireld` 与 `verify*` 脚本。
- `main` 的 `AGENTS.md` 仍是旧版结构（`## 3. 版本号与发版规范`、`## 4. 版本号硬编码禁令`、`## 5. 开发/测试环境构建的版本规则`），而当前工作区重排为 `## 4. 版本与发版` 并引用 `npm run verify`。
- `main` 的 `docs/DEPLOYMENT.md` 没有「官网静态源 / publish:quireld / latest.json」这一段；当前 dev 系文档有。

影响：

- 如果按当前工作区 AGENTS.md §5/§6 的要求在 main 上执行 `npm run verify`，会因 main 的 `package.json` 没有该脚本而失败。
- 发版时若以 main 的文档为准，会缺少 quireld 的官网发布步骤；若以 dev 的文档为准，又会引用 main 上不存在的脚本。
- 这说明「发布流程」当前实际依赖用户记忆/分支上下文，而不是单一可在 main 上执行的清单。

#### R4. release 脚本非原子，且把 tag 建立在构建之前

事实：

- 版本文件与 lock 文件在同一个脚本里先写盘，再跑 `npm install` / `cargo update`；任一步失败都会留下未提交的版本改动（L101-L141）。
- commit 成功后才打 tag；tag 失败会留下无 tag 的 release 提交（L143-L150）。
- 脚本不打构建、不跑 verify；`npm run tauri build` 是 tag 之后的独立人工步骤（L166）。
- 若构建失败，tag 已经存在，需要人工删除/重打。历史记录中确有 beta.2 tag 因构建失败被本地撤回的案例（[PROGRESS.md](file:///e:/MyWork/Quirel/docs/PROGRESS.md#L358-L362)）。

影响：

- 发布失败时容易产生「版本号已提交、tag 已存在、但无可用产物」的半成品状态。
- 需要人工记住回滚顺序（删 tag、reset 提交、恢复 lock）。

#### R5. 没有 CHANGELOG / 发布说明，版本号选择完全依赖人工判断

事实：

- 仓库无 `CHANGELOG*`，历史中也从未出现。
- release 脚本只校验「新版本 > 当前版本」，不校验 feature/fix/breaking 对应的增量级别。
- 轻量 tag 没有附注；GitHub Release 仅 quireld 可选生成，且 notes 是固定文案。

影响：

- 0.3.3 之后可以直接跳 1.0.0，脚本不会阻止；也可以把 feature 发成 patch。
- 用户无法从 tag 本身看出本次发布包含哪些变更，只能查 `git log`。
- 与 AGENTS.md §4 的语义化约定之间缺少机检闭环。

#### R6. 无 CI、无签名、客户端无自动更新

事实：

- 根目录无 `.github/`，无其他 CI 配置。
- `tauri.conf.json` 无签名配置，仓库无签名命令/证书。
- 无 `tauri-plugin-updater` 依赖，`plugins` 为空。

影响：

- 构建环境、依赖版本、产物可复现性完全依赖本机。
- NSIS 安装包未签名，分发时可能被 SmartScreen 拦截。
- 客户端升级只能靠用户重新下载安装，缺少版本检查/增量更新能力。
- quireld 虽有更新通道，但其发布仍依赖人工上传官网与可选的 GitHub 操作。

### 8.3 低优先级 / 一致性风险

#### R7. `publish:quireld` 只校验本地 tag，不校验 tag 是否在 main、是否已推送

事实：L94-L110 只做 `git rev-parse refs/tags/v{version}`，不检查 tag 指向的提交是否在 main，也不检查远端是否存在。

推断风险：`gh release create {tag}` 在远端 tag 不存在时，GitHub 可能按默认分支创建该 tag（取决于 gh/GitHub 行为），导致 Release 指向错误提交；即使不创建，也会因「tag 不存在」失败。无论哪种，都会让「本地 tag 正确、远端发布错误」成为可能。

#### R8. 手工安装与客户端自动安装的版本语义不一致

事实：

- 客户端自动安装：按客户端版本精确取 `{version}/...`（[provision.rs](file:///e:/MyWork/Quirel/src-tauri/src/provision.rs#L823-L830)）。
- `install-quireld.sh`：使用稳定别名，取官网当前最新（[install-quireld.sh](file:///e:/MyWork/Quirel/agent/deploy/install-quireld.sh#L92-L106)）。

影响：当官网 `latest.json` 对应的 quireld 版本领先于旧客户端时，用户手工执行引导脚本可能装到比客户端期望更新的 quireld，破坏「同版本号 = 协议兼容」的默认预期。客户端 UI 对「服务端超前」做了不诱导更新的保护（[agent_update.rs](file:///e:/MyWork/Quirel/src-tauri/src/agent_update.rs#L344-L348)），但手工安装绕过了该保护。

#### R9. 历史 tag 命名与形式不统一

事实：

- 0.1.2 / 0.1.3 / 0.1.3-bate / 0.2.0-beta / 0.2.1-beta / 0.3.0-beta 没有 `v` 前缀；`0.1.3-bate` 是拼写错误。
- `0.1.2` / `0.1.3-bate` / `0.2.x` / `0.3.0-beta` 为注解 tag，`0.1.3` 与 `v0.3.0-beta.1` 起为轻量 tag。
- 当前异常 `v0.3.4` 又是注解 tag。

影响：依赖 tag 的工具（`git describe --tags`、发布检查、GitHub release）需要额外兼容；版本号排序与人类预期容易出现歧义（例如同版本 `-beta` 与 `-beta.N` 混用）。

#### R10. 文档过期与文档/实现不一致

事实：

- [2026-08-21-version-system-design.md](file:///e:/MyWork/Quirel/docs/superpowers/specs/2026-08-21-version-system-design.md#L33-L43) 仍描述「Agent 与 Client 独立版本号」「build.rs 同步 tauri.conf.json / package.json / 生成 version.ts」「预发布通道为非目标」，与当前 lockstep、tauri.conf 无 version、beta 通道已落地的实现相反。该文档是历史设计稿，但 AGENTS.md §0 要求 docs 与实现不符时先报告并修文档。
- [PROGRESS.md](file:///e:/MyWork/Quirel/docs/PROGRESS.md#L323) 的「最新发布版」停留在 v0.3.0-beta.2。
- [PROGRESS.md](file:///e:/MyWork/Quirel/docs/PROGRESS.md#L399-L399) 写前端用 `getVersion()`，实际是 `invoke("get_app_version")`。

影响：新成员或 AI 按旧文档执行时，会得到与当前脚本相反的结论（例如去改 tauri.conf.json 的 version 字段，而 verify 会直接判失败）。

---

## 9. 事实与推断分界

### 9.1 仓库中确实如此（可直接核对）

- 版本文件、lockstep 机检、release 脚本行为与「不推送/不构建」边界。
- tag 列表、tag 类型、`v0.3.4` 的目标提交与包含分支。
- main 缺少 `verify.mjs` / `publish-quireld.mjs` / 对应 npm scripts。
- `publish-quireld.mjs` 的正则拼写与修复分支状态。
- 客户端仅 NSIS、无签名配置、无 updater 插件。
- quireld 官网源 / GitHub 兜底 / 版本钉扎 / SHA 严格校验。
- 无 CI 配置、无 CHANGELOG。

### 9.2 推断（基于框架默认行为或「未找到实现」）

- Tauri 2 在 `tauri.conf.json` 无 `version` 时回落到 `src-tauri/Cargo.toml` 的 package version。
- 未配置签名时 NSIS 产物默认未签名。
- NSIS 产物路径 `src-tauri/target/release/bundle/nsis/Quirel_<版本>_x64-setup.exe`。
- `gh release create <tag>` 在远端 tag 不存在时可能按默认分支创建 tag。
- 「客户端没有自动更新」是基于仓库内未找到 updater 依赖、配置与更新代码的判断，不排除仓库外分发渠道另有机制。

---

## 10. 建议（仅作后续决策参考，未实施）

1. 先处理 `v0.3.4` 异常 tag：确认远端状态；若远端不存在则删除本地 tag；若已推送则按「已发布版本不复用」原则单独决策。
2. 合并 `fix/publish-semver-regex`，并把 `verify.mjs` / `publish-quireld.mjs` 及其 npm scripts 带入 main，消除 main 与 dev 的发布工具链漂移。
3. 给 release 脚本增加前置门禁：`verify`（至少 lockstep + wire_compat）、tag 是否存在、main 是否与远端同步、dev 是否已并入 main；把「打 tag」与「构建成功」解耦或增加 `--no-tag` / `--tag-only` 模式。
4. 增加最小发布记录：从 `git log <上个tag>..HEAD` 生成 release notes 或 CHANGELOG，至少写入 tag 附注/GitHub Release。
5. 明确客户端分发与签名策略；若短期不做 Tauri updater，至少在发布清单中写清安装包交付路径与校验方式。
6. 统一 `install-quireld.sh` 与客户端自动安装的版本语义（例如支持 `--version` 或读取 `latest.json` 中与客户端兼容的版本），避免手工安装绕过 lockstep。
7. 清理/标注过期设计文档，特别是 2026-08-21 版本系统设计稿，避免与当前 AGENTS.md §4 冲突。

---

## 附录 A：关键文件索引

| 文件 | 作用 |
|---|---|
| [AGENTS.md](file:///e:/MyWork/Quirel/AGENTS.md) | 分支、版本、发版、验证、AI 产物纪律的唯一规则源 |
| [scripts/release.mjs](file:///e:/MyWork/Quirel/scripts/release.mjs) | 版本唯一写入口；版本文件 + lock + 提交 + tag + dev 快进 |
| [scripts/build-dev.mjs](file:///e:/MyWork/Quirel/scripts/build-dev.mjs) | 非发版构建，`-dev.N-g<sha>` 命名与数据目录隔离 |
| [scripts/publish-quireld.mjs](file:///e:/MyWork/Quirel/scripts/publish-quireld.mjs) | quireld 官网静态目录生成与可选 GitHub 上传（当前 dev 系有正则缺陷） |
| [scripts/verify.mjs](file:///e:/MyWork/Quirel/scripts/verify.mjs) | lockstep 机检与分层验证门禁（当前 main 缺失） |
| [scripts/catchup-dev.mjs](file:///e:/MyWork/Quirel/scripts/catchup-dev.mjs) | 发版欠账快进 + 任务分支 squash 归档的补救脚本 |
| [src-tauri/tauri.conf.json](file:///e:/MyWork/Quirel/src-tauri/tauri.conf.json) | 客户端打包配置：NSIS、无 version、无签名、无 updater |
| [agent/build.sh](file:///e:/MyWork/Quirel/agent/build.sh) | quireld 双架构 tar.gz 与 `SHA256SUMS` |
| [agent/deploy/install-quireld.sh](file:///e:/MyWork/Quirel/agent/deploy/install-quireld.sh) | 手工引导安装（稳定别名，非版本钉扎） |
| [src-tauri/src/provision.rs](file:///e:/MyWork/Quirel/src-tauri/src/provision.rs) | 客户端自动安装/更新：版本钉扎、源回落、SHA 校验 |
| [src-tauri/src/agent_update.rs](file:///e:/MyWork/Quirel/src-tauri/src/agent_update.rs) | 最新版本检查与 update 可用性判定 |
| [agent/src/manager/update_manager.rs](file:///e:/MyWork/Quirel/agent/src/manager/update_manager.rs) | 服务端 staging / 版本探测 / 回滚 |
| [docs/DEPLOYMENT.md](file:///e:/MyWork/Quirel/docs/DEPLOYMENT.md) | quireld 部署与官网发布步骤（dev 系版本） |
| [quirel-protocol/tests/wire_compat.rs](file:///e:/MyWork/Quirel/quirel-protocol/tests/wire_compat.rs) | 协议破坏性变更的回归防线 |

## 附录 B：研究使用的关键命令

```powershell
git status --short
git branch -a -vv
git tag --sort=-v:refname
git for-each-ref --sort=creatordate --format='%(refname:short)|%(creatordate:iso8601)|%(objectname:short)|%(taggername)|%(subject)' refs/tags
git cat-file -p refs/tags/v0.3.4
git branch -a --contains v0.3.4
git show v0.3.4:package.json
git ls-tree --name-only main scripts/
git diff --stat main..HEAD -- AGENTS.md docs/DEPLOYMENT.md package.json scripts/
git log --all --grep='chore(release)' --oneline
git log --all --oneline -- CHANGELOG.md
```
