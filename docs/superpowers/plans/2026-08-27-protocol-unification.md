# 线上协议统一（消除双份维护）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将客户端↔Agent 的线上协议类型（Envelope/Payload 及全部消息结构体）收拢到单一 crate `quirel-protocol`，两端以 path dependency 引用，彻底消除双份维护。

**Architecture:** 新建仓库根级 crate `quirel-protocol`（仅依赖 serde + serde_json，零异步依赖），以 Agent 端 `agent/src/protocol/serde.rs` 为基准取两端并集。线上格式（JSON over QUIC stream）**字节级不变**，用 golden test 锁死兼容性。客户端 `src-tauri/src/connection.rs` 与 Agent 端 `serde.rs` 删除本地定义、改为 re-export，两端既有代码路径（`crate::connection::Payload` / `crate::protocol::Payload`）不需要改动。

**Tech Stack:** Rust + serde/serde_json（保持现状，不引入 protobuf——线上格式不变是本计划的硬约束）。

**约束（用户规则）:**
- 保留所有原注释（移动代码时注释必须一起搬）
- 不操作 git（不回退、不提交），每个任务完成后**提示用户自行提交**
- 已知分歧（须在 Task 1 核实后并入并集）：
  - Agent 独有：`CalculateDiffRequest/Response`（calc_diff）
  - 客户端独有：`GetPathSuggestionsRequest/Response`（get_path_suggestions）——当前发到 Agent 会解码失败，并入并集后 Agent 需补 match 分支返回不支持错误
  - Agent 独有：`Error` variant（"error"）——需核实客户端是否也有

---

## 文件结构总览

| 操作 | 路径 | 职责 |
|---|---|---|
| Create | `quirel-protocol/Cargo.toml` | 共享 crate 清单 |
| Create | `quirel-protocol/src/lib.rs` | crate 入口 + re-export |
| Create | `quirel-protocol/src/envelope.rs` | Envelope + Payload（全量变体） |
| Create | `quirel-protocol/src/types.rs` | FileEntry/MetricsSnapshot/DiskInfo/MountInfo/FileDiff/DiffType/TransferDirection/default_frame_mode |
| Create | `quirel-protocol/src/subscription.rs` | SubscriptionType + type_name() |
| Create | `quirel-protocol/src/stats.rs` | AuthStatsSnapshot/ConnectionStatsSnapshot/PerformanceStatsSnapshot/ResponseTimePercentiles/StatsResponse |
| Create | `quirel-protocol/tests/wire_compat.rs` | golden JSON 兼容性测试 |
| Modify | `agent/Cargo.toml` | 增加 path 依赖 |
| Modify | `agent/src/protocol/serde.rs` | 删除本地类型定义，改为 re-export |
| Modify | `agent/src/auth/stats.rs` | 删除快照结构体定义，改用共享 crate 类型 |
| Modify | `agent/src/diff.rs` | 删除 FileDiff 定义，改用共享 crate 类型 |
| Modify | `agent/src/server/quic.rs` | 为新增 Payload 变体补 match 分支 |
| Modify | `src-tauri/Cargo.toml` | 增加 path 依赖 |
| Modify | `src-tauri/src/connection.rs` | 删除 L61-L500 区间协议类型定义，改为 re-export |

---

### Task 1: 核实两端变体分歧，产出权威变体清单

**Files:** 无代码改动，只做审计（结果写入本计划末尾附录）。

- [ ] **Step 1: 提取客户端 Payload 变体列表**

```powershell
Select-String -Path e:\MyWork\gnome-remote\src-tauri\src\connection.rs -Pattern 'rename = "' | ForEach-Object { $_.Line.Trim() }
```

- [ ] **Step 2: 提取 Agent Payload 变体列表**

```powershell
Select-String -Path e:\MyWork\gnome-remote\agent\src\protocol\serde.rs -Pattern 'rename = "' | ForEach-Object { $_.Line.Trim() }
```

