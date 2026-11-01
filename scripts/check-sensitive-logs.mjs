#!/usr/bin/env node
/**
 * 敏感日志守门员：扫描源码中可能把密钥/口令内容写进日志的语句。
 *
 * 背景（2026-11-01 审计）：私钥前缀、passphrase、连接 token 曾以 INFO 级
 * 落盘 client.log 两个月（累计 2000+ 处）。本脚本三道规则拦截再现：
 *   R1 Rust 日志宏同时含敏感标识与格式化占位符（内容插值）
 *   R2 认证上下文的 {:?} Payload dump（AuthPasswordRequest 携带明文密码）
 *   R3 TS 日志语句（含跨行对象字面量）携带敏感属性插值
 * 允许清单：长度类元信息（len()/长度/字节）、存在性（存在/Exists/is_some）、
 * 算法名（algorithm/算法）、结果流（成功/失败）、type_name()/discriminant()。
 *
 * 用法：
 *   node scripts/check-sensitive-logs.mjs            # 全量扫描，退出码 0/1
 *   node scripts/check-sensitive-logs.mjs --self-test  # 内置夹具自测
 */
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

const RUST_ROOTS = ["src-tauri/src", "agent/src", "quirel-protocol/src"];
const TS_ROOTS = ["src"];
const TS_EXT = new Set([".ts", ".tsx"]);

const SENSITIVE_ARG =
  /\b(password|passphrase|private[_-]?key|privkey|secret|master[_-]?key|key_data|pwd)\s*[),.]/i;
