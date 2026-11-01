# Quirel 全项目 Bug 与逻辑漏洞排查报告

日期：2026-11-01 · 分支：feature/v0.4 (f43570e) · 方式：只读审查，未改动任何源码

> **修复状态（2026-11-01 同日更新）**：P1×2、P2×5、P3×4、测试基建×2 已在
> feature/v0.4 修复并独立提交（74a29fd…7d41656，共 13 个提交）；websocket
> 通道按用户决策整体移除（0c414e9）。T6-02 经复跑改判为偶发不稳定测试
> （连跑两轮 5/5 通过，非代码回归），其根因排查另行开任务。T3-02（waitpid
> 超时）暂缓：合法大文件操作时长无上界，加超时会误杀，需产品决策。
> T1-01（type_name 派生）与 T-02（Linux CI 基建）未动，理由见各条目。
> agent lib 测试 108/108、protocol 34/34、src-tauri 62、前端 451/451，
> tsc 干净。
>
> **同日追加（用户实测暴露的新缺陷，均已在探针验证下修复）**：
> ① 浏览器关窗钩子读 preloadData 恒空致会话泄漏（375202c）；② 关窗销毁
> 前无预隐藏（2832d98）；③ 壳页面被 tab webview 遮挡触发 IWT 定时节流，
> 1s 心跳变 60s（bf91314）；④ **私钥/passphrase 明文入 client.log**
> （e343318，P0，探针实测捕获）；⑤ 快照恢复不校验单实例约束致双实例
> 互抢会话（bf0bfad）。探针（hb-probe/close-probe/自动连接开窗）暂留
> 分支，稳定确认后移除。

## 执行摘要

| 级别 | 数量 | 说明 |
|---|---|---|
| P1 | 2 | XSS 注入面、异步上下文阻塞 |
| P2 | 6 | 私钥权限、白名单绕过、红线违例、附加组缺失等 |
| P3 | 5 | 异味/脆弱点 |
| 测试基建 | 3 | 红测试、环境假设、覆盖率空洞 |

**Top 风险：**
1. **Terminal 错误横幅 XSS**（T6-01）：`escapeValue: false` + 手工 innerHTML，远端可控错误串可注入 Tauri webview。
2. **file handler 阻塞 async runtime**（T3-01）：fork+waitpid 在 tokio worker 上直接跑，并发文件操作可饿死 worker 线程池。
3. **quireld 私钥 0644**（T2-02）：root 服务写的自签 TLS 私钥全局可读。

计划文档：`docs/superpowers/plans/2026-11-01-bug-audit.md`。修复一律从当前 dev 拉 `fix/` 分支，本报告不附代码改动。

---

## P1

### [T6-01] Terminal 初始化失败横幅存在 HTML 注入面（远端可控）
- 位置：`src/apps/Terminal.tsx:485`、`src/i18n/index.ts:28`、`src/i18n/locales/zh-CN.ts:585`
- 现象：终端初始化失败时，`err`（invoke 失败链路的错误串，含 agent/远端透传的 `message`，如 `agent_rejected` 原文）经 `formatUnknownError` 原样取出，过 i18next 插值 `{{error}}` 后拼进字符串赋给 `container.innerHTML`。项目全局配置 `escapeValue: false`（注释「React 已转义」）——但这条路径没有经过 React 转义。错误串含 `<img src=x onerror=…>` 即在 Tauri webview 内执行 JS（该窗口持有完整 IPC 权限）。连接到被控/恶意服务器即可触发。
- 证据：
  ```tsx
  container.innerHTML = `<div class="terminal-init-failed">${i18n.t('terminal.initFailedHtml', { error: formatUnknownError(err) })}</div>`;
  ```
  ```ts
  interpolation: { escapeValue: false }, // React 已转义
  ```
  `src/types/errors.ts:229-234`：`formatUnknownError` 对 `Error.message`/裸字符串原样返回，无 HTML 转义。
- 建议：该行改用 React 渲染错误态（或对 `error` 先做 HTML 转义再进模板）；同时排查其余手工 innerHTML sink（AppIcon/AppBadge 的 svg 为内部常量，风险低）。