- [ ] **Step 3: 对照两份输出，确认以下三项分歧的准确形态**
  1. `calc_diff` / `calc_diff_resp`：仅 Agent 有 → 客户端是否真的从未发送（grep `CalculateDiff` src-tauri 全目录）
  2. `get_path_suggestions` / `path_suggestions_resp`：仅客户端有 → grep agent 全目录确认 Agent 无处理逻辑
  3. `error` variant：两端是否都有
  4. 同时核对 `agent/src/diff.rs` 的 `FileDiff` 与客户端 `connection.rs` L386-L409 的 `FileDiff/DiffType` 字段是否一致

- [ ] **Step 4: 将确认结果追加到本文件"附录：变体对照表"，标记每个变体归属（两端都有/仅A/仅B）**

本任务无提交内容（纯审计），完成后提示用户：审计完成，可以开始 Task 2。

---

### Task 2: 创建 quirel-protocol crate 骨架 + golden 兼容性测试（先写测试）

**Files:**
- Create: `quirel-protocol/Cargo.toml`
- Create: `quirel-protocol/src/lib.rs`
- Create: `quirel-protocol/tests/wire_compat.rs`

- [ ] **Step 1: 创建 crate 清单**

`quirel-protocol/Cargo.toml`：

```toml
[package]
name = "quirel-protocol"
version = "0.1.0"
edition = "2021"
description = "Quirel 客户端↔Agent 线上协议的单一真相源（JSON over QUIC stream）"

# 本 crate 是协议规范本体：
# - 只依赖 serde/serde_json，禁止引入 tokio/quinn 等运行时依赖
# - 任何字段变更都是线上破坏性变更，必须走版本协商（未来）
[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

- [ ] **Step 2: 创建占位 lib.rs（Task 3 才填类型，本步先让 crate 可编译）**

`quirel-protocol/src/lib.rs`：

```rust
// quirel-protocol: 客户端↔Agent 线上协议单一真相源
//
// 本 crate 定义的类型同时被两端引用：
// - src-tauri (Tauri 客户端)
// - agent (远程 Agent 服务端)
//
// ⚠️ 线上兼容性规则：
// - 不得改变任何 serde rename 字符串（线上 tag）
// - 不得移除字段；新增字段必须带 #[serde(default)]
// - tests/wire_compat.rs 的 golden 用例锁死字节级格式

pub mod envelope;
pub mod stats;
pub mod subscription;
pub mod types;

pub use envelope::{Envelope, Payload};
```

注意：lib.rs 引用了尚未创建的模块。本步先创建 4 个空的占位模块文件（每个只含一行注释），使 `cargo check` 通过：

`quirel-protocol/src/envelope.rs`：
```rust
// Envelope + Payload（Task 3 填充）
```

`quirel-protocol/src/types.rs`、`quirel-protocol/src/subscription.rs`、`quirel-protocol/src/stats.rs` 同样只放一行注释。

- [ ] **Step 3: 写 golden 兼容性测试（TDD——此刻必然编译失败，失败即正确）**

`quirel-protocol/tests/wire_compat.rs`：

```rust
//! 线上格式兼容性 golden 测试
//!
//! 这些断言的字节序列是当前线上真实格式（取自 agent 端 serde.rs 的序列化行为）。
//! 任何导致这些断言失败的改动都是线上破坏性变更，禁止合入。

use quirel_protocol::{Envelope, Payload};

fn roundtrip(env: &Envelope) -> Envelope {
    let bytes = env.encode().expect("编码失败");
    Envelope::decode(&bytes).expect("解码失败")
}

#[test]
fn ping_wire_format() {
    let env = Envelope::new(42, Payload::Ping { timestamp: 1700000000 });
    let json = serde_json::to_string(&env).unwrap();
    assert_eq!(
        json,
        r#"{"request_id":42,"payload":{"type":"ping","data":{"timestamp":1700000000}}}"#
    );
}

