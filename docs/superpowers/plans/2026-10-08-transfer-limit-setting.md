# 上传大小限制设置化 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把客户端设置页「文件→传输设置」的死占位做成真实设置：显示当前连接服务器的上传大小限制（默认 500 MB），root 可修改，立即对该 Agent 进程所有会话生效，并写回服务器 quireld.toml（保留注释）。

**Architecture:** 值的单一事实来源在 Agent 端——`AtomicU64` 热值（上传检查读它，修改后全局立即生效）+ toml_edit 写回配置文件（重启保留）。协议层新增 Get/Set/TransferLimitResponse 三个 payload 变体；**旧 Agent 识别不靠试探，靠认证期能力协商**：AuthResponse 携带可选 `capabilities` 字段，客户端认证时捕获并存入连接状态，设置页凭能力门控新协议命令（实测确认：向旧 Agent 盲发新 payload 会导致 stream 断流，客户端主循环把整条连接误判为断开并拆除）。

**Tech Stack:** Rust (Tauri 2 / quireld Agent / quirel-protocol) / toml_edit / React + TypeScript + Vitest + Testing Library

**设计决策（spec 已批准，见 `docs/superpowers/specs/2026-09-29-transfer-limit-setting-design.md`；7-9 为计划期实测后的设计补充）：**

1. 仅 root 可修改（`session.uid == 0`），非 root 只读（前端凭响应 `editable` 禁用输入）
2. toml_edit 写回 quireld.toml 保留注释；写失败则热值不回滚、`persisted=false` + warn 日志
3. 全局热生效（AtomicU64），root 修改后该服务器所有用户（含已登录会话）立即受新限制约束；重启后从写回配置恢复
4. 值域 1–102400 MB（1 MB – 100 GB）
5. 客户端不持久化该值（Agent 为单一事实来源，每次进入设置页查询）
6. 仅 upload 方向检查（现状不变），下载方向、per-user 限制不做（YAGNI）
7. **【实测补充·能力协商】** 旧 Agent 收到未知 payload 的行为链：`Envelope::decode` 失败 → `handle_stream` `?` 断流（无响应）→ 客户端 `send_and_receive_quic` 读失败 → **主循环判定「连接已断开」并拆除整条连接**（src-tauri/src/connection.rs:551-558）。spec §8 预设的「发请求看报错识别」会让用户会话直接断开，不可行。替代：AuthResponse 增加 `#[serde(default, skip_serializing_if = "Option::is_none")] capabilities: Option<Vec<String>>`（向后兼容手法与既有 `code` 字段完全一致，现有 golden 字节不变），Agent 单点填充 `["transfer_limit"]`，客户端认证时捕获，前端凭此门控——**绝不向无能力的 Agent 发送新 payload**
8. **【实测补充·断流加固】** 新 Agent 的请求流首消息解码失败改为回 `Payload::Error{code:400}` 帧后正常结束 stream（quic.rs:1140），而非 `?` 断流——未来客户端再发新 payload 时，本版 Agent 不会变成「拆客户端连接」的旧 Agent。仅改首消息路径，上传流中途的解码点（:2004/:2040/:2310）不动
9. 能力常量 `AGENT_CAPABILITIES` 定义在 `agent/src/transfer_limit.rs`，由 `send_auth_response`（quic.rs:1652，全部 18 个认证路径调用点的单点构造）统一携带
10. **【落位说明·错误映射】** spec §5 把 403/400→用户文案的映射画在客户端，§9 要求「客户端 Rust 错误映射单测」。实际落位：映射放 **Agent 端**（产生错误码的地方）——`transfer_limit::limit_error_payload` 纯函数产 `Error{403/400, 用户视角文案}`，有单测锁死（错误码语义发布后不可变更）；客户端 `get/set_transfer_limit` 原样透传 message（透传无逻辑可测），「不支持」态文案由前端能力门控 hint 承担（Task 4 有测试）。spec 的映射表语义全部兑现，测试落在真正有映射逻辑的一侧

**观察项（明确不做，仅记录）：**
- 客户端「任何 Send 失败 → 拆整条连接」的全局语义不改（connection.rs:551-558，改动风险大，本轮靠能力门控规避）
- WebSocket 认证仍处 TODO 阶段（websocket.rs:100），不上能力字段；其解码失败已只 warn 不断流
- 设置页「文件」分区的其他死占位（默认视图/排序/隐藏文件）不动

---

## 背景知识（执行者必读）

### 关键代码事实（行号基于当前暂存版本，动手前先 Read 确认）

**协议层（quirel-protocol/src/envelope.rs）**：
- `Payload` enum :125-562，`#[serde(tag = "type", content = "data")]` 邻接标记；`Error { code: i32, message: String }` 是最后一个变体（:561-562）
- `AuthResponse` :171-182 已有先例字段 `#[serde(default)] code: Option<AuthErrorCode>`（注释：旧版 Agent 不携带此字段，回退 None）
- `Payload::type_name()` :567-639（枚举遍历处需同步登记）
- golden 测试 `quirel-protocol/tests/wire_compat.rs`：`roundtrip` 辅助函数 :8-11；`auth_response_with_code_wire_format` :326-343 锁了 AuthResponse 含 code 时的字节（**该测试构造 AuthResponse，新增字段后需补 `capabilities: None`，skip_serializing_if 保证字节不变**）
- serde 邻接标记下空 struct 变体（`GetTransferLimit {}`）序列化为 `"data":{}`；真 unit 变体无 data 键。**若 Task 1 golden 断言因此红灯，以 serde 实际输出修正断言字符串**——前后端共用同一 serde 定义，形状由 serde 决定，锁住即可

**Agent 端（agent/src）**：
- handler.rs：`handle_envelope` :24（参数含 `session: &UserSession`，`session.uid` 判 root 的先例在 GetStats :161-240，403 先例 :164-173）；**`other =>` 兜底已存在（:256-259），不加 match arm 不会编译报错而是落到「未知的消息类型」——新变体的 arm 必须显式添加**；上传大小检查 :394-401（`cfg.limits.max_file_transfer_mb * 1024 * 1024`，超限文案「文件大小超过限制: {X}MB > {Y}MB」）；`error_response` :339-347（code: -1）
- config.rs：`LimitsConfig.max_file_transfer_mb` :89（`#[serde(default = "default_max_file_mb")]`），默认值函数 :204 返回 500；`load(path)` :237-256
- main.rs：`Args.config` :18（`--config`，默认 `quireld.toml`）；`run_manager_mode` :253-254 `config::load(&args.config)?` 之后是初始化挂点
- lib.rs :5-22 模块声明清单
- Cargo.toml :39 已有 `toml = "0.8"`（内部依赖 toml_edit 0.22，registry 缓存已有，离线可解）；:93 dev-dependencies 已有 `tempfile = "3"`
- quic.rs：`handle_stream` :1071（请求流入口，首消息解码在 :1140，`?` 断流是问题点）；`send_auth_response` :1652-1674（认证响应单点构造，18 个调用点）；manager/mod.rs `route_to_worker` :386（白名单适配，新 payload 返回 None → 回退本地 handler，无需改动）

**客户端 Rust（src-tauri/src/connection.rs）**：
- `remote_send` :737-771（`remote_send` 模式，get_stats :1296-1309 是命令先例）；`send_and_receive_quic` :2127-2175；`STREAM_TIMEOUT_SECS = 30` :1759
- 主循环 Send 分支 :545-561：**任何 send 失败 → response_tx 回「连接已断开」→ ConnectionLost → 拆整条连接**（能力门控的存在理由）
- 密码认证 AuthResponse 解构 :334；公钥认证最终响应解构 :2103（`perform_pubkey_auth` :1771，现返回 `Result<String, ConnectError>`）；`ActiveConnection` 结构 :78-89，唯一构造点 :386-392
- lib.rs :611 `connection::get_stats` 注册区

