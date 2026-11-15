# 开发治理体系 v2 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 落地治理体系 v2——AGENTS.md 重构为三层治理的宪法层、统一验证门禁、六个项目级技能、安全与产物纪律、根目录清整、README 重写。

**Architecture:** L1 宪法（AGENTS.md ≤130 行常驻）→ L2 操作（`.agents/skills/` 触发式技能）→ L3 背景（docs/ 深度文档）。机检点集中在 `scripts/verify.mjs`（lockstep / clippy / 敏感日志 / 分层验证）。

**Tech Stack:** Markdown（规则文档）、Node ESM 脚本（verify.mjs）、cargo/vitest（验证后端）、WSL（agent 构建环境）。

**Spec:** `docs/superpowers/specs/2026-11-15-dev-governance-design.md`（验收标准见其 §6）

**工作分支：** `feature/dev-governance`。每个 Task 结束提交一次（规范提交信息见各任务末步）。**禁止 `git add .`，一律显式指定路径。**

---

### Task 1: AGENTS.md v2 重写

**Files:**
- Modify: `AGENTS.md`（整文件替换，141 行 → ≤130 行）

- [ ] **Step 1: 用以下完整内容覆盖 AGENTS.md**

````markdown
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
````

- [ ] **Step 2: 校验行数 ≤130**

Run: `wc -l AGENTS.md`
Expected: ≤ 130。若超出：压缩 §4 或 §3 的换行密度（合并短句到同一行），不得删除红线条款。

- [ ] **Step 3: 提交**

```bash
git add AGENTS.md
git commit -m "docs(core): AGENTS.md 重构为治理 v2（红线总表/验证规范/安全与产物纪律）"
```
Expected: commitlint 通过，pre-commit 通过。

---

### Task 2: 新建 docs/browser-session-rules.md

**Files:**
- Create: `docs/browser-session-rules.md`

- [ ] **Step 1: 创建文件，内容如下**

````markdown
# 浏览器会话所有权红线（原子性）

> 本文承接 AGENTS.md §5 红线总表第 1 行；改动涉及本文范围时先读全文。
> 回归：`src/window-system/core/WindowManager.session.test.ts`（原子性契约单测）。

浏览器会话（host + TabBook + 全部 tab webview）的生命周期**唯一归属窗口系统**，与宿主前端页面（React 树）完全解耦——页面刷新/重载/死亡对会话零影响，新页面经 browser_open（幂等）+ browser_refresh_tabs（全量回灌）重新挂载视图。

- 会话销毁**唯一入口**：registry 的 `lifecycle.onAfterClose`（init.ts browser 注册项）→ `proxy_stop_session` 全量停止。React 组件的 effect/cleanup **禁止**触碰会话销毁（browser_close_window / proxy_stop_session）——页面刷新会执行 cleanup，曾致多 tab 会话反复丢失（2026-10 三轮返工）。
- webview 可见性由持有方（runtime）强制执行：前端 1s 心跳（browser_heartbeat）中断 >3s 自动隐藏全部 webview，恢复后按命令态重.assert——禁止依赖前端指令的"一次性"可见性变更作为唯一手段。
- 所有 portal 到 body 的浮层必须带 `data-browser-overlay`（webview 子窗口为独立 HWND，DOM z-index 无法遮挡；缺失标记 = 浮层被网页盖住）。
- 新增浮层/改动可见性链路后，回归 `src/window-system/core/WindowManager.session.test.ts`（原子性契约单测）。
````

- [ ] **Step 2: 提交**

```bash
git add docs/browser-session-rules.md
git commit -m "docs(core): 新增浏览器会话所有权红线文档"
```

---

### Task 3: verify 统一验证门禁

**Files:**
- Create: `scripts/verify.mjs`
- Modify: `package.json`（scripts 段追加四条 verify 命令）

- [ ] **Step 1: 创建 scripts/verify.mjs，完整内容如下**

