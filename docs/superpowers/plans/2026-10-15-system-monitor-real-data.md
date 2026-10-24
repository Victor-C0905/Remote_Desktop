# 系统监控真实化实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 消除 SystemMonitor 三处假数据：进程列表 demo 数据 → 真实进程枚举（新协议命令）、Swap 硬编码 0% → 真实 total 字段、文件系统页签伪造设备名/类型 → 真实 mounts 数据。

**Architecture:** 文件系统页签直接复用既有 `remote_get_mounts` 链路（零协议改动）；Swap 通过 `MetricsSnapshot` 加 `#[serde(default)]` 字段向后兼容扩展；进程列表走新增 `GetProcesses` 请求/响应命令 + `"processes"` 能力门控（前端凭 AuthResponse capabilities 决定是否可用，绝不盲发）。

**Tech Stack:** Rust（quirel-protocol / agent / src-tauri 三层，sysinfo 0.32）、React + vitest + @testing-library/react。

**规格:** `docs/superpowers/specs/2026-10-15-system-monitor-real-data-design.md`

**⚠️ 构建环境路由（硬约束）：**
- `quirel-protocol`、`src-tauri`、前端 npm —— 全部在 **Windows** 侧执行
- `agent` —— 必须在 **WSL** 内执行：`wsl -e bash -lc "cd /mnt/e/MyWork/gnome-remote/agent && cargo test"`
- 项目根 `rust-toolchain.toml` 钉定 1.99.0，以该文件为准

**⚠️ Git 约束（用户规则，优先于本技能模板）：** 计划中的 commit 步骤一律改为「提示用户提交」，**绝不自动执行 git commit/push/reset**，由用户自行操作。

---

## 文件结构总览

| 文件 | 动作 | 职责 |
|---|---|---|
| `quirel-protocol/src/types.rs` | 修改 | `MetricsSnapshot` 加 `swap_total_bytes`；新增 `ProcessEntry` |
| `quirel-protocol/src/envelope.rs` | 修改 | 新增 `GetProcesses`/`ProcessListResponse` 变体 + `type_name` |
| `quirel-protocol/src/lib.rs` | 修改 | 导出 `ProcessEntry` |
| `quirel-protocol/tests/wire_compat.rs` | 修改 | 4 个 golden 测试 |
| `agent/src/collectors/metrics.rs` | 修改 | `collect()` 填 `swap_total_bytes` |
| `agent/src/handler.rs` | 修改 | `collect_metrics()` 填 `swap_total_bytes`；新增 `GetProcesses` arm |
| `agent/src/collectors/processes.rs` | 新建 | 进程采集（专属 System 实例） |
| `agent/src/collectors/mod.rs` | 修改 | 挂载 `processes` 模块 |
| `agent/src/protocol/serde.rs`、`mod.rs` | 修改 | re-export `ProcessEntry` |
| `agent/src/transfer_limit.rs` | 修改 | `AGENT_CAPABILITIES` 加 `"processes"` |
| `agent/Cargo.toml` | 修改 | sysinfo 加 `user` feature |
| `src-tauri/src/connection.rs` | 修改 | 新增 `get_processes` 命令 |
| `src-tauri/src/lib.rs` | 修改 | 注册命令 |
| `src/apps/systemMonitor/display.ts` | 新建 | 展示层纯函数（toFsRow/swapDisplay/formatBytes） |
| `src/apps/systemMonitor/display.test.ts` | 新建 | 纯函数单测 |
| `src/apps/SystemMonitor.tsx` | 修改 | 删 demo 造假、接入 mounts/swap/processes |
| `src/i18n/locales/zh-CN.ts`、`en-US.ts` | 修改 | 进程空态词条 |

---

### Task 1: Swap total 字段端到端（协议 + Agent 两处构造点 + golden）

**Files:**
- Modify: `quirel-protocol/src/types.rs:24-33`（MetricsSnapshot）
- Modify: `agent/src/collectors/metrics.rs:105-114`（collect）
- Modify: `agent/src/handler.rs:344-368`（collect_metrics）
- Test: `quirel-protocol/tests/wire_compat.rs`

- [ ] **Step 1: 写失败的 golden 测试**

在 `quirel-protocol/tests/wire_compat.rs` 末尾追加（文件已 `use quirel_protocol::{ConnectionStatsSnapshot, Envelope, Payload, StatsResponse};`，需在 use 中加入 `MetricsSnapshot`）：

