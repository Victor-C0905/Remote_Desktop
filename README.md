# Quirel

远程 Linux 服务器控制客户端——本地桌面应用 + 服务器守护进程 + 共享协议。

| 组件 | 目录 | 说明 |
|---|---|---|
| `quirel` | `src-tauri/`（前端 `src/`） | Windows 桌面客户端（Tauri 2 + React + TypeScript） |
| `quireld` | `agent/` | 远程 Linux 守护进程（QUIC，systemd 部署，WSL 内构建） |
| `quirel-protocol` | `quirel-protocol/` | 双端共享线上协议 crate（协议单一真相源） |

三组件统一版本号（lockstep）。开发规范与红线总表见 [AGENTS.md](AGENTS.md)。

## 开发环境

- Node.js ≥ 20.19（Vite 7 引擎要求）；Rust stable（`rust-toolchain.toml` 统一，Windows 与 WSL 共用）
- Windows 客户端原生开发；`agent`（quireld）在 WSL 内构建测试
- `cargo-sweep`（构建缓存清理纪律，AGENTS.md §9）；`cargo-audit`（依赖公告扫描，AGENTS.md §7）

## 常用命令

| 命令 | 用途 |
|---|---|
| `npm run dev` / `npm run tauri dev` | 前端开发 / 客户端联调 |
| `npm run test` / `npm run test:run` | vitest 监听 / 单次（前端单测） |
| `npm run verify` | 快速验证（敏感日志 + lockstep + vitest + cargo check ×3） |
| `npm run verify:fe` / `npm run verify:rust` / `npm run verify:full` | 分层验证（AGENTS.md §6） |
| `npm run build:dev` | 非发版安装包（`<版本>-dev.N-g<sha>` 命名） |
| `npm run release <版本号>` | 发版（仅 main 分支，须用户指示，AGENTS.md §4） |

## 文档索引

- [AGENTS.md](AGENTS.md) — 开发规范与红线总表（**AI 工具入口：从本文件读起**）
- [docs/window-flash-rules.md](docs/window-flash-rules.md) — 窗口坏帧红线与探针回归
- [docs/browser-session-rules.md](docs/browser-session-rules.md) — 浏览器会话所有权红线
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — 架构深入解析（组件关系与核心机制最全入口）
- [docs/DEPLOYMENT.md](docs/DEPLOYMENT.md) — quireld 部署（systemd）
- [docs/IPC_PROTOCOL.md](docs/IPC_PROTOCOL.md) — 线上协议规范
- [docs/data-storage.md](docs/data-storage.md) — 数据存储与清除策略
- [docs/audits/](docs/audits/) — 安全与 bug 审计报告
- `.agents/skills/` — 项目级触发式技能（红线 / 验证 / 协议变更 / 安全 / Rust 惯例）

## 发布契约

跨项目（Quirel 桌面端 / quireld / quirel_web）的发布职责、接口契约、分阶段计划与红线，以
[docs/superpowers/plans/2026-11-24-cross-project-release-automation-plan.md](docs/superpowers/plans/2026-11-24-cross-project-release-automation-plan.md)
（RELEASE-CONTRACT v1）为唯一权威版本。

- 谁该读：发版负责人、维护发布脚本与 CI 的人、修改 quirel_web 发布接口或制品目录的人。
- 契约变更（端点、字段、错误码、制品命名与目录、时序、凭据作用域）必须回到该文档修改并升版，再同步 quirel_web 副本。
