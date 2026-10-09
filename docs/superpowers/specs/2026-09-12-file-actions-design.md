# 文件操作扩展设计：解压 / 本地浏览器打开 / 脚本运行

日期：2026-09-12
状态：待审阅
前置：`docs/superpowers/plans/2026-09-12-file-format-system.md`（文件格式识别与打开系统，已实施完成）

## 1. 背景与目标

文件格式系统（四层架构：检测层/网关层/渲染层/数据层）已完成"查看"能力（图片/PDF/文本/十六进制）。本次扩展三类"操作"能力，全部通过 Linux 服务器原生命令行实现：

| 场景 | 双击行为 | 执行方式 |
|---|---|---|
| 压缩包 `.zip/.tar.gz` 等 | 弹确认窗选目标 → 服务器解压 → 刷新目录 | Worker `ExecuteCommand`（unzip/tar/7z/unrar） |
| HTML `.html/.htm` | 下载到本地临时目录 → 系统默认浏览器打开 | Tauri Rust 端 ReadFile + `tauri-plugin-opener` |
| 脚本 `.sh`（含 shebang） | 弹确认窗 → 打开终端自动执行 | 终端 PTY 注入 `bash <脚本>` |

明确不做（YAGNI）：
- 不做压缩包内容浏览器（列出压缩包内文件）
- 不做压缩（右键压缩）
- 不做脚本类扩展语言（.py/.pl 仍走文本编辑器；未来可加插件）
- 不做解压进度条（同步命令无进度回调；大文件用长超时 + "解压中"状态提示）

## 2. 核心发现（复用已有基础设施）

| 组件 | 现状 | 结论 |
|---|---|---|
| Worker `ExecuteCommand` | `agent/src/worker/handlers/command.rs` 已完整实现（fork+setuid 用户隔离、argv 直执行不经过 shell、无 timeout） | **直接复用**，仅补 timeout 实现 |
| 线上协议 envelope | 无 ExecuteCommand 变体 | **新增 2 个变体** |
| quic.rs 路由 | `_ =>` 分支统一走 `manager.route_to_worker()` | **零改动**（新变体自动路由） |
| protocol_adapter | serde ↔ protobuf 双向转换 | **各加 1 个分支** |
| protobuf agent.proto | `ExecuteCommand { command, args, working_directory, env, timeout_secs }` 已定义（ManagerRequest field 8 已占位），`CommandOutput { stdout: bytes, stderr: bytes, exit_code }` 已定义（WorkerResponse field 6 已占位） | **零改动**（IPC 消息已存在） |
| Terminal | `preloadData: { workingDirectory }`，PTY 写通道 `remote_terminal_write` 已存在 | **扩展 initialCommand** |
| 本地打开 | `tauri-plugin-opener` 已在 Cargo.toml | **直接使用** |

关键安全事实：Worker 端 `std::process::Command::new(command).args(args)` 是**直接 argv 执行，不经 shell**——路径含空格/特殊字符天然无注入风险，无需引号转义。

## 3. 协议层设计

### 3.1 线上协议新变体（quirel-protocol/src/envelope.rs）

```rust
#[serde(rename = "execute_command")]
ExecuteCommandRequest {
    command: String,           // 白名单命令名，如 "unzip"（不是 shell 语句）
    args: Vec<String>,         // 参数数组（路径原样传入，无需转义）
    working_directory: Option<String>,
    timeout_secs: u32,         // 0 = 用 Agent 端默认（300s）
},

#[serde(rename = "command_output_resp")]
CommandOutputResponse {
    stdout: String,            // base64（与 ReadFileResponse 模式一致，兼容非 UTF-8 输出）
    stderr: String,            // base64
    exit_code: i32,
},
```

wire_compat.rs 新增 2 个 golden 测试锁定字节格式。

### 3.2 Agent 端变更

- `protocol_adapter.rs`：
  - `serde_to_worker_request` 加 `ExecuteCommandRequest` 分支（附 UserContext，模式同 FileInfo）
  - `worker_response_to_serde` 加 `CommandOutput` → `CommandOutputResponse` 分支（stdout/stderr 字节 → base64）
- `worker/handlers/command.rs`：**白名单 + timeout** 两处加固：
  - 白名单常量 `ALLOWED_COMMANDS: ["unzip", "tar", "7z", "unrar"]`，`command` 不在其中直接回 `Error { code: 403 }`（在执行点强制，客户端构造的命令永远无法越权）
  - timeout 实现：`timeout_secs > 0` 时用 `tokio::time::timeout` 包装 `cmd.output()`（当前 protobuf 字段已存在但未实现；超时 kill 子进程回 Error）
