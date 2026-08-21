# 版本系统设计

> 日期: 2026-08-21
> 状态: 设计稿(待实现)
> 关联文件: agent/Cargo.toml, src-tauri/Cargo.toml, src-tauri/tauri.conf.json, package.json, agent/src/main.rs, agent/src/protocol/serde.rs, src-tauri/build.rs, src-tauri/src/lib.rs, src/apps/Settings.tsx, src/config/version.ts(新增), scripts/release.ps1(新增)

## 1. 背景与现状

项目当前有 4 处独立的版本号声明,全部硬编码为 `0.1.0`,彼此互不同步:

| 位置 | 用途 | 是否被运行时使用 |
|---|---|---|
| [agent/Cargo.toml](file:///e:/MyWork/quirel/agent/Cargo.toml#L3) `version = "0.1.0"` | Agent 包版本 | 否(env! 未被引用) |
| [src-tauri/Cargo.toml](file:///e:/MyWork/quirel/src-tauri/Cargo.toml#L3) `version = "0.1.0"` | Client Rust crate 版本 | 否 |
| [src-tauri/tauri.conf.json](file:///e:/MyWork/quirel/src-tauri/tauri.conf.json#L4) `"version": "0.1.0"` | Tauri 打包元数据 | 仅打包元数据,运行时未读取 |
| [package.json](file:///e:/MyWork/quirel/package.json#L4) `"version": "0.1.0"` | npm 元数据 | 否 |

**Agent 侧完全没有版本暴露:**
- [agent/src/main.rs](file:///e:/MyWork/quirel/agent/src/main.rs#L13-L15) 的 clap 定义只有 `#[command(name, about)]`,没有 `version`,因此 `agent --version` / `agent -V` 不可用。
- 启动时不打印版本号。
- [agent/src/protocol/serde.rs](file:///e:/MyWork/quirel/agent/src/protocol/serde.rs) 的 `Payload` enum(Client↔Agent 协议)不携带版本信息,Client 连接后无法得知所连 Agent 的版本。

**Client 侧版本硬编码:**
- [src/apps/Settings.tsx](file:///e:/MyWork/quirel/src/apps/Settings.tsx#L1032) 关于页写死 `版本 0.1.0`,不从任何动态源读取。

**无版本号修改纪律:** 修复 bug 或新增功能后,没有标准流程同步更新版本号,导致部署后无法从版本号判断当前运行的代码状态。

**协议层结构澄清:** `protocol/agent.proto` 是 **Manager↔Worker 内部 IPC 协议**(Protobuf),与 Client↔Agent 无关。Client↔Agent 使用 [agent/src/protocol/serde.rs](file:///e:/MyWork/quirel/agent/src/protocol/serde.rs) 中基于 serde JSON 的 `Payload` enum,认证响应为 `AuthResponse { success, error, session_id }`。本设计的版本信息交换在此 enum 内完成。

## 2. 目标

1. **版本号修改纪律:** 修复 bug 升 PATCH、新增功能升 MINOR、协议不兼容升 MAJOR,有标准流程同步修改版本号(脚本辅助)。
2. **单一真相源:** Agent 与 Client 各自以 Cargo.toml 的 `version` 字段为唯一真相源,其他配置(tauri.conf.json、package.json、前端 version.ts)由构建脚本自动同步,杜绝手工多处维护。
3. **Agent 版本暴露:** `agent --version` 命令查看版本号;启动日志打印版本号;协议层向 Client 传递版本号。
4. **Client 版本暴露:** 关于页动态显示客户端版本号(从构建时生成的常量读取,非硬编码)。
5. **Agent 版本透传:** Client 连接 Agent 认证成功后,显示所连 Agent 的版本号。
6. **独立版本号:** Agent 与 Client 版本号独立维护,符合实际发布节奏(Agent 通过 systemd 热更新,Client 通过 MSI/NSIS 安装包发布)。

## 3. 非目标(YAGNI)

- **不**引入 git commit hash / git describe / dirty 构建元数据——版本号采用标准 SemVer 三段制即可,溯源通过 git log + 版本号对应。
- **不**引入 `--build-info`、systemd StatusText、完整 BuildInfo 结构——只需 `--version` 输出标准版本号。
- **不**引入预发布通道(dev/beta/rc)、`cargo-release`、changeset——本项目不发布到 crates.io。
- **不**强制协议版本协商拒绝连接——仅做版本号显示,不阻断。
- **不**新建 ServerHello 握手消息——版本信息并入现有 `AuthResponse` 字段,零额外往返。
- **不**修改 `protocol/agent.proto`(Manager↔Worker 内部协议)。
- **不**自动 commit、push、打 git tag——遵守用户 git 规则,脚本只做本地修改与提示,由用户手动执行 git 操作。

## 4. 设计决策

### 4.1 真相源:各自 Cargo.toml

- **Agent 真相源:** [agent/Cargo.toml](file:///e:/MyWork/quirel/agent/Cargo.toml#L3) 的 `version` 字段。
- **Client 真相源:** [src-tauri/Cargo.toml](file:///e:/MyWork/quirel/src-tauri/Cargo.toml#L3) 的 `version` 字段。

**理由:** Rust 生态惯例,`env!("CARGO_PKG_VERSION")` 编译时直接注入,零额外文件、零运行时读取开销。`build.rs` 运行时通过 `std::env::var("CARGO_PKG_VERSION")` 读取同一来源用于同步其他配置。

### 4.2 版本号规范(SemVer 2.0.0 + 修改纪律)

#### 4.2.1 版本号格式

标准 SemVer:`MAJOR.MINOR.PATCH`(如 `0.2.0`),无附加构建元数据。

#### 4.2.2 何时升版本号(修改纪律)

| 变更类型 | 升哪一段 | 示例 |
|---|---|---|
| 修复 bug(向下兼容) | PATCH +1 | `0.2.0` → `0.2.1` |
| 新增功能(向下兼容) | MINOR +1,PATCH 归 0 | `0.2.1` → `0.3.0` |
| 协议不兼容 breaking change | MAJOR +1,MINOR/PATCH 归 0 | `0.3.0` → `1.0.0` |

**触发时机:** 每次修复 bug 或新增功能后,用 `scripts/release.ps1` 同步修改版本号(见 4.5)。改完版本号 → build.rs 自动同步派生文件 → 提交 → 构建 → 部署。部署后通过 `agent --version` 或 Client 关于页即可看到新版本号,确认部署了对应改动。

#### 4.2.3 同步链路

修改 Cargo.toml 的 version 后,`cargo build` / `tauri dev` 触发 build.rs 自动同步:
- Agent: Cargo.toml → `env!("CARGO_PKG_VERSION")` 直接可用(无需 build.rs 中转,build.rs 仅做 proto rerun)
- Client: Cargo.toml → build.rs 同步到 tauri.conf.json version、package.json version、生成 `src/config/version.ts`

### 4.3 Agent 版本暴露

**命令行参数:** [agent/src/main.rs](file:///e:/MyWork/quirel/agent/src/main.rs#L13-L15) 的 clap derive 增加 `version`:

```rust
#[derive(Parser, Debug)]
#[command(name = "quireld")]
#[command(version, about = "Quirel Control — 远程 Agent 服务端")]
struct Args { /* ... */ }
```

`#[command(version)]` 自动从 `CARGO_PKG_VERSION` 读取并生成 `--version` / `-V` 参数。执行 `agent --version` 输出 `quireld 0.2.0`(clap 默认用 bin name + version)。

**启动日志:** [main.rs](file:///e:/MyWork/quirel/agent/src/main.rs) 日志初始化后、进入业务逻辑前:

```rust
tracing::info!(version = env!("CARGO_PKG_VERSION"), "quireld starting");
```

**协议层版本交换:** [agent/src/protocol/serde.rs](file:///e:/MyWork/quirel/agent/src/protocol/serde.rs#L142-L151) 的 `AuthResponse` 增加 `agent_version` 字段(向后兼容——serde 默认忽略未知字段,旧客户端忽略新字段;新客户端连旧 Agent 时为 None):

```rust
#[serde(rename = "auth_response")]
AuthResponse {
    success: bool,
    error: Option<String>,
    session_id: Option<String>,
    /// Agent 版本号(认证成功时返回,如 "0.2.0")
    #[serde(skip_serializing_if = "Option::is_none")]
    agent_version: Option<String>,
},
```

Agent 在认证成功构造 `AuthResponse` 时填入 `env!("CARGO_PKG_VERSION").to_string()`。实现时定位具体构造位置(候选 [agent/src/manager/auth.rs](file:///e:/MyWork/quirel/agent/src/manager/auth.rs) 或 [agent/src/handler.rs](file:///e:/MyWork/quirel/agent/src/handler.rs))。

### 4.4 Client 版本暴露

**[src-tauri/build.rs](file:///e:/MyWork/quirel/src-tauri/build.rs) 增强(当前仅一行 `tauri_build::build()`):**

1. 调用 `tauri_build::build()`(保留)。
2. 读取 `CARGO_PKG_VERSION`(build.rs 运行时 `std::env::var("CARGO_PKG_VERSION")`)。
3. **同步 tauri.conf.json:** 读 `tauri.conf.json`,更新 `version` 字段为 `CARGO_PKG_VERSION`,写回(保证 Tauri 打包元数据一致)。
4. **同步 package.json:** 读 `../package.json`(相对 src-tauri/build.rs 工作目录),更新 `version` 字段,写回。
5. **生成前端常量文件:** 写出 `../src/config/version.ts`(路径相对 src-tauri/):

```typescript
// 本文件由 src-tauri/build.rs 自动生成,请勿手工编辑
// 真相源: src-tauri/Cargo.toml 的 version 字段
export const CLIENT_VERSION = "0.2.0";
```

6. `println!("cargo:rerun-if-changed=Cargo.toml");` 保证 Cargo.toml 版本变更时重跑 build.rs。

**前端关于页:** [src/apps/Settings.tsx](file:///e:/MyWork/quirel/src/apps/Settings.tsx#L1025-L1041) 关于页改为:

```typescript
import { CLIENT_VERSION } from '../config/version';
// ...
<div className="st-about-version">版本 {CLIENT_VERSION}</div>
```

替代当前硬编码的 `版本 0.1.0`,显示动态客户端版本号。

**Agent 版本显示:** Client 连接 Agent 认证成功后,从 `AuthResponse.agent_version` 读取版本号,在关于页或连接信息区显示:

```typescript
{agentVersion && (
  <div className="st-agent-version">Agent: v{agentVersion}</div>
)}
```

数据来源为 `ServerManager` / `serversStore` 缓存的最近一次 `AuthResponse.agent_version`。未连接或旧 Agent 无此字段时显示"未知"或隐藏。

### 4.5 同步与发布脚本

#### 4.5.1 scripts/release.ps1(Windows)

参数:`-AgentVersion <ver>` 和/或 `-ClientVersion <ver>`(至少其一)。

行为:
1. 用 PowerShell 正则替换 `agent/Cargo.toml` 和/或 `src-tauri/Cargo.toml` 的 `version = "..."` 行。
2. 运行 `cargo build` 触发 build.rs 同步 tauri.conf.json / package.json / version.ts。
3. **不**执行 `git add` / `git commit` / `git push`——仅打印建议执行的 git 命令清单(遵守用户 git 规则)。

输出示例:
```
已更新版本号:
  agent/Cargo.toml          → 0.2.1  (修复 bug,PATCH+1)
  src-tauri/Cargo.toml      → 0.3.0  (新增功能,MINOR+1)
  src-tauri/tauri.conf.json → 0.3.0  (同步)
  package.json              → 0.3.0  (同步)
  src/config/version.ts     → 已重新生成

请手动执行以下命令完成提交:
  git add agent/Cargo.toml src-tauri/Cargo.toml src-tauri/tauri.conf.json package.json src/config/version.ts
  git commit -m "release: agent v0.2.1, client v0.3.0"
```

#### 4.5.2 scripts/sync-version.ps1

用途:开发中 Cargo.toml 未变但想强制重新生成派生文件(version.ts / tauri.conf.json / package.json)。逻辑:对 src-tauri 执行 `cargo build`(触发 build.rs)即可,脚本封装为单命令。

## 5. 实现清单(文件改动)

### 5.1 Agent 侧

| 文件 | 改动 |
|---|---|
| [agent/src/main.rs](file:///e:/MyWork/quirel/agent/src/main.rs#L14-L15) | clap `#[command]` 增加 `version` 属性;启动日志增加版本打印 |
| [agent/src/protocol/serde.rs](file:///e:/MyWork/quirel/agent/src/protocol/serde.rs#L142-L151) | `AuthResponse` 增加 `agent_version: Option<String>` 字段 |
| Agent 认证成功构造 `AuthResponse` 位置 | 填入 `env!("CARGO_PKG_VERSION").to_string()`(候选 [auth.rs](file:///e:/MyWork/quirel/agent/src/manager/auth.rs) / [handler.rs](file:///e:/MyWork/quirel/agent/src/handler.rs)) |

### 5.2 Client 侧

| 文件 | 改动 |
|---|---|
| [src-tauri/build.rs](file:///e:/MyWork/quirel/src-tauri/build.rs) | 重写:保留 `tauri_build::build()`;读 Cargo.toml version 同步 tauri.conf.json、同步 package.json、生成 `src/config/version.ts`;`rerun-if-changed=Cargo.toml` |
| [src/config/version.ts](file:///e:/MyWork/quirel/src/config/version.ts) | **新增**,由 build.rs 生成,提交初始占位版本 |
| [src/apps/Settings.tsx](file:///e:/MyWork/quirel/src/apps/Settings.tsx#L1032) | 关于页改用 `CLIENT_VERSION` 动态显示;新增 Agent 版本显示(从 ServerManager/serversStore 读 `agent_version`) |
| [src/types/server.ts](file:///e:/MyWork/quirel/src/types/server.ts) | 前端 AuthResponse 类型镜像补充 `agent_version?: string` |
| ServerManager / serversStore | 缓存最近一次 `AuthResponse.agent_version` 供 UI 读取 |

### 5.3 脚本与配置

| 文件 | 改动 |
|---|---|
| scripts/release.ps1 | **新增**,版本号修改 + 触发同步 + 打印 git 命令提示 |
| scripts/sync-version.ps1 | **新增**,触发 build.rs 重新生成派生文件 |
| [.gitignore](file:///e:/MyWork/quirel/.gitignore) | `src/config/version.ts` **不**加入忽略(提交初始占位,build 时更新) |

## 6. 验证标准

1. **Agent `--version`:** WSL 执行 `agent --version` 输出 `quireld 0.2.0`。
2. **Agent 启动日志:** 首行包含 `version=0.2.0`。
3. **Client 关于页:** 显示动态版本号(与 src-tauri/Cargo.toml 一致),非硬编码。
4. **Agent 版本透传:** Client 连接 Agent 后,关于页/连接信息区显示所连 Agent 版本号,与 Agent `--version` 输出一致。
5. **版本号修改纪律:** 修复 bug 后用 `release.ps1 -AgentVersion 0.2.1` 升 PATCH,重新构建,`agent --version` 输出 `0.2.1`;Client 同理。
6. **真相源同步:** 修改 `src-tauri/Cargo.toml` version 后构建,tauri.conf.json / package.json / version.ts 自动同步为同一版本号,无需手工改其他文件。
7. **向后兼容:** 新 Client 连接旧 Agent(无 agent_version 字段),关于页 Agent 版本显示"未知"而非报错;旧 Client 连接新 Agent,忽略新字段不报错。
8. **零编译错误:** `cargo build`(agent 与 src-tauri)、`tsc`、`vite build` 全部通过(warning 允许)。

## 7. 风险与注意事项

- **build.rs 修改 tauri.conf.json / package.json 不触发 cargo 重建循环:** 这两个文件不是 cargo 输入依赖,改它们不会重新触发 build.rs。但需 `cargo:rerun-if-changed=Cargo.toml` 保证 Cargo.toml 版本变更时重跑。
- **version.ts 提交策略:** 提交初始占位版本(如 `0.1.0`),build 时覆盖。若版本未变则文件内容不变,git 不显示 dirty;若版本变了显示 dirty 是合理的(提示需提交)。比 gitignore 更稳妥——避免首次 clone 未构建时 tsc 报 import 缺失。
- **serde 向后兼容:** `AuthResponse` 新增 `agent_version: Option<String>`,serde 默认对未知字段忽略(未加 `deny_unknown_fields`),旧客户端忽略新字段,新客户端连旧 Agent 字段为 None。
- **tauri.conf.json 含 $schema 字段:** 该字段是 JSON Schema 引用,非注释,serde_json 能正常解析与回写,不影响同步逻辑。
