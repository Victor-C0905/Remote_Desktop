#!/usr/bin/env node
/**
 * 发版脚本：npm run release <版本号>
 * 流程：校验（main 分支 / 工作区干净 / semver 合法且递增）
 *      → 同步版本文件（三组件 lockstep + package.json）
 *      → 刷新 lock 文件 → 提交 chore(release) → 在该提交上打 tag
 * 规则：脚本不执行 push；版本号唯一写入口即本脚本。
 */
import { execSync } from 'node:child_process';
import { readFileSync, writeFileSync, existsSync } from 'node:fs';

const sh = (cmd, opts = {}) =>
  execSync(cmd, { encoding: 'utf8', stdio: ['pipe', 'pipe', 'pipe'], ...opts }).trim();
const die = (msg) => {
  console.error(`\n✗ 发版中止：${msg}\n`);
  process.exit(1);
};

// ---------- 最小 semver 实现（含预发布排序，避免按字符串比较的 0.3.0-beta > 0.3.0 陷阱） ----------
function parseSemver(v) {
  const m = /^v?(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z][0-9A-Za-z.-]*))?$/.exec(String(v).trim());
  if (!m) return null;
  return {
    raw: m[0].replace(/^v/, ''),
    major: +m[1],
    minor: +m[2],
    patch: +m[3],
    pre: m[4] ? m[4].split('.') : null,
  };
}
const cmpPreId = (a, b) => {
  const na = /^\d+$/.test(a);
  const nb = /^\d+$/.test(b);
  if (na && nb) return Math.sign(Number(a) - Number(b));
  if (na) return -1;
  if (nb) return 1;
  return a < b ? -1 : a > b ? 1 : 0;
};
function cmpSemver(a, b) {
  for (const k of ['major', 'minor', 'patch']) if (a[k] !== b[k]) return Math.sign(a[k] - b[k]);
  if (!a.pre && !b.pre) return 0;
  if (!a.pre) return 1;
  if (!b.pre) return -1;
  const n = Math.max(a.pre.length, b.pre.length);
  for (let i = 0; i < n; i++) {
    if (a.pre[i] === undefined) return -1;
    if (b.pre[i] === undefined) return 1;
    const c = cmpPreId(a.pre[i], b.pre[i]);
    if (c) return c;
  }
  return 0;
}

// ---------- 参数 ----------
const target = parseSemver(process.argv[2]);
if (!target) die('用法：npm run release <版本号>（例：npm run release 0.3.0-beta.1）');

// ---------- 校验 ----------
let branch = '';
try {
  branch = sh('git branch --show-current');
} catch {
  die('无法确定当前 git 分支');
}
if (branch !== 'main') die(`必须在 main 分支执行发版（当前在 ${branch}）`);

let dirty = '';
try {
  dirty = sh('git status --porcelain');
} catch {
  die('无法读取 git 工作区状态');
}
if (dirty) die('工作区不干净，先提交或暂存全部改动：\n' + dirty);

const cargoMain = readFileSync('src-tauri/Cargo.toml', 'utf8');
const current = parseSemver(/^version\s*=\s*"([^"]+)"/m.exec(cargoMain)?.[1] ?? '');
if (!current) die('无法从 src-tauri/Cargo.toml 解析当前版本');
if (cmpSemver(target, current) <= 0)
  die(`新版本 ${target.raw} 必须大于当前版本 ${current.raw}（按 semver 排序，预发布计入）`);

// ---------- 预检报告 ----------
let lastTag = '';
try {
  lastTag = sh('git describe --tags --abbrev=0');
} catch {}
console.log('──────── 预检报告 ────────');
console.log(`分支：${branch}（工作区干净）`);
console.log(`版本：${current.raw} → ${target.raw}`);
console.log(`上个 tag：${lastTag || '（无，此为仓库首个 tag）'}`);
try {
  console.log('自上个 tag 以来的提交：');
  console.log(
    sh(`git log --oneline ${lastTag ? `${lastTag}..HEAD` : '-10'}`)
      .split('\n')
      .map((l) => '  ' + l)
      .join('\n'),
  );
} catch {}
console.log('──────────────────────────');

// ---------- 写入版本（lockstep：三组件 + package.json） ----------
const setCargo = (file) => {
  const s = readFileSync(file, 'utf8');
  if (!/^version\s*=\s*"/m.test(s)) die(`${file} 中找不到 package version 字段`);
  writeFileSync(file, s.replace(/^version\s*=\s*"[^"]+"/m, `version = "${target.raw}"`));
  console.log(`✓ ${file}`);
};
const setPkg = (file) => {
  const s = readFileSync(file, 'utf8');
  if (!/"version"\s*:\s*"/.test(s)) die(`${file} 中找不到 version 字段`);
  writeFileSync(file, s.replace(/"version"\s*:\s*"[^"]+"/, `"version": "${target.raw}"`));
  console.log(`✓ ${file}`);
};
console.log('写入版本号…');
setCargo('src-tauri/Cargo.toml');
setCargo('agent/Cargo.toml');
setCargo('quirel-protocol/Cargo.toml');
setPkg('package.json');

// ---------- 刷新 lock 文件 ----------
console.log('刷新 lock 文件…');
try {
  sh('npm install --package-lock-only');
  console.log('✓ package-lock.json');
} catch (e) {
  die('package-lock.json 刷新失败：' + e.message);
}
const cargoLocks = [
  { dir: 'src-tauri', pkgs: ['quirel', 'quirel-protocol'] },
  { dir: 'agent', pkgs: ['quireld', 'quirel-protocol'] },
];
for (const { dir, pkgs } of cargoLocks) {
  if (!existsSync(`${dir}/Cargo.lock`)) continue;
  const args = pkgs.map((p) => `-p ${p}`).join(' ');
  try {
    sh(`cargo update ${args}`, { cwd: dir });
    console.log(`✓ ${dir}/Cargo.lock`);
  } catch {
    die(`${dir}/Cargo.lock 刷新失败（cargo update ${args}）`);
  }
}

// ---------- 提交 + 打 tag（原子对齐：tag 打在版本提交上） ----------
try {
  sh('git add -A');
  sh(`git commit -m "chore(release): ${target.raw}"`);
  sh(`git tag v${target.raw}`);
} catch (e) {
  die('git 提交/打 tag 失败：' + e.message);
}

console.log('\n──────── 发版完成 ────────');
console.log(`提交：chore(release): ${target.raw}`);
console.log(`tag：v${target.raw}（已打在该发版提交上）`);
console.log('下一步：npm run tauri build（零版本参数，产物自动携带此版本）');
console.log('提醒：push 由你手动执行，脚本不推送。');
