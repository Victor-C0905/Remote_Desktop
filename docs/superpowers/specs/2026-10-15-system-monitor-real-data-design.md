# 系统监控真实化设计

- 日期：2026-10-15
- 状态：已批准
- 范围：SystemMonitor 三处假数据修复（文件系统页签 / Swap / 进程列表）

## 背景与问题

SystemMonitor（`src/apps/SystemMonitor.tsx`）存在三处假数据：

1. **进程页签**：`generateDemoProcesses()` 返回 10 条硬编码进程，切页签时生成一次后永不刷新；页脚「进程总数」也是假值。Agent 无进程枚举功能。
2. **Swap 卡片**：协议 `MetricsSnapshot` 只有 `swap_used_bytes`，缺 `swap_total_bytes`，前端硬编码百分比 0.0%、进度条 0%、total 显示 "—"。
3. **文件系统页签**：设备名 `/dev/sda{index+1}` 拼接、文件系统类型硬编码 `ext4`、硬编码 tmpfs /tmp 整行（4 GB / 128 MB / 3.9 GB）。

数据链路核实：Agent 端 `collectors/metrics.rs` 订阅推送为真实采集（sysinfo + /proc/net/dev + /proc/meminfo），stats 页签走 `GetStats` 协议为真实数据——本次均不动。

### 关键发现

`remote_get_mounts` 命令链路早已存在且为真实数据：

- 客户端 `src-tauri/src/connection.rs` `remote_get_mounts` → `Payload::GetMounts` → Agent `handler.rs` `handle_get_mounts()` → `MountInfo { mount_point, device, filesystem, total_bytes, used_bytes }`（sysinfo `Disk::name()` / `file_system()`）
- 该命令自 2026-06-15（FileManager 初建）存在，**早于能力系统**（transfer_limit 引入），不在 `AGENT_CAPABILITIES` 门控范围 → 所有存量 Agent 均支持，可直接使用，无需能力协商
- 真实 tmpfs 条目（/dev/shm 等）随 Agent 端 sysinfo `Disks` 列表自然返回，无需伪造

## 方案选型（进程列表数据流）

- **方案 A（已选）：请求/响应按需拉取**——前端切到进程页签时每 3 秒调 `GetProcesses`，离开即停。数据只在用户查看时产生，Agent 空闲开销为零；与 `get_stats`/`get_transfer_limit` 模式一致。
- 方案 B（否决）：并入 metrics 订阅推送——订阅即常推，Agent 每周期需刷新全量进程并推送 50–150KB JSON，用户看其他页签时全是浪费。

## 设计

### 1. 文件系统页签（前端改造，零协议改动）

- 删除 `SystemMonitor.tsx` 全部造假代码：`/dev/sda{index+1}` 拼接、硬编码 `ext4`、硬编码 tmpfs /tmp 行
- 切到页签时 `invoke('remote_get_mounts', { serverId })`，用 `MountInfo` 渲染六列：
  - 设备 = `device`（空值显示 "—"）
  - 挂载点 = `mount_point`
  - 类型 = `filesystem`
  - 总计 / 已用 = `total_bytes` / `used_bytes`
  - 可用 = `total_bytes - used_bytes`
- 资源页签的磁盘用量条保持 `metrics.disks`（订阅推送自动刷新，不额外发请求）

### 2. Swap 补齐（协议加法字段）

- `quirel-protocol/src/types.rs`：`MetricsSnapshot` 增加 `swap_total_bytes: u64`，标注 `#[serde(default)]`（与 `code`/`capabilities` 同一向后兼容手法，现有 golden 字节不变）
- Agent 两处构造点填充 `sys.total_swap()`：
  - `agent/src/collectors/metrics.rs` `collect()`（订阅推送路径）
  - `agent/src/handler.rs` `collect_metrics()`（请求路径）
- 前端 Swap 卡片：`total > 0` → 真实百分比 + 进度条 + used/total；`total == 0` → 占位符（"—"），同时覆盖「旧 Agent 无此字段」与「系统未配置 swap」，无需区分
- 旧 Agent 数据反序列化：缺字段 → default 0 → 占位符；新 Agent → 旧客户端 serde 忽略未知字段，不受影响

### 3. 进程列表（新命令，能力门控）