### [T3-01] 文件 handler 在 async 上下文直接执行阻塞 fork+waitpid
- 位置：`agent/src/auth/executor.rs:119-235`（fork/waitpid/管道阻塞读）；`agent/src/worker/handlers/file.rs:91,224,435`（async fn 内直接调用）；`agent/src/worker/mod.rs:64,170-182`（tokio::spawn 调度）
- 现象：`execute_as_user` 全程同步阻塞（fork → 子进程读完整文件 → 管道回传 → 父进程阻塞 waitpid）。file.rs 全部 13 个 handler 在 async fn 中直接调用；worker 按 `tokio::spawn` 并发分发请求。ReadFile 上限 10MB、WriteFile 无显式大小限制——几个并发的慢盘/大文件操作即可占满 tokio worker 线程池，worker 的心跳、PTY 转发、会话管理同池排队，表现为整台 agent 卡顿假死。command.rs 用了 `spawn_blocking`（command.rs:120），file.rs 没有——同类操作两种处理并存。
- 证据：
  ```rust
  // file.rs:224  async fn handle_read_file 内
  let result: AnyhowResult<FileData> = executor.execute_as_user(move || { ... fs::read(p) ... });
  ```
  ```rust
  // executor.rs:213  父进程阻塞等待
  libc::waitpid(child_pid, &mut status, 0);
  ```
- 建议：file.rs 各 handler 的 `execute_as_user` 调用包进 `spawn_blocking`（对齐 command.rs 先例），或 worker 分发层统一路由。

---

## P2

### [T2-02] quireld 自签 TLS 私钥以 0644 落盘
- 位置：`agent/src/cert.rs:44-48`
- 现象：`ensure_certificate` 生成私钥后 `fs::write` 不设权限位，umask 未配置（agent/src 全仓无 umask 调用），systemd 部署为 `User=root`（`systemd/quireld.service:11`）——私钥文件本机所有用户可读，可对后续 TLS 连接做中间人。项目其他敏感文件已用 `.mode(0o600).custom_flags(O_NOFOLLOW)`（main.rs:416/484/522），此处遗漏。
- 证据：
  ```rust
  fs::write(cert_path, &cert_pem)
      .with_context(|| format!("写入证书文件失败: {:?}", cert_path))?;
  fs::write(key_path, &key_pem)
      .with_context(|| format!("写入私钥文件失败: {:?}", key_path))?;
  ```
- 建议：对齐 main.rs 的写法（0600 + O_NOFOLLOW），并考虑对已存在的旧私钥补 `set_permissions`。

### [T2-03] 命令白名单按 argv[0] basename 匹配，可被同名校绕
- 位置：`agent/src/worker/handlers/command.rs:44-45,89-103,122`
- 现象：白名单 `["unzip","tar","7z","unrar"]` 只比较路径最后一段。客户端可先经 WriteFile 上传一个自己的 `~/tmp/unzip` 可执行文件，再以绝对路径传入 `ExecuteCommand`，通过校验后 `Command::new(&command)` 按原路径执行——即以登录用户身份执行任意代码。与注释声明的威胁模型（"即使客户端被完全控制"也无法越权）不符。缓解项：PTY 终端本就授予同一用户完整 shell，故未定 P1；但该通道无交互确认，等价于静默提权面。
- 证据：
  ```rust
  const ALLOWED_COMMANDS: &[&str] = &["unzip", "tar", "7z", "unrar"];
  ...
  let base_name = Path::new(&req.command).file_name().and_then(|s| s.to_str()).unwrap_or("");
  if !ALLOWED_COMMANDS.contains(&base_name) { ... }
  ...
  let mut cmd = std::process::Command::new(&command);
  ```
- 建议：白名单匹配 canonical 化后的完整路径（固定 `/usr/bin/unzip` 等已知集合），或至少拒绝可被用户写入目录下的命中项。

### [T2-04] 降权子进程未做 initgroups，用户附加组权限缺失
- 位置：`agent/src/auth/executor.rs:129-163`；全仓 grep `setgroups|initgroups` 仅命中 namespace.rs 的「deny」写入，无任何调用
- 现象：fork 后只做 `setgid(gid)`+`setuid(uid)`。父进程（root）的附加组被原样继承，而目标用户的附加组（如 audio/docker/tape 等功能性组）不会加载——用户在组内应有的权限在降权后反而失效（欠权）；同时若运行 quireld 的账号带有额外附加组，这些组权限会泄漏进降权子进程（越权方向）。两个方向都偏离「与终端登录后权限模型完全一致」的设计声明（path_guard.rs:4-11）。方向性影响取决于部署环境，故 P2 而非 P0。
- 证据：
  ```rust
  if libc::setgid(gid) != 0 { ... }
  if libc::setuid(uid) != 0 { ... }
  // 之后直接验证 uid/gid，无 setgroups/initgroups
  ```