```js
#!/usr/bin/env node
// Quirel 统一验证门禁（AGENTS.md §6）。
// 用法：node scripts/verify.mjs [quick|fe|rust|full] [--clippy-strict]
//   quick：敏感日志 + lockstep + vitest + 三 crate cargo check（默认）
//   fe   ：敏感日志 + vitest
//   rust ：三 crate cargo test
//   full ：quick + 三 crate cargo test + cargo clippy
// 输出每步 PASS/FAIL/SKIP 与末尾摘要；存在 FAIL 时退出码 1。
// agent（quireld）含 cfg(unix) 代码：Windows 主机经 WSL 执行，其余环境原生执行。
// clippy 默认 lenient；--clippy-strict 开启 -D warnings；存量警告清零后可将默认翻转为严格。

import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const WIN = process.platform === 'win32';
const ARGS = process.argv.slice(2);
const MODE = ARGS.find((a) => !a.startsWith('--')) || 'quick';
const CLIPPY_STRICT = ARGS.includes('--clippy-strict');

const results = [];

function record(step, status, detail = '') {
  results.push({ label: step.label, status });
  console.log(`[verify] ${status} ${step.label}${detail ? ` — ${detail}` : ''}`);
}

function finish(step, r) {
  const out = ((r.stdout || '') + '\n' + (r.stderr || '')).trim();
  if (r.error && r.error.code === 'ENOENT') {
    record(step, 'SKIP', `命令不可用: ${r.error.code || r.error.message}`);
    return;
  }
  if (r.error) {
    record(step, 'FAIL', `spawn 错误: ${r.error.code || r.error.message}`);
    if (out) console.error(out.split('\n').slice(-25).join('\n'));
    return;
  }
  if (r.status === 0) {
    const warnings = (out.match(/^.*\bwarning\b.*$/gm) || []).length;
    record(step, 'PASS', warnings && step.label.includes('clippy') ? `${warnings} 条 warning（lenient）` : '');
    if (out) console.log(out.split('\n').slice(-8).join('\n') + '\n');
    return;
  }
  record(step, 'FAIL');
  if (out) console.error(out.split('\n').slice(-25).join('\n'));
}

function runShellStep(step) {
  console.log(`\n[verify] ${step.label}\n[verify] $ ${step.cmdStr}`);
  const r = spawnSync(step.cmdStr, {
    cwd: step.cwd || ROOT,
    shell: true,
    encoding: 'utf8',
    maxBuffer: 10 * 1024 * 1024,
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  finish(step, r);
}

function runArgvStep(step) {
  console.log(`\n[verify] ${step.label}\n[verify] $ ${step.cmd} ${step.args.join(' ')}`);
  const r = spawnSync(step.cmd, step.args, {
    cwd: ROOT,
    encoding: 'utf8',
    maxBuffer: 10 * 1024 * 1024,
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  finish(step, r);
}

function lockstepStep() {
  const step = { label: 'lockstep 版本一致性' };
  console.log(`\n[verify] ${step.label}`);
  try {
    const pkg = JSON.parse(readFileSync(path.join(ROOT, 'package.json'), 'utf8'));
    const versions = { 'package.json': pkg.version };
    for (const c of ['src-tauri', 'agent', 'quirel-protocol']) {
      const m = readFileSync(path.join(ROOT, c, 'Cargo.toml'), 'utf8').match(/^version\s*=\s*"([^"]+)"/m);
      if (!m) throw new Error(`${c}/Cargo.toml 未找到 version 字段`);
      versions[`${c}/Cargo.toml`] = m[1];
    }
    const conf = JSON.parse(readFileSync(path.join(ROOT, 'src-tauri', 'tauri.conf.json'), 'utf8'));
    if ('version' in conf) throw new Error('tauri.conf.json 不允许设置 version 字段（AGENTS §4）');
    const uniq = new Set(Object.values(versions));
    if (uniq.size !== 1) {
      throw new Error(`版本不一致: ${Object.entries(versions).map(([k, v]) => `${k}=${v}`).join(', ')}`);
    }
    record(step, 'PASS', pkg.version);
  } catch (e) {
    record(step, 'FAIL', e.message);
    console.error(`[verify] ${e.message}`);
  }
}

let _wslRepo;
function wslRepo() {
  if (_wslRepo !== undefined) return _wslRepo;
  const r = spawnSync('wsl.exe', ['-e', 'wslpath', '-a', ROOT], { encoding: 'utf8', maxBuffer: 10 * 1024 * 1024 });
  _wslRepo = r.status === 0 ? r.stdout.trim() : null;
  return _wslRepo;
}

function agentStep(label, cargoSub) {
  if (!WIN) return { label: `${label} (agent)`, cmdStr: `cargo ${cargoSub} --manifest-path agent/Cargo.toml` };
  const repo = wslRepo();
  if (!repo) return { label: `${label} (agent, WSL)`, skip: 'WSL/wslpath 不可用' };
  return {
    label: `${label} (agent, WSL)`,
    cmd: 'wsl.exe',
    args: ['-e', 'bash', '-lc', `cd '${repo}' && cargo ${cargoSub} --manifest-path agent/Cargo.toml`],
  };
}

function nativeStep(label, crate, cargoSub) {
  return { label: `${label} (${crate})`, cmdStr: `cargo ${cargoSub} --manifest-path ${crate}/Cargo.toml` };
}

const CARGO_SUB = {
  check: 'check',
  test: 'test',
  clippy: CLIPPY_STRICT ? 'clippy --message-format short -- -D warnings' : 'clippy --message-format short',
};

const vitest = { label: 'vitest（前端单测）', cmdStr: 'npm run test:run' };
const sensitiveLogs = { label: 'check:sensitive-logs（敏感日志门禁）', cmdStr: 'npm run check:sensitive-logs' };
const lockstep = { label: 'lockstep 版本一致性', js: true, fn: lockstepStep };

const feSteps = [sensitiveLogs, vitest];
const quickSteps = [sensitiveLogs, lockstep, vitest,
  nativeStep('cargo check', 'src-tauri', CARGO_SUB.check),
  nativeStep('cargo check', 'quirel-protocol', CARGO_SUB.check),
  agentStep('cargo check', CARGO_SUB.check)];
const rustSteps = [
  nativeStep('cargo test', 'src-tauri', CARGO_SUB.test),
  nativeStep('cargo test', 'quirel-protocol', CARGO_SUB.test),
  agentStep('cargo test', CARGO_SUB.test)];
const fullSteps = [...quickSteps, ...rustSteps,
  nativeStep('cargo clippy', 'src-tauri', CARGO_SUB.clippy),
  nativeStep('cargo clippy', 'quirel-protocol', CARGO_SUB.clippy),
  agentStep('cargo clippy', CARGO_SUB.clippy)];

const MODES = { fe: feSteps, quick: quickSteps, rust: rustSteps, full: fullSteps };
if (!MODES[MODE]) {
  console.error(`[verify] 未知模式: ${MODE}（可选 quick|fe|rust|full）`);
  process.exit(2);
}

console.log(`[verify] 模式: ${MODE}${CLIPPY_STRICT ? '（clippy 严格）' : '（clippy lenient）'}`);
for (const s of MODES[MODE]) {
  if (s.skip) { record(s, 'SKIP', s.skip); continue; }
  if (s.js) { s.fn(); continue; }
  if (s.args) runArgvStep(s);
  else runShellStep(s);
}

console.log('\n===== verify 摘要 =====');
for (const r of results) console.log(`${r.status.padEnd(5)} ${r.label}`);
const fails = results.filter((r) => r.status === 'FAIL').length;
const skips = results.filter((r) => r.status === 'SKIP').length;
console.log(`共 ${results.length} 步：PASS ${results.length - fails - skips} / FAIL ${fails} / SKIP ${skips}`);
if (skips > 0) console.log('[verify] ⚠ 存在 SKIP：门禁未完整覆盖，请人工确认 SKIP 原因');
process.exit(fails ? 1 : 0);
```