**前端（src）**：
- Settings.tsx：「文件」分区死占位 :987-994（「传输设置→最大传输大小」`defaultValue="1000"`，未接任何状态）；`activeServer` 从 `useServerManager()` 解构 :531-540；「系统监控」分区凭 `activeServer?.id` 判断连接的模式 :1147-1166
- 组件样式体系：st-card / st-card-header / st-option-row / st-option-label / st-input / st-input-unit / st-hint / st-btn / st-btn-primary（主操作 accent 蓝底），全部已有
- 测试 mock 先例：`src/components/TransferStatusBar/TaskCard.test.tsx` :6-9 `vi.mock("@tauri-apps/api/core", ...)` + `mockImplementation` 按命令名分发
- Tauri v2 invoke 参数：JS camelCase → Rust snake_case（先例 `localPath` → `local_path`）；响应结构体 serde 字段为 snake_case（先例 StatsResponse 的 `active_connections`），前端直接用 snake_case 键

### 执行环境（Windows / PowerShell）

- **Git 规则（用户硬性约束）**：允许 `git add` 暂存，**禁止执行 `git commit` / `git push` / `git reset` 等历史操作**。每个任务收尾只暂存并提示用户提交，commit 文案在计划中给出，由用户执行。
- **cargo 包缓存锁**：用户的开发会话持有 `~/.cargo` 锁，直接 `cargo test` 可能阻塞。隔离方案（前轮验证可行）：
  ```powershell
  # 1. 临时 CARGO_HOME：registry 用 junction 指向真实缓存（只读共享，绕开锁）
  $tmpCargo = Join-Path $env:TEMP "cargo-test-home"
  New-Item -ItemType Directory -Path $tmpCargo -Force | Out-Null
  New-Item -ItemType Junction -Path (Join-Path $tmpCargo "registry") -Target "$env:USERPROFILE\.cargo\registry" -Force | Out-Null
  # 2. 离线测试 + 独立 target 目录
  $env:CARGO_HOME = $tmpCargo
  cargo test --offline --target-dir target-test
  ```
  适用于 quirel-protocol 与 src-tauri（均为 Windows 可编译）。
- **Agent 测试在 WSL 中跑**（Agent 面向 Linux，含 unix-only 依赖；开发期 WSL 编译是既有先例）。WSL 的 CARGO_HOME 独立，不与 Windows 锁冲突；首次运行会下载依赖（toml_edit 已是 toml 0.8 的传递依赖，版本一致性有保障）：
  ```powershell
  wsl -e bash -lc "cd /mnt/e/MyWork/gnome-remote/agent && cargo test --target-dir /tmp/gnome-agent-target"
  ```
- **target-test 目录清理必须用 .NET API**（`Remove-Item` 会 Access denied）：
  ```powershell
  Get-ChildItem -LiteralPath target-test -Recurse -Force -File | ForEach-Object { [System.IO.File]::Delete($_.FullName) }
  Get-ChildItem -LiteralPath target-test -Recurse -Force -Directory | Sort-Object { $_.FullName.Length } -Descending | ForEach-Object { [System.IO.Directory]::Delete($_.FullName) }
  [System.IO.Directory]::Delete((Resolve-Path target-test).Path)
  ```
- 前端验证：`npm run test:run`（基线 228/228 全绿）+ `npx tsc --noEmit`，在 `e:\MyWork\gnome-remote` 执行
- 上一轮 5 文件已暂存未提交；本轮改动继续 `git add` 追加暂存即可

---

### Task 1: 协议层 — 三个 Payload 变体 + AuthResponse 能力协商 + golden 测试

**Files:**
- Modify: `quirel-protocol/src/envelope.rs`（AuthResponse :171-182；Payload enum 末尾 :561-562 之前；type_name :567-639）
- Modify: `quirel-protocol/tests/wire_compat.rs`（文件末尾追加新 golden；:326 构造点补字段）

- [ ] **Step 1: 写失败测试（追加到 wire_compat.rs 文件末尾）**

```rust
// ===== 上传大小限制（设置页「文件→传输设置」）+ Agent 能力协商 =====

#[test]
fn get_transfer_limit_request_wire_format() {
    // 空 struct 变体在 tag/content 下序列化为 "data":{}
    // （若断言红灯且差异仅为 data 形状，以 serde 实际输出为准修正断言并锁死）
    let env = Envelope::new(11, Payload::GetTransferLimit {});
    let json = serde_json::to_string(&env).unwrap();
    assert_eq!(
        json,
        r#"{"request_id":11,"payload":{"type":"get_transfer_limit","data":{}}}"#
    );
    let back = roundtrip(&env);
    match back.payload {
        Payload::GetTransferLimit {} => {}
        _ => panic!("变体不匹配"),
    }
}

#[test]
fn transfer_limit_response_wire_format() {
    let env = Envelope::new(12, Payload::TransferLimitResponse {
        max_file_transfer_mb: 2048,
        editable: true,
        persisted: false,
    });
    let json = serde_json::to_string(&env).unwrap();
    assert_eq!(
        json,
        r#"{"request_id":12,"payload":{"type":"transfer_limit_resp","data":{"max_file_transfer_mb":2048,"editable":true,"persisted":false}}}"#
    );
    let back = roundtrip(&env);
    match back.payload {
        Payload::TransferLimitResponse { max_file_transfer_mb, editable, persisted } => {
            assert_eq!((max_file_transfer_mb, editable, persisted), (2048, true, false));
        }
        _ => panic!("变体不匹配"),
    }
}

#[test]
fn set_transfer_limit_request_wire_format() {
    let env = Envelope::new(13, Payload::SetTransferLimit { max_file_transfer_mb: 1024 });
    let json = serde_json::to_string(&env).unwrap();
    assert_eq!(
        json,
        r#"{"request_id":13,"payload":{"type":"set_transfer_limit","data":{"max_file_transfer_mb":1024}}}"#
    );
    let back = roundtrip(&env);
    match back.payload {
        Payload::SetTransferLimit { max_file_transfer_mb } => {
            assert_eq!(max_file_transfer_mb, 1024);
        }
        _ => panic!("变体不匹配"),
    }
}

#[test]
fn auth_response_with_capabilities_wire_format() {
    // 新 Agent 认证响应携带能力列表；旧客户端 serde 忽略未知字段，向前兼容
    let env = Envelope::new(14, Payload::AuthResponse {
        success: true,
        error: None,
        session_id: Some("sess-1".to_string()),
        code: None,
        capabilities: Some(vec!["transfer_limit".to_string()]),
    });
    let json = serde_json::to_string(&env).unwrap();
    assert_eq!(
        json,
        r#"{"request_id":14,"payload":{"type":"auth_response","data":{"success":true,"error":null,"session_id":"sess-1","code":null,"capabilities":["transfer_limit"]}}}"#
    );
}

#[test]
fn auth_response_without_capabilities_compat() {
    // 旧 Agent 响应不含 capabilities → 解码回退 None。
    // 客户端凭 None 识别旧 Agent 并门控新协议命令（旧 Agent 解码不了新 payload，盲发会断流拆连）
    let raw = r#"{"request_id":7,"payload":{"type":"auth_response","data":{"success":true,"error":null,"session_id":"s1","code":null}}}"#;
    let env: Envelope = serde_json::from_str(raw).expect("旧格式必须可解码");
    match env.payload {
        Payload::AuthResponse { success, capabilities, .. } => {
            assert!(success);
            assert_eq!(capabilities, None);
        }
        _ => panic!("变体不匹配"),
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run（在 `quirel-protocol` 目录，先设临时 CARGO_HOME，见背景知识）:
```powershell
cargo test --offline --target-dir target-test
```
Expected: 编译失败——`cannot find variant GetTransferLimit` / `no field capabilities`（红灯即 TDD 起点）

- [ ] **Step 3: 实现（envelope.rs 三处 + 既有测试构造点补字段）**

**(a) AuthResponse 增加能力字段**（:171-182，紧跟 `code` 字段之后）：

```rust
    /// 认证响应
    #[serde(rename = "auth_response")]
    AuthResponse {
        /// 认证是否成功
        success: bool,
        /// 错误信息（失败时）
        error: Option<String>,
        /// 会话ID（成功时返回）
        session_id: Option<String>,
        /// 结构化错误码（失败时；旧版 Agent 不携带此字段，回退 None）
        #[serde(default)]
        code: Option<AuthErrorCode>,
        /// Agent 能力声明（认证成功时携带；旧版 Agent 不含此字段 → None）。
        /// 客户端凭此门控新协议命令（如 transfer_limit）：旧 Agent 无法解码新 payload，
        /// 盲发会导致 Agent 断流、客户端误判整条连接断开
        #[serde(default, skip_serializing_if = "Option::is_none")]
        capabilities: Option<Vec<String>>,
    },