```rust
#[test]
fn metrics_data_wire_format() {
    let env = Envelope::new(
        1,
        Payload::MetricsData(MetricsSnapshot {
            cpu_percent: 1.0,
            mem_used_bytes: 1,
            mem_total_bytes: 2,
            swap_used_bytes: 3,
            swap_total_bytes: 4,
            disks: vec![],
            network_rx_bytes: 0,
            network_tx_bytes: 0,
            uptime_secs: 5,
        }),
    );
    let json = serde_json::to_string(&env).unwrap();
    assert_eq!(
        json,
        r#"{"request_id":1,"payload":{"type":"metrics_data","data":{"cpu_percent":1.0,"mem_used_bytes":1,"mem_total_bytes":2,"swap_used_bytes":3,"swap_total_bytes":4,"disks":[],"network_rx_bytes":0,"network_tx_bytes":0,"uptime_secs":5}}}"#
    );
}

#[test]
fn metrics_data_without_swap_total_compat() {
    // 旧版 Agent 推送的 metrics_data 不含 swap_total_bytes → 必须可解码并回退 0
    // （前端据此显示占位符，与「系统未配置 swap」同一呈现，无需区分）
    let raw = r#"{"request_id":1,"payload":{"type":"metrics_data","data":{"cpu_percent":12.5,"mem_used_bytes":1,"mem_total_bytes":2,"swap_used_bytes":0,"disks":[],"network_rx_bytes":0,"network_tx_bytes":0,"uptime_secs":10}}}"#;
    let env: Envelope = serde_json::from_str(raw).expect("旧格式必须可解码");
    match env.payload {
        Payload::MetricsData(m) => {
            assert_eq!(m.swap_total_bytes, 0);
            assert_eq!(m.cpu_percent, 12.5);
        }
        _ => panic!("变体不匹配"),
    }
}
```

- [ ] **Step 2: 运行确认编译失败（红灯）**

Run（Windows）: `cd quirel-protocol && cargo test --test wire_compat`
Expected: FAIL — `MetricsSnapshot` 无 `swap_total_bytes` 字段（编译错误）

- [ ] **Step 3: 协议加字段**

`quirel-protocol/src/types.rs` 的 `MetricsSnapshot` 在 `swap_used_bytes` 后插入：

```rust
    pub swap_used_bytes: u64,
    /// 交换分区总量。旧版 Agent 不携带此字段（#[serde(default)] 回退 0）；
    /// 0 = 旧 Agent 或未配置 swap，前端统一显示占位符
    #[serde(default)]
    pub swap_total_bytes: u64,
```

- [ ] **Step 4: 运行协议测试（绿灯）**

Run: `cd quirel-protocol && cargo test --test wire_compat`
Expected: 全部 PASS（含既有 golden，字节未破坏）

- [ ] **Step 5: Agent 两处构造点填充**

`agent/src/collectors/metrics.rs` `collect()` 的 `MetricsSnapshot` 字面量中 `swap_used_bytes: sys.used_swap(),` 后加：

```rust
            swap_total_bytes: sys.total_swap(),
```

`agent/src/handler.rs` `collect_metrics()` 的 `MetricsSnapshot` 字面量同样在 `swap_used_bytes` 对应行后加（该函数用 `System::new_all()`，`total_swap()` 直接可用）：

```rust
        swap_total_bytes: sys.total_swap(),
```

- [ ] **Step 6: WSL 运行 agent 测试**

Run: `wsl -e bash -lc "cd /mnt/e/MyWork/gnome-remote/agent && cargo test"`
Expected: 编译通过、全部 PASS（结构体字面量缺字段会被编译器强制找全，两处都已填充）

- [ ] **Step 7: 提示用户提交**

提示：Task 1 完成（swap_total_bytes 协议扩展），建议用户提交本次改动（由用户操作 git）。

---

### Task 2: 协议进程三件套 + re-export + golden

**Files:**
- Modify: `quirel-protocol/src/types.rs`（新增 ProcessEntry）
- Modify: `quirel-protocol/src/envelope.rs:17`（use 加 ProcessEntry）、`:580-586`（新变体）、`:662`（type_name）
- Modify: `quirel-protocol/src/lib.rs:23-25`
- Modify: `agent/src/protocol/serde.rs:4-11`、`agent/src/protocol/mod.rs:28-32`
- Modify: `src-tauri/src/connection.rs:72`（import）
- Test: `quirel-protocol/tests/wire_compat.rs`

- [ ] **Step 1: 写失败的 golden 测试**

`quirel-protocol/tests/wire_compat.rs` 末尾追加：

```rust
#[test]
fn get_processes_wire_format() {
    let env = Envelope::new(9, Payload::GetProcesses {});
    let json = serde_json::to_string(&env).unwrap();
    assert_eq!(
        json,
        r#"{"request_id":9,"payload":{"type":"get_processes","data":{}}}"#
    );
}

#[test]
fn process_list_response_roundtrip() {
    let env = Envelope::new(
        11,
        Payload::ProcessListResponse {
            processes: vec![ProcessEntry {
                pid: 1,
                name: "systemd".to_string(),
                user: "root".to_string(),
                cpu_percent: 0.1,
                mem_percent: 0.5,
                mem_bytes: 4096,
                state: "S".to_string(),
            }],
        },
    );
    let out = roundtrip(&env);
    match out.payload {
        Payload::ProcessListResponse { processes } => {
            assert_eq!(processes.len(), 1);
            assert_eq!(processes[0].pid, 1);
            assert_eq!(processes[0].name, "systemd");
            assert_eq!(processes[0].user, "root");
            assert_eq!(processes[0].state, "S");
        }
        _ => panic!("变体不匹配"),
    }
}
```

同步把 `ProcessEntry` 加进文件顶部 use：`use quirel_protocol::{ConnectionStatsSnapshot, Envelope, MetricsSnapshot, Payload, ProcessEntry, StatsResponse};`

