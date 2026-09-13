# 文件操作扩展（解压/本地浏览器/脚本运行）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 双击压缩包解压（服务器原生命令）、双击 HTML 用本地浏览器打开、双击 .sh 脚本在终端自动运行。

**Architecture:** 线上协议新增 `execute_command`/`command_output_resp` 变体（Worker 端已有 ExecuteCommand 处理器，含用户隔离），复用 `route_to_worker` 自动路由；前端检测层加 3 个插件（archive/html/executable）+ FileOpener 3 个新决策分支；Terminal 扩展 `initialCommand` 注入。

**Tech Stack:** Rust（quirel-protocol / agent / src-tauri）、TypeScript/React（file-formats / FileManager / Terminal）

**规格文档:** `docs/superpowers/specs/2026-09-12-file-actions-design.md`

**Git 约定:** 按用户规则，全程不执行 git 提交——完成后统一提示用户自行提交。

**验证环境:**
- 协议 crate 测试：`cargo test`（cwd: `e:\MyWork\gnome-remote\quirel-protocol`）
- Agent 测试（WSL）：`wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && cargo test"`
- 客户端 Rust：`cargo check`（cwd: `e:\MyWork\gnome-remote\src-tauri`）
- 前端：`npx tsc --noEmit` + `npm run test:run`（cwd: `e:\MyWork\gnome-remote`）

**预存失败基线（与本次无关，验证时忽略）：**
- Agent：`auth::executor::test_spawn_isolated_{reader,writer}_round_trip`（WSL BrokenPipe 预存）
- 前端：`src/apps/Terminal.test.tsx`（4 个，缺 Provider）、`src/window-system/tests/WindowManagerContext.test.tsx`（2 个，旧版 context API）

---

## File Structure（变更地图）

| 层 | 文件 | 职责 |
|---|---|---|
| 协议 | `quirel-protocol/src/envelope.rs` | +ExecuteCommandRequest/CommandOutputResponse 变体 |
| 协议 | `quirel-protocol/tests/wire_compat.rs` | +3 golden 测试 |
| Agent | `agent/src/manager/protocol_adapter.rs` | +2 转换分支 +2 测试 |
| Agent | `agent/src/worker/handlers/command.rs` | +白名单 +timeout |
| 客户端 | `src-tauri/src/connection.rs` | +remote_execute_command、+remote_open_locally |
| 客户端 | `src-tauri/src/lib.rs` | 注册 2 个新命令 |
| 前端 | `src/file-formats/types.ts` | FormatCategory +3 值 |
| 前端 | `src/file-formats/plugins/archive.ts` | 新建：压缩包识别 + 解压命令构造 |
| 前端 | `src/file-formats/plugins/html.ts` | 新建：HTML 识别 |
| 前端 | `src/file-formats/plugins/executable.ts` | 新建：脚本识别（shebang 校验） |
| 前端 | `src/file-formats/plugins/text.ts` | 排除 html/htm（同 svg 模式） |
| 前端 | `src/file-formats/FileOpener.ts` | 注册新插件 + SIZE_LIMITS.browserLocal |
| 前端 | `src/file-formats/__tests__/plugins.test.ts` | +新插件测试 |
| 前端 | `src/file-formats/__tests__/opener.test.ts` | +新决策测试 |
| 前端 | `src/apps/Terminal.tsx` | preloadData.initialCommand + PTY 就绪注入 |
| 前端 | `src/apps/FileManager.tsx` | handleOpen +extract/browser-local/run-script 分支 |

---

### Task 1: 协议层变体 + golden 测试

**Files:**
- Modify: `quirel-protocol/src/envelope.rs`（FileInfoResponse 之后、WriteFileRequest 之前，约 151 行处）
- Test: `quirel-protocol/tests/wire_compat.rs`（文件末尾追加）

- [ ] **Step 1: 写失败测试（golden）**

在 `quirel-protocol/tests/wire_compat.rs` 末尾追加：

```rust
#[test]
fn execute_command_wire_format() {
    let env = Envelope::new(
        11,
        Payload::ExecuteCommandRequest {
            command: "unzip".to_string(),
            args: vec!["-o".to_string(), "/tmp/a.zip".to_string(), "-d".to_string(), "/tmp/out".to_string()],
            working_directory: Some("/tmp".to_string()),
            timeout_secs: 600,
        },
    );
    let json = serde_json::to_string(&env).unwrap();
    assert_eq!(
        json,
        r#"{"request_id":11,"payload":{"type":"execute_command","data":{"command":"unzip","args":["-o","/tmp/a.zip","-d","/tmp/out"],"working_directory":"/tmp","timeout_secs":600}}}"#
    );
}

#[test]
fn execute_command_defaults_compat() {
    // working_directory/timeout_secs 带 #[serde(default)]：缺失字段可解码
    let raw = r#"{"request_id":12,"payload":{"type":"execute_command","data":{"command":"tar","args":["-xf","/a.tar"]}}}"#;
    let env: Envelope = serde_json::from_str(raw).expect("缺省字段必须可解码");
    match env.payload {
        Payload::ExecuteCommandRequest { working_directory, timeout_secs, .. } => {
            assert_eq!(working_directory, None);
            assert_eq!(timeout_secs, 0);
        }
        _ => panic!("变体不匹配"),
    }
}

#[test]
fn command_output_resp_wire_format() {
    let env = Envelope::new(
        13,
        Payload::CommandOutputResponse {
            stdout: "aGk=".to_string(),
            stderr: String::new(),
            exit_code: 0,
        },
    );
    let json = serde_json::to_string(&env).unwrap();
    assert_eq!(
        json,
        r#"{"request_id":13,"payload":{"type":"command_output_resp","data":{"stdout":"aGk=","stderr":"","exit_code":0}}}"#
    );
}
```

- [ ] **Step 2: 运行测试确认编译失败**

Run: `cargo test`（cwd: `e:\MyWork\gnome-remote\quirel-protocol`）
Expected: 编译错误 `no variant named ExecuteCommandRequest`（测试先行，变体未定义）

