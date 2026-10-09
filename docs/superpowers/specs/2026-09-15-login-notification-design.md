# 登录通知回应体系设计

- 日期：2026-09-15
- 状态：已与用户逐节确认
- 目标：解决连接失败时错误指代不明的问题，建立「错误码分类 + 用户可读通知」的完整登录通知回应

## 1. 背景与问题

### 1.1 起因：一次真实事故

用户在图书馆 WiFi 下连接泰国服务器失败，报错「读取挑战长度失败： connection lost」。实际根因是到该服务器的跨境网络路径劣化，但该报错完全无法区分是「服务器主动拒绝」「网络断了」还是「程序故障」。用户排查了一下午（怀疑服务器死机、密钥损坏、SSH 故障）才定位到网络问题。

### 1.2 现状三处断点（调研结论）

| 层 | 现状 | 问题 |
|---|---|---|
| 协议层 | `AuthResponse` 只有中文文案没有错误码；`Payload::Error` 有 code 但客户端 15 处全部丢弃 | 无机器可读分类 |
| 客户端 | 全程 `Result<T, String>` 裸字符串；`isRetryableConnectError` 靠 4 个关键词白名单匹配 | 分类脆弱，新错误落网外 |
| 前端 | Settings 卡片红字原样渲染裸字符串 | 用户看到「读取挑战长度失败」这类术语 |

关键归因黑洞：Agent 在挑战阶段提前 close 时不发任何通知（如挑战发送失败、响应流异常直接 close），客户端只能看到 `connection lost`，无法区分「服务器主动拒绝」与「网络断开」。

### 1.3 已确认的需求边界

- **覆盖范围**：登录 + 断连（不含业务命令 remote_* 的错误码利用）
- **错误码定义层**：协议层（quirel-protocol 共享 crate，单一真相源）
- **UI 呈现**：激活通知中心（NotificationCenter.tsx 从 demo 改为真实数据源）
- **诊断深度**：纯分类，不做主动探测（不 ping、不发探测包、不做 captive portal 检测）
- **附加**：连接失败通知附带多服务器对比提示（只读已有状态，零探测）
- **文案原则**：不暴露实现细节，使用专业而大众化的提示语

## 2. 方案选择

**方案 A（采纳）：共享枚举 + 客户端映射表**
- `quirel-protocol` 定义 `AuthErrorCode` 枚举，加到 `AuthResponse`（`#[serde(default)]`）
- Agent 每个失败点发送结构化错误码，文案保留供日志
- 客户端连接错误升级为 `ConnectError { code, detail }` 结构
- 前端映射表 `code → { title, message, action, retryable, severity }` 是唯一文案源

**方案 B（否决）**：协议 i32 常量 + 客户端枚举——无编译期类型安全，常量与枚举易不同步。

**方案 C（否决）**：完整错误信封（agent 填充文案）——agent 耦合用户文案，违反关注点分离。

方案 A 的关注点分离：Agent 只负责「是什么错」（分类），客户端负责「怎么告诉用户」（文案）。文案在前端，改文案无需重编译后端。

## 3. 协议层设计（quirel-protocol）

### 3.1 AuthErrorCode 枚举

数值按阶段分段，语义一旦发布不可变更（新增只能追加，不得改动已有数值含义）：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u16)]
pub enum AuthErrorCode {
    // 1-99：客户端本地错误（不经过网络，仅本地分类）
    MissingCredentials,      // 缺少认证凭据
    InvalidKeyFormat,        // 私钥格式不支持
    KeyParseFailed,          // 私钥解析失败（含密码错误）
    CertificateRejected,     // 用户拒绝信任服务器证书

    // 100-199：网络/传输阶段
    DnsFailed,               // 域名解析失败
    ConnectTimeout,          // 连接超时
    TlsHandshakeFailed,      // 安全握手失败
    NetworkUnreachable,      // 网络不可达
    StreamTimeout,           // 认证数据交换超时
    ConnectionLost,          // 连接中断

