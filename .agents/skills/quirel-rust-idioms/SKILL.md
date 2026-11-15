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