- [ ] **Step 2: 运行确认编译失败**

Run: `cd quirel-protocol && cargo test --test wire_compat`
Expected: FAIL — `GetProcesses`/`ProcessListResponse`/`ProcessEntry` 不存在

- [ ] **Step 3: 实现**

`quirel-protocol/src/types.rs` 末尾追加：

```rust
/// 单个进程条目（GetProcesses 响应）
/// 字段与前端 SystemMonitor 的 ProcessInfo 接口对齐（snake_case）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessEntry {
    pub pid: u32,
    pub name: String,
    /// 用户名（sysinfo Users 缓存查得；跨用户不可见时为空串）
    pub user: String,
    pub cpu_percent: f32,
    pub mem_percent: f32,
    pub mem_bytes: u64,
    /// 单字符状态（R/S/I/Z/T/D/?），ps 风格
    pub state: String,
}
```

`quirel-protocol/src/envelope.rs`：
1. L17 use 行加 `ProcessEntry`：`use crate::types::{default_frame_mode, one, FileDiff, FileEntry, MetricsSnapshot, MountInfo, ProcessEntry};`
2. `SetTransferLimit` 变体后、`Error` 变体前插入：

```rust
    /// 进程列表查询（任何已认证用户可查；按需拉取，非订阅）
    /// 能力串 "processes" 门控：旧 Agent 无法解码新 payload，盲发会断流
    #[serde(rename = "get_processes")]
    GetProcesses {},

    /// 进程列表响应
    #[serde(rename = "process_list_response")]
    ProcessListResponse { processes: Vec<ProcessEntry> },
```

3. `type_name()` 的 `Payload::SetTransferLimit { .. } => "SetTransferLimit",` 后加：

```rust
            Payload::GetProcesses {} => "GetProcesses",
            Payload::ProcessListResponse { .. } => "ProcessListResponse",
```

`quirel-protocol/src/lib.rs` L23-25 改为：

```rust
pub use types::{
    DiffType, DiskInfo, FileDiff, FileEntry, MetricsSnapshot, MountInfo, ProcessEntry,
};
```

`agent/src/protocol/serde.rs` 的 pub use 列表加 `ProcessEntry`（放 `MetricsSnapshot, DiskInfo, MountInfo, FileEntry,` 行）。`agent/src/protocol/mod.rs` L28-32 的 pub use serde 列表加 `ProcessEntry`。`src-tauri/src/connection.rs` L72 附近从 quirel_protocol 导入的列表加 `ProcessEntry`。

- [ ] **Step 4: 运行协议测试（绿灯）**

Run: `cd quirel-protocol && cargo test --test wire_compat`
Expected: 全部 PASS。若 `get_processes_wire_format` 断言的 `"data":{}` 与实际序列化有出入，以**测试实际打印的字节**为准修正断言（golden 纪律：当前线上真实格式是权威）。

- [ ] **Step 5: 双端编译确认**

Run（Windows）: `cd src-tauri && cargo check`
Run（WSL）: `wsl -e bash -lc "cd /mnt/e/MyWork/gnome-remote/agent && cargo check"`
Expected: 两端编译通过（新变体未 match 会由 Task 3 的 handler arm 解决；`type_name` 已补齐，若有其它 match 穷尽性报错按编译器提示补通配或分支）

- [ ] **Step 6: 提示用户提交**

提示：Task 2 完成（进程协议三件套），建议用户提交。

---

### Task 3: Agent 进程采集器 + handler arm + 能力上报

**Files:**
- Create: `agent/src/collectors/processes.rs`
- Modify: `agent/src/collectors/mod.rs`、`agent/src/handler.rs`（GetStats arm 后）、`agent/src/transfer_limit.rs:31`、`agent/Cargo.toml:62`
- Test: `agent/src/collectors/processes.rs`（内嵌 #[cfg(test)]）

- [ ] **Step 1: sysinfo 加 user feature**

`agent/Cargo.toml` L62：

```toml
sysinfo = { version = "0.32", default-features = false, features = ["system", "disk", "network", "user"] }
```

（`Users`/`User` 类型在 sysinfo 0.32 受 `user` feature 门控，default-features=false 时必须显式开启，否则进程的属主名无法解析。）

- [ ] **Step 2: 写失败的采集器测试**

新建 `agent/src/collectors/processes.rs`，先只写测试骨架（list 尚未实现，编译失败即红灯）：

```rust
// agent/src/collectors/processes.rs

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_returns_nonempty_with_valid_fields() {
        let list = list();
        assert!(!list.is_empty(), "当前系统至少应有 1 个进程");
        // 容器/WSL 均有 pid 1；首次调用 cpu_percent 允许为 0（无前次采样窗口）
        let init = list.iter().find(|p| p.pid == 1).expect("pid 1 必然存在");
        assert!(!init.name.is_empty());
        assert!((0.0..=100.0).contains(&init.mem_percent));
        assert_eq!(init.state.len(), 1);
    }
}
```

- [ ] **Step 3: 运行确认失败**

Run: `wsl -e bash -lc "cd /mnt/e/MyWork/gnome-remote/agent && cargo test processes"`
Expected: FAIL — `list` 未定义

