# 传输错误文案治理与孤儿临时文件回收 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 治理传输 Error 状态的用户文案（不暴露 stream/帧/库错误等实现细节），回收用户显式移除任务后的孤儿临时文件，并修复中断下载重试必失败的数据错位 bug。

**Architecture:** `mark_failed` 是 `task.error` 的唯一写入点，在该处单点接入前缀映射函数 `user_facing_transfer_error`——底层技术文案转为用户视角，原始错误完整记录 tracing 日志；Agent 直传的远端语义文案（如「远程文件不存在」）原样保留。下载临时文件为确定性命名（`quirel_{hash16}.tmp`），新增 `delete_transfer_temp(local_path)` 命令按路径重算哈希定位删除，命令内部 spawn 后台重试（Windows 文件占用），前端在 TaskCard 显式移除任务时 fire-and-forget 调用。网络错误退出路径保留临时文件，使「切回重试」真正断点续传。

**Tech Stack:** Rust (Tauri 2, QUIC) / React + TypeScript + Vitest + Testing Library

**设计决策（已与用户确认）：**
1. 文案映射放 **Rust `mark_failed` 单点**（与 Interrupted 文案管理方式一致），不引入错误码、不改协议
2. 中断续传数据错位 bug **本轮一并修复**（错误退出路径 `writer.preserve()`）
3. `delete_transfer_temp` 删除被占用文件时**命令内后台重试**（500ms × 10 次），命令立即返回
4. 清理触发范围：**仅用户显式移除任务记录**（TaskCard X 按钮）；5 分钟周期清理器淘汰记录时**不删**（前端记录还在、可重试续传）
5. 前端 `restartTransfer` 兜底重发路径**不删**临时文件（新任务需要它续传）

**观察项（明确不修，仅记录）：**
- 断网与用户操作的竞态时序
- `cancel_task` 无状态白名单校验（任意状态可被标记 Cancelled）
- 上传路径无本地临时文件（FileWriter 仅用于下载），本方案不涉及

---

## 背景知识（执行者必读）

### 关键代码事实（行号基于当前暂存版本）

**错误流**：Rust `mark_failed`（transfer.rs:448-471）→ `TransferTask.error` → 前端 TaskCard（:374-376）直接显示 `task.error` 及其 `title`。`mark_failed` 的终态守卫（:456）已挡住迟到的失败标记（保留 Interrupted 状态与文案）。

**mark_failed 的 13 个调用点与错误来源**：
- :650 / :1397 —— 后台任务 `perform_transfer` 的 `Err` 透传（含握手、上传循环经 `?` 传播的所有底层错误）
- :1818 「部分 stream 上传失败」、:1842 Agent 合并错误、:1926/:1989 Agent `FileTransferComplete` 错误或「未知错误」、:1997 Agent `Payload::Error` message
- :1939 「读取数据帧失败: {e}」、:1943 「读取数据帧超时」、:1964 「读取数据块失败: {e}」、:1968 「读取数据块超时」
- :2014 「传输不完整」

**临时文件机制**：
- 确定性命名 `generate_temp_path`（:145-156）：目标文件同目录下 `quirel_{hash前16位}.tmp`，同一目标路径哈希恒定（断点续传直接定位，无需 glob）
- `FileWriter::Drop`（:1280-1291）：`is_temporary && !preserved` → 删除；`preserved` → 保留
- `preserve()`（:1263-1268）：取消/暂停检测路径调用（:1876-1883）
- **孤儿来源**：① cancelled 任务（preserve 保留 .tmp，前端删卡片后无人回收）② 进程退出时 active/paused 任务（强杀无 Drop）③ 连接断开场景见下

**中断续传 bug（本轮修复）**：连接断开 → `recv` 读失败 → :1939 `mark_failed`（终态守卫忽略）→ `return Err` → `Drop`（preserved=false）→ **.tmp 被删**。前端「切回重试」走 `retry_transfer`（:575，resume_from=已传字节）→ `find_existing_temp_file` 找不到文件 → `create_fresh_temp_file` 从 0 写，但 Agent 从断点位置发数据 → transferred(0) < file_size → :2014「传输不完整」→ **重试必然失败**。