    // 200-299：Agent 认证拒绝
    RateLimited,             // IP 速率限制
    AccountLocked,           // 账户锁定（15 分钟）
    InvalidCredentials,      // 用户名或密码错误
    PubkeyNotAuthorized,     // 公钥未授权
    SignatureVerificationFailed, // 签名验证失败
    ChallengeExpired,        // 挑战过期
    AuthServiceUnavailable,  // 认证服务不可用
    ProtocolError,           // 协议格式错误

    // 300-399：会话生命周期（登录后）
    SessionExpired,          // 会话超时（24 小时不活动）

    Unknown,                 // 999：兜底
}
```

枚举变体与 Agent 审计日志的机器可读失败类别字符串一一对应（`ip_rate_limited` / `account_locked` / `invalid_password` / `pubkey_not_authorized` / `pubkey_mismatch` / `invalid_signature` / `auth_error` 等）。

### 3.2 协议扩展（遵守向后兼容规则）

遵循 envelope.rs 顶部注释的协议变更守则（不得移除字段；新增字段必须带 `#[serde(default)]`）。已核实 Payload 为 serde tag/content 风格内部标签枚举、无 `deny_unknown_fields`，新增字段两侧均安全（旧端忽略未知字段、新端缺字段取 default）：

1. **`AuthResponse` 新增 `code: Option<AuthErrorCode>` 字段**（envelope.rs:89-96）——Agent 拒绝时携带结构化码。新 Agent → 旧客户端：未知字段被忽略；旧 Agent → 新客户端：字段缺省 `None` 走 fallback。
2. **`Payload::Error.code` 保持 `i32` 类型不变**（envelope.rs:448）——不迁移类型。直接改类型会导致新客户端无法解析旧 Agent 的整数 code（401/403 等），违反变更守则。归因黑洞补丁复用该现有字段，填入 `AuthErrorCode` 的 `#[repr(u16)]` 数值；客户端提供 `AuthErrorCode::from_i32` 映射。数值空间无冲突：`AuthErrorCode` 占 1-299 与 999，既有 HTTP 风格码（400/401/403/404/408/500/501）全部 ≥400，互不重叠。

### 3.3 归因黑洞补丁（关键）

Agent 在挑战阶段「直接 close 无通知」的点，改为先发一帧 `Payload::Error`（code 填 `AuthErrorCode` 数值）再 close。客户端侧配合：认证流读取阶段先按 `Envelope` 解码，再按 payload 类型分支——收到 `Payload::Error` 则取 code；收到 `AuthPubKeyChallenge` 则继续原流程：

| 现状（quic.rs） | 补丁 |
|---|---|
| 挑战发送失败直接 close（:709） | 先发 `Error { code: ConnectionLost }` |
| 挑战编码失败直接 close（:715） | 先发 `Error { code: AuthServiceUnavailable }` |
| 响应流异常直接 close（:734/:939/:932/:947） | 先发 `Error { code: StreamTimeout }` |

归因规则：客户端读到错误帧 → Agent 侧主动通知，有明确 code；读到裸 `connection lost` → 纯网络断开。

## 4. Agent 侧设计（quic.rs）

### 4.1 失败点 → 错误码映射

每个失败点在现有「中文文案 + close」之外补发 code，中文文案保留给日志：

| 失败点（quic.rs 行号） | code |
|---|---|
| IP 速率限制（:491/:604） | RateLimited |
| 账户锁定（:513/:626） | AccountLocked |
| PAM 密码错误（:556） | InvalidCredentials |
| PAM/authorized_keys 系统错误（:574/:646/:813） | AuthServiceUnavailable |
| 公钥未授权（:674） | PubkeyNotAuthorized |
| 签名验证失败（:785/:867/:891） | SignatureVerificationFailed |
| 挑战过期（:921） | ChallengeExpired |
| 协议格式错误（:455） | ProtocolError |
| 认证流异常关闭（:435） | ProtocolError |