- [ ] **Step 4: 实现 list()**

`agent/src/collectors/processes.rs` 顶部（tests 模块之前）加入：

```rust
// 进程采集：GetProcesses 请求/响应路径专用（按需拉取，非订阅推送）
//
// 专属 System 实例：与 MetricsCollector 的实例完全分离——
// sysinfo 的 cpu_usage 依赖两次刷新的时间差，共用实例会让
// 订阅推送的刷新节奏污染进程页签的采样窗口

use crate::protocol::ProcessEntry;
use std::sync::{Mutex, OnceLock};
use sysinfo::{ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System, Users};

/// 进程查询专属 System 实例（进程内单例）
fn process_system() -> &'static Mutex<System> {
    static SYSTEM: OnceLock<Mutex<System>> = OnceLock::new();
    SYSTEM.get_or_init(|| Mutex::new(System::new()))
}

/// sysinfo ProcessStatus → ps 风格单字符
fn status_char(s: ProcessStatus) -> &'static str {
    match s {
        ProcessStatus::Run => "R",
        ProcessStatus::Sleep => "S",
        ProcessStatus::Idle => "I",
        ProcessStatus::Zombie => "Z",
        ProcessStatus::Traced => "T",
        ProcessStatus::Dead => "D",
        _ => "?",
    }
}

/// 枚举全量进程。任何已认证用户可查（与 GNOME 行为一致；
/// 内核自动限制跨用户进程的字段可见性，sysinfo 按权限降级）
pub fn list() -> Vec<ProcessEntry> {
    let guard = process_system().lock().unwrap();
    let sys = &mut *guard;

    // cpu_usage 依赖上一次刷新的时间差：客户端 3s 轮询天然构成采样窗口，
    // 首次调用 cpu_percent 为 0，下次轮询即准确（sysinfo 按时间差归一化）
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::everything(),
    );

    let users = Users::new_with_refreshed_list();
    let total_mem = sys.total_memory();

    sys.processes()
        .iter()
        .map(|(_pid, p)| ProcessEntry {
            pid: p.pid().as_u32(),
            name: p.name().to_string_lossy().to_string(),
            user: p
                .user_id()
                .and_then(|uid| users.iter().find(|u| u.id() == uid))
                .map(|u| u.name().to_string())
                .unwrap_or_default(),
            cpu_percent: p.cpu_usage(),
            mem_percent: if total_mem > 0 {
                (p.memory() as f32 / total_mem as f32) * 100.0
            } else {
                0.0
            },
            mem_bytes: p.memory(),
            state: status_char(p.status()).to_string(),
        })
        .collect()
}
```

`agent/src/collectors/mod.rs` 改为：

```rust
// agent/src/collectors/mod.rs

pub mod metrics;
pub mod processes;

pub use metrics::MetricsCollector;
```

- [ ] **Step 5: 运行采集器测试（绿灯）**

Run: `wsl -e bash -lc "cd /mnt/e/MyWork/gnome-remote/agent && cargo test processes"`
Expected: PASS

- [ ] **Step 6: handler arm + 能力上报**

`agent/src/handler.rs` 在 `Payload::GetStats { stats_type }` arm（约 L160）之前插入：

```rust
        // 进程列表查询（任何已认证用户；能力串 "processes" 由客户端门控）
        Payload::GetProcesses {} => {
            tracing::info!("进程列表查询: uid={}", session.uid);
            Envelope::new(
                envelope.request_id,
                Payload::ProcessListResponse {
                    processes: crate::collectors::processes::list(),
                },
            )
        }
```

`agent/src/transfer_limit.rs` L31：

```rust
pub const AGENT_CAPABILITIES: &[&str] = &["transfer_limit", "processes"];
```

（该常量是能力上报单点，quic.rs 认证路径自动携带。若 handler 里还有对 `AGENT_CAPABILITIES` 的注释说明数量，如「18 个认证路径统一携带」无关则不用动；但需检查 `transfer_limit.rs` 内是否有 `assert_eq!(AGENT_CAPABILITIES.len(), 1)` 之类断言，有则同步改为 2。）

- [ ] **Step 7: WSL 全量 agent 测试**

Run: `wsl -e bash -lc "cd /mnt/e/MyWork/gnome-remote/agent && cargo test"`
Expected: 全部 PASS

- [ ] **Step 8: 提示用户提交**

提示：Task 3 完成（Agent 进程采集 + 能力上报），建议用户提交。

---

### Task 4: 客户端 get_processes 命令

**Files:**
- Modify: `src-tauri/src/connection.rs:1339`（get_stats 之后）
- Modify: `src-tauri/src/lib.rs:612`（invoke_handler 注册）

- [ ] **Step 1: 实现命令（复刻 get_stats 模式，无独立逻辑故先实现后由编译/既有测试覆盖）**

`src-tauri/src/connection.rs` 在 `get_stats` 函数（L1326-1339）之后加入：