**前端删除链路**（TaskCard.tsx）：
- `handleClose`（:301-318）：active/paused 先 `cancel_transfer` 再 `onRemove`；终态直接 `onRemove`
- `restartTransfer`（:224-246）：重试兜底成功后 `onRemove`（:233）——此路径的移除**不是放弃**，新任务正在续传 .tmp
- `onRemove` → `removeTask`（useTransferProgress.ts:256）只删 Zustand + Tauri Store
- `clearAll` / `clearCompleted` 无 UI 使用（grep 验证过），无需处理
- 前端 `TransferTask.local_path` 是可选字段（useTransferProgress.ts:53），调用前必须判空

**文案治理参照**：`src/types/errors.ts` 的 AuthErrorCode 映射表遵循「三不暴露」（不暴露协议名词/库错误原文/内部机制）；Interrupted 文案「连接已断开，传输已中断」由 Rust 侧 `cleanup_by_connection` 写入——传输文案在 Rust 侧管理是既有惯例。

### 执行环境（Windows / PowerShell）

- **Git 规则（用户硬性约束）**：允许 `git add` 暂存，**禁止执行 `git commit` / `git push` / `git reset` 等历史操作**。每个任务收尾只暂存并提示用户提交，commit 文案在计划中给出，由用户执行。
- **cargo 包缓存锁**：用户的开发会话持有 `~/.cargo` 锁，直接 `cargo test` 可能阻塞。隔离方案（上轮验证可行）：
  ```powershell
  # 1. 临时 CARGO_HOME：registry 用 junction 指向真实缓存（只读共享，绕开锁）
  $tmpCargo = Join-Path $env:TEMP ("cargo-test-home")
  New-Item -ItemType Directory -Path $tmpCargo -Force | Out-Null
  New-Item -ItemType Junction -Path (Join-Path $tmpCargo "registry") -Target "$env:USERPROFILE\.cargo\registry" -Force | Out-Null
  # 2. 离线测试 + 独立 target 目录
  $env:CARGO_HOME = $tmpCargo
  cargo test --offline --target-dir target-test
  ```
- **target-test 目录清理必须用 .NET API**（`Remove-Item` 会 Access denied）：
  ```powershell
  Get-ChildItem -LiteralPath target-test -Recurse -Force -File | ForEach-Object { [System.IO.File]::Delete($_.FullName) }
  Get-ChildItem -LiteralPath target-test -Recurse -Force -Directory | Sort-Object { $_.FullName.Length } -Descending | ForEach-Object { [System.IO.Directory]::Delete($_.FullName) }
  [System.IO.Directory]::Delete((Resolve-Path target-test).Path)
  ```
- 前端验证：`npm run test:run`（基线 223/223 全绿、23 文件）+ `npx tsc --noEmit`，在 `e:\MyWork\gnome-remote` 执行
- 上轮 12 文件已暂存未提交；本轮改动继续 `git add` 追加暂存即可

---

### Task 1: Rust — user_facing_transfer_error 文案映射 + mark_failed 接入

**Files:**
- Modify: `src-tauri/src/transfer.rs`（mark_failed :448-471；文件末尾已有 `#[cfg(test)] mod tests`，追加测试）

- [ ] **Step 1: 写失败测试（追加到文件末尾现有 `mod tests` 中）**