```

（skip_serializing_if 保证 None 时字节与旧 golden 完全一致；Some 时新增 `"capabilities":[...]` 键，旧客户端 serde 忽略未知字段。）

**(b) Payload enum 新增三个变体**（插在 `#[serde(rename = "error")] Error {...}` :561-562 之前）：

```rust
    /// 查询 Agent 上传大小限制（任何已认证用户可查）
    #[serde(rename = "get_transfer_limit")]
    GetTransferLimit {},

    /// 上传大小限制响应（查询与修改共用）
    /// persisted=false 表示写回配置文件失败——热值不回滚，重启后恢复旧值
    #[serde(rename = "transfer_limit_resp")]
    TransferLimitResponse {
        max_file_transfer_mb: u64,
        /// 当前会话是否可修改（root）
        editable: bool,
        /// 修改是否已持久化到配置文件（查询响应恒 true）
        persisted: bool,
    },

    /// 修改 Agent 上传大小限制（仅 root；值域 1–102400 MB）
    #[serde(rename = "set_transfer_limit")]
    SetTransferLimit { max_file_transfer_mb: u64 },
```

**(c) type_name() 登记**（`Payload::Error { .. } => "Error",` :637 之前插入三条）：

```rust
            Payload::GetTransferLimit {} => "GetTransferLimit",
            Payload::TransferLimitResponse { .. } => "TransferLimitResponse",
            Payload::SetTransferLimit { .. } => "SetTransferLimit",
```