```rust
/// Tauri Command: 查询远程进程列表（任何已认证用户）
/// 前端必须先经 get_agent_capabilities 确认支持 "processes" 再调用本命令：
/// 旧 Agent 无法解码新 payload，直接发送会断流，主循环会把整条连接误判为断开
#[tauri::command]
pub async fn get_processes(server_id: String, app: tauri::AppHandle) -> Result<Vec<ProcessEntry>, AppError> {
    tracing::debug!("[Connection] get_processes: server_id={}", server_id);
    let resp = remote_send(server_id, Payload::GetProcesses {}, app).await?;
    match resp.payload {
        Payload::ProcessListResponse { processes } => Ok(processes),
        Payload::Error { message, .. } => Err(agent_error(message)),
        _ => Err(AppError::new(codes::UNEXPECTED_RESPONSE)),
    }
}
```

（`ProcessEntry` 已在 Task 2 加入该文件顶部 import。）

`src-tauri/src/lib.rs` invoke_handler 列表 `connection::get_stats,` 后加一行：

```rust
            connection::get_processes,          // 进程列表查询（SystemMonitor 进程页签）
```

- [ ] **Step 2: 编译 + 测试**

Run: `cd src-tauri && cargo test`
Expected: 编译通过、全部 PASS

- [ ] **Step 3: 提示用户提交**

提示：Task 4 完成（客户端 get_processes 命令），建议用户提交。

---

### Task 5: 前端展示层纯函数 display.ts + 单测

**Files:**
- Create: `src/apps/systemMonitor/display.ts`
- Test: `src/apps/systemMonitor/display.test.ts`

- [ ] **Step 1: 写失败的测试**

新建 `src/apps/systemMonitor/display.test.ts`：

```ts
import { describe, expect, it } from "vitest";
import { formatBytes, swapDisplay, toFsRow } from "./display";

describe("formatBytes", () => {
  it("0 → 0 B", () => expect(formatBytes(0)).toBe("0 B"));
  it("1 GiB → 1.0 GB", () => expect(formatBytes(1024 ** 3)).toBe("1.0 GB"));
});

describe("toFsRow", () => {
  it("设备名缺失回退占位符 —", () => {
    const row = toFsRow({
      device: "",
      mount_point: "/",
      filesystem: "ext4",
      total_bytes: 2 * 1024 ** 3,
      used_bytes: 1024 ** 3,
    });
    expect(row.device).toBe("—");
    expect(row.type).toBe("ext4");
  });

  it("可用 = 总量 − 已用", () => {
    const row = toFsRow({
      device: "/dev/sda1",
      mount_point: "/boot",
      filesystem: "ext4",
      total_bytes: 2 * 1024 ** 3,
      used_bytes: 1024 ** 3,
    });
    expect(row.avail).toBe("1.0 GB");
    expect(row.total).toBe("2.0 GB");
  });
});

describe("swapDisplay", () => {
  it("离线 → 全占位符", () => {
    expect(swapDisplay(1024, 2048, true)).toEqual({
      value: "—%",
      widthPercent: 0,
      detail: "— / —",
    });
  });

  it("total=0（旧 Agent 或未配置 swap）→ 占位符，宽度 0", () => {
    const d = swapDisplay(1024, 0, false);
    expect(d.value).toBe("—%");
    expect(d.widthPercent).toBe(0);
    // used 有真实值仍展示（旧 Agent 场景 used 真实、total 缺失）
    expect(d.detail).toBe("1.0 KB / —");
  });

  it("total>0 → 真实百分比与明细", () => {
    const d = swapDisplay(512 * 1024 ** 2, 2 * 1024 ** 3, false);
    expect(d.value).toBe("25.0%");
    expect(d.widthPercent).toBe(25);
    expect(d.detail).toBe("512.0 MB / 2.0 GB");
  });
});
```

- [ ] **Step 2: 运行确认失败**

Run（Windows）: `npm run test:run -- src/apps/systemMonitor/display.test.ts`
Expected: FAIL — `./display` 模块不存在

- [ ] **Step 3: 实现 display.ts**

新建 `src/apps/systemMonitor/display.ts`：

```ts
// src/apps/systemMonitor/display.ts
// SystemMonitor 展示层纯函数（与渲染解耦便于单测，同 FileManager 筛选规则模式）

/** 通用字节格式化（自 SystemMonitor.tsx 迁入，二者共用） */
export function formatBytes(bytes: number): string {
  if (bytes === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  return (bytes / Math.pow(1024, i)).toFixed(i === 0 ? 0 : 1) + " " + units[i];
}

const P = "\u2014"; // em dash 占位符，语义与 offlineDefaults.PLACEHOLDER 一致

/** remote_get_mounts 返回的挂载点信息（snake_case 与协议一致） */
export interface MountInfo {
  mount_point: string;
  device: string;
  filesystem: string;
  total_bytes: number;
  used_bytes: number;
}

/** 文件系统页签表格行 */
export interface FsRow {
  device: string;
  dir: string;
  type: string;
  total: string;
  used: string;
  avail: string;
}

/** MountInfo → 表格行：设备名缺失回退占位符，可用 = 总量 − 已用 */
export function toFsRow(m: MountInfo): FsRow {
  return {
    device: m.device || P,
    dir: m.mount_point,
    type: m.filesystem,
    total: formatBytes(m.total_bytes),
    used: formatBytes(m.used_bytes),
    avail: formatBytes(m.total_bytes - m.used_bytes),
  };
}

/** Swap 卡片展示三元组 */
export interface SwapDisplay {
  /** 百分比文案 */
  value: string;
  /** 进度条宽度（0-100） */
  widthPercent: number;
  /** 明细行文案 */
  detail: string;
}

/**
 * Swap 展示逻辑：
 * - 离线 → 全占位符
 * - total<=0（旧 Agent 无字段 或 未配置 swap）→ 百分比/进度条占位，used 有值仍展示
 * - 正常 → 真实百分比 + used/total 明细
 */
export function swapDisplay(used: number, total: number, offline: boolean): SwapDisplay {
  if (offline) {
    return { value: `${P}%`, widthPercent: 0, detail: `${P} / ${P}` };
  }
  if (total <= 0) {
    return { value: `${P}%`, widthPercent: 0, detail: `${formatBytes(used)} / ${P}` };
  }
  const percent = (used / total) * 100;
  return {
    value: percent.toFixed(1) + "%",
    widthPercent: Math.min(percent, 100),
    detail: `${formatBytes(used)} / ${formatBytes(total)}`,
  };
}
```