```rust
    // ── user_facing_transfer_error 文案映射 ──────────────────────

    #[test]
    fn network_read_error_is_user_facing() {
        // 不暴露 stream/帧/库错误原文（如 "connection closed"）
        assert_eq!(
            user_facing_transfer_error("读取数据帧失败: connection closed"),
            "与服务器之间的数据传输中断"
        );
        assert_eq!(
            user_facing_transfer_error("读取数据块失败: ConnectionClosed"),
            "与服务器之间的数据传输中断"
        );
    }

    #[test]
    fn network_timeout_maps_to_timeout_text() {
        assert_eq!(
            user_facing_transfer_error("读取数据帧超时"),
            "服务器响应超时，传输已停止"
        );
        assert_eq!(
            user_facing_transfer_error("发送数据块超时 (60s)"),
            "服务器响应超时，传输已停止"
        );
    }

    #[test]
    fn stream_failure_vs_timeout_distinguished() {
        // 同一前缀 "打开 stream"，按是否含「超时」细分（前缀表 + contains 两步规则）
        assert_eq!(
            user_facing_transfer_error("打开 stream 2 失败: TimedOut"),
            "与服务器之间的数据传输中断"
        );
        assert_eq!(
            user_facing_transfer_error("打开 stream 2 超时"),
            "服务器响应超时，传输已停止"
        );
    }

    #[test]
    fn local_file_error_maps_to_local_text() {
        assert_eq!(
            user_facing_transfer_error("无法访问本地文件: (os error 13)"),
            "无法读写本地文件，请检查文件权限和磁盘状态"
        );
        assert_eq!(
            user_facing_transfer_error("写入文件失败: 磁盘已满"),
            "无法读写本地文件，请检查文件权限和磁盘状态"
        );
    }

    #[test]
    fn save_error_maps_to_save_text() {
        assert_eq!(
            user_facing_transfer_error("重命名文件失败: 拒绝访问"),
            "文件保存失败，请检查保存目录的写入权限"
        );
        assert_eq!(
            user_facing_transfer_error("临时文件不存在: C:\\x\\quirel_abc.tmp (长度: 30 字符)\n最终路径: C:\\x\\a (长度: 10 字符)"),
            "文件保存失败，请检查保存目录的写入权限"
        );
    }

    #[test]
    fn upload_incomplete_maps_to_incomplete_text() {
        assert_eq!(
            user_facing_transfer_error("部分 stream 上传失败"),
            "传输中断，文件未能完整送达"
        );
        assert_eq!(
            user_facing_transfer_error("主 stream 任务 panic: JoinError"),
            "传输中断，文件未能完整送达"
        );
        assert_eq!(
            user_facing_transfer_error("传输不完整"),
            "传输中断，文件未能完整送达"
        );
    }

    #[test]
    fn agent_prefix_stripped_but_message_kept() {
        // "Agent 返回错误: " 是内部术语前缀，剥离；远端语义保留
        assert_eq!(
            user_facing_transfer_error("Agent 返回错误: 远程文件不存在: /a/b"),
            "远程文件不存在: /a/b"
        );
    }

    #[test]
    fn agent_direct_message_kept_verbatim() {
        // Agent 直传文案（FileTransferComplete error / Payload::Error）原样保留
        assert_eq!(
            user_facing_transfer_error("远程文件不存在: /a/b"),
            "远程文件不存在: /a/b"
        );
        assert_eq!(user_facing_transfer_error("未知错误"), "未知错误");
    }
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test user_facing --offline --target-dir target-test`（在 `src-tauri`，需先设置临时 CARGO_HOME，见背景知识）
Expected: 编译失败，`cannot find function user_facing_transfer_error`

- [ ] **Step 3: 实现映射函数（放在「临时文件工具函数」区块之前，约 :134 处）**