#[test]
fn auth_password_request_wire_format() {
    let env = Envelope::new(
        7,
        Payload::AuthPasswordRequest {
            username: "root".to_string(),
            password: "pw".to_string(),
        },
    );
    let json = serde_json::to_string(&env).unwrap();
    assert_eq!(
        json,
        r#"{"request_id":7,"payload":{"type":"auth_password_request","data":{"username":"root","password":"pw"}}}"#
    );
}

#[test]
fn file_transfer_request_defaults_preserved() {
    // 旧客户端不带 frame_mode/stream_count 字段 → 反序列化回退 default
    // 这是已部署的兼容性行为，golden 必须锁死
    let raw = r#"{"request_id":1,"payload":{"type":"file_transfer","data":{"direction":"upload","path":"/tmp/f"}}}"#;
    let env: Envelope = serde_json::from_str(raw).expect("旧格式必须可解码");
    match env.payload {
        Payload::FileTransferRequest { frame_mode, stream_count, .. } => {
            assert_eq!(frame_mode, "json");
            assert_eq!(stream_count, None);
        }
        _ => panic!("变体不匹配"),
    }
}

#[test]
fn read_file_resp_wire_format() {
    let env = Envelope::new(
        3,
        Payload::ReadFileResponse {
            path: "/a.txt".to_string(),
            content: "aGk=".to_string(),
            mtime: 100,
            size: 2,
        },
    );
    let json = serde_json::to_string(&env).unwrap();
    assert_eq!(
        json,
        r#"{"request_id":3,"payload":{"type":"read_file_resp","data":{"path":"/a.txt","content":"aGk=","mtime":100,"size":2}}}"#
    );
}

