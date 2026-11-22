/**
 * publish-quireld.mjs - quireld 发布产物（官网静态目录 + GitHub Releases）
 *
 * 用法:
 *   node scripts/publish-quireld.mjs [--dry-run] [--github] [--out <目录>]
 *   （package.json: npm run publish:quireld）
 *
 * 职责:
 *   1. 读取 agent/Cargo.toml 版本号（lockstep，唯一来源）
 *   2. 校验 git tag v{version} 已存在（发版纪律：版本由 npm run release 锁定）
 *   3. 校验 dist/ 产物存在（build.sh release amd64 + arm64 的产出）
 *   4. 生成官网静态目录（默认: agent/dist/publish），布局与客户端契约一致:
 *        {out}/latest.json                                 最新版本（更新检查）
 *        {out}/install-quireld.sh                          curl|bash 引导脚本
 *        {out}/SHA256SUMS                                  稳定别名 + 版本名的完整清单
 *        {out}/quireld-linux-{arch}.tar.gz                 稳定别名（引导脚本用）
 *        {out}/{version}/quireld-{version}-release-linux-{arch}.tar.gz
 *        {out}/{version}/SHA256SUMS                        客户端按版本精确匹配
 *   5. 可选 --github: 通过 gh CLI 上传同一批资产到 GitHub Releases（兜底源）
 *
 * 部署（生成目录后由用户上传到 Nginx 静态源根，顺序敏感）:
 *   1) 版本目录 {version}/  2) 根文件（排除版本目录与 latest.json）
 *   3) latest.json 最后（更新检查开关，避免客户端在产物就绪前发现新版本）
 *   具体 rsync 命令见脚本末尾的部署提示
 *
 * 版本号零参数原则: 本脚本不接收版本参数，只读版本、只传产物。
 */

import { execSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

// ── 常量 ──────────────────────────────────────────────────
const REPO = process.env.QUIREL_PUBLISH_REPO || "Victor-C0905/Remote_Desktop";
const ROOT = path.resolve(fileURLToPath(new URL("..", import.meta.url)));
const AGENT_DIR = path.join(ROOT, "agent");
const DIST_DIR = path.join(AGENT_DIR, "dist");
const BOOTSTRAP_SCRIPT = path.join(AGENT_DIR, "deploy", "install-quireld.sh");

const ARCHES = ["amd64", "arm64"]; // 与 build.sh 架构命名映射一致

// ── 工具 ──────────────────────────────────────────────────
const log = (msg) => console.log(`[publish] ${msg}`);
const die = (msg) => {
  console.error(`[publish] 错误: ${msg}`);
  process.exit(1);
};

function sh(cmd, opts = {}) {
  return execSync(cmd, { encoding: "utf8", stdio: opts.quiet ? ["ignore", "pipe", "pipe"] : "inherit", ...opts }).trim();
}

function sha256File(file) {
  return createHash("sha256").update(fs.readFileSync(file)).digest("hex");
}

// ── 参数 ──────────────────────────────────────────────────
const argv = process.argv.slice(2);
const DRY_RUN = argv.includes("--dry-run");
const UPLOAD_GITHUB = argv.includes("--github");
if (argv.some((a) => /^--version/.test(a) || /^\d+\.\d+/.test(a))) {
  die("本脚本不接收版本参数：版本号由 npm run release 在 main 上锁定（发版纪律）");
}
let PUBLISH_DIR = path.join(DIST_DIR, "publish");
if (argv.includes("--out")) {
  const out = argv[argv.indexOf("--out") + 1];
  if (!out) die("--out 缺少目录参数");
  PUBLISH_DIR = path.resolve(out);
  // 删除目标限制在 agent/dist 下（C-02）：--out 会触发 rmSync 递归删除，
  // 任意目录即脚枪；误传 ~ 或 / 的代价不可接受
  const distRoot = path.resolve(DIST_DIR);
  if (PUBLISH_DIR !== distRoot && !PUBLISH_DIR.startsWith(distRoot + path.sep)) {
    die(`--out 必须位于 agent/dist 下（当前: ${PUBLISH_DIR}）`);
  }
}

// ── 1. 读取版本 ───────────────────────────────────────────
const cargoToml = fs.readFileSync(path.join(AGENT_DIR, "Cargo.toml"), "utf8");
const versionMatch = cargoToml.match(/^version\s*=\s*"([^"]+)"/m);
if (!versionMatch) die("无法从 agent/Cargo.toml 读取版本号");
const VERSION = versionMatch[1];
// semver 复验（C-02）：版本串将插入 gh/shell 命令，Cargo.toml 被手改时
// 不得注入
if (!/^d+.d+.d+(-[0-9A-Za-z.-]+)?$/.test(VERSION)) {
  die(`版本号非合法 semver: ${VERSION}`);
}
log(`版本: ${VERSION}`);