```rust
// ── 传输错误的用户视角文案映射 ──────────────────────────────────
//
// 约定（与 src/types/errors.ts 的 AuthErrorCode 映射一致）「三不暴露」：
// 不暴露协议名词（stream/帧）、不暴露库错误原文、不暴露内部机制。
// 底层技术细节只进 tracing 日志；Agent 返回的远端语义文案原样保留。

/// 网络 I/O 类技术错误的前缀集合（读写帧/建流/join/ACK 等内部术语）
const NETWORK_IO_PREFIXES: &[&str] = &[
    "读取数据帧", "读取数据块", "读取块长度", "读取块数据",
    "读帧头", "读数据帧", "读控制帧", "读取响应",
    "打开文件传输 Stream", "打开 stream", "打开 Stream",
    "发送请求", "发送块", "发送数据", "发送完成", "发送 join",
    "刷新发送流", "写数据帧", "写控制帧",
    "读取合并结果", "stream", "主 stream", "非主 stream",
];

/// 本地文件/磁盘类错误前缀
const LOCAL_FILE_PREFIXES: &[&str] = &[
    "无法访问本地文件", "无法打开文件", "无法获取文件元数据", "文件 seek 失败",
    "读取文件失败", "读取任务 panic", "打开保留的临时文件失败",
    "无法打开临时文件", "无法获取临时文件元数据", "无法创建临时文件",
    "定位到文件末尾失败", "写入文件失败", "刷新文件失败",
    "同步文件失败", "同步临时文件到磁盘失败", "临时文件大小不匹配",
    "同步后重新获取文件元数据失败",
];

/// 保存（原子重命名）类错误前缀
const SAVE_PREFIXES: &[&str] = &["重命名文件失败", "临时文件不存在"];

/// 上传完整性类错误前缀
const UPLOAD_INCOMPLETE_PREFIXES: &[&str] =
    &["部分 stream 上传失败", "主 stream 任务 panic", "非主 stream"];

/// 把底层技术错误文案转换为用户视角文案
///
/// 匹配顺序即优先级；匹配不到任何已知模式时原样返回
/// （Agent 直传的远端语义文案走此兜底，如「远程文件不存在: /x」）
fn user_facing_transfer_error(raw: &str) -> String {
    // 1. Agent 拒绝：剥离内部术语前缀，保留远端语义
    if let Some(msg) = raw.strip_prefix("Agent 返回错误: ") {
        return msg.to_string();
    }
    // 2. 上传完整性
    if UPLOAD_INCOMPLETE_PREFIXES.iter().any(|p| raw.starts_with(p)) || raw == "传输不完整" {
        return "传输中断，文件未能完整送达".to_string();
    }
    // 3. 保存失败
    if SAVE_PREFIXES.iter().any(|p| raw.starts_with(p)) {
        return "文件保存失败，请检查保存目录的写入权限".to_string();
    }
    // 4. 本地文件读写
    if LOCAL_FILE_PREFIXES.iter().any(|p| raw.starts_with(p)) {
        return "无法读写本地文件，请检查文件权限和磁盘状态".to_string();
    }
    // 5. 网络 I/O：按是否含「超时」细分
    if NETWORK_IO_PREFIXES.iter().any(|p| raw.starts_with(p)) {
        return if raw.contains("超时") {
            "服务器响应超时，传输已停止".to_string()
        } else {
            "与服务器之间的数据传输中断".to_string()
        };
    }
    // 6. 兜底：保留原文
    raw.to_string()
}
```

**顺序说明（勿调整）**：「主 stream 任务 panic」在 NETWORK_IO_PREFIXES（"主 stream"）与 UPLOAD_INCOMPLETE 中都出现，故上传完整性组必须先于网络组判断；「临时文件不存在」必须先于本地文件组（避免被更宽前缀误捕）。

- [ ] **Step 4: mark_failed 接入（修改 :448-471 的 mark_failed）**

在终态守卫之后、写入 task.error 之前：

```rust
        // 用户视角文案：底层技术细节不进 UI，完整原始错误记录在日志（「三不暴露」约定）
        let user_message = user_facing_transfer_error(&error);
        if user_message != error {
            tracing::warn!(task_id, raw_error = %error, user_message = %user_message, "传输失败（文案已转换）");
        } else {
            tracing::info!(task_id, error = %user_message, "传输失败");
        }

        task.status = TransferStatus::Error;
        task.error = Some(user_message);
```

（原 `task.error = Some(error);` 行替换为上述写入 `user_message`。）

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test user_facing --offline --target-dir target-test`
Expected: 8 个新测试全部通过，既有测试无回归

- [ ] **Step 6: 全量回归**

Run: `cargo test --offline --target-dir target-test`
Expected: 上轮基线 15/15 + 本轮新增全部通过

- [ ] **Step 7: 暂存**

Run: `git add src-tauri/src/transfer.rs`
（commit 由用户执行：`fix(transfer): mark_failed 错误文案映射为用户视角，底层细节只进日志`）

---

### Task 2: Rust — 网络错误退出路径保留临时文件（修复中断续传数据错位）

**Files:**
- Modify: `src-tauri/src/transfer.rs`（run_download_loop :1938-1970 区域）

无独立单测（run_download_loop 依赖真 QUIC 流），靠既有测试回归 + 手动冒烟（Task 5 清单第 1 条）验证。

- [ ] **Step 1: 裸帧模式两处错误退出加 preserve（:1938-1945）**

```rust
                Ok(Err(e)) => {
                    manager.mark_failed(task_id, format!("读取数据帧失败: {}", e)).await?;
                    // 连接断开时任务会被标记为 Interrupted：保留临时文件供切回后断点续传。
                    // （若连接仍在，任务为 Error 终态；临时文件在用户移除任务记录时由
                    //  delete_transfer_temp 回收，见 Task 3/4）
                    writer.preserve();
                    return Err(format!("读取数据帧失败: {}", e));
                }
                Err(_) => {
                    manager.mark_failed(task_id, "读取数据帧超时".to_string()).await?;
                    writer.preserve();
                    return Err("读取数据帧超时".to_string());
                }