**协议**（`quirel-protocol/src/envelope.rs`，snake_case rename 对齐现有风格）：

```rust
#[serde(rename = "get_processes")]
GetProcesses {},

#[serde(rename = "process_list_response")]
ProcessListResponse { processes: Vec<ProcessEntry> },
```

```rust
/// 单个进程条目（字段与前端 ProcessInfo 接口对齐）
pub struct ProcessEntry {
    pub pid: u32,
    pub name: String,
    pub user: String,
    pub cpu_percent: f32,
    pub mem_percent: f32,
    pub mem_bytes: u64,
    pub state: String,
}
```

**Agent**（新模块 `agent/src/collectors/processes.rs`，与现有 `collectors/metrics.rs` 同层）：

- 专属 `System` 实例（`Mutex` 保护），每次请求 `refresh_processes`——与 MetricsCollector 的 `System` 分离，避免 CPU 增量窗口互相干扰
- `user`：sysinfo `Users` 缓存按 `user_id()` 查用户名
- `mem_percent`：`process.memory() / sys.total_memory() * 100`
- `state`：`process.status()` 映射单字符（R/S/Z/T/D/I…）
- `cpu_percent`：`process.cpu_usage()`；首次采样为 0，后续轮询按时间差归一化即准确
- 所有已认证用户可查（与 GNOME 行为一致；内核自动限制跨用户字段可见性）
- `AGENT_CAPABILITIES` 增加 `"processes"`——单点构造自动随全部认证路径上报
- handler 增加 `Payload::GetProcesses` arm

**客户端 Rust**（`src-tauri/src/connection.rs`）：

- `get_processes` 命令，复刻 `get_stats` 的 `remote_send` 模式：`Payload::ProcessListResponse` → Ok，`Payload::Error` → `agent_error`，其他 → `UNEXPECTED_RESPONSE`
- 注册进 `lib.rs` invoke_handler

**前端**（`src/apps/SystemMonitor.tsx`）：

- 删除 `generateDemoProcesses()` 及 demo 注入 effect
- 能力门控：`get_agent_capabilities` 结果不含 `"processes"` → 进程页签显示「Agent 版本过旧，不支持进程列表」空态（复用现有禁用态样式）；**绝不盲发**（旧 Agent 解码新 payload 会断流，客户端会误判整条连接断开）
- 能力具备：页签激活时拉取一次 + 3 秒 setInterval 自动刷新；切页签 / 断连 / 卸载时清理定时器
- 排序为客户端本地排序，不重发请求
- 新增 i18n 词条（zh-CN / en-US：进程页签空态文案）

### 4. 错误码

无需新增。能力门控让「不支持」在发送前就被拦住，呈现为空态而非错误路径，规避错误码四处同步（errors.rs / zh-en 词典 / errors.ts / 测试）的成本。

## 测试与验证

| 层 | 测试 | 环境 |
|---|---|---|
| quirel-protocol | golden 测试：旧字节（无新字段）反序列化 default 生效；`GetProcesses`/`ProcessListResponse` 编码字节稳定性；ProcessListResponse round-trip | Windows |
| agent | processes 采集单测（当前系统进程非空、字段合法）；metrics 含 `swap_total_bytes` | WSL（`wsl -e bash -lc "cd /mnt/e/MyWork/gnome-remote/agent && cargo test"`） |
| src-tauri | `get_processes` 沿用 `get_stats` 模式，命令层薄，重点在协议层 | Windows |
| 前端 | vitest：能力门控空态、轮询启停生命周期、swap 显示逻辑（total=0 占位）、文件系统页签无伪造行 | Windows（npm） |

构建环境路由（项目硬约束）：`src-tauri` 与 `quirel-protocol` 在 Windows 侧构建测试；`agent` 必须在 WSL 内构建测试；前端 npm 命令 Windows 侧执行。

## 手动冒烟

1. 新客户端 + 新 Agent：进程列表实时刷新、Swap 真实百分比、文件系统真实设备/类型
2. 新客户端 + 旧 Agent：进程页签空态不崩溃、Swap 显示 "—"、文件系统仍显示真实 mounts（老命令）
3. 断连：回到离线占位符（骨架屏 + 离线徽标），重连后恢复