const PLACEHOLDER = /\{(?::[^}]*)?\}/;
const SAFE_ARG = /(len\(\)|is_some\(\)|is_ok\(\))/;
const LOG_MACRO = /\b(info|debug|warn|error|trace|println)!\s*\(/;
const TS_LOG = /(console\.(log|warn|error|info)|\blog\.(info|warn|error|debug))\s*\(/;
const AUTH_PAYLOAD_DUMP = /收到: *\{:\?\}"?\s*,\s*(other|payload|envelope)\b/i;
const TS_SENSITIVE_PROP = /\b(password|privateKey|passphrase|secret|token)\s*[:=]/i;
const TS_ALLOW = /(Exists|!!|\bnull\b|\bundefined\b|\bfalse\b)/i;

function* walk(dir) {
  let entries;
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch {
    return;
  }
  for (const e of entries) {
    const p = join(dir, e.name);
    if (e.isDirectory()) yield* walk(p);
    else yield p;
  }
}

/** 单行/块规则判定，findings 收集 [规则, 位置, 摘要]
 *
 * R1：日志宏的参数区出现「裸敏感标识符」直接插值（private_key.chars()、
 *     passphrase 等）——长度/存在性等元信息形态（private_key.len()、
 *     passphrase.is_some()）放行。消息里出现「密码/密钥」字样但参数仅为
 *     remote/username/enable 开关的（收到密码认证请求、认证器已初始化）不匹配。
 * R2：认证相位「收到: {:?}, other」形态的 Payload 全量 dump（收紧到该形态，
 *     避免误伤 [PubKeyAuth] 的 IO 错误日志）；type_name/discriminant 放行。
 */
function scanRustLine(path, line, i, findings) {
  if (!LOG_MACRO.test(line)) return;
  if (SENSITIVE_ARG.test(line) && PLACEHOLDER.test(line) && !SAFE_ARG.test(line)) {
    findings.push([`[R1] ${path}:${i + 1}`, line.trim().slice(0, 150)]);
    return;
  }
  if (AUTH_PAYLOAD_DUMP.test(line) && !/type_name|discriminant/.test(line)) {
    findings.push([`[R2] ${path}:${i + 1}`, line.trim().slice(0, 150)]);
  }
}

/** TS 跨行日志块：log(/console( 起，括号配平或 15 行封顶 */
function* logBlocks(lines) {
  for (let i = 0; i < lines.length; i++) {
    if (!TS_LOG.test(lines[i])) continue;
    let depth = 0;
    const start = i;
    for (let j = i; j < Math.min(i + 15, lines.length); j++) {
      for (const ch of lines[j]) {
        if (ch === "(") depth++;
        else if (ch === ")") depth--;
      }
      if (depth <= 0) {
        yield { first: start, last: j, text: lines.slice(start, j + 1).join("\n") };
        i = j;
        break;
      }
    }
  }
}

function scanTsFile(path, text, findings) {
  if (path.includes(`${join("src", "i18n")}`)) return; // 文案翻译非敏感
  const lines = text.split(/\r?\n/);
  for (const block of logBlocks(lines)) {
    if (TS_SENSITIVE_PROP.test(block.text) && !TS_ALLOW.test(block.text)) {
      findings.push([
        `[R3] ${path}:${block.first + 1}-${block.last + 1}`,
        block.text.replace(/\s+/g, " ").slice(0, 150),
      ]);
    }
  }
}

function scanTree(findings) {
  for (const root of RUST_ROOTS) {
    for (const f of walk(root)) {
      if (!f.endsWith(".rs")) continue;
      const lines = readFileSync(f, "utf8").split(/\r?\n/);
      lines.forEach((line, i) => scanRustLine(f, line, i, findings));
    }
  }
  for (const root of TS_ROOTS) {
    for (const f of walk(root)) {
      if (!TS_EXT.has(f.slice(f.lastIndexOf(".")))) continue;
      if (/\.test\.|\/test\//.test(f.replace(/\\/g, "/"))) continue;
      scanTsFile(f, readFileSync(f, "utf8"), findings);
    }
  }
}

/* ── 自测夹具：2026-11-01 四类真实泄漏样本（必须命中）+ 安全样本（必须放行）── */

const FIXTURE_MUST_FLAG = [
  // R1: 私钥前缀内容插值（历史泄漏，connection.rs 原文）
  `tracing::info!("[PubKeyAuth] 私钥前150字符: {}", &private_key.chars().take(150).collect::<String>());`,
  `tracing::info!("[KeyParser] 私钥前100字符: {}", &key_data.chars().take(100).collect::<String>());`,
  // R1: passphrase 内容 dump（历史泄漏）
  `tracing::info!("[KeyParser] 密码字段: {:?}", passphrase);`,
  // R2: 认证 Payload 全量 dump（AuthPasswordRequest 携带明文密码）
  `tracing::warn!("期望认证请求,收到: {:?}", other);`,
];
const FIXTURE_MUST_PASS = [
  `tracing::info!("[PubKeyAuth] 私钥原始长度={}字节", private_key.len());`,
  `tracing::info!("[KeyParser] 密码字段存在: {}", passphrase.is_some());`,
  `tracing::warn!("期望公钥认证响应，收到: {}", other.type_name());`,
  `tracing::warn!("未处理的消息类型: {:?}", std::mem::discriminant(other));`,
  `tracing::info!("[PubKeyAuth] 使用私钥签名挑战...");`,
  `tracing::info!("收到密码认证请求: remote={}, username={}", remote, username);`,
  `tracing::info!("认证器已初始化 (公钥认证: {}, 密码认证: {})", enable_pubkey, enable_password);`,
];

function scanFixture(lines) {
  const findings = [];
  lines.forEach((line, i) => scanRustLine("fixture.rs", line, i, findings));
  return findings;
}

function selfTest() {
  let fail = 0;
  const flagged = scanFixture(FIXTURE_MUST_FLAG);
  for (const fixture of FIXTURE_MUST_FLAG) {
    if (!flagged.some(([, text]) => text === fixture.trim())) {
      console.error(`[self-test] 漏报（必须命中却未命中）: ${fixture.slice(0, 90)}`);
      fail++;
    }
  }
  const passedTexts = new Set(flagged.map(([, text]) => text));
  for (const fixture of FIXTURE_MUST_PASS) {
    if (passedTexts.has(fixture.trim())) {
      console.error(`[self-test] 误报（安全样本被命中）: ${fixture.slice(0, 90)}`);
      fail++;
    }
  }
  if (flagged.length > FIXTURE_MUST_FLAG.length) {
    console.error(`[self-test] 安全样本被误报: 命中 ${flagged.length}，应命中 ${FIXTURE_MUST_FLAG.length}`);
    fail++;
  }
  console.log(
    fail === 0
      ? `[self-test] 通过: 泄漏样本 ${FIXTURE_MUST_FLAG.length}/${FIXTURE_MUST_FLAG.length} 命中，安全样本零误报`
      : `[self-test] 失败: ${fail} 项`,
  );
  return fail;
}

/* ── 入口 ── */

if (process.argv.includes("--self-test")) {
  process.exit(selfTest() === 0 ? 0 : 1);
}

const findings = [];
scanTree(findings);
if (findings.length > 0) {
  console.error(`[check-sensitive-logs] 发现 ${findings.length} 处疑似敏感日志：`);
  for (const [loc, text] of findings) console.error(`  ${loc}\n    ${text}`);
  console.error("如属误报，请改写为元信息（长度/存在性/类型名）或补充 ALLOW 清单。");
  process.exit(1);
}
console.log("[check-sensitive-logs] 通过: 未发现敏感日志语句");