- 建议：setuid 前以目标用户身份 `initgroups(username, gid)`（需要解析 /etc/group，可与 file.rs:1177 的手写解析共用代码）。

### [T5-01] Aero Snap 预览 portal 到 body 缺少 data-browser-overlay（§5.1 红线 3 违例）
- 位置：`src/components/window-shell/WindowShell.tsx:652-663`
- 现象：snap 预览明确 portal 到 `document.body`，div 上无 `data-browser-overlay`。浏览器 webview 是独立 HWND，DOM z-index 无法遮挡——浏览器窗口在下层时，snap 预览矩形会被网页内容盖住。全前端其余 body portal（BrowserApp/FileManager/ContextMenu/Dialog/ToolbarMenu/NotificationCenter/ArrivalToast）均带标记；TextEditor/Terminal 的 portal 目标是窗口标题栏槽位（非 body），无需标记。
- 证据：
  ```tsx
  const snapPreviewNode = snapPreview ? createPortal(
    <div
      className={styles.snapPreview}
      style={{ left: snapPreview.x, top: snapPreview.y, width: snapPreview.width, height: snapPreview.height }}
    />,
    document.body,
  ) : null;
  ```
- 建议：该 div 补 `data-browser-overlay`；按 AGENTS.md §5.1 回归 `WindowManager.session.test.ts`。

### [T6-02] 基线红测试：TextEditor openFile 失败横幅未出现
- 位置：`src/apps/TextEditor/TextEditor.test.tsx:100-104`（vitest 441/442 唯一失败）
- 现象：mock invoke 返回 undefined 时，期望错误内联横幅 `role="alert"` 出现并保持页签，实测超时未出现。是回归（错误横幅链路断了）或测试过期，需定性后修复——二者之一都必须处理，否则该文件的回归信号失效。
- 证据：见附录基线输出（`1 failed | 441 passed`，失败点 `findByRole('alert')`）。
- 建议：先确认实现侧 openFile 错误上报路径是否还渲染横幅；是测试过期则更新断言，是回归则修复渲染条件。

### [T2-06] websocket 端口默认开放但认证/业务整体未实现
- 位置：`agent/src/server/websocket.rs:92-118`、`agent/src/config.rs:23-25`（`ws_port` 默认启用）
- 现象：WS 通道对一切 Envelope 一律回 `Error{code:-1, "认证功能尚未实现"}`，TODO 挂在 quic 认证已完工之后（quic.rs 公钥+密码双通道完整）。安全上当前是"全拒绝"（好），但暴露了一个只回错误的公网端口，且该状态无配置开关提示、无版本规划记录。
- 证据：
  ```rust
  // TODO: Phase 3集成认证流程时恢复,需要传入 session 参数
  // let response = crate::handler::handle_envelope(&envelope, &cfg, &session).await;
  let response = crate::protocol::Envelope::new(envelope.request_id, crate::protocol::Payload::Error { code: -1, message: "认证功能尚未实现".to_string() });
  ```
- 建议：短期给 ws 监听加配置开关（默认关）或在启动日志声明其占位状态；长期按 Phase 3 计划恢复或删除该入口。

---

## P3

### [T2-05] `execute_in_user_context` 是无降权且零调用的死代码
- 位置：`agent/src/auth/executor.rs:251-260`。文档注释自述"以当前进程权限执行、不 fork 不降权"。留存在 auth 模块内极易被误用于本应隔离的路径。建议删除或改名 `execute_without_isolation` 并加 `#[doc(hidden)]`。

### [T2-07] `ChallengeManager::verify_response` 的 `_signature` 参数从未使用
- 位置：`agent/src/auth/challenge.rs:88-93`。签名验证实际由调用方 quic.rs:802 `ssh_auth.verify_signature` 完成，链路本身安全（已核实：授权检查→挑战单次消费+60s TTL→签名验证→/etc/passwd 建会话）。但「收下签名不验」的 API 形态是陷阱：未来新调用点极易漏验。建议删除该参数或把验证下沉进 ChallengeManager。

### [T3-02] `execute_as_user` 父进程 waitpid 无超时
- 位置：`agent/src/auth/executor.rs:210-235`。子进程卡死（NFS hang、FUSE 等）时父侧永久阻塞；与 T3-01 叠加会放大为文件通道整体冻结。建议对 waitpid 加超时兜底（如超时 kill 子进程）。