- [ ] **Step 3: 实现变体**

在 `quirel-protocol/src/envelope.rs` 的 `FileInfoResponse` 块（`}` 之后、`#[serde(rename = "write_file")]` 之前）插入：

```rust
    /// 白名单命令执行请求（解压等文件操作；Worker 端白名单强制）
    /// command 是命令名（如 "unzip"），不是 shell 语句——Worker 端 argv 直执行，路径无需转义
    #[serde(rename = "execute_command")]
    ExecuteCommandRequest {
        command: String,
        args: Vec<String>,
        #[serde(default)]
        working_directory: Option<String>,
        #[serde(default)]
        timeout_secs: u32, // 0 = Agent 端默认（300s）
    },

    /// 命令执行响应（stdout/stderr 为 base64，兼容非 UTF-8 输出）
    #[serde(rename = "command_output_resp")]
    CommandOutputResponse {
        stdout: String,
        stderr: String,
        exit_code: i32,
    },
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test`（cwd: `e:\MyWork\gnome-remote\quirel-protocol`）
Expected: `test result: ok. 15 passed`（原 12 + 新 3）

---

### Task 2: Agent protocol_adapter 转换 + 测试

**Files:**
- Modify: `agent/src/manager/protocol_adapter.rs`（serde_to_worker_request 的 FileInfo 分支后 + worker_response_to_serde 的 FileInfoResult 分支后；tests 模块末尾）
- Test: 同文件 `#[cfg(test)]` 模块

- [ ] **Step 1: 写失败测试**

在 `protocol_adapter.rs` 的 `tests` 模块（`test_file_info_result_response_conversion` 之后）追加。文件顶部 use 已有 `FileInfo, FileInfoResult`，追加导入 `ExecuteCommand, CommandOutput`：

```rust
    #[test]
    fn test_execute_command_request_conversion() {
        let payload = Payload::ExecuteCommandRequest {
            command: "unzip".to_string(),
            args: vec!["-o".to_string(), "/tmp/a.zip".to_string()],
            working_directory: Some("/tmp".to_string()),
            timeout_secs: 600,
        };
        let user = test_user_context();

        let result = serde_to_worker_request(&payload, &user);

        match result {
            Some(manager_request::Payload::ExecuteCommand(req)) => {
                assert_eq!(req.command, "unzip");
                assert_eq!(req.args, vec!["-o".to_string(), "/tmp/a.zip".to_string()]);
                assert_eq!(req.working_directory, "/tmp");
                assert_eq!(req.timeout_secs, 600);
                assert_eq!(req.uid, user.uid);
                assert_eq!(req.gid, user.gid);
                assert_eq!(req.username, user.username);
                assert_eq!(req.home_dir, user.home_dir);
            }
            _ => panic!("Expected ExecuteCommand conversion"),
        }
    }

    #[test]
    fn test_command_output_response_conversion() {
        use crate::protocol::generated::WorkerResponse;

        let resp = WorkerResponse {
            request_id: 1,
            payload: Some(worker_response::Payload::CommandOutput(CommandOutput {
                stdout: b"hi".to_vec(),
                stderr: Vec::new(),
                exit_code: 0,
            })),
        };

        let result = worker_response_to_serde(&resp);

        match result {
            Some(Payload::CommandOutputResponse { stdout, stderr, exit_code }) => {
                assert_eq!(stdout, "aGk="); // base64("hi")
                assert_eq!(stderr, "");
                assert_eq!(exit_code, 0);
            }
            _ => panic!("Expected CommandOutputResponse"),
        }
    }
```

- [ ] **Step 2: 运行测试确认编译失败**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && cargo test test_execute_command 2>&1 | tail -5"`
Expected: 编译错误（payload 中无 ExecuteCommandRequest 分支 / 未导入）

- [ ] **Step 3: 实现转换**

`protocol_adapter.rs` 顶部 use 块加 `ExecuteCommand, CommandOutput`（`FileInfo, FileInfoResult` 之后）。

`serde_to_worker_request` 的 `Payload::FileInfoRequest` 分支之后插入：

```rust
        Payload::ExecuteCommandRequest { command, args, working_directory, timeout_secs } => {
            tracing::debug!("适配 ExecuteCommandRequest: command={}, args={:?}, timeout={}s", command, args, timeout_secs);
            Some(manager_request::Payload::ExecuteCommand(ExecuteCommand {
                command: command.clone(),
                args: args.clone(),
                working_directory: working_directory.clone().unwrap_or_default(),
                env: Default::default(),
                timeout_secs: *timeout_secs,
                uid: user.uid,
                gid: user.gid,
                username: user.username.clone(),
                home_dir: user.home_dir.clone(),
            }))
        }
```

`worker_response_to_serde` 的 `worker_response::Payload::FileInfoResult` 分支之后插入：

```rust
        Some(worker_response::Payload::CommandOutput(output)) => {
            tracing::debug!("适配 CommandOutput: exit_code={}", output.exit_code);
            Some(Payload::CommandOutputResponse {
                stdout: BASE64.encode(&output.stdout),
                stderr: BASE64.encode(&output.stderr),
                exit_code: output.exit_code,
            })
        }
```

- [ ] **Step 4: 运行测试确认通过**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && cargo test test_execute_command test_command_output 2>&1 | grep -E 'test |result'"`
Expected: `2 passed`（转换测试通过；protobuf ExecuteCommand 字段已存在，无需改 .proto）

---

### Task 3: Agent Worker 白名单 + timeout

**Files:**
- Modify: `agent/src/worker/handlers/command.rs`

- [ ] **Step 1: 加白名单常量与超时常量**

在 `use` 块之后、`build_user_session` 之前插入：

```rust
/// 允许远程执行的命令白名单（安全关键：在执行点强制）
/// 客户端构造的命令永远无法越权——即使客户端被完全控制
/// 语法：命令名（argv[0]）；绝对路径调用按最后一段匹配（/usr/bin/unzip → unzip）
const ALLOWED_COMMANDS: &[&str] = &["unzip", "tar", "7z", "unrar"];

/// timeout_secs 为 0 时的默认超时（秒）
const DEFAULT_TIMEOUT_SECS: u64 = 300;
```