- [ ] **Step 2: package.json scripts 段追加命令**

Edit `package.json`：
old_string:
```json
    "check:sensitive-logs": "node scripts/check-sensitive-logs.mjs"
```
new_string:
```json
    "check:sensitive-logs": "node scripts/check-sensitive-logs.mjs",
    "verify": "node scripts/verify.mjs quick",
    "verify:fe": "node scripts/verify.mjs fe",
    "verify:rust": "node scripts/verify.mjs rust",
    "verify:full": "node scripts/verify.mjs full"
```

- [ ] **Step 3: 首跑 quick 验证**

Run: `npm run verify`
Expected: 末尾摘要 `共 6 步：PASS 6 / FAIL 0 / SKIP 0`（若 WSL 不可用或 wslpath 失败，agent 步为 SKIP 且摘要如实显示——此时向用户报告，不得视为通过）。时长参考：增量编译下 ≤2 分钟；lockstep 步应显示 `PASS — 0.3.3`。

- [ ] **Step 4: clippy 存量基线评估**

Run: `npm run verify:full -- --clippy-lenient`
Expected: 全部 PASS（lenient 下 clippy 警告不致失败）；记录三个 clippy 步的 warning 条数。

决策规则：
- 三个 crate 合计 warning = 0 → 保持脚本默认严格（`CLIPPY_STRICT = true`），无需改动；
- warning > 0 → Edit verify.mjs 把 `const CLIPPY_STRICT = !ARGS.includes('--clippy-lenient');`
  改为 `const CLIPPY_STRICT = ARGS.includes('--clippy-strict');`（默认 lenient，警告清单
  已在步骤输出中，记入实施报告作为后续清理项），并在报告写明「clippy 严格门禁待存量清零后开启」。