- [ ] **Step 4: 运行测试（绿灯）**

Run: `npm run test:run -- src/apps/systemMonitor/display.test.ts`
Expected: 全部 PASS

- [ ] **Step 5: 提示用户提交**

提示：Task 5 完成（展示层纯函数），建议用户提交。

---

### Task 6: 前端文件系统页签真实化（接 mounts，删伪造）

**Files:**
- Modify: `src/apps/SystemMonitor.tsx`（imports、mounts 状态、effect、filesystems 渲染块 L575-614、断连清理 L185-203）

- [ ] **Step 1: 删伪造渲染，接真实数据**

`src/apps/SystemMonitor.tsx`：

1. imports 区加：

```tsx
import { invoke } from "@tauri-apps/api/core";
import { toFsRow } from "./systemMonitor/display";
import type { MountInfo } from "./systemMonitor/display";
```

2. 组件状态区（`const [processes, ...]` 附近）加：

```tsx
  // 文件系统页签：remote_get_mounts 真实挂载点（设备名/文件系统类型来自 sysinfo，
  // 是 2026-06 就存在的老命令，早于能力系统，所有存量 Agent 均支持，无需门控）
  const [mounts, setMounts] = useState<MountInfo[]>([]);
```

3. 断连清理 effect（`if (!activeServerId) {` 块内，`setProcesses([]);` 旁）加：

```tsx
      setMounts([]);
```

4. demo 进程 effect（L287-292 的 `useEffect(() => { if (activeTab === "processes") ... })`）之后新增：

```tsx
  // 文件系统页签：切到时拉取一次真实挂载点（挂载信息变化低频，无需定时刷新）
  useEffect(() => {
    if (activeTab !== "filesystems" || !activeServerId) return;
    let cancelled = false;
    invoke<MountInfo[]>("remote_get_mounts", { serverId: activeServerId })
      .then((m) => {
        if (!cancelled) setMounts(m);
      })
      .catch((e) => log.error("获取挂载点失败:", e));
    return () => {
      cancelled = true;
    };
  }, [activeTab, activeServerId]);
```

5. `activeTab === "filesystems"` 渲染块整体替换为（删除 `/dev/sda{indexOf+1}` 拼接、硬编码 `ext4`、硬编码 tmpfs /tmp 行）：

```tsx
        {activeTab === "filesystems" && (
          <div className="sm-filesystems">
            <div className="sm-fs-header">
              <span className="sm-fs-device">{t('monitor.colDevice')}</span>
              <span className="sm-fs-dir">{t('monitor.colDir')}</span>
              <span className="sm-fs-type">{t('monitor.colType')}</span>
              <span className="sm-fs-total">{t('monitor.colTotal')}</span>
              <span className="sm-fs-used">{t('monitor.colUsed')}</span>
              <span className="sm-fs-avail">{t('monitor.colAvail')}</span>
            </div>
            <div className="sm-fs-list">
              {mounts.length === 0 ? (
                <div className="sm-fs-row" style={{ justifyContent: 'center', color: 'var(--quirel-text-disabled)', padding: '20px 0' }}>
                  {t('monitor.noDisks')}
                </div>
              ) : (
                mounts.map((m) => {
                  const row = toFsRow(m);
                  return (
                    <div key={`${row.device}-${row.dir}`} className="sm-fs-row">
                      <span className="sm-fs-device">{row.device}</span>
                      <span className="sm-fs-dir">{row.dir}</span>
                      <span className="sm-fs-type">{row.type}</span>
                      <span className="sm-fs-total">{row.total}</span>
                      <span className="sm-fs-used">{row.used}</span>
                      <span className="sm-fs-avail">{row.avail}</span>
                    </div>
                  );
                })
              )}
            </div>
          </div>
        )}
```

注意：此块在「真实数据」分支内（`showOffline`/`showSkeleton` 均为假时才渲染），无需 isOffline 判断。

- [ ] **Step 2: 类型检查 + 全量前端测试**