- [ ] **Step 2: 白名单校验**

在 `handle_execute_command` 的"验证工作目录"块之后、"构建用户会话"之前插入：

```rust
    // 白名单校验（执行点强制；命令可能以绝对路径传入，取最后一段匹配）
    let base_name = Path::new(&req.command)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if !ALLOWED_COMMANDS.contains(&base_name) {
        tracing::warn!("命令不在白名单，拒绝执行: {}", req.command);
        return WorkerResponse {
            payload: Some(worker_response::Payload::Error(Error {
                code: 403,
                message: format!("Command not allowed: {}", req.command),
            })),
            ..Default::default()
        };
    }
```

- [ ] **Step 3: timeout 实现（替换执行块）**

将现有的 `let result = executor.execute_as_user(move || {...});` 及其后的 `match result` 整块替换为：

```rust
    // 超时 + 子进程 PID 追踪：超时 kill 命令进程，避免死循环命令占住通道
    let timeout_secs = if req.timeout_secs > 0 { req.timeout_secs as u64 } else { DEFAULT_TIMEOUT_SECS };
    let child_pid = std::sync::Arc::new(std::sync::Mutex::new(None::<u32>));
    let pid_slot = child_pid.clone();

    // spawn_blocking：execute_as_user 是同步阻塞（fork+wait），放入阻塞线程池
    let exec_handle = tokio::task::spawn_blocking(move || {
        executor.execute_as_user(move || {
            let mut cmd = std::process::Command::new(&command);

            if !args.is_empty() {
                cmd.args(&args);
            }

            if !working_directory.is_empty() {
                cmd.current_dir(&working_directory);
            }

            cmd.stdout(std::process::Stdio::piped());
            cmd.stderr(std::process::Stdio::piped());

            // 先 spawn 再记录 PID（供超时 kill），随后等待输出
            let mut child = cmd.spawn()?;
            *pid_slot.lock().unwrap() = Some(child.id());
            let output = child.wait_with_output()?;

            Ok(CommandResult {
                stdout: output.stdout,
                stderr: output.stderr,
                exit_code: output.status.code().unwrap_or(-1),
            })
        })
    });

    let result = match tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        exec_handle,
    ).await {
        Ok(Ok(inner)) => inner,          // spawn_blocking 正常完成
        Ok(Err(e)) => {                  // spawn_blocking panic / JoinError
            tracing::error!("命令执行线程异常: command={}, error={}", req.command, e);
            return WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error {
                    code: 500,
                    message: format!("Command execution thread failed: {}", e),
                })),
                ..Default::default()
            };
        }
        Err(_) => {                      // 超时：kill 子进程后返回错误
            if let Some(pid) = *child_pid.lock().unwrap() {
                let _ = nix::sys::signal::kill(
                    nix::unistd::Pid::from_raw(pid as i32),
                    nix::sys::signal::Signal::SIGKILL,
                );
            }
            tracing::warn!("命令执行超时（{}s），已 kill: command={}", timeout_secs, req.command);
            return WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error {
                    code: 408,
                    message: format!("Command timed out after {}s", timeout_secs),
                })),
                ..Default::default()
            };
        }
    };
```

后续的 `match result { Ok(result) => ... Err(e) => ... }` 保持不变（类型兼容：spawn_blocking 返回 execute_as_user 的 Result）。

- [ ] **Step 4: 编译 + 全量测试**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && cargo test 2>&1 | grep -E 'result:' | tail -3"`
Expected: 与基线一致（118 passed；2 个 auth::executor 预存失败不变）
（若 `nix` 依赖缺失：`agent/Cargo.toml` 已有 nix——session.rs 中 `use nix::unistd::Pid` 在用，无需新增）

---

### Task 4: Tauri remote_execute_command

**Files:**
- Modify: `src-tauri/src/connection.rs`（remote_file_info 命令之后追加）

- [ ] **Step 1: 实现命令**

在 `remote_read_file_binary` 之后插入：

```rust
// ── 远程命令执行（解压等文件操作）──────────────────────────

/// 远程命令执行结果（stdout/stderr 已转文本：UTF-8 优先，GB18030 回退）
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteCommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// 执行服务器白名单命令（unzip/tar/7z/unrar；Worker 端强制白名单）
///
/// 解压等文件操作走此通道：argv 直执行不经 shell，路径无需转义。
/// 超时独立于 remote_send 的 30s Stream 超时（大压缩包解压耗时长），
/// 故不直接复用 remote_send，而是内联其发送模式 + 自定义外层超时。
#[tauri::command]
#[tracing::instrument(skip(app), fields(server_id = %server_id))]
pub async fn remote_execute_command(
    server_id: String,
    command: String,
    args: Vec<String>,
    working_directory: Option<String>,
    timeout_secs: Option<u32>,
    app: tauri::AppHandle,
) -> Result<RemoteCommandOutput, String> {
    use base64::Engine;
    tracing::info!("[ExecuteCommand] server_id={}, command={}, args={:?}", server_id, command, args);

    // 外层超时兜底：比命令自身 timeout 多 30s（Agent 端负责 kill 命令进程并回错误）
    let cmd_timeout = timeout_secs.unwrap_or(600);
    let outer_timeout = std::time::Duration::from_secs(cmd_timeout as u64 + 30);

    let manager = app.state::<ConnectionManager>();
    let (tx, request_id) = {
        let conns = manager.connections.lock().unwrap();
        let conn = conns.get(&server_id).ok_or("未找到连接")?;
        (conn.tx.clone(), manager.next_request_id())
    };

    let envelope = Envelope::new(request_id, Payload::ExecuteCommandRequest {
        command,
        args,
        working_directory,
        timeout_secs: cmd_timeout,
    });
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();

    tx.send(ClientRequest::Send { envelope, response_tx }).await
        .map_err(|_| "发送请求失败".to_string())?;

    let data = tokio::time::timeout(outer_timeout, response_rx)
        .await
        .map_err(|_| {
            tracing::warn!("[ExecuteCommand] 命令执行超时: server_id={}", server_id);
            "命令执行超时".to_string()
        })?
        .map_err(|_| "等待响应超时".to_string())??;

    let resp = Envelope::decode(&data)?;
    match resp.payload {
        Payload::CommandOutputResponse { stdout, stderr, exit_code } => {
            // base64 → 字节 → UTF-8 优先，GB18030 回退（服务器命令输出可能是中文 GBK）
            let decode_text = |b64: String| -> String {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(b64.as_bytes())
                    .unwrap_or_default();
                match String::from_utf8(bytes) {
                    Ok(s) => s,
                    Err(e) => {
                        let (decoded, _, _) = encoding_rs::GB18030.decode(e.as_bytes());
                        decoded.into_owned()
                    }
                }
            };
            Ok(RemoteCommandOutput {
                stdout: decode_text(stdout),
                stderr: decode_text(stderr),
                exit_code,
            })
        }
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}
```