实现方式：扩展 `send_auth_response` 辅助函数签名，接受 `Option<AuthErrorCode>`；`log_auth_failure` 的审计类别字符串与 code 一一对应（顺手统一，不改审计语义）。

## 5. 客户端设计（src-tauri/src/connection.rs）

### 5.1 ConnectError 结构

`remote_connect` 错误通道从 `Result<T, String>` 升级为结构化错误（serde 序列化，Tauri invoke reject 时前端收到 JSON 对象）：

```rust
#[derive(Debug, Clone, Serialize)]
pub struct ConnectError {
    pub code: AuthErrorCode,      // 分类（必填）
    pub detail: Option<String>,  // 用户可读上下文（如主机名），不含实现细节
}
```

### 5.2 错误分支收敛

`remote_connect`（:147-559）、`try_quic_connect`（:1400-1474）、`perform_pubkey_auth`（:1624-1919）的约 40 个裸字符串分支全部收敛到 `ConnectError { code, detail }`：

- 网络阶段：`QUIC 连接失败: QUIC 握手失败: ...` → `code: TlsHandshakeFailed/ConnectTimeout/...`，技术细节进 tracing 日志
- 认证流阶段：`读取挑战长度失败: connection lost` → `code: ConnectionLost`（若收到 Agent 错误帧则用帧内 code）
- 本地阶段：密钥解析各分支 → `code: InvalidKeyFormat/KeyParseFailed`
- Agent 返回的 AuthResponse 带 code 时直接采用；无 code（旧 Agent）时走 fallback 字符串分类

私钥解析的现有友好提示（generate_key_parse_error 的 PuTTY 转换指引等）作为 detail 保留——这是用户操作指引，不是实现细节。

### 5.3 日志与技术细节

完整技术细节（底层库错误链，如 quinn 错误的 `{:?}` 输出）只进 tracing 日志。原则：日志记完整链条（`{:?}` 记录源错误），用户面只记分类结果与用户可读上下文。

### 5.4 客户端 close reason 真实化

客户端 `conn.close()` 的 reason 修正为真实原因，消除误导（connection.rs:296 的 `authentication failed` 实为认证流网络超时）：

| 位置 | 现状 reason | 改为 |
|---|---|---|
| :272/:296（认证失败） | `b"authentication failed"` | `b"auth network timeout"`（网络类）/ 保留（Agent 确认拒绝类） |
| :234 | `b"cert rejected"` | 保留（真实） |
| :277 | `b"unexpected response"` | 保留（真实） |

### 5.5 兼容 fallback

新客户端连旧 Agent（AuthResponse 无 code 字段，serde default None）→ 客户端将 Agent 中文文案走现有字符串分类（保留现 4 关键词逻辑作为降级路径，映射到最近似 code；无法识别 → Unknown）。

## 6. 断连事件细化

### 6.1 connection-lost payload 扩展

保留 `source` 字段兼容现有自动重连门控，新增 `code` 与 `detail`（可选，向后兼容）：

```ts
{ server_id, source, code?: number, detail?: string }
```

`code` 复用 `AuthErrorCode`（网络段 100-199 + 会话段 300-399）。

### 6.2 服务端 close code 空间启用

现有：`0x01` idle timeout、`0x02` session timeout（quic.rs:958-1003）。新增 `0x03` auth-rejected。客户端 status_task（connection.rs:447-451）将 close_reason 映射进 payload：

| close code/reason | 事件 code |
|---|---|
| 0x01 connection idle timeout | StreamTimeout |
| 0x02 session timeout | SessionExpired（登录后 24 小时不活动超时，区别于登录阶段的 ChallengeExpired） |
| 0x03 auth-rejected | 按认证拒绝 code（若认证阶段已知） |
| 其他（对端 close / 网络断） | ConnectionLost |

## 7. 前端设计

### 7.1 错误码常量与映射表