- quic.rs：零改动（自动路由）

### 3.3 Tauri 命令层（src-tauri/src/connection.rs）

```rust
// 远程执行白名单命令（解压等文件操作）
remote_execute_command(server_id, command, args, working_directory, timeout_secs)
  -> RemoteCommandOutput {
       stdout: String,   // base64 解码后 UTF-8 优先，GB18030 回退（复用 remote_read_file 模式）
       stderr: String,
       exit_code: i32,
     }
```

超时：不复用 `remote_send` 的 30s 外层超时（解压大文件不够），本命令独立超时 `timeout_secs + 30s` 缓冲。

## 4. 前端检测层扩展（src/file-formats/）

### 4.1 类型扩展（types.ts）

```ts
export type FormatCategory = 'image' | 'pdf' | 'text' | 'hex'
  | 'archive'          // 压缩包 → 解压流程
  | 'browser-local'    // HTML → 下载到本地用系统浏览器打开
  | 'run-script';      // 脚本 → 终端执行
```

### 4.2 新插件（3 个文件）

| 插件 | 识别规则 | category |
|---|---|---|
| `archive.ts` | 扩展名：zip/tar/7z/rar；复合扩展名（从 `path` 判断，file_info 的 `extension` 只取最后一段，`x.tar.gz` 的 extension 是 `gz`）：`.tar.gz/.tgz/.tar.bz2/.tar.xz`；magic：`PK\x03\x04`（zip 系）、`ustar`（tar 偏移 257） | `archive` |
| `html.ts` | 扩展名：html/htm | `browser-local` |
| `executable.ts` | 扩展名 `sh` **且** magicBytes 以 `#!` 开头（无 shebang 的 .sh 仍走文本编辑器） | `run-script` |

注：纯 `.gz`（非 `.tar.gz`）不支持——gzip 单文件流解压是另一类操作（YAGNI）。

注册顺序（FileOpener.ts `createDefaultRegistry`）：
```
pdf → archive → html → executable → image → text
```
- archive 在 image/text 前：`.zip` 等不会被 hex/text 误兜底
- html 在 text 前：textPlugin 同时排除 `html/htm`（同 svg 模式）

### 4.3 OpenDecision 扩展（FileOpener.ts）

- `kind` 联合类型加 `'extract' | 'browser-local' | 'run-script'`
- `SIZE_LIMITS` 加 `browserLocal: 20MB`（下载到本地的确认阈值）；archive/run-script 无大小确认（解压结果大小不可预知，脚本与大小无关）

## 5. 三个功能的数据流

### 5.1 解压（FileManager handleOpen 'extract' 分支）

```
双击 archive → confirm 弹窗（两个选项 + 取消）
  ├─ "解压到当前位置"（target = 压缩包所在目录）
  └─ "解压到独立文件夹"（target = 所在目录/<主文件名>/，先 remote_mkdir）
→ 构造命令（按扩展名分发，客户端只构造白名单命令）：
   .zip            → unzip -o <archive> -d <target>
   .tar/.tar.gz/.tgz/.tar.bz2/.tar.xz → tar -xf <archive> -C <target>
   .7z             → 7z x <archive> -o<target> -y
   .rar            → unrar x -o+ <archive> <target>/
→ invoke remote_execute_command（timeout 600s）
→ exit_code == 0 → 刷新当前目录
→ 失败 → 弹错误窗（stderr 内容 + exit code；127 = 服务器未安装对应工具，提示安装）
```

覆盖策略统一为覆盖同名文件，弹窗文案明示"目标位置同名文件将被覆盖"。

### 5.2 HTML 本地浏览器打开

新 Tauri 命令 `remote_open_locally(server_id, remote_path)`（Rust 端一体完成，前端零中间状态）：

```
ReadFileRequest（全量 base64）→ 解码
→ 写入 %TEMP%/quirel-view/<原文件名>（目录自动创建，同名覆盖）
→ tauri_plugin_opener::open_path(temp_path, None)（系统默认应用 = 浏览器）
```

- 失败（下载/写盘/打开）→ 前端 alert 错误
- 不清理临时文件（系统 temp 自然回收；记录在案）

### 5.3 脚本运行（'run-script' 分支）

```
双击 .sh → confirm 弹窗（显示完整将执行的命令）
→ manager.create('terminal', {
    preloadData: {
      workingDirectory: <脚本所在目录>,
      initialCommand: `bash <脚本绝对路径>`,   // PTY 直写，无 shell 拼接
    }
  })
```