- [ ] **Step 2: 注册命令**

`src-tauri/src/lib.rs` 的 invoke_handler 中，`connection::remote_read_file_binary` 之后加一行：

```rust
            connection::remote_execute_command, // 白名单命令执行（解压）
```

- [ ] **Step 3: 编译验证**

Run: `cargo check`（cwd: `e:\MyWork\gnome-remote\src-tauri`）
Expected: `Finished` 无错误
（`ClientRequest` 与 `remote_send` 同文件定义，直接可用）

---

### Task 5: Tauri remote_open_locally

**Files:**
- Modify: `src-tauri/src/connection.rs`（remote_execute_command 之后）
- Modify: `src-tauri/src/lib.rs`（注册）

- [ ] **Step 1: 实现命令**

在 `remote_execute_command` 之后插入：

```rust
/// 下载远程文件到本地临时目录并用系统默认应用打开（HTML → 浏览器）
///
/// 数据流：ReadFileRequest（base64 全量）→ %TEMP%/quirel-view/<文件名> → opener
/// 临时文件不主动清理（系统 temp 自然回收）
#[tauri::command]
#[tracing::instrument(skip(app), fields(server_id = %server_id, remote_path = %remote_path))]
pub async fn remote_open_locally(
    server_id: String,
    remote_path: String,
    app: tauri::AppHandle,
) -> Result<String, String> {
    use base64::Engine;
    use tauri_plugin_opener::OpenerExt;
    tracing::info!("[OpenLocally] server_id={}, path={}", server_id, remote_path);

    // 1. 读取远程文件（base64 全量；SIZE_LIMITS.browserLocal 20MB 确认在前端完成）
    let resp = remote_send(server_id, Payload::ReadFileRequest { path: remote_path.clone() }, app).await?;
    let base64_content = match resp.payload {
        Payload::ReadFileResponse { content, .. } => content,
        Payload::Error { message, .. } => return Err(message),
        _ => return Err("意外响应".into()),
    };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(base64_content.as_bytes())
        .map_err(|e| format!("base64 解码失败: {}", e))?;

    // 2. 写入本地临时目录（%TEMP%/quirel-view/，不存在则创建，同名覆盖）
    let file_name = remote_path.rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or("download.html");
    let dir = std::env::temp_dir().join("quirel-view");
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建临时目录失败: {}", e))?;
    let local_path = dir.join(file_name);
    std::fs::write(&local_path, &bytes).map_err(|e| format!("写入临时文件失败: {}", e))?;

    // 3. 系统默认应用打开（HTML 的默认应用即浏览器）
    app.opener()
        .open_path(local_path.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| format!("调用本地应用失败: {}", e))?;

    Ok(local_path.to_string_lossy().to_string())
}
```

- [ ] **Step 2: 注册命令**

`lib.rs` invoke_handler 中 `connection::remote_execute_command` 之后加：

```rust
            connection::remote_open_locally, // 下载到本地用系统应用打开（HTML）
```

- [ ] **Step 3: 编译验证**

Run: `cargo check`（cwd: `e:\MyWork\gnome-remote\src-tauri`）
Expected: `Finished` 无错误（tauri_plugin_opener 已在 lib.rs:546 注册插件）

---

### Task 6: 前端类型 + 3 个新插件

**Files:**
- Modify: `src/file-formats/types.ts`
- Create: `src/file-formats/plugins/archive.ts`
- Create: `src/file-formats/plugins/html.ts`
- Create: `src/file-formats/plugins/executable.ts`
- Test: `src/file-formats/__tests__/plugins.test.ts`

- [ ] **Step 1: 写失败测试**

先读 `src/file-formats/__tests__/plugins.test.ts` 现有结构（helper 函数 `makeInfo` 之类），然后在文件末尾按现有模式追加：