前端定义 `AuthErrorCode` 数字常量（与 Rust 枚举 `#[repr(u16)]` 数值对应）+ 映射表。映射表是全项目唯一用户文案源：

```ts
// code → { title, message, action, retryable, severity }
```

用 `satisfies Record<AuthErrorCode, …>` 保证编译期全覆盖（每个 code 有条目）。

### 7.2 文案原则（三不暴露 / 三保留）

**三不暴露**：
- 协议名词：QUIC、流（Stream）、挑战（Challenge）、握手（Handshake）、TLS
- 库错误原文：connection lost、timed out、tls error
- 内部机制：读取哪一步失败、哪个流异常、哪层编码出错

**三保留**：
- 用户视角结果：连不上 / 被拒绝 / 已锁定
- 可执行动作：换网络 / 等 15 分钟 / 检查地址配置
- 服务器安全策略：锁定时长、速率限制

### 7.3 映射表全量文案

| code | 标题 | 消息 | 行动建议 | retryable |
|---|---|---|---|---|
| MissingCredentials | 缺少登录信息 | 未提供所需的登录凭据 | 请补全服务器登录配置 | false |
| InvalidKeyFormat | 密钥格式不支持 | 该密钥文件的格式不受支持 | 请使用 OpenSSH 格式的密钥文件 | false |
| KeyParseFailed | 密钥无法读取 | 密钥文件无法解析，可能已损坏或密码错误 | 请确认密钥文件与密码 | false |
| CertificateRejected | 未信任服务器 | 服务器证书未获信任，连接已取消 | 如需连接请重新发起并确认证书 | false |
| DnsFailed | 服务器地址无法解析 | 找不到该服务器的网络地址 | 请检查服务器地址配置 | false |
| ConnectTimeout | 无法连接服务器 | 连接超时，未能建立连接 | 请检查网络后重试 | true |
| TlsHandshakeFailed | 安全连接建立失败 | 无法与服务器建立安全连接 | 请检查网络或稍后重试 | true |
| NetworkUnreachable | 网络不可达 | 当前网络无法到达该服务器 | 请检查网络连接 | true |
| StreamTimeout | 认证超时 | 认证过程耗时过长 | 请重新连接 | true |
| ConnectionLost | 网络连接中断 | 与服务器的连接已中断 | 请检查网络后重试 | true |
| RateLimited | 请求过于频繁 | 短时间内连接请求过多，服务器暂时限制了访问 | 请稍候片刻再试 | true（重连退避） |
| AccountLocked | 账户已临时锁定 | 出于安全考虑，多次认证失败后账户被暂时锁定 | 请于 15 分钟后重试 | false |
| InvalidCredentials | 用户名或密码错误 | 服务器拒绝了当前凭据 | 请检查后重试 | false |
| PubkeyNotAuthorized | 密钥未获授权 | 该密钥未被服务器授权登录 | 请在服务器上添加公钥授权 | false |
| SignatureVerificationFailed | 密钥验证失败 | 密钥与服务器记录不匹配 | 请确认使用正确的密钥文件 | false |
| ChallengeExpired | 认证超时 | 认证流程耗时过长，已失效 | 重新连接即可 | true |
| AuthServiceUnavailable | 服务暂时不可用 | 服务器暂时无法处理登录请求 | 请稍后重试 | true |
| ProtocolError | 通信异常 | 与服务器的通信出现异常 | 请更新客户端后重试 | false |
| SessionExpired | 会话已超时 | 长时间未操作，会话已结束 | 请重新连接 | true |
| Unknown | 连接失败 | 发生未知错误 | 请重试 | false |

retryable 语义：true → 自动重连状态机可续期；false → 确定性失败，不自动重试（避免触发服务端 5 次失败锁定 15 分钟机制）。

### 7.4 通知中心激活

`NotificationCenter.tsx` 从 demo 硬编码改为真实数据源：