#[test]
fn envelope_roundtrip_all_directions() {
    let env = Envelope::new(9, Payload::FileChunk {
        session_id: "s1".to_string(),
        seq: 1,
        data: vec![0xde, 0xad],
        size: 2,
    });
    let back = roundtrip(&env);
    assert_eq!(back.request_id, 9);
    match back.payload {
        Payload::FileChunk { session_id, seq, data, size } => {
            assert_eq!((session_id.as_str(), seq, data.as_slice(), size),
                       ("s1", 1, [0xde, 0xad].as_slice(), 2));
        }
        _ => panic!("变体不匹配"),
    }
}
```

- [ ] **Step 4: 运行测试确认失败（TDD 红灯）**

```powershell
cd e:\MyWork\gnome-remote\quirel-protocol
cargo test
```

预期：编译失败（`Payload` 没有变体定义 / `Envelope::new` 不存在）——这就是本步的正确结果。

- [ ] **Step 5: 提示用户提交**

> "Task 2 完成：quirel-protocol crate 骨架 + golden 测试已就位（红灯）。建议提交：`git add quirel-protocol` 然后提交，例如 `feat: 新增 quirel-protocol 共享 crate 骨架与线上格式 golden 测试`。"

---

### Task 3: 将 Agent 端协议定义搬入共享 crate（类型迁移主体）

**Files:**
- Modify: `quirel-protocol/src/envelope.rs`（从占位填充为完整定义）
- Modify: `quirel-protocol/src/types.rs`
- Modify: `quirel-protocol/src/subscription.rs`
- Modify: `quirel-protocol/src/stats.rs`

**搬迁基准**：以 `agent/src/protocol/serde.rs` 为源（它有 Envelope::new/encode/decode 实现和完整注释），叠加 Task 1 确认的并集变体。**所有注释原样保留搬迁**。

- [ ] **Step 1: 搬迁 Envelope + Payload 到 envelope.rs**

从 `agent/src/protocol/serde.rs` 搬运 L73-L528（Envelope 定义与实现、Payload 枚举、`Payload::type_name()`），做以下调整：

1. 删除 `use crate::diff::FileDiff;` 与 `use crate::auth::stats::*;`，改为 `use crate::types::FileDiff;` 与 `use crate::stats::{AuthStatsSnapshot, ConnectionStatsSnapshot, PerformanceStatsSnapshot};`
2. `use crate::subscription::SubscriptionType;`
3. `MetricsData(MetricsSnapshot)` 改 import 为 `use crate::types::MetricsSnapshot;`
4. 按附录变体对照表补齐客户端独有变体（已知至少 `GetPathSuggestionsRequest/Response`，照客户端 `connection.rs` L281-L284 的定义和注释原样搬入），并在 `type_name()` 补对应分支
5. 若 Task 1 核实客户端无 `Error` variant，则保持仅来自 Agent（无需动作）；`CalculateDiffRequest/Response` 原样保留在并集中

- [ ] **Step 2: 搬迁基础类型到 types.rs**

从 `agent/src/protocol/serde.rs` 搬运：`default_frame_mode`（L6-L9）、`TransferDirection`（L12-L20）、`MetricsSnapshot`（L531-L541）、`DiskInfo`（L544-L549）、`MountInfo`（L552-L559）、`FileEntry`（L562-L569）。

`FileDiff/DiffType` 从**客户端** `src-tauri/src/connection.rs` L385-L409 搬运（保留客户端注释；若 Task 1 发现两端不一致，以字段名/serde 行为完全一致者为准，有差异则停下来向用户报告）。

- [ ] **Step 3: 搬迁 SubscriptionType 到 subscription.rs**

从 `agent/src/protocol/serde.rs` L22-L71 原样搬运（含 `type_name()` 实现与注释）。

- [ ] **Step 4: 搬迁统计快照到 stats.rs**

从客户端 `src-tauri/src/connection.rs` L451-L497 搬运：`AuthStatsSnapshot`、`ConnectionStatsSnapshot`、`PerformanceStatsSnapshot`、`ResponseTimePercentiles`、`StatsResponse`（保留注释）。与 Agent 端 `agent/src/auth/stats.rs` 中的同名定义对照——若字段有差异，以 Agent 端为准（Agent 是数据的产生方），并在计划附录记录差异。

- [ ] **Step 5: 运行 golden 测试（绿灯）**

```powershell
cd e:\MyWork\gnome-remote\quirel-protocol
cargo test
```

预期：5 个测试全部 PASS。若 `file_transfer_request_defaults_preserved` 失败，说明搬迁中改动了 serde 属性——回查 `#[serde(default = "default_frame_mode")]` 是否原样保留。

- [ ] **Step 6: 提示用户提交**

> "Task 3 完成：协议类型已全部入住 quirel-protocol，golden 测试通过（线上格式字节级不变）。建议提交：`git add quirel-protocol`，例如 `feat: 线上协议类型统一至 quirel-protocol（格式不变，golden 锁定）`。"

---

### Task 4: Agent 端切换到共享 crate

**Files:**
- Modify: `agent/Cargo.toml`
- Modify: `agent/src/protocol/serde.rs`
- Modify: `agent/src/auth/stats.rs`
- Modify: `agent/src/diff.rs`
- Modify: `agent/src/server/quic.rs`（补新增变体的 match 分支）

- [ ] **Step 1: 添加 path 依赖**

`agent/Cargo.toml` `[dependencies]` 末尾追加：

```toml
# 线上协议单一真相源（与 src-tauri 共享）
quirel-protocol = { path = "../quirel-protocol" }
```

- [ ] **Step 2: serde.rs 改为 re-export**

将 `agent/src/protocol/serde.rs` 全部类型定义删除，替换为：

```rust
// agent/src/protocol/serde.rs
// 线上协议定义已统一至 quirel-protocol crate（单一真相源）
// 此文件仅保留 re-export，维持既有 `crate::protocol::Payload` 引用路径不变
pub use quirel_protocol::{
    Envelope, Payload, SubscriptionType, TransferDirection,
    MetricsSnapshot, DiskInfo, MountInfo, FileEntry, FileDiff, DiffType,
    AuthStatsSnapshot, ConnectionStatsSnapshot, PerformanceStatsSnapshot,
    ResponseTimePercentiles, StatsResponse,
};

/// 消息信封快捷构造（保持既有调用点不变）
impl Envelope {
    pub fn new(request_id: u32, payload: Payload) -> Self {
        Self { request_id, payload }
    }
}
```