```typescript
// ── archive 插件 ──────────────────────────────────────────

describe('archivePlugin', () => {
  const archive = require('../../file-formats/plugins/archive').archivePlugin;

  test.each(['zip', 'tar', '7z', 'rar', 'tgz'].map(ext => [ext]))(
    '扩展名 .%s 命中 archive',
    (ext) => {
      const info = makeInfo({ extension: ext, path: `/tmp/a.${ext}` });
      expect(archive.detect(info)).toMatchObject({ category: 'archive' });
    },
  );

  test('复合扩展名 .tar.gz 从 path 判断（extension 只取最后一段 gz）', () => {
    const info = makeInfo({ extension: 'gz', path: '/tmp/a.tar.gz' });
    expect(archive.detect(info)).toMatchObject({ category: 'archive' });
  });

  test.each(['.tar.bz2', '.tar.xz'])('复合扩展名 %s 命中', (suffix) => {
    const info = makeInfo({ extension: suffix.split('.').pop()!, path: `/tmp/a${suffix}` });
    expect(archive.detect(info)).toMatchObject({ category: 'archive' });
  });

  test('纯 .gz（非 .tar.gz）不命中', () => {
    const info = makeInfo({ extension: 'gz', path: '/tmp/a.gz' });
    expect(archive.detect(info)).toBeNull();
  });

  test('ZIP magic 命中（改错扩展名兜底）', () => {
    const info = makeInfo({ extension: 'dat', path: '/tmp/a.dat', magicBytes: [0x50, 0x4b, 0x03, 0x04, 0x00] });
    expect(archive.detect(info)).toMatchObject({ category: 'archive', confidence: 0.95 });
  });
});

// ── html 插件 ─────────────────────────────────────────────

describe('htmlPlugin', () => {
  const html = require('../../file-formats/plugins/html').htmlPlugin;

  test.each(['html', 'htm'])('扩展名 .%s 命中 browser-local', (ext) => {
    const info = makeInfo({ extension: ext, path: `/tmp/a.${ext}` });
    expect(html.detect(info)).toMatchObject({ category: 'browser-local' });
  });

  test('其他扩展名不命中', () => {
    expect(html.detect(makeInfo({ extension: 'js' }))).toBeNull();
  });
});

// ── executable 插件 ────────────────────────────────────────

describe('executablePlugin', () => {
  const exe = require('../../file-formats/plugins/executable').executablePlugin;

  test('.sh 且含 shebang 命中 run-script', () => {
    const info = makeInfo({ extension: 'sh', magicBytes: [0x23, 0x21, 0x2f, 0x62, 0x69, 0x6e] }); // "#!/bin"
    expect(exe.detect(info)).toMatchObject({ category: 'run-script', confidence: 0.95 });
  });

  test('.sh 无 shebang 不命中（文本编辑器兜底）', () => {
    const info = makeInfo({ extension: 'sh', magicBytes: [0x65, 0x63, 0x68, 0x6f] }); // "echo"
    expect(exe.detect(info)).toBeNull();
  });

  test('非 .sh 不命中', () => {
    const info = makeInfo({ extension: 'py', magicBytes: [0x23, 0x21] });
    expect(exe.detect(info)).toBeNull();
  });
});
```

注：`makeInfo` 的字段名/签名以现有测试文件为准（若现有 helper 不支持覆盖 path/magicBytes 参数，按其实际模式构造）。

- [ ] **Step 2: 运行测试确认失败**

Run: `npm run test:run -- plugins.test.ts`（cwd: `e:\MyWork\gnome-remote`）
Expected: FAIL（模块不存在）

- [ ] **Step 3: 扩展类型**

`src/file-formats/types.ts` 的 FormatCategory 替换为：

```typescript
/** 格式分类（决定路由到哪个应用/流程） */
export type FormatCategory =
  | 'image' | 'pdf' | 'text' | 'hex'
  | 'archive'          // 压缩包 → 解压流程（FileManager，服务器原生命令）
  | 'browser-local'    // HTML → 下载到本地用系统浏览器打开
  | 'run-script';      // 脚本 → 终端自动执行（类 Windows 双击运行脚本）
```

- [ ] **Step 4: 创建 3 个插件**

`src/file-formats/plugins/archive.ts`：

```typescript
/**
 * 压缩包格式插件
 *
 * 双击 → FileManager 解压流程（服务器端 unzip/tar/7z/unrar 原生命令，Worker 白名单强制）
 * 识别：扩展名为主（复合扩展名从 path 判断）+ ZIP magic 兜底
 */
import type { FileFormatPlugin, FormatMatch, RemoteFileInfo } from '../types';

/** ZIP 头 "PK\x03\x04" */
const ZIP_MAGIC = [0x50, 0x4b, 0x03, 0x04];

/** 复合压缩扩展名（file_info 的 extension 只取最后一段，x.tar.gz → "gz"，需从 path 判断） */
const COMPOUND_EXTS = ['.tar.gz', '.tar.bz2', '.tar.xz'];

/** 单段压缩扩展名（extension 字段可直接判断） */
const SIMPLE_EXTS = ['zip', 'tar', '7z', 'rar', 'tgz'];

export const archivePlugin: FileFormatPlugin = {
  id: 'archive',
  detect(info: RemoteFileInfo): FormatMatch | null {
    const lowerPath = info.path.toLowerCase();
    const byExt =
      SIMPLE_EXTS.includes(info.extension) ||
      COMPOUND_EXTS.some(ext => lowerPath.endsWith(ext));
    if (byExt) {
      return { pluginId: 'archive', category: 'archive', confidence: 0.9 };
    }
    // magic 兜底（改错扩展名的 zip）
    if (info.magicBytes.length >= ZIP_MAGIC.length && ZIP_MAGIC.every((b, i) => info.magicBytes[i] === b)) {
      return { pluginId: 'archive', category: 'archive', confidence: 0.95 };
    }
    return null;
  },
};

/**
 * 构造解压命令（仅白名单命令；Worker 端 argv 直执行，路径无需转义）
 *
 * @param archivePath 压缩包绝对路径
 * @param targetDir   解压目标目录（须已存在：tar -C 要求；unzip/7z/unrar 自动创建，先 mkdir 统一）
 * @param fileName    压缩包文件名（用于按扩展名分发）
 */
export function buildExtractCommand(
  archivePath: string,
  targetDir: string,
  fileName: string,
): { command: string; args: string[] } {
  const lower = fileName.toLowerCase();
  if (lower.endsWith('.zip')) {
    return { command: 'unzip', args: ['-o', archivePath, '-d', targetDir] };
  }
  if (/\.(tar|tar\.gz|tgz|tar\.bz2|tar\.xz)$/.test(lower)) {
    return { command: 'tar', args: ['-xf', archivePath, '-C', targetDir] };
  }
  if (lower.endsWith('.7z')) {
    return { command: '7z', args: ['x', archivePath, `-o${targetDir}`, '-y'] };
  }
  // .rar
  return { command: 'unrar', args: ['x', '-o+', archivePath, `${targetDir}/`] };
}
```