**(d) 补齐既有 AuthResponse 构造点**：编译错误会逐一指出。已知一处——wire_compat.rs:326-343 `auth_response_with_code_wire_format` 的构造加 `capabilities: None,`（skip_serializing_if 下字节断言不变，继续通过）。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test --offline --target-dir target-test`（在 `quirel-protocol`）
Expected: 编译通过；新增 5 个 golden 全绿；既有用例（含 auth_response_with_code / current_user_resp 等）无回归

- [ ] **Step 5: 暂存**

Run: `git add quirel-protocol/src/envelope.rs quirel-protocol/tests/wire_compat.rs`
（commit 由用户执行：`feat(protocol): 上传大小限制 payload 变体 + AuthResponse 能力协商字段`）

---

### Task 2: Agent — transfer_limit 模块（热值+写回）+ handler + 能力上报 + 断流加固

**Files:**
- Create: `agent/src/transfer_limit.rs`
- Modify: `agent/Cargo.toml`（:39 附近加 toml_edit）
- Modify: `agent/src/lib.rs`（:5-22 模块清单）
- Modify: `agent/src/handler.rs`（两个 match arm 插在 :243 之前；上传检查 :394-401 改读热值）
- Modify: `agent/src/main.rs`（run_manager_mode :254 之后加 init）
- Modify: `agent/src/server/quic.rs`（send_auth_response :1660-1668；handle_stream 首消息解码 :1140）

- [ ] **Step 1: 写失败测试（创建 agent/src/transfer_limit.rs，仅含测试模块 + 文件头注释）**

```rust
//! 上传大小限制：全局热值 + quireld.toml 写回（实现见 Step 3，本步先建红灯测试）

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_limit_preserves_comments_and_other_keys() {
        // toml_edit 核心价值：精准改值，段落注释与其他配置项原样保留
        let content = r#"# 服务配置
[server]
bind = "0.0.0.0"

# 传输限制
[limits]
max_terminal_sessions = 10
max_file_transfer_mb = 500
metrics_interval_secs = 2
"#;
        let out = apply_limit_to_toml(content, 2048).unwrap();
        assert!(out.contains("# 服务配置"), "段前注释必须保留");
        assert!(out.contains("# 传输限制"));
        assert!(out.contains("max_terminal_sessions = 10"));
        assert!(out.contains("metrics_interval_secs = 2"));
        assert!(out.contains("max_file_transfer_mb = 2048"));
        assert!(!out.contains("max_file_transfer_mb = 500"));
    }

    #[test]
    fn apply_limit_creates_missing_limits_section() {
        // 防御：老配置缺 [limits] 段落时补建（默认配置必有该段落）
        let out = apply_limit_to_toml("[server]\nbind = \"0.0.0.0\"\n", 100).unwrap();
        assert!(out.contains("[limits]"));
        assert!(out.contains("max_file_transfer_mb = 100"));
    }

    #[test]
    fn apply_limit_rejects_invalid_toml() {
        assert!(apply_limit_to_toml("not [ valid toml", 1).is_err());
    }

    #[test]
    fn set_limit_scenarios() {
        // 全局静态是进程级共享的：set_limit 相关场景合并为单个测试函数顺序执行，
        // 避免并行测试互相干扰热值
        let dir = tempfile::tempdir().unwrap();
        let cfg_path = dir.path().join("quireld.toml");
        let cfg_path_str = cfg_path.to_str().unwrap().to_string();
        std::fs::write(&cfg_path, "[limits]\nmax_file_transfer_mb = 500\n").unwrap();

        // 1) init：启动时从配置初始化热值与写回路径
        init(500, &cfg_path_str);
        assert_eq!(current_mb(), 500);

        // 2) root 修改成功：热值更新 + 写回成功
        let out = set_limit(0, 2048).unwrap();
        assert!(out.persisted);
        assert_eq!(out.max_file_transfer_mb, 2048);
        assert_eq!(current_mb(), 2048);
        assert!(std::fs::read_to_string(&cfg_path).unwrap().contains("max_file_transfer_mb = 2048"));

        // 3) 非 root 拒绝（403 语义）：热值不变
        assert_eq!(set_limit(1000, 512), Err(TransferLimitError::NotRoot));
        assert_eq!(current_mb(), 2048);

        // 4) 值域拒绝（400 语义）：0 与 102401 均非法，热值不变
        assert_eq!(set_limit(0, 0), Err(TransferLimitError::InvalidValue(0)));
        assert_eq!(set_limit(0, 102401), Err(TransferLimitError::InvalidValue(102401)));
        assert_eq!(current_mb(), 2048);

        // 5) 写回失败：热值不回滚，persisted=false（重启后回到配置旧值）
        init(500, "/nonexistent-quireld-test-dir/quireld.toml");
        let out = set_limit(0, 4096).unwrap();
        assert!(!out.persisted);
        assert_eq!(current_mb(), 4096);
    }

    #[test]
    fn limit_error_payload_maps_codes_and_user_text() {
        // 错误码语义发布后不可变更（403=权限、400=参数）；
        // 文案须为用户视角（「三不暴露」：不说 uid/session/内部机制）
        match limit_error_payload(&TransferLimitError::NotRoot) {
            Payload::Error { code, message } => {
                assert_eq!(code, 403);
                assert_eq!(message, "权限不足：需要以 root 用户连接才能修改");
            }
            _ => panic!("NotRoot 应映射为 Error payload"),
        }
        match limit_error_payload(&TransferLimitError::InvalidValue(0)) {
            Payload::Error { code, message } => {
                assert_eq!(code, 400);
                assert!(message.contains("1–102400"), "文案需包含值域: {}", message);
                assert!(message.contains("当前输入: 0"));
            }
            _ => panic!("InvalidValue 应映射为 Error payload"),
        }
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `wsl -e bash -lc "cd /mnt/e/MyWork/gnome-remote/agent && cargo test transfer_limit --target-dir /tmp/gnome-agent-target"`
Expected: 编译失败——`cannot find function apply_limit_to_toml / set_limit / init / current_mb / limit_error_payload`（红灯）

- [ ] **Step 3: 实现 transfer_limit.rs（替换文件，保留 Step 1 的测试模块）**

文件头与实现（测试模块原样保留在文件末尾）：

```rust
//! 上传大小限制：全局热值 + quireld.toml 写回
//!
//! 值的单一事实来源在 Agent 端：
//! - 内存热值（AtomicU64）：修改后立即对当前进程所有会话生效（含其他已登录用户）
//! - 配置文件持久化（toml_edit 保留注释）：重启后从配置恢复
//! 写回失败时热值不回滚（persisted=false，本次进程生命周期内仍生效）。

use std::sync::atomic::{AtomicU64, Ordering};

use crate::protocol::Payload;

lazy_static::lazy_static! {
    /// Agent 自身配置文件路径（启动时由 main 注入，写回用；测试可注入临时路径）
    static ref CONFIG_PATH: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
}

/// 全局热值：当前进程的上传大小限制（MB）。未 init 时为默认值 500
pub static MAX_FILE_TRANSFER_MB: AtomicU64 = AtomicU64::new(500);

/// 值域：1 MB – 100 GB
pub const MIN_LIMIT_MB: u64 = 1;
pub const MAX_LIMIT_MB: u64 = 102400;

/// Agent 能力声明（认证响应上报给客户端）。
/// 客户端凭此决定是否可用新协议命令——避免向旧 Agent 发送其无法解码的 payload
/// （旧 Agent 解码失败会断流，客户端主循环会把整条连接误判为断开并拆除）。
/// 新增协议功能时在此追加。
pub const AGENT_CAPABILITIES: &[&str] = &["transfer_limit"];

/// 修改结果：新值 + 是否已持久化到配置文件
#[derive(Debug, PartialEq)]
pub struct SetOutcome {
    pub max_file_transfer_mb: u64,
    pub persisted: bool,
}

#[derive(Debug, PartialEq)]
pub enum TransferLimitError {
    /// 非 root 会话（沿用 HTTP 语义 403）
    NotRoot,
    /// 超出值域（沿用 HTTP 语义 400）
    InvalidValue(u64),
}

/// 错误 → 协议 Error 响应（纯函数：与 handler 解耦，映射可单测）。
/// 错误码语义发布后不可变更：403=权限、400=参数；文案为用户视角（「三不暴露」）。
pub fn limit_error_payload(e: &TransferLimitError) -> Payload {
    match e {
        TransferLimitError::NotRoot => Payload::Error {
            code: 403,
            message: "权限不足：需要以 root 用户连接才能修改".to_string(),
        },
        TransferLimitError::InvalidValue(v) => Payload::Error {
            code: 400,
            message: format!(
                "上传大小限制需在 {}–{} MB 之间（当前输入: {}）",
                MIN_LIMIT_MB,
                MAX_LIMIT_MB,
                v
            ),
        },
    }
}

/// 启动时初始化（main 在 config::load 之后调用）
pub fn init(max_mb: u64, config_path: &str) {
    MAX_FILE_TRANSFER_MB.store(max_mb, Ordering::SeqCst);
    *CONFIG_PATH.lock().unwrap() = Some(config_path.to_string());
}

/// 当前限制（MB）——上传大小检查读此热值
pub fn current_mb() -> u64 {
    MAX_FILE_TRANSFER_MB.load(Ordering::SeqCst)
}

/// 修改限制（仅 root）。root 校验先于值域校验（非 root 的非法值也报 403）。
/// 先更新热值（立即全局生效），再写回配置文件；写回失败不回滚热值。
pub fn set_limit(uid: u32, new_mb: u64) -> Result<SetOutcome, TransferLimitError> {
    if uid != 0 {
        return Err(TransferLimitError::NotRoot);
    }
    if !(MIN_LIMIT_MB..=MAX_LIMIT_MB).contains(&new_mb) {
        return Err(TransferLimitError::InvalidValue(new_mb));
    }
    // 先热生效：对该进程所有会话（含其他已登录用户）立即生效
    MAX_FILE_TRANSFER_MB.store(new_mb, Ordering::SeqCst);
    let persisted = match persist_to_config(new_mb) {
        Ok(()) => true,
        Err(e) => {
            // 热值不回滚：本次进程生命周期内已生效；重启后回到配置文件旧值
            tracing::warn!(error = %e, new_mb, "上传大小限制写回配置文件失败，重启后恢复原值");
            false
        }
    };
    Ok(SetOutcome { max_file_transfer_mb: new_mb, persisted })
}

/// 写回配置文件（读取启动时注入的路径）
fn persist_to_config(new_mb: u64) -> Result<(), String> {
    let path = CONFIG_PATH
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "配置文件路径未初始化".to_string())?;
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("读取配置文件失败: {}", e))?;
    let new_content = apply_limit_to_toml(&content, new_mb)?;
    std::fs::write(&path, new_content)
        .map_err(|e| format!("写入配置文件失败: {}", e))
}

/// 纯函数：把新限制写入 toml 文本（toml_edit 精准改值，保留注释与格式）。
/// 与配置业务解耦，便于单测。
fn apply_limit_to_toml(content: &str, new_mb: u64) -> Result<String, String> {
    let mut doc = content
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| format!("配置文件解析失败: {}", e))?;
    if !doc.contains_key("limits") {
        doc["limits"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    doc["limits"]["max_file_transfer_mb"] = toml_edit::value(new_mb);
    Ok(doc.to_string())
}
```

- [ ] **Step 4: 依赖与模块接线**

**(a) Cargo.toml**（:39 `toml = "0.8"` 之后）：

```toml
# 配置写回（保留注释精准改值；与 toml 0.8 内部使用的同一 toml_edit 版本线）
toml_edit = "0.22"
```

**(b) lib.rs 模块清单**（`pub mod transfer_session;` :18 之前，保持分组）：

```rust
pub mod transfer_limit;
pub mod transfer_session;
```

- [ ] **Step 5: 运行测试确认通过**

Run: `wsl -e bash -lc "cd /mnt/e/MyWork/gnome-remote/agent && cargo test transfer_limit --target-dir /tmp/gnome-agent-target"`
Expected: 5 个测试全绿

- [ ] **Step 6: handler.rs 两个 match arm（插在 `Payload::GetPathSuggestionsRequest` arm :243 之前；`other =>` 兜底 :256 已存在，不加 arm 会静默落到「未知的消息类型」，必须显式添加）**

```rust
        // ===== 上传大小限制（设置页「文件→传输设置」；仅 upload 方向检查）=====

        // 查询：任何已认证用户可查；editable=当前会话是否 root（前端据此禁用输入）
        Payload::GetTransferLimit {} => {
            Envelope::new(
                envelope.request_id,
                Payload::TransferLimitResponse {
                    max_file_transfer_mb: crate::transfer_limit::current_mb(),
                    editable: session.uid == 0,
                    // 查询响应无「未持久化」语义，恒 true
                    persisted: true,
                },
            )
        }

        // 修改：仅 root；成功返回新值与写回结果（403/400 沿用 HTTP 语义，文案为用户视角）
        Payload::SetTransferLimit { max_file_transfer_mb } => {
            match crate::transfer_limit::set_limit(session.uid, *max_file_transfer_mb) {
                Ok(outcome) => Envelope::new(
                    envelope.request_id,
                    Payload::TransferLimitResponse {
                        max_file_transfer_mb: outcome.max_file_transfer_mb,
                        editable: true,
                        persisted: outcome.persisted,
                    },
                ),
                // 403/400→用户文案映射抽在 transfer_limit::limit_error_payload
                // （纯函数，单测锁死错误码语义与文案）
                Err(e) => Envelope::new(
                    envelope.request_id,
                    crate::transfer_limit::limit_error_payload(&e),
                ),
            }
        }
```

- [ ] **Step 7: handler.rs 上传检查改读热值（:394-401 替换）**

```rust
            // 检查文件大小限制（读全局热值：root 在设置页修改后立即生效，无需重启 Agent）
            let max_mb = crate::transfer_limit::current_mb();
            let max_size = max_mb * 1024 * 1024;
            if file_size > max_size {
                return Err(format!(
                    "文件大小超过限制: {}MB > {}MB",
                    file_size / (1024 * 1024),
                    max_mb
                ));
            }
```

**覆盖说明（spec §9「上传检查读热值」）**：`handle_file_transfer_request` 依赖 UserSession/UserExecutor/AuditLogger 全套运行时脚手架，为一次比较搭 handler 级单测不划算。热值语义（`set_limit` 后 `current_mb()` 立即反映新值）已由 `set_limit_scenarios` 锁定；本步接线（读 `current_mb()`）是一行改动，正确性由冒烟场景②端到端验证（改值后第二个会话上传超限被拒且文案显示新值）。

- [ ] **Step 8: main.rs 启动初始化（run_manager_mode :254 `config::load` 之后）**

```rust
    let cfg = config::load(&args.config)?;

    // 上传大小限制：初始化全局热值与配置写回路径（设置页修改时写回此文件）
    quireld::transfer_limit::init(cfg.limits.max_file_transfer_mb, &args.config);
```

- [ ] **Step 9: quic.rs 认证响应携带能力（send_auth_response :1660-1668 的构造加一个字段）**

```rust
    let response = Envelope::new(
        request_id,
        Payload::AuthResponse {
            success,
            error: error.map(|s| s.to_string()),
            session_id: session_id.map(|s| s.to_string()),
            code,
            // 能力上报：客户端凭此门控新协议命令（单点构造，18 个认证路径统一携带）
            capabilities: Some(
                crate::transfer_limit::AGENT_CAPABILITIES
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            ),
        },
    );
```

（调用点无需改动，自动携带。）

- [ ] **Step 10: quic.rs 断流加固（handle_stream 首消息解码 :1140 替换）**

原代码：
```rust
    let envelope = Envelope::decode(&data).map_err(|e| anyhow::anyhow!(e))?;
```
替换为：
```rust
    let envelope = match Envelope::decode(&data) {
        Ok(env) => env,
        Err(e) => {
            // 解码失败（如未来客户端发送本版 Agent 尚不认识的 payload）：
            // 回 Error 帧后正常结束 stream，而非 ?断流——客户端 remote_send
            // 能优雅收到错误文案；若直接断流，客户端主循环会把整条连接误判为断开并拆除
            tracing::warn!("消息解码失败: {}", e);
            let resp = Envelope::new(
                0,
                Payload::Error {
                    code: 400,
                    message: "无法识别的消息类型，请升级 Agent".to_string(),
                },
            );
            if let Ok(bytes) = resp.encode() {
                let _ = write_message(&mut send, &bytes).await;
            }
            return Ok(());
        }
    };
```

（范围仅此一处首消息路径；上传流中途解码点 :2004/:2040/:2310 有各自的上游错误处理，不动。）

- [ ] **Step 11: 编译 + 全量回归**

Run: `wsl -e bash -lc "cd /mnt/e/MyWork/gnome-remote/agent && cargo test --target-dir /tmp/gnome-agent-target"`
Expected: 编译通过，全部测试通过（transfer_limit 5 个 + 既有用例）

- [ ] **Step 12: 暂存**

Run: `git add agent/Cargo.toml agent/src/transfer_limit.rs agent/src/lib.rs agent/src/handler.rs agent/src/main.rs agent/src/server/quic.rs`
（commit 由用户执行：`feat(agent): 上传大小限制热值+写回；认证上报能力；未知 payload 不断流`）

---

### Task 3: 客户端 Rust — 认证捕获能力 + 三个命令

**Files:**
- Modify: `src-tauri/src/connection.rs`（ActiveConnection :78-89；声明 + 两条认证路径 :305-378；插入点 :386-392；新命令在 get_stats :1296-1309 之后；perform_pubkey_auth :1771/:2103）
- Modify: `src-tauri/src/lib.rs`（:611 注册区）

无新增单测：本任务是 I/O 边界接线（命令透传 + 状态捕获），核心逻辑（值域/权限/写回）已在 Agent 端单测覆盖，命令形态与既有 `get_stats` 先例一致（该命令同样无单测）。验证 = 编译 + 全量回归。

- [ ] **Step 1: ActiveConnection 增加能力字段（:78-89）**

```rust
pub struct ActiveConnection {
    #[allow(dead_code)]
    info: ConnectionInfo,
    tx: mpsc::Sender<ClientRequest>,
    // QUIC Connection（用于创建持久 Stream）
    pub quic_conn: Option<Arc<quinn::Connection>>,
    // 持久 Stream 监听任务
    subscription_task: Option<tokio::task::JoinHandle<()>>,
    // 用户主动断开标志：remote_disconnect 置位（先于 close，避免竞态误判），
    // 统一清理块据此区分 connection-lost 事件来源，前端仅对网络断开自动重连
    pub user_initiated: Arc<AtomicBool>,
    // Agent 能力声明（认证响应携带；旧 Agent 为空列表）
    pub capabilities: Vec<String>,
}
```

- [ ] **Step 2: 捕获能力——密码认证路径（:332-349）**

`match creds.method {` 之前声明（约 :304）：

```rust
        // Agent 能力声明（两条认证路径填充；旧 Agent 为空列表，用于门控新协议命令）
        let mut agent_capabilities: Vec<String> = Vec::new();
```

密码认证的 AuthResponse 解构（:334）补 `capabilities` 并在成功分支捕获：

```rust
                // 验证返回类型（增加类型检查）
                match envelope.payload {
                    Payload::AuthResponse { success, error, session_id: _, code, capabilities } => {
                        if !success {
                            // 优先结构化 code；旧版 Agent 按文案 fallback 分类
                            let err_code = code
                                .unwrap_or_else(|| classify_legacy_error(error.as_deref().unwrap_or("")));
                            // 关闭连接（Agent 确认拒绝，close reason 如实标注）
                            conn.close(0u32.into(), b"authentication failed");
                            return Err(ConnectError::with_detail(err_code, error.unwrap_or_else(|| "认证失败".to_string())));
                        }
                        // 认证成功：捕获能力声明（旧 Agent 无此字段 → 空列表）
                        agent_capabilities = capabilities.unwrap_or_default();
                    }
                    other => {
```

（`other =>` 分支及之后原样不动。）

- [ ] **Step 3: 捕获能力——公钥认证路径**

`perform_pubkey_auth`（:1771）返回类型改为 `Result<(String, Vec<String>), ConnectError>`（签名其余部分不动）；最终响应 match（:2103-2114）改为：

```rust
    match final_envelope.payload {
        Payload::AuthResponse { success, error, session_id, code, capabilities } => {
            if success {
                tracing::info!("[PubKeyAuth] 公钥认证成功: username={}, session_id={:?}", username, session_id);
                // 认证成功：携带能力声明返回（旧 Agent 无此字段 → 空列表）
                Ok((session_id, capabilities.unwrap_or_default()))
            } else {
                // 优先使用结构化 code；旧版 Agent 无 code 时按文案 fallback 分类
                let err_code = code
                    .unwrap_or_else(|| classify_legacy_error(error.as_deref().unwrap_or("")));
                tracing::error!("[PubKeyAuth] 公钥认证失败: code={:?}, error={:?}", err_code, error);
                Err(ConnectError::with_detail(err_code, error.unwrap_or_else(|| "公钥认证失败".to_string())))
            }
        }
```

（后续 `Payload::Error` / `other` 分支原样不动。）

调用点（:358-376）改为：

```rust
                let (session_id, pk_capabilities) = perform_pubkey_auth(
                    &conn,
                    creds.username.clone(),
                    private_key,
                    creds.passphrase,
                ).await.map_err(|e| {
                    // close reason 真实化：网络类（100-199）与 Agent 确认拒绝区分，
                    // 避免服务端日志将网络超时误判为认证失败
                    let reason: &'static [u8] = if (100..=199).contains(&e.code.as_i32()) {
                        b"auth network timeout"
                    } else {
                        b"authentication failed"
                    };
                    conn.close(0u32.into(), reason);
                    e
                })?;
                agent_capabilities = pk_capabilities;
```

（:376 的成功日志行原样保留。）

- [ ] **Step 4: 存入连接状态（:386-392 构造点加一行）**

```rust
            conns.insert(server_id.clone(), ActiveConnection {
                info: info.clone(),
                tx: tx.clone(),
                quic_conn: Some(Arc::new(conn.clone())),
                subscription_task: None,
                user_initiated: user_initiated.clone(),
                capabilities: agent_capabilities,
            });
```

- [ ] **Step 5: 三个新命令（get_stats :1309 之后插入）**

```rust
// ── 上传大小限制（设置页「文件→传输设置」）─────────────────────────
//
// 值的单一事实来源在 Agent 端，客户端不持久化（每次进入设置页查询）。
// 前端必须先经 get_agent_capabilities 确认支持，再调用本组命令：
// 旧 Agent 无法解码新 payload，直接发送会断流，主循环会把整条连接误判为断开。

/// Tauri Command: 查询当前连接 Agent 的能力列表
///
/// 空列表 = 旧 Agent（不支持新协议命令）；未连接返回 Err
#[tauri::command]
pub fn get_agent_capabilities(server_id: String, app: tauri::AppHandle) -> Result<Vec<String>, String> {
    let manager = app.state::<ConnectionManager>();
    let conns = manager.connections.lock().unwrap();
    conns
        .get(&server_id)
        .map(|c| c.capabilities.clone())
        .ok_or_else(|| "未连接服务器".to_string())
}

/// 上传大小限制信息（透传 Agent 响应；字段命名与 StatsResponse 一致为 snake_case）
#[derive(Debug, serde::Serialize)]
pub struct TransferLimitInfo {
    pub max_file_transfer_mb: u64,
    pub editable: bool,
    pub persisted: bool,
}

/// Tauri Command: 查询 Agent 上传大小限制（任何已认证用户）
#[tauri::command]
pub async fn get_transfer_limit(server_id: String, app: tauri::AppHandle) -> Result<TransferLimitInfo, String> {
    let resp = remote_send(server_id, Payload::GetTransferLimit {}, app).await?;
    match resp.payload {
        Payload::TransferLimitResponse { max_file_transfer_mb, editable, persisted } =>
            Ok(TransferLimitInfo { max_file_transfer_mb, editable, persisted }),
        // Agent 的 403/400 响应已是用户视角文案（映射在 Agent 端有单测锁定），原样透传
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

/// Tauri Command: 修改 Agent 上传大小限制（仅 root；立即对该服务器所有会话生效）
#[tauri::command]
pub async fn set_transfer_limit(server_id: String, max_file_transfer_mb: u64, app: tauri::AppHandle) -> Result<TransferLimitInfo, String> {
    let resp = remote_send(server_id, Payload::SetTransferLimit { max_file_transfer_mb }, app).await?;
    match resp.payload {
        Payload::TransferLimitResponse { max_file_transfer_mb, editable, persisted } =>
            Ok(TransferLimitInfo { max_file_transfer_mb, editable, persisted }),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}
```

- [ ] **Step 6: 注册命令（lib.rs :611 `connection::get_stats,` 之后）**

```rust
            connection::get_stats,
            connection::get_agent_capabilities, // Agent 能力查询（前端门控新协议命令）
            connection::get_transfer_limit,     // 查询上传大小限制（设置页）
            connection::set_transfer_limit,     // 修改上传大小限制（仅 root）
```

- [ ] **Step 7: 编译 + 全量回归**

Run（在 `src-tauri`，先设临时 CARGO_HOME，见背景知识）:
```powershell
cargo test --offline --target-dir target-test -j 2
```
Expected: 编译通过；既有 24/24 全绿，无回归

- [ ] **Step 8: 暂存**

Run: `git add src-tauri/src/connection.rs src-tauri/src/lib.rs`
（commit 由用户执行：`feat(client): 认证捕获 Agent 能力；新增上传大小限制查询/修改命令`）

---

### Task 4: 前端 — TransferLimitCard 组件 + Settings 接入

**Files:**
- Create: `src/components/TransferLimitCard.tsx`
- Create: `src/components/TransferLimitCard.test.tsx`
- Modify: `src/apps/Settings.tsx`（import 区 + 「文件」分区 :987-994 替换死占位）

- [ ] **Step 1: 写失败测试（创建 TransferLimitCard.test.tsx）**

```tsx
// src/components/TransferLimitCard.test.tsx
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

// mock Tauri invoke：按命令名分发（用例以 mockImplementation 覆盖）
const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import { TransferLimitCard } from "./TransferLimitCard";

const noTransferCalls = () =>
  invokeMock.mock.calls.every(
    ([cmd]) => !["get_transfer_limit", "set_transfer_limit"].includes(cmd as string)
  );

beforeEach(() => {
  invokeMock.mockReset();
  invokeMock.mockImplementation(async (..._args: unknown[]) => {});
});

describe("TransferLimitCard · 上传大小限制设置", () => {
  it("未连接：输入禁用，显示连接提示，不发起任何请求", () => {
    render(<TransferLimitCard serverId={null} />);
    expect(screen.getByRole("spinbutton")).toBeDisabled();
    expect(screen.getByText("连接服务器后可查看")).toBeTruthy();
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("旧 Agent：显示不支持提示，绝不调用新协议命令（盲发会拆掉连接）", async () => {
    invokeMock.mockImplementation(async () => ["metrics"]); // 无 transfer_limit 能力
    render(<TransferLimitCard serverId="s1" />);
    await waitFor(() => {
      expect(screen.getByText("当前 Agent 版本不支持此设置，请升级 Agent")).toBeTruthy();
    });
    expect(invokeMock).toHaveBeenCalledWith("get_agent_capabilities", { serverId: "s1" });
    expect(noTransferCalls()).toBe(true);
  });

  it("已连接 root：回填当前值并可修改，应用后提示已生效", async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "get_agent_capabilities") return ["transfer_limit"];
      if (cmd === "get_transfer_limit")
        return { max_file_transfer_mb: 500, editable: true, persisted: true };
      if (cmd === "set_transfer_limit")
        return { max_file_transfer_mb: 2048, editable: true, persisted: true };
      return {};
    });
    render(<TransferLimitCard serverId="s1" />);
    const input = screen.getByRole("spinbutton");
    await waitFor(() => expect((input as HTMLInputElement).value).toBe("500"));
    expect(input).not.toBeDisabled();

    fireEvent.change(input, { target: { value: "2048" } });
    fireEvent.click(screen.getByRole("button", { name: "应用" }));
    await waitFor(() => expect(screen.getByText("已生效")).toBeTruthy());
    expect(invokeMock).toHaveBeenCalledWith("set_transfer_limit", {
      serverId: "s1",
      maxFileTransferMb: 2048,
    });
  });

  it("应用成功但写回配置失败：提示重启后恢复原值", async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "get_agent_capabilities") return ["transfer_limit"];
      if (cmd === "get_transfer_limit")
        return { max_file_transfer_mb: 500, editable: true, persisted: true };
      if (cmd === "set_transfer_limit")
        return { max_file_transfer_mb: 2048, editable: true, persisted: false };
      return {};
    });
    render(<TransferLimitCard serverId="s1" />);
    await waitFor(() =>
      expect((screen.getByRole("spinbutton") as HTMLInputElement).value).toBe("500")
    );
    fireEvent.change(screen.getByRole("spinbutton"), { target: { value: "2048" } });
    fireEvent.click(screen.getByRole("button", { name: "应用" }));
    await waitFor(() =>
      expect(screen.getByText("已生效，但配置文件写入失败，重启后恢复原值")).toBeTruthy()
    );
  });

  it("非 root：回填当前值但输入禁用并提示需要 root", async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "get_agent_capabilities") return ["transfer_limit"];
      if (cmd === "get_transfer_limit")
        return { max_file_transfer_mb: 500, editable: false, persisted: true };
      return {};
    });
    render(<TransferLimitCard serverId="s1" />);
    await waitFor(() =>
      expect((screen.getByRole("spinbutton") as HTMLInputElement).value).toBe("500")
    );
    expect(screen.getByRole("spinbutton")).toBeDisabled();
    expect(screen.getByText("需要以 root 用户连接才能修改")).toBeTruthy();
  });

  it("本地校验：超出值域时应用禁用且不发送请求", async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "get_agent_capabilities") return ["transfer_limit"];
      if (cmd === "get_transfer_limit")
        return { max_file_transfer_mb: 500, editable: true, persisted: true };
      return {};
    });
    render(<TransferLimitCard serverId="s1" />);
    await waitFor(() =>
      expect((screen.getByRole("spinbutton") as HTMLInputElement).value).toBe("500")
    );
    for (const bad of ["0", "200000"]) {
      fireEvent.change(screen.getByRole("spinbutton"), { target: { value: bad } });
      expect(screen.getByRole("button", { name: "应用" })).toBeDisabled();
    }
    expect(noTransferCalls()).toBe(true);
  });
});
```

- [ ] **Step 2: 运行测试确认失败**

Run: `npm run test:run`（在 `e:\MyWork\gnome-remote`）
Expected: 新文件 6 个用例失败（无法解析 `./TransferLimitCard`），既有 228 个用例不动

- [ ] **Step 3: 实现组件（创建 TransferLimitCard.tsx）**

```tsx
// src/components/TransferLimitCard.tsx
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { createLogger } from "../utils/logger";

const log = createLogger("TransferLimitCard");

/** 值域与 Agent 端一致（1 MB – 100 GB；Agent 侧是权威校验，此处仅前置拦截） */
const MIN_LIMIT_MB = 1;
const MAX_LIMIT_MB = 102400;

/** Agent 返回的限制信息（snake_case 字段，与 StatsResponse 命名惯例一致） */
interface TransferLimitInfo {
  max_file_transfer_mb: number;
  editable: boolean;
  persisted: boolean;
}

/** 查询阶段状态（决定输入可用性与 hint 文案） */
type LimitState =
  | { kind: "disconnected" } // 未连接
  | { kind: "unsupported" }  // 旧 Agent（认证未上报 transfer_limit 能力）
  | { kind: "loading" }      // 查询中
  | { kind: "error"; message: string } // 查询失败
  | { kind: "ready"; editable: boolean }; // 已回填（editable=当前会话是否 root）

/** 应用结果提示（与查询状态分离，输入变更时清除） */
interface Feedback {
  text: string;
  warn: boolean;
}

export function TransferLimitCard({ serverId }: { serverId: string | null }) {
  const [inputValue, setInputValue] = useState("");
  const [state, setState] = useState<LimitState>({ kind: "disconnected" });
  const [feedback, setFeedback] = useState<Feedback | null>(null);

  // 进入文件分区（或连接切换）时查询：能力门控 → 实际限制值。
  // 能力门控是硬前提：旧 Agent 无法解码新 payload，盲发会断流、
  // 客户端主循环会把整条连接误判为断开并拆除
  useEffect(() => {
    if (!serverId) {
      setState({ kind: "disconnected" });
      return;
    }
    let cancelled = false;
    setState({ kind: "loading" });
    (async () => {
      try {
        // 1) 能力门控
        const caps = await invoke<string[]>("get_agent_capabilities", { serverId });
        if (cancelled) return;
        if (!caps.includes("transfer_limit")) {
          setState({ kind: "unsupported" });
          return;
        }
        // 2) 查询当前限制（Agent 为单一事实来源，客户端不持久化）
        const info = await invoke<TransferLimitInfo>("get_transfer_limit", { serverId });
        if (cancelled) return;
        setInputValue(String(info.max_file_transfer_mb));
        setState({ kind: "ready", editable: info.editable });
      } catch (e) {
        if (!cancelled) setState({ kind: "error", message: String(e) });
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [serverId]);

  const parsed = Number.parseInt(inputValue, 10);
  const isValidLocal =
    Number.isInteger(parsed) && parsed >= MIN_LIMIT_MB && parsed <= MAX_LIMIT_MB;

  const editable = state.kind === "ready" && state.editable;
  const applyDisabled = !editable || !isValidLocal;

  const handleApply = async () => {
    if (!serverId || !editable || !isValidLocal) return;
    try {
      const info = await invoke<TransferLimitInfo>("set_transfer_limit", {
        serverId,
        maxFileTransferMb: parsed,
      });
      setInputValue(String(info.max_file_transfer_mb));
      setFeedback(
        info.persisted
          ? { text: "已生效", warn: false }
          : { text: "已生效，但配置文件写入失败，重启后恢复原值", warn: true }
      );
    } catch (e) {
      // Agent 的 403/400 已是用户视角文案；其余为网络/连接类
      setFeedback({ text: String(e), warn: true });
      log.warn("修改上传大小限制失败:", e);
    }
  };

  // hint 文案（spec 状态矩阵）
  const hint = (() => {
    switch (state.kind) {
      case "disconnected":
        return "连接服务器后可查看";
      case "unsupported":
        return "当前 Agent 版本不支持此设置，请升级 Agent";
      case "loading":
        return "正在查询…";
      case "error":
        return state.message;
      case "ready":
        return state.editable
          ? "仅对当前连接的服务器生效，修改后该服务器所有用户适用"
          : "需要以 root 用户连接才能修改";
    }
  })();

  // 本地校验失败时优先展示值域提示（应用按钮同时禁用，不会发请求）
  const invalidLocal =
    state.kind === "ready" && state.editable && inputValue !== "" && !isValidLocal;
  const hintText = invalidLocal
    ? `请输入 ${MIN_LIMIT_MB} – ${MAX_LIMIT_MB} 之间的整数`
    : hint;

  return (
    <div className="st-card">
      <div className="st-card-header text-title">传输设置</div>
      <div className="st-option-row">
        <span className="st-option-label text-body">上传大小限制</span>
        <input
          type="number"
          className="st-input"
          value={inputValue}
          onChange={(e) => {
            setInputValue(e.target.value);
            setFeedback(null);
          }}
          disabled={!editable}
        />
        <span className="st-input-unit">MB</span>
        <button
          className="st-btn st-btn-primary"
          disabled={applyDisabled}
          onClick={handleApply}
        >
          应用
        </button>
      </div>
      <div className="st-hint">{hintText}</div>
      {feedback && <div className="st-hint">{feedback.text}</div>}
    </div>
  );
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `npm run test:run`
Expected: 全部通过（228 既有 + 6 新增）

- [ ] **Step 5: Settings.tsx 接入（替换死占位）**

**(a) import 区**（与其他组件 import 并列，约 :13）：

```tsx
import { TransferLimitCard } from "../components/TransferLimitCard";
```

**(b) 「文件」分区 :987-994**，把整个「传输设置」死占位卡片：

```tsx
            <div className="st-card">
              <div className="st-card-header text-title">传输设置</div>
              <div className="st-option-row">
                <span className="st-option-label text-body">最大传输大小</span>
                <input type="number" className="st-input" defaultValue="1000" />
                <span className="st-input-unit">MB</span>
              </div>
            </div>
```

替换为（activeServer 非 null 即已连接，与「系统监控」分区 :1150 的判断模式一致）：

```tsx
            {/* 上传大小限制：值的单一事实来源在 Agent 端，组件内完成能力门控+查询+应用 */}
            <TransferLimitCard serverId={activeServer?.id || null} />
```

- [ ] **Step 6: 全量前端验证**

Run: `npm run test:run && npx tsc --noEmit`
Expected: 全部通过，tsc 零错误

- [ ] **Step 7: 暂存**

Run: `git add src/components/TransferLimitCard.tsx src/components/TransferLimitCard.test.tsx src/apps/Settings.tsx`
（commit 由用户执行：`feat(settings): 设置页「文件→传输设置」接入上传大小限制卡片`）

---

### Task 5: 全量验证 + 手动冒烟

**Files:** 无新改动（纯验证；发现问题回对应 Task 修复后重跑本步）

- [ ] **Step 1: 协议层全量回归**

Run（在 `quirel-protocol`，临时 CARGO_HOME 见背景知识）:

```powershell
cargo test --offline --target-dir target-test
```

Expected: 全部通过，含 5 个新增 golden 用例；`auth_response_with_code_wire_format` 字节断言不变（`skip_serializing_if="Option::is_none"` 保证旧客户端零差异）

- [ ] **Step 2: 客户端 Rust 全量回归**

Run（在 `src-tauri`，临时 CARGO_HOME 见背景知识）:

```powershell
cargo test --offline --target-dir target-test -j 2
```

Expected: 全部通过（基线 24/24 + 新增），无新增编译警告

- [ ] **Step 3: Agent 全量回归（WSL）**

Run: `wsl -e bash -lc "cd /mnt/e/MyWork/gnome-remote/agent && cargo test --target-dir /tmp/gnome-agent-target"`

Expected: 全部通过（transfer_limit 5 个新用例 + 既有用例）

- [ ] **Step 4: 前端全量回归**

Run: `npm run test:run && npx tsc --noEmit`

Expected: 全部通过（228 基线 + 6 新增），tsc 零错误

- [ ] **Step 5: 清理编译产物**

分别在 `quirel-protocol` 与 `src-tauri` 目录执行（.NET API 删除，见背景知识；`Remove-Item` 会 Access denied）:

```powershell
Get-ChildItem -LiteralPath target-test -Recurse -Force -File | ForEach-Object { [System.IO.File]::Delete($_.FullName) }
Get-ChildItem -LiteralPath target-test -Recurse -Force -Directory | Sort-Object { $_.FullName.Length } -Descending | ForEach-Object { [System.IO.Directory]::Delete($_.FullName) }
[System.IO.Directory]::Delete((Resolve-Path target-test).Path)
```

Run: `wsl -e bash -lc "rm -rf /tmp/gnome-agent-target"`

Expected: 三处编译产物目录全部删除成功

- [ ] **Step 6: 确认暂存状态**

Run: `git status`

Expected: 本计划涉及文件全部已暂存（staged），工作区无本计划之外的意外改动；不执行 commit（用户操作）

- [ ] **Step 7: 手动冒烟（双端部署新构建，逐项核对）**

前置：客户端运行新构建；Agent 用新构建部署到测试机（`quireld` 以 systemd 运行）。

| # | 场景 | 操作 | 预期 |
|---|------|------|------|
| 1 | root 查询+修改 | root 连接 → 设置 → 文件 → 传输设置 | 显示当前限制（默认 500）；输入 2048 → 应用 → 提示「已生效」 |
| 2 | 热值即时生效 | 保持原会话，另开第二个客户端会话上传 | 上传 >2048MB 被拒、错误文案显示新值 2048；<2048MB 正常 |
| 3 | 写回持久化 | 重启 Agent（`systemctl restart quireld`）→ 重连查看 | 仍显示 2048；`vi /etc/quireld/quireld.toml` 查看 `[limits]` 段值为 2048 且原有注释保留 |
| 4 | 非 root 只读 | 普通用户连接 → 传输设置 | 值可见，输入框与应用按钮禁用，提示需 root |
| 5 | 旧 Agent 兼容 | 用旧版 Agent 连接 → 传输设置；再开终端、浏览文件 | 显示「当前 Agent 版本不支持」；终端/文件功能正常（连接未被拆除） |
| 6 | 值域校验 | 前端输入 0 / 200000；绕过前端直接发 400 | 前端两值应用按钮禁用、不发请求；Agent 侧返回 400 文案 |
| 7 | 错误文案 | 断网时点应用；只读文件系统上改值 | 文案为用户视角（如「配置文件写入失败，重启后恢复原值」），不含协议/内部字样 |

- [ ] **Step 8: 提示用户提交**

向用户确认全部验证通过后，提示执行 commit（二选一）：

整功能单提交（推荐，功能内聚）:

```bash
git commit -m "feat(transfer): 上传大小限制设置化（Agent 热值+写回 / 协议能力协商 / 设置页接入）"
```

按层分组提交（若用户偏好小步历史）: 按 Task 1→4 顺序分别提交（各 Task 暂存时已给出对应 commit 文案）。

---

## 验证汇总

| 层 | 验证方式 | 基线/预期 |
|----|----------|-----------|
| 协议（quirel-protocol） | golden 字节级测试，Windows offline | 既有用例字节不变 + 5 个新 golden 全通过 |
| Agent | 单测 + 回归，WSL Linux 目标 | transfer_limit 5 个新用例（注释保留/补建段落/非法 toml/set 场景/403·400→用户文案映射）+ 既有全通过 |
| 客户端 Rust | cargo test，Windows offline | 基线 24/24 全通过，无新增警告 |
| 前端 | vitest + tsc | 228 基线 + 6 新增全通过，tsc 零错误 |
| 端到端 | 手动冒烟 7 场景（Task 5 Step 7） | root 改值/热值/持久化/非 root/旧 Agent/值域/文案 全符合 |