- 若 WSL 内 clippy 组件缺失（bash 报 `clippy` not found）：在 WSL 执行 `rustup component add clippy` 后重跑。

- [ ] **Step 5: 契约测试盘点（仅评估，不实施）**

```bash
grep -rln "cfg(test)" src-tauri/src | xargs grep -ln "credentials" 2>/dev/null; echo "---store-versions---"; grep -rn "EXPECTED_STORE_VERSIONS" src src-tauri/src --include=*.ts --include=*.rs -l
```
判定并记入实施报告：credentials.rs 的 keyring+AES-GCM 加解密 roundtrip 是否有 Rust 侧测试；store 版本迁移（EXPECTED_STORE_VERSIONS）是否有测试覆盖。**缺口的补测试作为后续独立任务提出，本轮不改代码。**

- [ ] **Step 6: 提交**

```bash
git add scripts/verify.mjs package.json
git commit -m "build(core): 新增 verify 统一验证门禁（lockstep/clippy/分层验证）"
```

---

### Task 4: data-storage.md 修正 + .trae/rules 历史方案声明

**Files:**
- Modify: `docs/data-storage.md`（凭据描述与 credentials.rs 信封加密实现对齐）
- Modify: `.trae/rules/项目规范.md`（仅顶部加声明，不改正文）

- [ ] **Step 1: 修正 servers.bin 数据行并补 credentials/ 行**

Edit `docs/data-storage.md`：
old_string:
```
| `servers.bin` | 服务器列表：host/port/username/certFingerprint/状态 + **auth 凭据（私钥/密码，当前为明文 JSON）** | 🔴 高 |
```
new_string:
```
| `servers.bin` | 服务器列表：host/port/username/certFingerprint/状态（**不含凭据**，2026-10 起凭据独立加密存储） | 🟡 低 |
| `credentials/` | 每服务器 `<serverId>.bin`：凭据载荷（密码/私钥/口令）经主密钥 AES-256-GCM 信封加密（nonce‖密文）；主密钥存 OS 凭据库 | 🔴 高 |
```

- [ ] **Step 2: 修正「凭据安全化」描述为信封加密形态**

Edit `docs/data-storage.md`：
old_string:
```
1. **🔴→✅ 凭据安全化（成熟形态）**：SSH 私钥/密码/口令**唯一持久化形态 = OS 凭据管理器**
   (keyring crate：Windows Credential Manager / macOS Keychain / Linux keyutils，
   service=quirel-server-credentials，account=serverId)。完整防线：
```
new_string:
```
1. **🔴→✅ 凭据安全化（信封加密形态）**：OS 凭据管理器（keyring：Windows Credential
   Manager / macOS Keychain / Linux keyutils）只存 32 字节**主密钥**
   （service=quirel-server-credentials，account=__master__；dev 构建 identifier 不同，
   用 quirel-server-credentials-dev）；凭据载荷 AES-256-GCM 加密落盘
   `app_data_dir/credentials/<serverId>.bin`（见 src-tauri/src/credentials.rs）。完整防线：
```

- [ ] **Step 3: 历史记录追加升级条目**

Edit `docs/data-storage.md`：
old_string:
```
- 2026-10-25：落地凭据 keyring 化 + 应用内完全清除 + NSIS 卸载删除数据 + .bak 自动清理（feature/new_ui 分支）。
```
new_string:
```
- 2026-10-25：落地凭据 keyring 化 + 应用内完全清除 + NSIS 卸载删除数据 + .bak 自动清理（feature/new_ui 分支）。
- 2026-11：凭据存储升级为信封加密（keyring 仅存主密钥 + credentials/*.bin AES-GCM），servers.bin 不再含凭据（src-tauri/src/credentials.rs）。
```

- [ ] **Step 4: .trae/rules 顶部加历史方案声明（不改正文）**

Edit `.trae/rules/项目规范.md`：
old_string:
```
# Adwaita 远程控制客户端 — 完整方案
```
new_string:
```
> ⚠️ **历史方案存档（2026-05 立项时）**：本文为早期完整方案，大量内容已被后续实现取代；现行开发规范以仓库根 `AGENTS.md` 为唯一权威，两者冲突时一律以 AGENTS.md 为准。

# Adwaita 远程控制客户端 — 完整方案
```

- [ ] **Step 5: 提交**

```bash
git add docs/data-storage.md .trae/rules/项目规范.md
git commit -m "docs(core): 修正 data-storage 凭据描述并标注 trae 历史方案"
```

---