`src/file-formats/plugins/html.ts`：

```typescript
/**
 * HTML 格式插件
 *
 * 双击 → 下载到本地临时目录 → 系统默认浏览器打开
 * （Tauri webview 受 CSP 限制，不适合直接渲染本地 HTML；系统浏览器独立进程隔离更安全）
 */
import type { FileFormatPlugin, FormatMatch, RemoteFileInfo } from '../types';

export const htmlPlugin: FileFormatPlugin = {
  id: 'html',
  detect(info: RemoteFileInfo): FormatMatch | null {
    if (info.extension === 'html' || info.extension === 'htm') {
      return { pluginId: 'html', category: 'browser-local', confidence: 0.9 };
    }
    return null;
  },
};
```

`src/file-formats/plugins/executable.ts`：

```typescript
/**
 * 可执行脚本插件
 *
 * .sh 且含 shebang（#!）→ 双击在终端运行（类 Windows 双击运行脚本）
 * 无 shebang 的 .sh 仍是文本（text 插件兜底，编辑器打开）
 */
import type { FileFormatPlugin, FormatMatch, RemoteFileInfo } from '../types';

export const executablePlugin: FileFormatPlugin = {
  id: 'executable',
  detect(info: RemoteFileInfo): FormatMatch | null {
    if (info.extension !== 'sh') return null;
    // shebang 校验：首两字节 "#!"（0x23 0x21）
    if (info.magicBytes.length >= 2 && info.magicBytes[0] === 0x23 && info.magicBytes[1] === 0x21) {
      return { pluginId: 'executable', category: 'run-script', confidence: 0.95 };
    }
    return null; // 无 shebang → 文本插件兜底
  },
};
```

- [ ] **Step 5: 运行测试确认通过**

Run: `npm run test:run -- plugins.test.ts`
Expected: 全部 PASS（原 12 + 新 ~12）

---

### Task 7: FileOpener 扩展 + text 插件排除 html

**Files:**
- Modify: `src/file-formats/FileOpener.ts`
- Modify: `src/file-formats/plugins/text.ts`
- Test: `src/file-formats/__tests__/opener.test.ts`

- [ ] **Step 1: 写失败测试**

在 `opener.test.ts` 末尾按现有模式追加：

```typescript
describe('文件操作扩展决策', () => {
  test('archive 决策：kind=extract，无大小确认', () => {
    const info = makeInfo({ extension: 'zip', path: '/tmp/a.zip', size: 100 * 1024 * 1024 });
    const decision = decideOpenTarget(info, createDefaultRegistry());
    expect(decision.kind).toBe('extract');
    expect(decision.needsSizeConfirm).toBe(false);
  });

  test('html 决策：kind=browser-local；超 20MB 需确认', () => {
    const small = makeInfo({ extension: 'html', path: '/tmp/a.html', size: 1024 });
    expect(decideOpenTarget(small, createDefaultRegistry()).kind).toBe('browser-local');
    expect(decideOpenTarget(small, createDefaultRegistry()).needsSizeConfirm).toBe(false);

    const big = makeInfo({ extension: 'html', path: '/tmp/a.html', size: 25 * 1024 * 1024 });
    expect(decideOpenTarget(big, createDefaultRegistry()).needsSizeConfirm).toBe(true);
  });

  test('run-script 决策：.sh 含 shebang → kind=run-script', () => {
    const info = makeInfo({ extension: 'sh', path: '/tmp/a.sh', magicBytes: [0x23, 0x21, 0x2f, 0x62] });
    expect(decideOpenTarget(info, createDefaultRegistry()).kind).toBe('run-script');
  });

  test('.sh 无 shebang → kind=text（编辑器）', () => {
    const info = makeInfo({ extension: 'sh', path: '/tmp/a.sh', magicBytes: [0x65, 0x63, 0x68] });
    expect(decideOpenTarget(info, createDefaultRegistry()).kind).toBe('text');
  });

  test('.html 被 text 插件排除（browser-local 优先）', () => {
    const info = makeInfo({ extension: 'html', path: '/tmp/a.html', isText: true, magicBytes: [0x3c] });
    expect(decideOpenTarget(info, createDefaultRegistry()).kind).toBe('browser-local');
  });

  test('原文本行为不受影响：.txt → text', () => {
    const info = makeInfo({ extension: 'txt', path: '/tmp/a.txt', isText: true, magicBytes: [0x68] });
    expect(decideOpenTarget(info, createDefaultRegistry()).kind).toBe('text');
  });
});
```

注：`makeInfo`/`decideOpenTarget`/`createDefaultRegistry` 的导入与 helper 以现有测试文件为准。`extract` kind 是否成立取决于 Task 6 的 FormatCategory 是否已扩展（本 Task 的 FileOpener 改动使其生效）。

- [ ] **Step 2: 运行测试确认失败**

Run: `npm run test:run -- opener.test.ts`
Expected: FAIL（kind=extract 分支不存在 / html 被兜底为 text）

- [ ] **Step 3: 实现**

`src/file-formats/plugins/text.ts` 的 svg 排除处扩展（同时排除 html/htm）：

```typescript
    // SVG 是文本，但应路由到图片查看器；
    // HTML 也是文本，但应路由到本地浏览器打开（html 插件）
    if (info.extension === 'svg' || info.extension === 'html' || info.extension === 'htm') {
      return null;
    }
```

`src/file-formats/FileOpener.ts` 三处修改：

（a）SIZE_LIMITS 加 browserLocal：

```typescript
export const SIZE_LIMITS = {
  /** 图片：20MB（data URI 全量加载，base64 膨胀 1.33 倍） */
  image: 20 * 1024 * 1024,
  /** PDF：30MB（pdfjs 全量加载） */
  pdf: 30 * 1024 * 1024,
  /** 文本：5MB（编辑器全量加载 + 差异计算） */
  text: 5 * 1024 * 1024,
  /** 本地打开：20MB（下载到本地临时目录） */
  browserLocal: 20 * 1024 * 1024,
} as const;
```