// ── 2. 校验 git tag ───────────────────────────────────────
const tag = `v${VERSION}`;
let tagExists = false;
try {
  tagExists = sh(`git rev-parse -q --verify "refs/tags/${tag}"`, { quiet: true }) !== "";
} catch {
  /* tag 不存在 */
}
if (!tagExists) {
  die(`tag ${tag} 不存在——请先在 main 上执行 npm run release 完成发版`);
}
log(`tag 已确认: ${tag}`);

// ── 3. 校验产物 ───────────────────────────────────────────
const tarballs = ARCHES.map((arch) => {
  // build.sh 产物命名: quireld-{ver}-{env}-linux-{arch}.tar.gz（env=release）
  const name = `quireld-${VERSION}-release-linux-${arch}.tar.gz`;
  const file = path.join(DIST_DIR, name);
  if (!fs.existsSync(file)) die(`缺少产物: ${name}（先在 WSL 内执行 bash agent/build.sh release ${arch}）`);
  return { arch, name, file };
});
log(`产物校验通过: ${tarballs.map((t) => t.name).join(", ")}`);

// ── 4. 计算稳定别名与校验清单 ─────────────────────────────
const stableAssets = tarballs.map((t) => {
  // 稳定别名: quireld-linux-{arch}.tar.gz（引导脚本按固定名取最新）
  const stableName = `quireld-linux-${t.arch}.tar.gz`;
  const stableFile = path.join(DIST_DIR, stableName);
  if (!DRY_RUN) fs.copyFileSync(t.file, stableFile);
  return { ...t, stableName, stableFile };
});

// SHA256SUMS 同时含版本名与稳定别名条目：
// - 客户端自动安装/更新按版本名精确匹配（src-tauri provision.rs fetch_and_extract）
// - install-quireld.sh 引导脚本按稳定名校验（grep 稳定名行）
const shaLines = [
  ...tarballs.map((t) => `${sha256File(t.file)}  ${t.name}`),
  ...stableAssets.map((a) => `${sha256File(a.file)}  ${a.stableName}`),
];
const shaFile = path.join(DIST_DIR, "SHA256SUMS");

// 官网静态源根地址（与客户端 DEFAULT_RELEASE_BASE 一致）
// 两个域名指向同一个 quirel_web 应用，产物只需上传一份；
// 校验时两个域名都要能取到（客户端按主源 → 备用源顺序尝试）
const SITE_BASE = "https://quireld.cn/downloads/quireld";
const SITE_BASE_ALIASES = ["https://quireld.icu/downloads/quireld"];

// ── 5. 生成官网静态目录 ───────────────────────────────────
const versionDir = path.join(PUBLISH_DIR, VERSION);
const planned = [
  path.join(PUBLISH_DIR, "latest.json"),
  path.join(PUBLISH_DIR, "install-quireld.sh"),
  path.join(PUBLISH_DIR, "SHA256SUMS"),
  ...stableAssets.map((a) => path.join(PUBLISH_DIR, a.stableName)),
  path.join(versionDir, "SHA256SUMS"),
  ...tarballs.map((t) => path.join(versionDir, t.name)),
];

if (DRY_RUN) {
  log("── dry-run 计划（未写入任何文件）──");
  log(`官网静态目录: ${PUBLISH_DIR}`);
  for (const f of planned) log(`  生成: ${path.relative(PUBLISH_DIR, f)}`);
  if (UPLOAD_GITHUB) {
    log("── GitHub 上传计划 ──");
    log(`仓库: ${REPO}`);
    log(`release: ${tag}（已存在则仅上传资产）`);
    for (const a of [...tarballs, ...stableAssets]) log(`  上传: ${a.name} / ${a.stableName}`);
    log("  上传: SHA256SUMS / install-quireld.sh");
  }
  log("── 结束（未执行任何变更）──");
  process.exit(0);
}

fs.rmSync(PUBLISH_DIR, { recursive: true, force: true });
fs.mkdirSync(versionDir, { recursive: true });