注意：若 `Envelope::new/encode/decode` 已在共享 crate 实现，此处 impl 块会报冲突——那就直接删掉 impl 块，只留 `pub use`。

- [ ] **Step 3: agent 内部模块去重**

- `agent/src/auth/stats.rs`：删除与 `quirel_protocol::stats` 重复的快照结构体定义，改为 `pub use quirel_protocol::{AuthStatsSnapshot, ConnectionStatsSnapshot, PerformanceStatsSnapshot, ResponseTimePercentiles};`（保留该文件内的统计逻辑代码 `StatsManager` 等）
- `agent/src/diff.rs`：删除 `FileDiff/DiffType` 定义，改为 `pub use quirel_protocol::{FileDiff, DiffType};`（保留差异算法逻辑）
- `agent/src/protocol/mod.rs`：如引用了本地模块符号，确认 re-export 链完整

- [ ] **Step 4: 编译并处理新增变体的 match 分支**

```powershell
cd e:\MyWork\gnome-remote\agent
cargo check
```

预期：`quic.rs`（或其他对 `Payload` 做 exhaustive match 的文件）报"未覆盖变体"编译错误——涉及 `GetPathSuggestionsRequest/Response`（并入并集后 Agent 新可见）。在每个报错位置补：

```rust
// 客户端请求了路径建议，但 Agent 端尚未实现该 handler
// 返回明确错误，避免静默失败
Payload::GetPathSuggestionsRequest { .. } => {
    let resp = Envelope::new(request_id, Payload::Error {
        code: 501,
        message: "path suggestions not supported".to_string(),
    });
    // 按该 match 所在函数的既有响应发送方式写回（参照相邻分支）
    // ...（照抄相邻分支的 send 代码）
}
```

若客户端也缺 `Error` variant 导致共享枚举里 `Payload::Error` 与客户端枚举不对称——没有不对称，共享枚举是并集，两端现在看到的是同一个枚举。

重复 `cargo check` 直到零错误。

- [ ] **Step 5: 运行 Agent 测试套件**

```powershell
cd e:\MyWork\gnome-remote\agent
cargo test
```

预期：既有测试全部通过（Agent 端行为不变，只是类型来源变了）。若有序列化相关测试失败，对照 golden 字符串排查是否搬迁时改了 serde 属性。

- [ ] **Step 6: 提示用户提交**

> "Task 4 完成：Agent 端已切换至共享 crate，测试全绿。建议提交：`git add agent quirel-protocol`，例如 `refactor: Agent 端线上协议切换至 quirel-protocol 共享 crate`。"

---