Run: `npx tsc --noEmit` 与 `npm run test:run`
Expected: 均通过

- [ ] **Step 3: 提示用户提交**

提示：Task 6 完成（文件系统页签真实化），建议用户提交。

---

### Task 7: 前端 Swap 卡片真实化

**Files:**
- Modify: `src/apps/SystemMonitor.tsx:26-35`（TS 接口）、`:497-509`（Swap 卡片渲染）、imports

- [ ] **Step 1: TS 接口加字段**

`MetricsSnapshot` 接口（L26-35）`swap_used_bytes` 后加：

```ts
  swap_total_bytes: number;
```

imports 加 `swapDisplay`：

```tsx
import { swapDisplay } from "./systemMonitor/display";
```

（离线占位值 `OFFLINE_METRICS` 已含 `swap_total_bytes: 0`，见 `src/utils/offlineDefaults.ts:51`，无需改动。）

- [ ] **Step 2: Swap 卡片替换为 swapDisplay 驱动**

`activeTab === "resources"` 内 Swap 卡片（原 L497-509）替换：

```tsx
              <div className="sm-res-bar-card">
                <div className="sm-bar-header">
                  <span className="sm-bar-title">{t('monitor.swap')}</span>
                  <span className="sm-bar-value">{swapDisplay(
                    metrics.swap_used_bytes,
                    metrics.swap_total_bytes,
                    isOffline
                  ).value}</span>
                </div>
                <div className="sm-bar-track">
                  <div className="sm-bar-fill" style={{
                    width: `${swapDisplay(metrics.swap_used_bytes, metrics.swap_total_bytes, isOffline).widthPercent}%`,
                    background: "#e8a416",
                    opacity: isOffline ? 0.3 : 1,
                  }} />
                </div>
                <div className="sm-bar-detail">
                  {swapDisplay(metrics.swap_used_bytes, metrics.swap_total_bytes, isOffline).detail}
                </div>
              </div>
```

（为避免三次调用，可在 `activeTab === "resources"` 分支开头提一个局部常量：`const swap = swapDisplay(metrics.swap_used_bytes, metrics.swap_total_bytes, isOffline);` 然后渲染处直接用 `swap.value`/`swap.widthPercent`/`swap.detail`——实施时采用此写法。）

- [ ] **Step 3: 类型检查 + 全量前端测试**

Run: `npx tsc --noEmit` 与 `npm run test:run`
Expected: 均通过

- [ ] **Step 4: 提示用户提交**

提示：Task 7 完成（Swap 真实化），建议用户提交。

---

### Task 8: 前端进程列表（能力门控 + 3s 轮询）+ i18n

**Files:**
- Modify: `src/apps/SystemMonitor.tsx`（删 generateDemoProcesses L71-85、selectedPid、demo effect；新增能力查询 + 轮询 effect；进程页签渲染块）
- Modify: `src/i18n/locales/zh-CN.ts:653`、`src/i18n/locales/en-US.ts`（monitor 块）

- [ ] **Step 1: i18n 词条（先加文案，供组件引用）**

`zh-CN.ts` monitor 块（`colAvail: '可用',` 后）加：

```ts
    processUnsupported: 'Agent 版本过旧，不支持进程列表',
```

`en-US.ts` monitor 块对应位置加（与 zh 相同 key）：

```ts
    processUnsupported: 'Agent version is too old and does not support the process list',
```

- [ ] **Step 2: 组件逻辑改造**

`SystemMonitor.tsx`：

1. **删除** `generateDemoProcesses`（L71-85）、demo 注入 effect（L287-292）、`selectedPid` state 及行渲染中的 `selectedPid === proc.pid` 判断与 `onClick`（无操作支撑的选中态属 demo 遗留，YAGNI）。
2. imports 加 `formatUnknownError`：

```tsx
import { formatUnknownError } from "../types/errors";
```

3. 常量（组件外）：

```tsx
// 进程列表轮询间隔：页签可见 + 能力支持时按需拉取，离开页签即停
const PROCESSES_REFRESH_MS = 3000;
```

4. 状态（`processes` state 旁）：

```tsx
  // 能力门控：null=查询中，false=旧 Agent（绝不盲发——盲发会断流）
  const [procSupported, setProcSupported] = useState<boolean | null>(null);
  const [procError, setProcError] = useState<string | null>(null);
```

5. 能力查询 effect（连接建立时一次；断连由既有 `!activeServerId` effect 清理）：

```tsx
  // 能力查询：决定进程页签是真实数据还是「不支持」空态
  useEffect(() => {
    if (!activeServerId) {
      setProcSupported(null);
      return;
    }
    let cancelled = false;
    setProcSupported(null);
    invoke<string[]>("get_agent_capabilities", { serverId: activeServerId })
      .then((caps) => {
        if (!cancelled) setProcSupported(caps.includes("processes"));
      })
      .catch(() => {
        // 查询失败按不支持处理（空态），绝不发送 GetProcesses
        if (!cancelled) setProcSupported(false);
      });
    return () => {
      cancelled = true;
    };
  }, [activeServerId]);
```

6. 既有断连清理 effect 的 `if (!activeServerId) {` 块内补：

