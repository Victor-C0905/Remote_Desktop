#!/usr/bin/env node
/**
 * 开发/测试构建包装：npm run build:dev [-- --dry-run]
 *
 * 非发版构建给产物名注入 -dev.N-g<sha>[-dirty] 后缀，例如：
 *   Quirel_0.3.1-beta.2-dev.3-g1a2b3c4_x64-setup.exe
 *
 * 规则（AGENTS.md §3/§5）：
 * - 版本文件一律不写；后缀经 `tauri build --config <临时文件>` 覆盖注入，
 *   仅影响本次产物命名，不落盘任何版本字段；
 * - HEAD 精确等于 tag → 正式构建，零后缀（等同 npm run tauri build）；
 * - -dev.N = 自上个 tag 起的提交数，g<sha> = 短哈希，工作区脏追加 -dirty。
 */
import { execSync } from 'node:child_process';
import { readFileSync, mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const sh = (cmd, opts = {}) =>
  execSync(cmd, { encoding: 'utf8', stdio: ['pipe', 'pipe', 'pipe'], ...opts }).trim();
const die = (msg) => {
  console.error(`\n✗ 开发构建中止：${msg}\n`);
  process.exit(1);
};

const dryRun = process.argv.includes('--dry-run');

// ---------- 基础版本：Cargo.toml 单一来源（只读，绝不回写） ----------
const cargo = readFileSync('src-tauri/Cargo.toml', 'utf8');
const base = /^version\s*=\s*"([^"]+)"/m.exec(cargo)?.[1];
if (!base) die('无法从 src-tauri/Cargo.toml 解析当前版本');

// ---------- 构建判定：与 build.rs 的 QUIREL_BUILD_INFO 同源（git describe） ----------
let describe = '';
try {
  describe = sh('git describe --tags --dirty --always');
} catch {
  die('git describe 失败：无法确定构建基点');
}

// 全仓库最新发布 tag：用于发版收尾检查。漏做「main 合回 dev」时，
// describe 基点与 Cargo.toml 会一致地过时（基点比对探不到），只有它才能暴露
let latestTag = '';
try {
  latestTag = sh('git for-each-ref --count=1 --sort=-v:refname --format=%(refname:short) refs/tags');
} catch { /* git 异常时跳过该检查 */ }

let version;
const m = /^(.+?)-(\d+)-g([0-9a-f]+)(-dirty)?$/.exec(describe);
if (m) {
  // dev 构建基点在 tag 之后
  const [, tag, n, sha, dirty] = m;
  if (tag.replace(/^v/, '') !== base) {
    console.warn(
      `⚠ describe 基点 ${tag} 与 Cargo.toml 版本 ${base} 不一致（上次发版未合回 dev？），仍以 ${base} 为基准`,
    );
  }
  version = `${base}-dev.${n}-g${sha}${dirty ? '-dirty' : ''}`;
} else if (/^[0-9a-f]+$/.test(describe)) {
  // 仓库尚无任何 tag：仅以短哈希标识
  version = `${base}-dev.0-g${describe}`;
} else {
  // HEAD 精确等于 tag（可能带 -dirty）：正式构建，零后缀
  version = base;
  if (describe.endsWith('-dirty')) {
    console.warn('⚠ 工作区有未提交改动，正式构建产物将混入脏状态');
  }
}

// 发版收尾检查：最新发布与 Cargo.toml 不一致 → 漏做「main 合回 dev」
if (latestTag && latestTag.replace(/^v/, '') !== base) {
  console.warn(
    `⚠ 仓库最新发布 ${latestTag}，Cargo.toml 仍为 ${base}：` +
    '上次发版后未执行发版收尾（main 合回 dev），dev 构建版本将落后于实际发布',
  );
}

console.log('──────── 开发构建预检 ────────');
console.log(`基础版本：${base}`);
console.log(`最新发布：${latestTag || '（仓库尚无 tag）'}`);
console.log(`构建描述：${describe}`);
console.log(`产物版本：${version}${version === base ? '（正式构建，无后缀）' : ''}`);

if (dryRun) process.exit(0);

// ---------- 执行构建 ----------
// 有后缀时经临时配置文件注入（避免跨平台命令行 JSON 转义差异）
const dir = mkdtempSync(join(tmpdir(), 'quirel-build-'));
try {
  let cmd = 'npx tauri build';
  if (version !== base) {
    const cfg = join(dir, 'dev-version.json');
    writeFileSync(cfg, JSON.stringify({ version }));
    cmd += ` --config "${cfg}"`;
  }
  execSync(cmd, { stdio: 'inherit' });
} finally {
  rmSync(dir, { recursive: true, force: true });
}