- 新增通知 store（zustand，内存瞬态，不持久化——遵循「存储全部走 Rust」约束，通知属于瞬态 UI 状态）
- ServerManager 在以下时点派发结构化通知：connect 失败（带 code + 对比提示）、connection-lost（带 code）、重连成功、自动重连最终失败
- 通知条目：严重度图标 + 标题 + 消息 + 行动建议 + 时间戳
- 关闭窗口/断开连接时清理对应通知（遵循「关窗取消订阅」约束——通知与服务器绑定，服务器删除/断开时其通知保留但不再更新）

### 7.5 重连门控升级

`isRetryableConnectError(msg)` 字符串白名单（ServerManager.tsx:66-70）→ 直接查映射表 `code.retryable`：

- 前端 invoke reject 收到 `ConnectError` JSON（含 code）→ 查表
- 收到裸字符串（fallback/旧路径）→ 保留现有 4 关键词逻辑作为降级

### 7.6 既有 UI 同步升级

- Settings.tsx 连接卡片（:647-649）红字 → 映射表文案（title + message + action）
- Terminal.tsx 降级提示（:223-234）ANSI 红字裸字符串 → 映射表文案

### 7.7 多服务器对比提示

连接失败通知派发时，只读 serversStore 已有状态：若存在其他已连接服务器，通知附带：

```
ℹ️ 其他 N 台服务器连接正常，仅此台无法连接
   可能是本机与该服务器之间的网络问题，建议更换网络环境后重试
```

仅在其他服务器状态为 `connected` 且 ≥1 台时显示；零探测（不 ping、不发探测包）。

## 8. 测试策略

| 层 | 测试 |
|---|---|
| quirel-protocol | `AuthErrorCode` serde 往返（序列化→反序列化等值）；wire_compat.rs 新增 golden 用例：AuthResponse 带 code 与不带 code 两种字节形态，锁定兼容性 |
| Agent | 失败点映射单测（mock 各失败条件，断言发出的 code） |
| 客户端 | ConnectError 序列化格式；fallback 字符串分类路径单测（旧 Agent 文案 → code） |
| 前端 | 映射表 `satisfies` 编译期全覆盖；映射表与 Rust 枚举数值一致性（可加单测对照常量表） |

## 9. 实施边界

**本次做**：协议扩展（AuthErrorCode 枚举 + AuthResponse 新增 code 字段 + Payload::Error 数值复用）、Agent 失败点补发 code、客户端 ConnectError 结构化、connection-lost 事件扩展、前端映射表 + 通知中心激活 + 重连门控升级 + Settings/Terminal 文案升级、多服务器对比提示、wire_compat golden 用例。

**本次不做**：业务命令（remote_* 15 个命令的 Payload::Error code 利用）、主动网络探测（captive portal 检测、ICMP/TCP 主机探测）、wss://443 TCP 回退传输、多语言（文案先中文，结构已支持扩展）。

## 10. 涉及文件清单

| 文件 | 变更 |
|---|---|
| quirel-protocol/src/envelope.rs | 新增 AuthErrorCode 枚举；AuthResponse 加 code 字段；from_i32 映射 |
| quirel-protocol/tests/wire_compat.rs | golden 用例 ×2 |
| agent/src/server/quic.rs | 失败点补发 code；归因黑洞补丁；close code 0x03 |
| src-tauri/src/connection.rs | ConnectError 结构；~40 错误分支收敛；close reason 真实化；事件 payload 扩展 |
| src-tauri/src/lib.rs | 命令返回类型同步 |
| src/context/ServerManager.tsx | 重连门控查表；通知派发 |
| src/stores/（新增通知 store） | 通知状态管理 |
| src/shell/NotificationCenter.tsx | demo → 真实数据源 |
| src/apps/Settings.tsx | 连接卡片文案升级 |
| src/apps/Terminal.tsx | 降级提示文案升级 |
| src/types/（新增错误码常量与映射表） | 前端唯一文案源 |