### Task 5: 根目录清整 + .gitignore（受限操作，逐项经用户确认后执行）

**Files:**
- Move: `debug-maximize-black-flash.md` → `docs/audits/`
- Move: `debug-quic-connection-drop.md` → `docs/audits/`
- Delete（待确认）: `debug-settings-not-working.md`、`vitest_output.txt`、`color-analysis.html`、`test-run-script.sh`、`test_private_key.rs`
- Move: `quirel-client-plan.md` → `docs/superpowers/plans/2026-05-28-quirel-client-plan.md`
- Move: `.trae/documents/filemanager-crud-consistency.md`、`.trae/documents/notification-server-isolation-plan.md` → `docs/superpowers/plans/`
- Delete: `e2e/`（仅驱动残留）
- Modify: `.gitignore`（增补 `.superpowers/`）

- [ ] **Step 1: 处置依据核查（向用户出示）**

```bash
head -8 debug-settings-not-working.md; find e2e -type f | head -20; git status --short
```
依据（已勘察）：`debug-maximize-black-flash.md`（FIXED-VERIFIED，沉淀已在 window-flash-rules.md，本文是证据记录→保留归档）；`debug-quic-connection-drop.md`（CLOSED FIXED→保留归档）；`debug-settings-not-working.md`（**OPEN 状态滞留自 2026-05-28**→建议删除）；`test_private_key.rs`（64 行独立 main 工具，读私钥文件路径做解析，非测试→建议删除）。

- [ ] **Step 2: 向用户逐项确认处置清单，按确认结果执行**

归档移动（已获 spec 批准的方向）：
```bash
git mv debug-maximize-black-flash.md docs/audits/debug-maximize-black-flash.md
git mv debug-quic-connection-drop.md docs/audits/debug-quic-connection-drop.md
git mv quirel-client-plan.md docs/superpowers/plans/2026-05-28-quirel-client-plan.md
mv .trae/documents/filemanager-crud-consistency.md docs/superpowers/plans/
mv .trae/documents/notification-server-isolation-plan.md docs/superpowers/plans/
```
删除（逐项口头确认后）：
```bash
git rm debug-settings-not-working.md vitest_output.txt color-analysis.html test-run-script.sh test_private_key.rs
rm -rf e2e
```

- [ ] **Step 3: .gitignore 增补 AI 会话产物目录**

Edit `.gitignore`：
old_string:
```
.zcode/
```
new_string:
```
.zcode/
.superpowers/
```

- [ ] **Step 4: 核查无遗漏**

Run: `git status --short`
Expected: 根目录无散落的 md/txt/html/sh/rs 临时文件；`.superpowers/` 不再出现在未跟踪列表。

- [ ] **Step 5: 提交**

```bash
git add docs/audits docs/superpowers/plans .gitignore
git add -u
git commit -m "chore(core): 根目录清整与会话产物归位（治理 v2 §8）"
```

---

### Task 6: README 重写

**Files:**
- Modify: `README.md`（整文件替换）

- [ ] **Step 1: 用以下内容覆盖 README.md**

````markdown
# Quirel

远程 Linux 服务器控制客户端——本地桌面应用 + 服务器守护进程 + 共享协议。

| 组件 | 目录 | 说明 |
|---|---|---|
| `quirel` | `src-tauri/`（前端 `src/`） | Windows 桌面客户端（Tauri 2 + React + TypeScript） |
| `quireld` | `agent/` | 远程 Linux 守护进程（QUIC，systemd 部署，WSL 内构建） |
| `quirel-protocol` | `quirel-protocol/` | 双端共享线上协议 crate（协议单一真相源） |

三组件统一版本号（lockstep）。开发规范与红线总表见 [AGENTS.md](AGENTS.md)。

## 开发环境

- Node.js ≥ 20；Rust stable（`rust-toolchain.toml` 统一，Windows 与 WSL 共用）
- Windows 客户端原生开发；`agent`（quireld）在 WSL 内构建测试
- `cargo-sweep`（构建缓存清理纪律，AGENTS.md §9）；`cargo-audit`（依赖公告扫描，AGENTS.md §7）

## 常用命令

| 命令 | 用途 |
|---|---|
| `npm run dev` / `npm run tauri dev` | 前端开发 / 客户端联调 |
| `npm run test` / `npm run test:run` | vitest 监听 / 单次（前端单测） |
| `npm run verify` | 快速验证（敏感日志 + lockstep + vitest + cargo check ×3） |
| `npm run verify:fe` / `verify:rust` / `verify:full` | 分层验证（AGENTS.md §6） |
| `npm run build:dev` | 非发版安装包（`<版本>-dev.N-g<sha>` 命名） |
| `npm run release <版本号>` | 发版（仅 main 分支，须用户指示，AGENTS.md §4） |