```

- [ ] **Step 2: JSON 模式两处错误退出加 preserve（:1961-1970）**

对 `Ok(Err(e))`（读取数据块失败）与 `Err(_)`（读取数据块超时）两个分支做同样处理：`return` 前加 `writer.preserve();`（注释同上，一处写全、另一处简注「同上：保留供断点续传」即可，**保留注释**为用户规则）。

**不 preserve 的退出路径（勿动，删除 .tmp 是正确行为）**：
- :1926/:1989 `FileTransferComplete success=false` → 服务器明确拒绝，无续传意义
- :2014 「传输不完整」→ 数据错位风险文件，不能保留
- `writer.write_chunk` / `finish` 本地写入失败 → 本地磁盘问题
- 暂停/取消路径已有 preserve（:1877/:1881）

- [ ] **Step 3: 编译 + 回归**

Run: `cargo test --offline --target-dir target-test`
Expected: 编译通过，全部测试通过（无行为回归）

- [ ] **Step 4: 暂存**

Run: `git add src-tauri/src/transfer.rs`
（commit 由用户执行：`fix(transfer): 中断下载保留临时文件，修复切回重试数据错位`）

---

### Task 3: Rust — delete_transfer_temp 命令 + 注册

**Files:**
- Modify: `src-tauri/src/transfer.rs`（命令放在 `local_path_is_file` 附近 :1293-1300；测试追加到 `mod tests`）
- Modify: `src-tauri/src/lib.rs`（invoke_handler :622-629 区域）

- [ ] **Step 1: 写失败测试（追加到 `mod tests`）**

```rust
    // ── 临时文件确定性命名 ──────────────────────────────────────

    #[test]
    fn temp_path_is_deterministic_and_hash_named() {
        // 同一目标路径 → 同一临时文件名（断点续传直接定位）
        let p1 = generate_temp_path(Path::new("/data/报告.pdf"));
        let p2 = generate_temp_path(Path::new("/data/报告.pdf"));
        assert_eq!(p1, p2);
        // 命名格式：quirel_{hash}.tmp
        let name = p1.file_name().unwrap().to_string_lossy().to_string();
        assert!(name.starts_with("quirel_"), "实际: {}", name);
        assert!(name.ends_with(".tmp"), "实际: {}", name);
        // 不同目标路径不碰撞
        assert_ne!(p1, generate_temp_path(Path::new("/data/其他.zip")));
        // 临时文件与目标文件同目录（删除命令按 local_path 重算即可定位）
        assert_eq!(p1.parent(), Path::new("/data"));
    }