Terminal.tsx 扩展：
- `preloadData` 类型加 `initialCommand?: string`
- `TerminalInstance` 收到 `initialCommand` 且 PTY 创建成功后，调用 `remote_terminal_write(sessionId, initialCommand + "\n")`
- 时序：PTY stdin 有内核缓冲，shell 就绪后自然读走（无需 sleep 等待）

## 6. 安全设计

1. **命令白名单（执行点强制）**：Worker `handle_execute_command` 校验 `command ∈ ALLOWED_COMMANDS`（unzip/tar/7z/unrar），越权直接 403。即使客户端被完全控制，也无法通过此通道执行任意命令。
2. **argv 直执行**：全程不经 shell，路径参数（空格/引号/`$` 等）原样传递，无注入面。
3. **用户隔离**：复用 Worker 已有 fork+setuid（uid/gid/home_dir），解压以登录用户身份执行，权限天然受限。
4. **写操作确认**：解压（覆盖文件）与脚本运行（任意代码执行）均前置 confirm，弹窗展示完整命令与目标路径。
5. **HTML 本地打开**：临时目录限定 `%TEMP%/quirel-view/`；只对白名单扩展（html/htm）触发；系统浏览器独立进程渲染，与 Tauri webview 隔离。
6. **timeout 兜底**：远程命令默认 600s 超时 kill，防止死循环命令占住通道。

## 7. 错误处理

| 场景 | 表现 |
|---|---|
| 服务器未装 unzip/7z/unrar | exit_code 127，stderr "command not found" → 弹窗提示"服务器未安装 xx，请安装后重试" |
| 解压失败（损坏/密码） | exit_code ≠ 0 → 弹窗显示 stderr 摘要 |
| 超时 | Error envelope（"命令执行超时"） |
| 白名单拒绝 | Error code 403（正常使用不会触发） |
| HTML 下载失败 | alert 错误原因 |
| 终端创建失败 | 现有 Terminal 降级演示模式，脚本命令不执行 |
| 旧版 Agent（无 execute_command 但有 file_info，半新 Agent） | remote_execute_command 收到 Error 未知请求 → 弹错误窗提示"请升级服务器 Agent"（探测已成功，回退 hex 无意义） |
| 旧版 Agent（无 file_info，全旧） | detectFileFormat 失败 → 现有 catch 回退编辑器（保持既有行为） |
| .sh 无执行权限 | `bash <path>` 不需要执行权限位，天然规避 |

## 8. 测试策略

- 协议层：wire_compat golden ×2（execute_command / command_output_resp）
- Agent：protocol_adapter 转换测试 ×2；白名单拒绝测试；timeout 测试（短超时命令）
- 前端：新插件 detect 测试（archive magic/扩展名、html、sh+shebang/无 shebang）；FileOpener 决策测试（新 kind 分支、SIZE_LIMITS）
- 集成（手动冒烟）：
  1. 双击 .zip → 独立文件夹解压 → 目录刷新出现新文件夹
  2. 双击 .tar.gz → 当前位置解压 → 覆盖提示文案正确
  3. 双击 .7z（服务器未装 7z）→ 弹"未安装"错误
  4. 双击 .html → 本地浏览器打开
  5. 双击 .sh → confirm → 终端自动执行，输出正确
  6. 双击无 shebang 的 .sh → 文本编辑器打开
  7. 双击 .py → 文本编辑器打开（不受影响）
  8. 断开服务器双击 .zip → 回退 hex 查看器

## 9. 涉及文件清单

| 层 | 文件 | 变更 |
|---|---|---|
| 协议 | quirel-protocol/src/envelope.rs | +2 变体 |
| 协议 | quirel-protocol/tests/wire_compat.rs | +2 golden |
| Agent | agent/src/manager/protocol_adapter.rs | +2 转换分支 |
| Agent | agent/src/worker/handlers/command.rs | +白名单 +timeout |
| 客户端 | src-tauri/src/connection.rs | +remote_execute_command、+remote_open_locally |
| 客户端 | src-tauri/src/lib.rs | 注册 2 个新命令 |
| 前端 | src/file-formats/types.ts | FormatCategory 扩展 |
| 前端 | src/file-formats/FileOpener.ts | kind 扩展 + 注册新插件 |
| 前端 | src/file-formats/plugins/{archive,html,executable}.ts | 新建 3 插件 |
| 前端 | src/file-formats/plugins/text.ts | 排除 html/htm 扩展名 |
| 前端 | src/apps/FileManager.tsx | handleOpen 三分支（extract/browser-local/run-script） |
| 前端 | src/apps/Terminal.tsx | preloadData.initialCommand + PTY 就绪后注入 |
