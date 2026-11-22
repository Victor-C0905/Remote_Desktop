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
    env: { ...process.env, ...(step.env || {}) },
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
    // 单引号包裹前做 POSIX 转义（C-03）：仓库路径含单引号时命令断裂
    args: ['-e', 'bash', '-lc', `cd '${repo.replace(/'/g, "'\\''")}' && cargo ${cargoSub} --manifest-path agent/Cargo.toml`],
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

// jsdom 依赖链（@csstools/css-calc）以 ESM 形态发布，Node 20 直接 require 会
// ERR_REQUIRE_ESM；--experimental-require-module 开启后 vitest worker 才能启动。
// Node >=22.12 默认支持 require(ESM)，重复传该 flag 也无副作用，但按版本判断更干净。
function nodeRequireModuleEnv() {
  const [major, minor] = process.versions.node.split('.').map(Number);
  if (major > 22 || (major === 22 && minor >= 12)) return {};
  const merged = [process.env.NODE_OPTIONS, '--experimental-require-module'].filter(Boolean).join(' ');
  return { NODE_OPTIONS: merged };
}

const vitest = { label: 'vitest（前端单测）', cmdStr: 'npm run test:run', env: nodeRequireModuleEnv() };
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