for (const t of tarballs) {
  fs.copyFileSync(t.file, path.join(versionDir, t.name));
}
for (const a of stableAssets) {
  fs.copyFileSync(a.file, path.join(PUBLISH_DIR, a.stableName));
}
fs.copyFileSync(BOOTSTRAP_SCRIPT, path.join(PUBLISH_DIR, "install-quireld.sh"));

// 三处清单内容一致（含版本名 + 稳定别名）；消费方都按文件名精确匹配
const shaContent = shaLines.join("\n") + "\n";
for (const file of [shaFile, path.join(versionDir, "SHA256SUMS"), path.join(PUBLISH_DIR, "SHA256SUMS")]) {
  fs.writeFileSync(file, shaContent);
}

fs.writeFileSync(
  path.join(PUBLISH_DIR, "latest.json"),
  JSON.stringify({ version: VERSION, tag, updated: new Date().toISOString().slice(0, 10) }, null, 2) + "\n",
);

log(`官网静态目录已生成: ${PUBLISH_DIR}`);
for (const f of planned) log(`  ${path.relative(PUBLISH_DIR, f)}`);

// ── 6. 可选：上传 GitHub Releases（兜底源）────────────────
if (UPLOAD_GITHUB) {
  let ghAvailable = false;
  try {
    sh("gh --version", { quiet: true });
    ghAvailable = true;
  } catch {
    /* gh 不可用 */
  }
  if (!ghAvailable) {
    die("未找到 gh CLI（https://cli.github.com）");
  }

  const gh = (cmd) => sh(`gh ${cmd} --repo ${REPO}`, { quiet: true });
  const releaseExists = (() => {
    try {
      gh(`release view ${tag}`);
      return true;
    } catch {
      return false;
    }
  })();

  const uploadAssets = [
    ...tarballs.map((t) => ({ file: t.file, label: t.name })),
    ...stableAssets.map((a) => ({ file: a.stableFile, label: a.stableName })),
    { file: shaFile, label: "SHA256SUMS" },
    { file: BOOTSTRAP_SCRIPT, label: "install-quireld.sh" },
  ];
  const assetArgs = uploadAssets.map((a) => `"${a.file}"#${a.label}`).join(" ");

  if (releaseExists) {
    log(`release ${tag} 已存在，上传资产...`);
    gh(`release upload ${tag} --clobber ${assetArgs}`);
  } else {
    log(`创建 release ${tag} ...`);
    gh(
      `release create ${tag} --title "Quireld ${VERSION}" ` +
        `--notes "quireld 服务端产物（amd64/arm64）。安装: 见 README 或客户端自动安装。" ` +
        assetArgs,
    );
  }
  log(`GitHub 发布完成: https://github.com/${REPO}/releases/tag/${tag}`);
}

// ── 7. 部署提示 ───────────────────────────────────────────
echoDeployHint();

function echoDeployHint() {
  const rel = path.relative(ROOT, PUBLISH_DIR);
  // quirel_web 的数据目录：/downloads/** 直接映射 data/downloads/**
  const remote = "<user>@<官网服务器>:/opt/quirel-web/data/downloads/quireld";
  console.log("");
  log("下一步：把静态目录上传到官网（quirel_web 的 data/downloads/quireld）");
  log("上传顺序敏感：先版本目录 → 再根文件 → latest.json 必须最后。");
  log("latest.json 是更新检查的开关，先传它会让客户端在产物就绪前发现新版本。");
  log(`  1) 版本目录（tarball + SHA256SUMS，客户端按版本精确取包）:`);
  log(`     rsync -av ${rel}/${VERSION}/ ${remote}/${VERSION}/`);
  log(`  2) 根文件（稳定别名 / 引导脚本 / 根清单）:`);
  log(`     rsync -av --exclude '${VERSION}' --exclude 'latest.json' ${rel}/ ${remote}/`);
  log(`  3) 最后发布 latest.json（更新检查生效）:`);
  log(`     rsync -av ${rel}/latest.json ${remote}/latest.json`);
  log(`官网主源: ${SITE_BASE}/{version}/...`);
  log(`官网备源: ${SITE_BASE_ALIASES.join(", ")}/{version}/...`);
  log(`验证（两个域名都要通过）:`);
  for (const base of [SITE_BASE, ...SITE_BASE_ALIASES]) {
    log(`  curl -fsSL ${base}/${VERSION}/SHA256SUMS`);
    log(`  curl -fsSL ${base}/latest.json`);
  }
}