### Task 5: 客户端切换到共享 crate

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/connection.rs`

- [ ] **Step 1: 添加 path 依赖**

`src-tauri/Cargo.toml` `[dependencies]` 追加：

```toml
# 线上协议单一真相源（与 agent 共享）
quirel-protocol = { path = "../quirel-protocol" }
```

- [ ] **Step 2: connection.rs 删除本地协议定义并 re-export**

对 `src-tauri/src/connection.rs`：

1. 删除 L61-L497 区间的协议类型定义：`Envelope`、`Payload`、`SubscriptionType`、`FileDiff`、`DiffType`、`FileEntry`、`MetricsSnapshot`、`DiskInfo`、`MountInfo`、`AuthStatsSnapshot`、`ConnectionStatsSnapshot`、`PerformanceStatsSnapshot`、`ResponseTimePercentiles`、`StatsResponse`、`default_frame_mode` 函数
   - **保留** L16-L57 的 `ConnectionInfo/AuthMethod/Credentials`（客户端本地配置类型，非线上协议，共享 crate 不含）
   - **保留** L500 起的 `Envelope` impl（若其 new/encode/decode 与共享 crate 冲突则删除，改用共享 crate 实现）
   - **保留** L516 起的 `ConnectionManager/ActiveConnection` 及全部连接逻辑
   - **保留** L1019 起的 Tauri 命令响应结构体（`PingResult/RemoteReadDirResponse` 等——它们是 Tauri 命令层类型，不是线上协议）
2. 在删除位置放置：

```rust
// ── 线上协议（单一真相源：quirel-protocol crate）──────────
// 此前此处有一份与 agent/src/protocol/serde.rs 镜像的手写定义，
// 已收拢至共享 crate，消除双份维护。既有引用路径不变。
pub use quirel_protocol::{
    Envelope, Payload, SubscriptionType, TransferDirection,
    FileDiff, DiffType, FileEntry, MetricsSnapshot, DiskInfo, MountInfo,
    AuthStatsSnapshot, ConnectionStatsSnapshot, PerformanceStatsSnapshot,
    ResponseTimePercentiles, StatsResponse,
};
```

- [ ] **Step 3: 编译客户端**

```powershell
cd e:\MyWork\gnome-remote\src-tauri
cargo check
```

预期：若客户端曾使用 `Payload::CalculateDiffRequest` 之外的 Agent 独有变体报错——不会发生（并集只会多不会少）。若报 `Error` variant 相关错误（客户端代码此前用自己版本的 Error 定义），按共享 crate 的 `Payload::Error { code: i32, message: String }` 修正字段类型。重复直到零错误。

- [ ] **Step 4: 运行客户端测试**

```powershell
cd e:\MyWork\gnome-remote\src-tauri
cargo test
```

预期：全部通过。

- [ ] **Step 5: 前端冒烟（连接与文件读取路径走线上协议）**

```powershell
cd e:\MyWork\gnome-remote
npm run tauri dev
```

手动验证：连接一台服务器 → 打开文件管理器（走 `read_dir`）→ 打开一个文本文件（走 `read_file`）→ 打开终端（走 `terminal_spawn`）。全部正常即线上格式兼容性实证。

- [ ] **Step 6: 提示用户提交**

> "Task 5 完成：客户端切换至共享 crate，双份维护正式消除。建议提交：`git add src-tauri`，例如 `refactor: 客户端线上协议切换至 quirel-protocol，消除双份维护`。"

---

### Task 6: 收尾——协议守则文档化

**Files:**
- Modify: `docs/IPC_PROTOCOL.md`（追加一节）

- [ ] **Step 1: 在 IPC_PROTOCOL.md 追加"线上协议变更守则"章节**

```markdown
## 线上协议变更守则（quirel-protocol）

客户端↔Agent 线上协议类型统一维护于 `quirel-protocol` crate，
两端（`src-tauri`、`agent`）通过 path dependency 引用，禁止任何一端
重新本地定义协议类型。

变更规则：
1. 线上格式为 JSON（serde tag="type" content="data"），由
   `quirel-protocol/tests/wire_compat.rs` 的 golden 用例锁死。
2. 新增 Payload 变体：安全（旧端解码新消息会失败并返回 Error，
   属于可接受行为）。
3. 新增字段：必须带 `#[serde(default)]` 或 `#[serde(default = "...")]`。
4. 删除变体/字段、修改 serde rename 字符串：破坏性变更，禁止。
5. 修改后必须运行 `cd quirel-protocol && cargo test` 确认 golden 通过。
```

- [ ] **Step 2: 提示用户提交**

> "Task 6 完成：协议守则已文档化。建议提交：`git add docs`，例如 `docs: 补充线上协议变更守则`。"

---

## 附录：变体对照表（Task 1 完成后填写）

| 变体 | 线上 tag | 客户端 | Agent | 处置 |
|---|---|---|---|---|
| （待 Task 1 审计填写） | | | | |

已知待核实项：calc_diff、get_path_suggestions、error、FileDiff 字段一致性。
