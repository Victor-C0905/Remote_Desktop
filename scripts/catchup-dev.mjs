#!/usr/bin/env node
/**
 * dev 欠账收尾：node scripts/catchup-dev.mjs <任务分支> "<squash 合并提交信息>"
 *
 * 一次性处理两类欠账（正常流程由 release 脚本与用户 squash 覆盖，不经本脚本）：
 *   1. main 领先 dev 的发版收尾欠账 → `git fetch . main:dev` 快进合回（仅快进，分叉即中止）；
 *   2. 任务分支未 squash 合并 → 在 dev 上 merge --squash 并按规范信息提交，分支改名 archive/<原名>。
 *
 * 安全边界：全程不做 push；dev 与 main 分叉、工作区不干净、归档分支重名时立即中止；
 * 快进与 squash 均可通过 reflog/reset 回退。结束后停在 dev。
 */
import { execSync } from 'node:child_process';

const sh = (cmd, opts = {}) =>
  execSync(cmd, { encoding: 'utf8', stdio: ['pipe', 'pipe', 'pipe'], ...opts }).trim();
const ok = (cmd) => {
  try { execSync(cmd, { stdio: ['pipe', 'pipe', 'pipe'] }); return true; } catch { return false; }
};
const die = (msg) => {
  console.error(`\n✗ 收尾中止：${msg}\n`);
  process.exit(1);
};

const [branch, message] = process.argv.slice(2);
if (!branch || !message) die('用法：node scripts/catchup-dev.mjs <任务分支> "<squash 合并提交信息>"');
if (!/^[\w./-]+$/.test(branch)) die(`分支名不合法：${branch}`);
if (branch === 'dev' || branch === 'main') die('不能对 dev / main 执行 squash 合并');

// ---------- 校验 ----------
if (sh('git status --porcelain')) die('工作区不干净，先提交或暂存全部改动');
if (!ok(`git rev-parse --verify --quiet refs/heads/${branch}`)) die(`分支不存在：${branch}`);
if (ok(`git rev-parse --verify -q refs/heads/archive/${branch}`)) die(`归档分支已存在：archive/${branch}`);
if (!ok('git merge-base --is-ancestor dev main'))
  die('dev 与 main 已分叉，无法快进合回。请手动执行：git checkout dev && git merge main');

const ahead = Number(sh(`git rev-list --count dev..${branch}`));
const devBefore = sh('git rev-parse --short dev');

console.log('──────── 收尾预检 ────────');
console.log(`任务分支：${branch}（领先 dev ${ahead} 个提交）`);
console.log(`dev 现位：${devBefore} → 将快进至 main（${sh('git rev-parse --short main')}）`);

// ---------- 1. 发版欠账：main 快进合回 dev ----------
sh('git fetch . main:dev');
console.log(`✓ main 已快进合回 dev（${devBefore} → ${sh('git rev-parse --short dev')}），tag 与版本提交回流`);

// ---------- 2. 任务分支 squash 合并进 dev ----------
sh('git checkout dev');
if (ahead > 0) {
  try {
    sh(`git merge --squash ${branch}`);
  } catch {
    try { sh('git merge --abort'); } catch { try { sh('git reset --hard'); } catch { /* 保持现场供排查 */ } }
    die(`squash 合并 ${branch} 产生冲突，已尝试回退现场，请手动处理`);
  }
  sh('git commit -m ' + JSON.stringify(message));
  console.log(`✓ ${branch} 已 squash 合并进 dev（${sh('git rev-parse --short dev')}）`);
} else {
  console.log(`－ ${branch} 无领先提交，跳过 squash 合并`);
}

// ---------- 3. 任务分支改名归档 ----------
sh(`git branch -m ${branch} archive/${branch}`);
console.log(`✓ 分支已归档：archive/${branch}`);

console.log('\n──────── 收尾完成 ────────');
console.log(`dev 最新：${sh('git log --oneline -1')}`);
console.log(`构建基点：${sh('git describe --tags --dirty')}`);
console.log('提醒：push 由你手动执行，脚本不推送。');