```tsx
      setProcSupported(null);
      setProcError(null);
      setProcesses([]);
```

7. 轮询 effect：

```tsx
  // 进程拉取：页签可见 + 能力支持时 3s 轮询；切页签/断连/卸载即停
  useEffect(() => {
    if (activeTab !== "processes" || !activeServerId || procSupported !== true) return;
    let cancelled = false;
    const fetchOnce = () => {
      invoke<ProcessInfo[]>("get_processes", { serverId: activeServerId })
        .then((list) => {
          if (!cancelled) {
            setProcesses(list);
            setProcError(null);
          }
        })
        .catch((e) => {
          if (!cancelled) setProcError(formatUnknownError(e));
        });
    };
    fetchOnce();
    const timer = setInterval(fetchOnce, PROCESSES_REFRESH_MS);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [activeTab, activeServerId, procSupported]);
```

8. 进程页签渲染：`activeTab === "processes"` 块中，把原 `sm-process-list` 内容包进门控分支（footer 的 `processes.length` 现在是真实总数）：

```tsx
            <div className="sm-process-list">
              {procSupported === false ? (
                <div style={{ padding: '20px 0', color: 'var(--quirel-text-disabled)', textAlign: 'center' }}>
                  {t('monitor.processUnsupported')}
                </div>
              ) : procError ? (
                <div style={{ padding: '20px 0', color: 'var(--quirel-text-error, var(--quirel-text-secondary))', textAlign: 'center' }}>
                  {procError}
                </div>
              ) : (
                sortedProcesses().map((proc) => (
                  <div key={proc.pid} className="sm-process-row">
                    <span className="sm-pr-pid">{proc.pid}</span>
                    <span className="sm-pr-name">{proc.name}</span>
                    <span className="sm-pr-user">{proc.user}</span>
                    <span className="sm-pr-cpu">{formatPercentSafe(proc.cpu_percent, isOffline)}</span>
                    <span className="sm-pr-mem">{formatPercentSafe(proc.mem_percent, isOffline)}</span>
                    <span className="sm-pr-state">{proc.state}</span>
                  </div>
                ))
              )}
            </div>
```

- [ ] **Step 3: 类型检查 + 全量前端测试**

Run: `npx tsc --noEmit` 与 `npm run test:run`
Expected: 均通过

- [ ] **Step 4: 提示用户提交**

提示：Task 8 完成（进程列表真实化），建议用户提交。

---

### Task 9: 全量验证

**Files:** 无新改动，只验证。

- [ ] **Step 1: 协议层（Windows）**

Run: `cd quirel-protocol && cargo test`
Expected: 全部 PASS（含新增 4 个 golden + 既有 golden）

- [ ] **Step 2: Agent（WSL）**

Run: `wsl -e bash -lc "cd /mnt/e/MyWork/gnome-remote/agent && cargo test"`
Expected: 全部 PASS

- [ ] **Step 3: 客户端 Rust（Windows）**

Run: `cd src-tauri && cargo test`
Expected: 全部 PASS

- [ ] **Step 4: 前端（Windows）**

Run: `npx tsc --noEmit` 与 `npm run test:run`
Expected: 均通过

- [ ] **Step 5: 手动冒烟（需真实/测试环境连接）**

1. 新客户端 + 新 Agent：进程页签 3s 刷新、数字变动；资源页签 Swap 显示真实百分比；文件系统页签显示真实设备名（如 /dev/vda1、nvme0n1p2）与真实类型（ext4/btrfs/tmpfs…）
2. 新客户端 + 旧 Agent：进程页签显示「Agent 版本过旧」空态且**连接不断**；Swap 显示 "—"；文件系统页签仍显示真实 mounts
3. 断连：回到骨架屏 + 离线徽标；重连后三项恢复
4. 资源页签 CPU/内存/网络行为与改造前一致（回归）

- [ ] **Step 6: 提示用户提交 + 更新项目记忆**

提示用户做最终提交（由用户操作）。将「系统监控三项假数据已真实化」结论写入项目记忆（processes 能力串、swap_total_bytes 字段、remote_get_mounts 复用决策）。

---

## Self-Review 记录

- **规格覆盖：** 规格 §1（文件系统 mounts）→ Task 6；§2（swap_total_bytes）→ Task 1/7；§3（进程三件套）→ Task 2/3/4/8；测试矩阵 → Task 1-5 各测试 + Task 9；i18n → Task 8 Step 1。无遗漏。
- **占位符扫描：** 无 TBD/TODO；所有代码步骤含完整代码；实施注释「若 get_processes_wire_format 断言与实际序列化有出入，以实际字节为准」是 golden 纪律规则而非占位符。
- **类型一致性：** `ProcessEntry` 七字段在协议（Task 2）/Agent 采集（Task 3）/前端 `ProcessInfo` 接口（Task 8 渲染 cpu_percent/mem_percent/state）三处一致；`"processes"` 能力串在 Task 3（上报）与 Task 8（门控）一致；`swapDisplay`/`toFsRow` 签名与 Task 5 测试一致；`MountInfo` TS 接口五字段与协议 `MountInfo`（types.rs:45-51）一致。