```

- [ ] **Step 2: 运行测试确认状态**

Run: `cargo test temp_path --offline --target-dir target-test`
Expected: 该测试直接通过（`generate_temp_path` 已存在，此为回归保护），继续实现命令

- [ ] **Step 3: 实现命令（放在 `local_path_is_file` 之后）**

```rust
/// Tauri Command: 回收下载任务的本地临时文件
///
/// 用户显式移除任务记录时调用（视为放弃断点续传）。
/// 临时文件为确定性命名（quirel_{hash}.tmp），按 local_path 重新计算即可定位，
/// 无需任务 ID（应用重启后 Rust 侧内存 map 已空，前端 Store 记录仍可触发回收）。
///
/// 幂等：文件不存在视为成功。文件被占用时（取消中的后台任务尚未释放句柄，
/// Windows 上删除会 sharing violation）由内部重试循环兜底，命令立即返回。
#[command]
pub async fn delete_transfer_temp(local_path: String) {
    let temp_path = generate_temp_path(Path::new(&local_path));
    tokio::spawn(async move {
        // 最多尝试 10 次、间隔 500ms：覆盖取消检测周期（后台任务轮询到 Cancelled
        // 后退出，FileWriter Drop 释放句柄）
        for attempt in 1..=10u32 {
            match std::fs::remove_file(&temp_path) {
                Ok(()) => {
                    tracing::debug!(?temp_path, attempt, "已回收临时文件");
                    return;
                }
                // 幂等：不存在即视为已回收
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
                Err(e) => {
                    tracing::debug!(?temp_path, attempt, error = %e, "临时文件暂不可删，稍后重试");
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
            }
        }
        tracing::warn!(?temp_path, "临时文件回收失败（多次重试后放弃）");
    });
}
```

**注意**：命令返回 `()`（fire-and-forget，最终结果只进日志）；`Path` 已在文件头部 use（generate_temp_path 签名即用），无需新增导入。

- [ ] **Step 4: 注册命令（lib.rs invoke_handler，transfer 组 :622-629 区域）**

```rust
            transfer::transfer_file,
            transfer::local_path_is_file,
            transfer::delete_transfer_temp, // 回收下载残留的临时文件（用户移除任务记录时）
            transfer::pause_transfer,
```

- [ ] **Step 5: 编译 + 回归**

Run: `cargo test --offline --target-dir target-test`
Expected: 编译通过（含 lib.rs），全部测试通过

- [ ] **Step 6: 暂存**

Run: `git add src-tauri/src/transfer.rs src-tauri/src/lib.rs`
（commit 由用户执行：`feat(transfer): delete_transfer_temp 命令回收孤儿临时文件（占用时后台重试）`）

---

### Task 4: 前端 — TaskCard 移除任务时回收临时文件

**Files:**
- Modify: `src/components/TransferStatusBar/TaskCard.tsx`（handleClose :301-318）
- Modify: `src/components/TransferStatusBar/TaskCard.test.tsx`（追加测试，先读现有 mock 模式）

- [ ] **Step 1: 写失败测试（追加到 TaskCard.test.tsx，mock 方式参照文件内现有用例）**

测试用例（每个用例 mock `@tauri-apps/api/core` 的 `invoke` 后断言调用序列）：
1. **终态下载任务点 X**：`invoke` 收到 `cancel_transfer`（不，终态无 cancel）——断言 `delete_transfer_temp` 以 `{ localPath: task.local_path }` 被调用，且 `onRemove` 被调用
2. **active 下载任务点 X**：断言 `cancel_transfer`、`delete_transfer_temp`、`onRemove` 依次都被调用
3. **上传任务点 X**：断言 `delete_transfer_temp` **未**被调用（上传无本地临时文件，且 local_path 是源文件，绝不能误删同目录哈希碰撞的他人文件）
4. **local_path 缺失的下载任务点 X**：断言 `delete_transfer_temp` **未**被调用（可选字段防御）
5. **重试兜底重发成功路径**（`retry_transfer` reject「任务不存在」→ `transfer_file` resolve）：断言 `delete_transfer_temp` **未**被调用（新任务需要 .tmp 续传，移除旧记录 ≠ 放弃）

- [ ] **Step 2: 运行测试确认失败**

Run: `npm run test:run`
Expected: 新增用例失败（delete_transfer_temp 未被调用），既有 223 用例不动

- [ ] **Step 3: 实现（handleClose 开头）**

```tsx
  /**
   * 关闭任务（先取消传输，再移除）
   */
  const handleClose = async (e: React.MouseEvent) => {
    e.stopPropagation();  // 阻止事件冒泡

    // 显式移除任务 = 放弃断点续传：回收下载残留的本地临时文件。
    // 命令内部对占用文件后台重试，这里 fire-and-forget，失败只记日志。
    // 注意：restartTransfer 兜底重发成功后的 onRemove 不走此路径（新任务正在续传临时文件）。
    if (task.direction === 'download' && task.local_path) {
      invoke('delete_transfer_temp', { localPath: task.local_path }).catch((err) => {
        log.warn('回收临时文件失败:', err);
      });
    }

    try {
      // ... 原有 cancel + onRemove 逻辑保持不变
```

- [ ] **Step 4: 运行测试确认通过**

Run: `npm run test:run && npx tsc --noEmit`
Expected: 全部通过（223 + 新增），tsc 零错误

- [ ] **Step 5: 暂存**

Run: `git add src/components/TransferStatusBar/TaskCard.tsx src/components/TransferStatusBar/TaskCard.test.tsx`
（commit 由用户执行：`feat(transfer): 移除任务记录时回收下载临时文件`）

---

### Task 5: 全量验证 + 手动冒烟清单

- [ ] **Step 1: Rust 全量**

Run: `cargo test --offline --target-dir target-test`（临时 CARGO_HOME，见背景知识）
Expected: 全部通过

- [ ] **Step 2: 前端全量**

Run: `npm run test:run && npx tsc --noEmit`
Expected: 全部通过、零类型错误

- [ ] **Step 3: 清理隔离测试产物（用 .NET API，命令见背景知识）**

- [ ] **Step 4: 确认暂存状态**

Run: `git status`
Expected: 本轮 5 个文件已暂存（transfer.rs / lib.rs / TaskCard.tsx / TaskCard.test.tsx），无意外文件

- [ ] **Step 5: 交付手动冒烟清单（写入最终汇报，供用户验证）**

1. **错误文案**：制造下载错误（如服务器文件被删后重试）→ 任务卡片显示「与服务器之间的数据传输中断」而非「读取数据帧失败: …」；日志中有 raw_error 原文
2. **中断续传**：大文件下载中断开连接 → 本地目录存在 `quirel_*.tmp` → 切回服务器重试 → 进度从断点继续 → 完成后 .tmp 消失（重命名）
3. **孤儿回收（终态）**：取消一个下载 → 删除该任务卡片 → 几秒内同目录 `quirel_*.tmp` 消失
4. **孤儿回收（进行中）**：下载进行中直接点 X → cancel → .tmp 在重试周期内（约 5s）被回收
5. **兜底重发不受影响**：重启应用后对 interrupted 任务点「切回服务器并重试」（走 transfer_file 兜底）→ 新任务正常续传，.tmp 不被误删
6. **上传回归**：上传文件 → 完成后本地源文件完好

- [ ] **Step 6: 建议 commit 文案（用户执行，可整功能或按文件分组）**

整功能单 commit：
```
fix(transfer): 治理传输错误文案并回收孤儿临时文件

- mark_failed 单点映射用户视角文案，底层细节只进日志（三不暴露）
- 中断下载保留临时文件，修复切回重试数据错位（传输不完整）
- 新增 delete_transfer_temp：确定性哈希定位、占用时后台重试
- TaskCard 移除任务记录时回收下载临时文件（重发兜底路径除外）
```

按文件分组（4 个 commit）：
1. `fix(transfer): mark_failed 错误文案映射为用户视角，底层细节只进日志`（transfer.rs 映射部分）
2. `fix(transfer): 中断下载保留临时文件，修复切回重试数据错位`（transfer.rs preserve 部分）
3. `feat(transfer): delete_transfer_temp 命令回收孤儿临时文件`（transfer.rs 命令 + lib.rs）
4. `feat(transfer): 移除任务记录时回收下载临时文件`（TaskCard.tsx + test）

注意：transfer.rs 的三部分改动在同文件内，按文件分组提交需用户自行用 `git add -p` 拆分，或接受同文件多 commit 时的整体暂存；建议整功能单 commit。

---

## 验证汇总

| 层 | 验证方式 | 基线 |
|---|---|---|
| Rust | `cargo test --offline --target-dir target-test`（临时 CARGO_HOME） | 15/15 + 新增 |
| 前端 | `npm run test:run` | 223/223 + 新增 |
| 类型 | `npx tsc --noEmit` | 零错误 |
| 手动 | Task 5 Step 5 冒烟清单 | 用户执行 |