## 文档索引

- [AGENTS.md](AGENTS.md) — 开发规范与红线总表（**AI 工具入口：从本文件读起**）
- [docs/window-flash-rules.md](docs/window-flash-rules.md) — 窗口坏帧红线与探针回归
- [docs/browser-session-rules.md](docs/browser-session-rules.md) — 浏览器会话所有权红线
- [docs/IPC_PROTOCOL.md](docs/IPC_PROTOCOL.md) — 线上协议规范
- [docs/data-storage.md](docs/data-storage.md) — 数据存储与清除策略
- [docs/audits/](docs/audits/) — 安全与 bug 审计报告
- `.agents/skills/` — 项目级触发式技能（红线 / 验证 / 协议变更 / 安全 / Rust 惯例）
````

- [ ] **Step 2: 提交**

```bash
git add README.md
git commit -m "docs(core): README 重写为项目真实入口"
```

---

### Task 7: .agents/skills/ 六个触发式技能

**Files:**
- Create: `.agents/skills/quirel-redlines-window/SKILL.md`
- Create: `.agents/skills/quirel-redlines-browser/SKILL.md`
- Create: `.agents/skills/quirel-protocol-change/SKILL.md`
- Create: `.agents/skills/quirel-verify/SKILL.md`
- Create: `.agents/skills/quirel-safety/SKILL.md`
- Create: `.agents/skills/quirel-rust-idioms/SKILL.md`

- [ ] **Step 1: quirel-redlines-window**

````markdown
---
name: quirel-redlines-window
description: Quirel 窗口坏帧红线。触碰 lib.rs 窗口创建/Resized/NCHITTEST 子类/no_erase_proc、fullscreen.rs 全屏切换、壁纸系统（wallpaperStore/WallpaperContext）、browser webview bounds（webview_host.rs/browser_window.rs）、升级 tauri/wry/tao 版本时必须先加载本技能。
---

# 窗口坏帧红线（AGENTS.md §5 红线表第 2 行）

## 改动前（强制）
1. 通读 `docs/window-flash-rules.md`（根因链、已否定方案、硬红线 §5、探针回归 §7）。
2. 对照硬红线自查：主窗口 WM_ERASEBKGND 必须以 WINDOW_BG_COLORREF 填色；子窗口链
   必须维持「不擦」；壁纸变更必须经 set_window_background_color 同步双层背景色；
   主窗口必须 transparent(false)。

## 改动后（强制回归）
- 按 `docs/window-flash-rules.md` §7 用 dupcap 探针（DXGI Desktop Duplication 帧级
  抓屏）回归最大化/缩放路径；判定标准见该文档（frames.csv h0 无 >55000 尖峰）。
- 无法在本机执行探针时：如实声明未回归，禁止声称「已验证」。

## 背景速览
最大化/缩放时窗口暴露区在 WebView2 新帧就位前必然存在坏帧（13~55ms，与刷新率无关），
现方案为「以当前壁纸平均色填充暴露区」（实验 G 定稿）。任何「简化」该链路的改动
都曾引发闪黑回归。
````

- [ ] **Step 2: quirel-redlines-browser**

````markdown
---
name: quirel-redlines-browser
description: Quirel 浏览器会话所有权红线。触碰浏览器会话生命周期（host/TabBook/tab webview）、init.ts browser 注册项、webview 可见性/心跳（browser_heartbeat）、data-browser-overlay 浮层时必须先加载本技能。React effect/cleanup 中的会话销毁是历史事故源。
---

# 浏览器会话所有权红线（AGENTS.md §5 红线表第 1 行）

## 改动前（强制）
1. 通读 `docs/browser-session-rules.md`（全文）。
2. 自查：本次改动是否在 React 组件 effect/cleanup 中新增或改动 browser_close_window /
   proxy_stop_session 调用？→ 禁止。会话销毁唯一入口是 registry 的
   lifecycle.onAfterClose（init.ts browser 注册项）。

## 改动后（强制回归）
- 运行契约单测：`npx vitest run src/window-system/core/WindowManager.session.test.ts`
- 新增 portal 浮层必须带 data-browser-overlay（webview 子窗口是独立 HWND，DOM
  z-index 挡不住）。
- 可见性变更不得依赖前端一次性指令：1s 心跳（browser_heartbeat）中断 >3s 自动隐藏，
  恢复后按命令态重.assert。

