# 上传大小限制设置化 — 设计文档

日期：2026-09-29
状态：已批准（root 独占修改 + 写回 quireld.toml 持久化，全局热生效）

## 1. 背景与目标

Agent 端 `quireld.toml [limits] max_file_transfer_mb`（默认 500）限制单次上传文件大小，
超限文件在 `FileTransferRequest` 一开始即被拒绝（错误如「文件大小超过限制: 1301MB > 500MB」）。
客户端设置页「文件」分区已有一个死占位 UI：「传输设置 → 最大传输大小」（defaultValue=1000，未接任何状态）。

**目标**：
- 把该占位做成真实设置：显示当前连接服务器的实际上传限制，root 用户可修改，默认 500 MB
- 修改立即对 Agent 进程全局生效（该服务器上所有用户，含已登录的其他会话）
- 写回服务器上的 quireld.toml（保留注释），Agent 重启后仍保留新值
- 值单一事实来源在 Agent 端，客户端不持久化（每次连接后查询，避免双端不一致）

**非目标（YAGNI）**：
- 下载方向大小限制（现状仅 upload 检查，不动）
- per-user 限制（保持 Agent 全局策略语义）
- 「文件」分区其他死占位（默认视图/排序/隐藏文件等）

## 2. 数据流

```
设置 UI（文件 → 传输设置卡片）
  查询：get_transfer_limit ──▶ GetTransferLimit ──▶ Agent 返回 { max_file_transfer_mb, editable, persisted }
  修改：set_transfer_limit ──▶ SetTransferLimit{mb} ──▶ Agent：
      ├─ 非 root → Error 403
      ├─ 值域非法（1–102400 MB 之外）→ Error 400
      ├─ 更新 AtomicU64 全局热值（立即对所有会话生效）
      └─ toml_edit 写回 quireld.toml（保留注释）
           ├─ 成功 → persisted=true
           └─ 失败（权限/只读）→ 热值不回滚，persisted=false + warn 日志
```

## 3. 协议层（quirel-protocol）

新增两个 Payload 变体，serde 风格与现有变体一致（camelCase 字段、`#[serde(default)]` 兼容）：

```
GetTransferLimit {}
TransferLimitResponse { max_file_transfer_mb: u64, editable: bool, persisted: bool }
SetTransferLimit { max_file_transfer_mb: u64 }
```

- `GetTransferLimit`：任何已认证用户可查
  - `editable` = 当前会话是否 root（前端据此禁用输入框）
  - `persisted` 恒为 true（查询响应无「未持久化」语义）
- `SetTransferLimit`：仅 root；成功返回 `TransferLimitResponse`（含新值与写回结果）
- 按惯例在 `quirel-protocol/tests/wire_compat.rs` 添加字节级 golden 测试，锁死线格式
- Payload::type_name() 等枚举遍历处同步登记

## 4. Agent 端

- 新增全局静态 `MAX_FILE_TRANSFER_MB: AtomicU64`（仿 handler.rs 中 TRANSFER_SESSIONS 的 lazy_static 先例），启动时从 `cfg.limits.max_file_transfer_mb` 初始化
- `handle_file_transfer_request` 上传检查（handler.rs:394）改读该热值；其余 limits 不动
- `SetTransferLimit` 处理：
  1. `session.uid == 0` 校验，否则 `Error{code: 403}`
  2. 值域校验 1..=102400（1 MB – 100 GB），否则 `Error{code: 400}`
  3. 更新热值 → 对当前进程所有会话立即生效
  4. toml_edit 写回配置文件：仅精准修改 `[limits].max_file_transfer_mb`，保留注释与格式
  5. 写回失败：热值不回滚，`persisted=false`，`tracing::warn!`
- Agent 需知道自身配置文件路径（启动时 `--config` 参数）用于写回；测试环境可注入临时路径

## 5. 客户端 Rust（src-tauri）

- `connection.rs` 新增两个 tauri command（`remote_send` 模式，get_stats 先例）：
  - `get_transfer_limit(server_id) -> TransferLimitResponse`
  - `set_transfer_limit(server_id, max_file_transfer_mb) -> TransferLimitResponse`
- 错误映射遵循「三不暴露」原则，统一用户视角文案：
  - 未连接 → 「未连接服务器」
  - 旧 Agent 不支持 → 「当前 Agent 版本不支持此设置」（识别方式见 §8）
  - 权限不足（403）→ 「需要以 root 用户连接才能修改」
  - 值域非法（400）→ 「请输入 1 – 102400 之间的数值」
- `lib.rs` 注册命令

## 6. 前端 UI（Settings.tsx 文件分区）

替换「传输设置」卡片死占位：

- 标题行：「上传大小限制」+ 数字输入（MB 单位）+「应用」按钮（accent 主操作按钮样式）
- 查询时机：进入文件分区且有活跃连接时查询 Agent 实际值并回填输入框
- 状态矩阵：

| 状态 | 展示 |
|------|------|
| 未连接 | 输入禁用 + hint「连接服务器后可查看」 |
| 已连接，root | 回填当前值，可修改，「应用」启用 |
| 已连接，非 root | 回填当前值，输入禁用 + hint「需要 root 权限修改」 |
| 旧 Agent | 输入禁用 + hint「当前 Agent 版本不支持，请升级 Agent」 |
| 应用成功 persisted=true | 成功提示「已生效」 |
| 应用成功 persisted=false | 提示「已生效，但配置文件写入失败，重启后恢复原值」 |

- 前端本地校验：非整数 / 超出 1–102400 时「应用」禁用并提示，不发请求
- hint 说明作用域：「仅对当前连接的服务器生效，修改后该服务器所有用户适用」

## 7. 样式

复用 st-card / st-option-row / st-input / st-btn 既有体系；「应用」用主操作 accent 样式；
状态提示用 st-hint。不新增设计语言。

## 8. 兼容性

- **新客户端 + 旧 Agent**：旧 Agent 无法反序列化新 Payload 变体 → remote_send 报错 →
  前端降级为「不支持」禁用态，不崩溃（符合项目兼容性硬约束）
- **旧客户端 + 新 Agent**：Agent 新增 match arm 不影响既有命令
- **风险（计划阶段验证）**：旧 Agent 收到未知 payload 的具体行为（返回 Error envelope vs
  流异常/超时）决定前端如何可靠识别「不支持」——需要实测确定错误识别特征，不靠字符串猜
- 错误码语义遵循「发布后不可变更」约束：403=权限、400=参数 的语义与现有用法一致

## 9. 测试策略

- 协议：wire_compat.rs golden 测试（新变体字节形状 + 往返）
- Agent：handler 单测——root 修改成功（热值+persisted）、非 root 403、值域 400、
  写回失败降级（persisted=false 热值仍生效）、上传检查读热值
- 客户端 Rust：命令错误映射单测
- 前端：Settings 组件测试——状态矩阵各行、本地校验、应用成功/降级提示
- 手动冒烟：root 修改 → 第二个用户会话上传受限 → 重启 Agent 后值保留