（b）`decideOpenTarget` 的 limit 查询改为可缺省（archive/run-script 无大小确认）：

```typescript
  const limit = SIZE_LIMITS[match.category as keyof typeof SIZE_LIMITS];
  return {
    kind: match.category,
    mimeType: match.mimeType,
    needsSizeConfirm: limit !== undefined && info.size > limit,
  };
```

（c）`createDefaultRegistry` 注册顺序（archive/html/executable 在 image/text 前，防误兜底）：

```typescript
export function createDefaultRegistry(): FileFormatRegistry {
  const registry = new FileFormatRegistry();
  registry.register(pdfPlugin);          // %PDF- magic，最精确
  registry.register(archivePlugin);       // 压缩包（zip magic + 扩展名）
  registry.register(htmlPlugin);          // HTML（browser-local）
  registry.register(executablePlugin);    // .sh + shebang（run-script）
  registry.register(imagePlugin);         // 图片 magic 表
  registry.register(textPlugin);          // isText 启发式兜底
  return registry;
}
```

顶部 import 加：

```typescript
import { archivePlugin } from './plugins/archive';
import { htmlPlugin } from './plugins/html';
import { executablePlugin } from './plugins/executable';
```

同时更新 `OpenDecision.kind` 的注释：

```typescript
  /**
   * 目标类型：
   * - 'directory'：目录（FileManager 自行导航）
   * - 'image' | 'pdf' | 'text' | 'hex'：对应应用
   * - 'extract'：压缩包（FileManager 解压流程）
   * - 'browser-local'：HTML（下载本地，系统浏览器打开）
   * - 'run-script'：脚本（终端自动执行）
   */
```

- [ ] **Step 4: 运行前端测试 + 类型检查**

Run: `npm run test:run -- file-formats` && `npx tsc --noEmit`
Expected: file-formats 全部 PASS；tsc 无错误

---

### Task 8: Terminal initialCommand

**Files:**
- Modify: `src/apps/Terminal.tsx`（4 处：TerminalInstanceProps、connectRemotePty、TerminalAppProps、tab 渲染 + initialTabIdRef）

- [ ] **Step 1: TerminalInstanceProps 加字段**

`TerminalInstanceProps` 的 `workingDirectory` 之后加：

```typescript
  initialCommand?: string | null;
```

`TerminalInstance` 解构参数同步加 `initialCommand`。

- [ ] **Step 2: connectRemotePty 加参数与注入**

`connectRemotePty` 签名加参数（`workingDirectory` 之后）：

```typescript
  initialCommand?: string | null,
```

在"6. resize 事件监听器"块（`onResizeDisposable` 注册）之后、`return sessionId;` 之前插入：

```typescript
  // 7. 注入初始命令（脚本运行：PTY stdin 有内核缓冲，shell 就绪后自然读走，无需等待）
  if (initialCommand) {
    const bytes = new TextEncoder().encode(initialCommand + '\n');
    invoke('remote_terminal_write', {
      sessionId: sessionId,
      data: Array.from(bytes),
      serverId: serverId,
    }).catch(e => log.warn('初始命令写入失败:', e));
  }
```

调用处（TerminalInstance 挂载 effect 第 6 步）同步传参：

```typescript
            await connectRemotePty(terminal, activeServerId, activeServerName, activeServerHost, activeServerPort, sessionIdRef, unlistenRefs, workingDirectory, initialCommand);
```

- [ ] **Step 3: TerminalAppProps 扩展**

```typescript
interface TerminalAppProps {
  windowId: string;
  preloadData?: {
    workingDirectory?: string;
    /** 窗口创建后自动执行的命令（脚本运行：双击 .sh 由 FileManager 注入） */
    initialCommand?: string;
  };
}
```

- [ ] **Step 4: initialTabId 标记 + 渲染传参**

TerminalApp 中"首次打开：创建第一个 tab"的 effect 记录初始 tab id：

```typescript
  // ── 首次打开：创建第一个 tab ─────────────────────────
  const initialTabIdRef = useRef<string | null>(null);
  useEffect(() => {
    if (tabs.length === 0) {
      const id = newTabId();
      initialTabIdRef.current = id;  // 记录初始 tab：initialCommand 仅对它生效
      setTabs([{ id, label: activeServer?.name || '本地演示' }]);
      setActiveTabId(id);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
```

`tabs.map` 渲染 TerminalInstance 处（约 851 行），`workingDirectory` prop 之后加：

```typescript
              initialCommand={tab.id === initialTabIdRef.current ? (preloadData?.initialCommand ?? null) : null}
```

（新建 tab 的 id ≠ initialTabIdRef，不携带 initialCommand，避免重复执行脚本）

- [ ] **Step 5: 类型检查**

Run: `npx tsc --noEmit`
Expected: 无错误

---

### Task 9: FileManager 三分支

**Files:**
- Modify: `src/apps/FileManager.tsx`（handleOpen 的 switch + import）

- [ ] **Step 1: import buildExtractCommand**

文件顶部 file-formats 相关 import 处（现有 `detectFileFormat`/`decideOpenTarget` 导入附近）加：

```typescript
import { buildExtractCommand } from '../file-formats/plugins/archive';
```

- [ ] **Step 2: switch 加三分支**

`handleOpen` 的 `switch (decision.kind)` 中，`case 'hex'` 之前插入：