## 背景速览
浏览器会话生命周期唯一归属窗口系统，与 React 树解耦；页面刷新/死亡对会话零影响。
2026-10 曾因 cleanup 触碰销毁导致多 tab 会话反复丢失（三轮返工）。
````

- [ ] **Step 3: quirel-protocol-change**

````markdown
---
name: quirel-protocol-change
description: Quirel 线上协议变更规范。修改 quirel-protocol/src 下任何类型、Envelope、serde 字段、新增消息类型，或 agent/src/protocol 与客户端消费点需要同步时，必须先加载本技能。
---

# 协议变更 checklist（AGENTS.md §5 红线表第 4 行）

## 破坏性判定（不确定时按破坏性处理并报告用户）
破坏性 = 必须客户端与 quireld 同发，0.x 阶段至少升次版本号（AGENTS.md §4）：
- 删除/重命名枚举 variant、结构体字段；字段类型变更；序列化格式（tag/表示）变更；
  收紧校验使旧消息被拒。
兼容性变更（仍需评估）：
- 新增 `#[serde(default)]` 可选字段（前提：旧端反序列化忽略未知字段）
- 新增消息类型（前提：旧端按 unknown 处理）

## 改动步骤
1. TDD：先在 `quirel-protocol/tests/wire_compat.rs` 加/改 wire 兼容用例（先红后绿）。
2. 改 `quirel-protocol/src`，保持双端语义一致。
3. 检查 agent 侧（agent/src/protocol/）与客户端侧（src-tauri）消费点是否同步。
4. `npm run verify:rust` 全绿；同步更新 `docs/IPC_PROTOCOL.md`。
5. 判定为破坏性时：在完成摘要中明确声明，等待用户安排双端同发版本。
````

- [ ] **Step 4: quirel-verify**

````markdown
---
name: quirel-verify
description: Quirel 任务完成验证与证据模板。在任何任务完成声明、请求合并、提交检查点之前必须加载本技能；修 bug、改前端、改 Rust、触碰红线后的必跑命令组合在此选择。
---

# 完成验证（AGENTS.md §6）

## 按改动面选命令
| 改动面 | 必跑 |
|---|---|
| 前端 src/ | `npm run verify:fe` |
| Rust 任一 crate | `npm run verify:rust` |
| 触碰红线 | 红线表对应回归（WindowManager.session.test.ts / dupcap 探针 / wire_compat.rs / check:sensitive-logs） |
| 完成声明 / 请求合并前 | `npm run verify:full` |

## bug 修复黄金流程（强制）
1. 先写复现测试（此刻必须 FAIL——红）。
2. 根因修复（配合 systematic-debugging 技能：先定位根因，禁止症状压制）。
3. 复现测试转 PASS（绿）+ `npm run verify:full` 全绿。
4. 复现测试永久入库；禁止删除或 ignore 它。

## 完成摘要四件套（模板）
```
变更摘要：<改了什么 / 为什么>
影响面：<模块 / 接口 / 受影响的现有测试（列出测试文件名）>
测试证据：<命令 + 结果（修 bug 时含复现测试红→绿记录）>
建议 squash 提交信息：<type(scope): 中文简述>
```

## 定位相关现有测试
按改动文件就近找：src/ 同目录 `*.test.ts(x)`；src-tauri / agent 文件内 `#[cfg(test)]`；
协议改动看 `quirel-protocol/tests/wire_compat.rs`。
````

- [ ] **Step 5: quirel-safety**

````markdown
---
name: quirel-safety
description: Quirel 安全红线。触碰 src-tauri/src/credentials.rs、agent/src/auth/（含 path_guard.rs/rate_limiter.rs）、socks5 代理相关代码、新增或修改日志输出时必须先加载本技能。
---

# 安全红线（AGENTS.md §7）

## 改动前
1. 必读 `docs/audits/` 中与触碰模块相关的审计文档——2026-11-01 审计曾发现私钥/passphrase
   明文写入 client.log（P0，e343318），先读懂事故再动手。
2. 确认凭据链路现状：keyring 存 32 字节主密钥 + `credentials/<serverId>.bin`
   AES-256-GCM 信封加密（见 credentials.rs 模块注释）。只许加强不许降级：禁止把
   载荷改回明文落盘，禁止绕过 isKeyringReady 门控。

## 硬性禁令
- 日志（tracing/log/console）禁止输出：密钥、私钥片段、口令、token、证书指纹全文、
  用户主目录等真实路径（需输出时打码/截断）。
- 测试 fixture 禁止使用真实凭据；用显式假数据并在常量名标注 fake。
- 新增日志语句遵循 quirel-rust-idioms 的 tracing 级别惯例。

