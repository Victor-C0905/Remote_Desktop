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