```typescript
        case 'extract': {
          // 压缩包：弹窗选目标 → 服务器原生命令解压 → 刷新目录
          const stem = entry.name.replace(/\.(zip|tar|tar\.gz|tgz|tar\.bz2|tar\.xz|7z|rar)$/i, '');
          const toFolder = confirm(`解压 "${entry.name}" 到独立文件夹 "${stem}/"？\n目标位置同名文件将被覆盖。`);
          let targetDir: string;
          if (toFolder) {
            targetDir = currentPath === '/' ? `/${stem}` : `${currentPath}/${stem}`;
            try {
              // tar -C 要求目标目录存在；unzip/7z/unrar 自动创建。已存在时 mkdir 返回成功
              await invoke('remote_mkdir', { serverId: activeServerId, path: targetDir });
            } catch (err) {
              log.warn(`创建目标文件夹失败（可能已存在）: ${targetDir}`, err);
            }
          } else {
            const here = confirm(`改为解压到当前位置 "${currentPath}"？\n同名文件将被覆盖。`);
            if (!here) return;
            targetDir = currentPath;
          }
          const cmd = buildExtractCommand(fullPath, targetDir, entry.name);
          try {
            const output = await invoke<{ stdout: string; stderr: string; exitCode: number }>(
              'remote_execute_command', {
                serverId: activeServerId,
                command: cmd.command,
                args: cmd.args,
                workingDirectory: currentPath,
                timeoutSecs: 600,
              });
            if (output.exitCode === 0) {
              loadDir(currentPath);  // 解压成功：刷新目录
            } else {
              alert(`解压失败（退出码 ${output.exitCode}）：\n\n${output.stderr || output.stdout || '无输出'}`);
            }
          } catch (err) {
            const msg = String(err);
            // spawn 失败（如服务器未装 7z/unrar）走此分支
            const hint = msg.includes('No such file') || msg.includes('not found')
              ? '\n\n服务器未安装对应的解压工具，请安装后重试（如 apt install p7zip-full）'
              : '';
            alert(`解压失败: ${msg}${hint}`);
          }
          break;
        }
        case 'browser-local':
          // HTML：下载到本地临时目录 → 系统默认浏览器打开（Rust 端一体完成）
          try {
            await invoke('remote_open_locally', { serverId: activeServerId, remotePath: fullPath });
          } catch (err) {
            log.error('本地打开失败:', err);
            alert(`打开失败: ${err}`);
          }
          break;
        case 'run-script': {
          // 脚本：确认后打开终端自动执行（类 Windows 双击运行脚本）
          const cmd = `bash ${fullPath}`;
          const ok = confirm(`运行脚本？\n\n${fullPath}\n\n将在终端中执行: ${cmd}`);
          if (!ok) return;
          const dir = fullPath.slice(0, fullPath.lastIndexOf('/')) || '/';
          manager.create('terminal', {
            serverId: activeServerId,
            preloadData: { workingDirectory: dir, initialCommand: cmd },
          });
          break;
        }
```

注意：`needsSizeConfirm` 的通用确认（switch 之前）对 browser-local（20MB）已生效，无需分支内重复处理。

- [ ] **Step 3: 类型检查**

Run: `npx tsc --noEmit`
Expected: 无错误（switch 已穷尽新 kind）

---

### Task 10: 全量验证

- [ ] **Step 1: 协议 crate 测试**

Run: `cargo test`（cwd: `e:\MyWork\gnome-remote\quirel-protocol`）
Expected: 15 passed（12 原有 + 3 新 golden）

- [ ] **Step 2: Agent 测试**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && cargo test 2>&1 | grep -E 'result:' | tail -3"`
Expected: 118 passed；仅 2 个 auth::executor 预存失败（基线一致）

- [ ] **Step 3: 客户端 Rust**

Run: `cargo check`（cwd: `e:\MyWork\gnome-remote\src-tauri`）
Expected: Finished 无错误

- [ ] **Step 4: 前端类型 + 测试**

Run: `npx tsc --noEmit` && `npm run test:run`（cwd: `e:\MyWork\gnome-remote`）
Expected: tsc 通过；测试 73+12+6 ≈ 新增 ~18 通过，仅 6 个预存失败（Terminal 4 + WindowManagerContext 2，基线一致）

- [ ] **Step 5: 手动冒烟（需连接真实服务器，npm run tauri dev）**

| # | 操作 | 预期 |
|---|---|---|
| 1 | 双击 .zip | 弹窗选独立文件夹 → 解压成功 → 目录刷新出现新文件夹 |
| 2 | 双击 .tar.gz | 选"当前位置" → 解压成功刷新 |
| 3 | 双击 .7z（服务器未装 7z） | 弹"服务器未安装解压工具"提示 |
| 4 | 双击 .html | 本地系统浏览器打开页面 |
| 5 | 双击 .sh（含 shebang） | 确认弹窗 → 终端自动执行，输出可见 |
| 6 | 双击无 shebang 的 .sh | 文本编辑器打开 |
| 7 | 双击 .py / .txt | 文本编辑器打开（行为不变） |
| 8 | 断开服务器双击 .zip | 回退编辑器（现有探测失败路径） |

（dev 模式 StrictMode 下脚本可能执行两次——首次挂载的 session 会成为僵尸；生产构建不受影响。验证时知悉即可。）

- [ ] **Step 6: 提示用户提交**

按用户规则不执行 git 操作，输出涉及文件清单提示用户自行提交。

---

## Self-Review 记录

- **Spec 覆盖**：设计文档 §3 协议（Task 1-3）、§4 检测层（Task 6-7）、§5.1 解压（Task 4+9）、§5.2 HTML（Task 5+9）、§5.3 脚本（Task 8+9）、§6 安全（白名单 Task 3、确认弹窗 Task 9）、§7 错误处理（Task 9 catch 分支）、§8 测试（各 Task 内嵌 + Task 10）——全覆盖 ✓
- **占位符扫描**：无 TBD/TODO；所有代码步骤含完整代码 ✓
- **类型一致性**：`RemoteCommandOutput`（camelCase serde）↔ 前端 `{ stdout, stderr, exitCode }`；`buildExtractCommand` 在 Task 6 定义、Task 9 使用，签名一致；`initialCommand` 在 Task 8 的 4 处传递路径闭合 ✓
- **已知边界**：`makeInfo`/`decideOpenTarget` 测试 helper 以现有测试文件实际签名微调（Task 6/7 Step 1 已注明）；`execute_as_user` 闭包内 `?` 返回类型与 `spawn_blocking` 包装兼容（Task 3 实现保持原闭包结构）