## 改动后
- `npm run check:sensitive-logs` 必须通过（pre-commit 也会拦）。
- 依赖变更后跑 `cargo audit`（需 `cargo install cargo-audit`；联网操作，非本地门禁）。
````

- [ ] **Step 6: quirel-rust-idioms**

````markdown
---
name: quirel-rust-idioms
description: Quirel Rust 代码惯例。在三个 crate（src-tauri/agent/quirel-protocol）新增或修改错误类型、日志语句、async 任务生命周期（spawn/关停）、状态枚举时加载本技能，保证与既有代码风格一致。
---

# Rust 惯例（与周边代码保持一致）

## 错误处理边界
- 目标边界（现状：agent 实际统一用 anyhow，thiserror 仅在 Cargo.toml 声明未启用；src-tauri 命令以 `Result<_, AppError>` 为主、少数遗留 String。新增代码按下述边界执行，不得延续混用）：
- src-tauri 命令边界沿用 `AppError` + codes 字典契约（`src-tauri/src/errors.rs`：新增只能追加、code 语义一经发布不可变更）
- 库/协议/模块层：thiserror 定义类型化错误；
- main / 命令处理器顶层：anyhow 收敛；
- 禁止在库层新增 anyhow::Result（丢失类型信息）；禁止在顶层新造一次性错误枚举。

## tracing 级别惯例
- error=需人工介入的失败；warn=自动降级/重试可恢复；info=生命周期与关键动作；
  debug=诊断细节。
- 敏感内容任何级别都禁止输出（以 quirel-safety 技能红线为准）。
- 客户端与 quireld 均已有滚动日志（tracing-appender），不要自建文件日志。

## async / tokio 关停（孤儿进程为第九轮 kill-9 实测踩坑）
- 后台任务用 tokio::spawn + `Arc<Notify>` + `tokio::select!` 关停，与 agent/src/worker 既有模式
  一致；退出路径必须可取消（select! on shutdown signal）；若确需 JoinSet/CancellationToken，
  仅 CancellationToken（tokio-util）需新增依赖并取得同意。
- 会话/进程子任务必须随父会话关停（agent 曾修「按 SID 全杀」）。
- 禁止 fire-and-forget 裸 spawn 持有进程句柄。

## 类型建模
- 状态机用 enum（必要时 #[non_exhaustive]）而非裸 String / 魔法数字——让非法状态
  不可表示。
- 协议 wire 类型变更前先加载 quirel-protocol-change 技能。

## 构建环境
- src-tauri、quirel-protocol 在 Windows 原生构建；agent（quireld）在 WSL 内构建
  （rust-toolchain.toml 统一 stable）。验证一律走 `npm run verify:*`（已处理 WSL 分发）。
````

- [ ] **Step 7: 核查与提交**

```bash
ls .agents/skills/*/SKILL.md
git add .agents/skills
git commit -m "feat(core): 新增六个项目级触发式技能"
```
Expected: 列出 6 个 SKILL.md。

---

### Task 8: 终验与交付

- [ ] **Step 1: 全量验证**

Run: `npm run verify:full`
Expected: 摘要 `FAIL 0`（SKIP 仅当无 WSL）；将完整摘要粘贴进交付报告作为测试证据。

- [ ] **Step 2: 输出变更摘要四件套并等待用户决定合并**

按 AGENTS.md §6 四件套输出：变更摘要 / 影响面（含受影响现有测试）/ 测试证据 /
建议 squash 提交信息（建议：`docs(core): 开发治理体系 v2 落地（AGENTS 重构/验证门禁/技能/清整）`）。
**不得自行 push 或合并**；归档重命名（`archive/feature/dev-governance`）属受限操作，需用户届时同意。

---

## 计划自审记录

1. **Spec 覆盖**：spec §4.1 各节 → Task 1；browser-session 文档 → Task 2；§4.2 verify/clippy/契约盘点 → Task 3；§4.4 → Task 1（§7 条款）+ Task 4；§4.5 → Task 1（§8 条款）+ Task 5；§4.6 → Task 6；§4.3 六技能 → Task 7；§6 验收 → Task 8。无遗漏。
2. **占位符扫描**：无 TBD/TODO；所有文件内容完整给出。
3. **一致性**：技能名（6 个）在 AGENTS.md §5/§7 与 Task 7 一致；verify 命令名在 AGENTS.md §6、README、quirel-verify 技能、package.json 四处一致；clippy 严格度决策规则与 spec §4.2 一致。