### [T4-01] 终端 resize 响应读取整段静默忽略
- 位置：`src-tauri/src/terminal.rs:337-347`。`let _ = tokio::time::timeout(...)` 连超时带读取错误全部吞掉（注释声明 best-effort，功能上成立）；但 resize 失败时 UI 显示新尺寸而后端实际未生效，且无日志可查。建议至少 `tracing::warn!` 记录失败。

### [T1-01] SubscriptionType::type_name 与 serde rename 手工双写
- 位置：`quirel-protocol/src/subscription.rs:47-58`。代码已自我警示"必须两边同改"，属已知的脆弱点；建议用 strum 或宏从 rename 派生，消除手工同步。另：serde 未知枚举变体会整体 decode 失败（worker/mod.rs 与 quic.rs:1143 均已优雅回错），兼容策略成立，无需改动。

---

## 测试基建

### [T-01] agent 单测在 Windows 必挂（环境假设写死）
`agent/src/collectors/processes.rs:80`：`panic!("pid 1 必然存在")`——collectors 面向 Linux /proc，测试却在三平台跑的 lib test 里。基线实测：`102 passed; 1 failed`。建议加 `#[cfg(target_os = "linux")]` 门控或按平台跳过。

### [T-02] agent 集成测试在 Windows 大面积空转
`--tests --no-fail-fast` 实测：13 个测试目标中多数 0 用例、auth_integration 5 个全部 ignored（"需要真实Linux环境"）。当前 CI/本地信号主要来自 lib test，manager/worker/server 层无有效回归网。建议引入 Linux CI 任务跑真实集成测试。

### [T-03] serversStore persist 无 version/migrate
`src/stores/serversStore.ts:42`：`persist` 未声明 version 与 migrate。当前字段演进会靠 zustand 浅合并兜底，凭证字段结构调整时存在静默丢字段风险。建议声明 version 起点并写迁移函数。

---

## 核对通过项（无问题）

- **§6 窗口坏帧红线（T4）**：`lib.rs:1053` Windows 主窗口 `transparent(false)`（非 Windows 分支 1076 的 `transparent(true)` 是 cfg 门控，红线不适用）；`no_erase_proc` 主窗口分支以 `WINDOW_BG_COLORREF` 填充、子窗口链维持「不擦」（lib.rs:1106-1145）；壁纸变更经 `set_window_background_color` 同步（WallpaperContext.tsx:61）。
- **§5.1 红线 1（T5）**：`browser_close_window`/`proxy_stop_session` 全部 3 个前端调用点均在用户操作回调或钦定 lifecycle 入口（init.ts:102），无 React effect/cleanup 触碰销毁。
- **§5.1 红线 2（T5）**：前端 1s 心跳（BrowserApp.tsx:284-287）+ runtime 侧 3s 阈值强制隐藏与恢复 re-assert（browser_window.rs:34,469,1512），链路完整。
- **公钥认证链（T2）**：授权预检 → 挑战（单次消费 + 60s TTL）→ 签名验证（ssh.rs，算法兼容性检查在位）→ 服务端 /etc/passwd 解析 uid/gid 建会话；客户端无法自报 uid（manager/protocol_adapter.rs:37-41 统一盖章）。IP 限流 + 用户名锁定双保险，且两个 cleanup 都有周期任务（quic.rs:317-319）。
- **协议数据面（T1）**：帧长上限 10MB 且 0/超限拒绝（raw_frame.rs:16,34）；chunk size 先验证后用（raw_frame.rs:71-74）；wire_compat 34 测试全绿。
- **PTY 输出洪泛（T3）**：manager↔读任务通道有界（容量 32 + 自然背压，pty_output.rs:99-100）。
- **凭证存储（T4）**：信封加密（keyring 主密钥 + AES-256-GCM 载荷落盘）、account 字符消毒防穿越（credentials.rs:60-71）。
- **file.rs 路径校验覆盖**：13 个 handler 全部走 `validate_path`（16 处，rename/copy/move 双端校验）；传输流隔离 spawn 带 namespace（file_stream.rs:665,848）。

## 覆盖说明（诚实边界）

本轮因子代理通道不可用（并发额度限制）全部内联执行，以下区域为**定向抽查**而非逐行通读：agent/manager 深层死锁环分析（锁序图未画全）、orphan_reaper/crash_detector 的时序缝隙、重连残留（quic.rs 1500 行仅审认证段）、transfer.rs/transfer_session.rs 限额绕过细节、src/apps 其余 viewer 的解析边界、i18n 缺键扫描。这些区域的 finding 数量不应被解读为"已审干净"。Phase 0 基线命令输出全量留存于会话记录。
