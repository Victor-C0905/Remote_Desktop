# Session 进程架构设计：SSH 式不断线更新

**日期**: 2026-08-08
**状态**: 设计完成，待用户审查
**作者**: 架构重构

## 1. 背景与目标

### 1.1 问题

当前架构中，Manager 持有所有 PTY 的 master_fd。`systemctl restart` 杀 Manager 时，master_fd 被内核关闭，PTY 会话全部中断。这导致：

- agent 无法自我更新（通过 agent 自己的终端执行 update 会断线）
- 必须依赖 SSH 执行更新
- 与项目"开箱即用、不依赖外部工具"的设计目标冲突

### 1.2 目标

实现 SSH 式进程分离架构，使 Manager/Worker 重启不影响已建立的终端会话：

- **Manager restart 不中断终端**：Manager 只负责 QUIC 监听和路由，不持有 master_fd
- **Worker 热更新不中断终端**：Worker 不参与 PTY 数据通路
- **agent 自我更新**：可通过 agent 自己的终端、文件管理器、或客户端内置按钮触发更新
- **最小侵入**：不影响文件操作、命令执行、认证等非 PTY 功能

### 1.3 对标

SSH 的 per-connection 进程模型：主进程只监听，per-connection 子进程持有 PTY master_fd，主进程重启不影响已建立连接。

## 2. 架构设计

### 2.1 进程拓扑

```
systemd
  └── Manager (主进程, $MAINPID)
        ├── QUIC 监听 (接受客户端连接)
        ├── 认证 (PAM + 公钥)
        ├── 路由层 (Client ↔ Session 数据转发)
        ├── WorkerManager (管理 Worker 子进程)
        ├── IpcServer (与 Worker 通信)
        └── Worker (子进程, 无状态请求处理)
              ├── 文件操作 (ReadDir/ReadFile/WriteFile...)
              ├── 命令执行 (ExecuteCommand)
              ├── 系统信息 (GetSystemInfo)
              └── Session 创建器 (fork Session 进程)

独立进程 (fork 自 Worker, 与 Manager/Worker 无父子关系):
  └── Session 进程 (持有 master_fd, 纯同步 I/O 中继)
        └── bash (孙进程, execvp)
```

### 2.2 职责划分

| 组件 | 职责 | 不负责 |
|------|------|--------|
| Manager | QUIC 监听、认证、Client↔Session 路由、Worker 管理 | PTY I/O、master_fd 持有 |
| Worker | 无状态请求（文件/命令/系统信息）、Session 创建 | PTY I/O、master_fd 持有 |
| Session 进程 | master_fd I/O 中继、窗口调整、bash 退出检测 | 认证、文件操作、命令执行 |
| bash | 用户 shell | - |

### 2.3 通信架构

```
Client ←──QUIC──→ Manager ←──UnixSocket──→ Session ←──PTY──→ bash
                   Manager ←──UnixSocket──→ Worker (IPC)
```

| 通信段 | 协议 | 传输内容 |
|--------|------|---------|
| Client ↔ Manager | QUIC | 加密的业务数据、PTY 数据 |
| Manager ↔ Session | UnixSocket (abstract) | PTY 输入/输出、Resize、EOF、Close |
| Manager ↔ Worker | UnixSocket (文件路径) | IPC 请求/响应（现有） |

## 3. Session 进程设计

### 3.1 创建流程

```
Worker 收到 CreateSession 请求:
1. openpty(&master, &slave) 创建 PTY 对
2. 生成 session_id 和 abstract socket 名称
3. fork() → child 是 Session 进程

Session 进程 (child), 同时持有 master 和 slave:
4. setgid + setuid 降权到目标用户 (必须在 fork 孙进程前完成)
5. bind abstract UnixSocket (监听 Manager 连接)
6. fork() → 孙进程
   孙进程 (继承 master 和 slave 的 fd):
   a. close(master)  // 孙进程不需要 master
   b. setsid()  // 新会话, 脱离控制终端
   c. ioctl(slave, TIOCSCTTY, 0)  // slave 成为控制终端
   d. dup2(slave, 0)  // stdin
   e. dup2(slave, 1)  // stdout
   f. dup2(slave, 2)  // stderr
   g. close(slave)  // 原始 slave fd 已 dup 到 0/1/2
   h. setgid + setuid (显式设置, 与 Session 进程身份一致)
   i. execvp(shell)  // 替换为 bash
7. close(slave)  // Session 进程关闭 slave (只需 master)
8. accept() 等待 Manager 连接
9. 启动两个线程:
   - 线程 A (输出): read(master_fd) → write(socket)
   - 线程 B (输入): read(socket) → write(master_fd)
10. waitpid(孙进程) → bash 退出 → send(EOF) → close → exit

注意: 步骤 4 降权在 fork 孙进程前, 确保孙进程继承降权后的身份。
      步骤 6 孙进程的 setuid 是显式防御, 防止 fork 与 setuid 之间的竞态。
```

### 3.2 为什么用纯同步代码

fork 后 tokio 多线程 runtime 残缺（其他工作线程未继承、锁状态损坏、reactor 失效）。Session 进程不能用 tokio。

Session 进程逻辑极简（双向 I/O + resize + waitpid），纯同步代码完全够用。参考项目已有的 `auth/executor.rs` 的 fork+同步通信模式。

### 3.3 进程身份

```
Worker (root)
  └─ fork → Session 进程
       ├─ setgid(target_gid) + setuid(target_uid)  // 降权
       ├─ fork → 孙进程 (已降权身份)
       │    └─ execvp(bash)  // bash 以目标用户身份运行
       └─ Session 进程 (目标用户身份, 持有 master_fd)
```

Session 进程以目标用户身份运行，比当前架构（root 的 Manager 持有 master_fd）更安全。

### 3.4 退出与清理

| 事件 | Session 进程行为 |
|------|-----------------|
| bash 退出（正常） | waitpid 返回 → read(master_fd) EOF → send(Eof) → close(socket) → exit(0) |
| Manager 发 Close | 收到 Close → close(master_fd) → waitpid(bash) → exit(0) |
| Manager 断开（如重启） | read(socket) EOF → **不退出**，accept 等待重连（30秒超时后关闭） |
| Session 进程崩溃 | bash 变孤儿被 init 领养，Manager 检测 socket 断开 |

### 3.5 Manager 断开后的超时机制

Manager 重启期间，Session 进程的 socket 连接断开。Session 不立即退出，而是：

1. 关闭旧 socket 连接
2. 重新 listen + accept，等待新 Manager 连接
3. 30 秒内无连接 → close(master_fd) → kill(bash, SIGHUP) → waitpid → exit

超时确保残留 Session 进程不会无限存活。

## 4. 通信协议

### 4.1 Session ↔ Manager 协议

二进制帧格式，不依赖 protobuf（Session 进程是纯同步代码，避免引入依赖）：

```
[1字节类型][4字节长度(big-endian)][数据]

类型:
0x01 PtyInput    (Manager→Session)  data=键盘输入字节
0x02 PtyOutput   (Session→Manager)  data=终端输出字节
0x03 Resize      (Manager→Session)  data=4字节cols+4字节rows
0x04 Eof         (Session→Manager)  data=空(可选4字节退出码)
0x05 Close       (Manager→Session)  data=空
0x06 Hello       (双向)             data=session_id字符串(首次连接验证)
```

### 4.2 协议版本兼容

帧头第一个字节的高 4 位保留为版本号（当前 v1 = 0x01~0x06）。低 4 位是类型。

如果未来需要协议变更，高 4 位版本号用于协商。当前版本只支持 v1。

### 4.3 安全：Abstract Socket

Session 进程使用 Linux abstract socket（不创建文件）：

```
socket 名称: \0quirel-session-{session_id}
```

优势：
- 不创建文件，无符号链接攻击
- 只有知道完整名称的进程能连接
- 进程退出后自动消失，无需清理

Manager 从 Worker 获知 socket 名称，直接连接。其他用户无法猜测 session_id，无法连接。

## 5. Manager 重启恢复

### 5.1 不恢复旧终端

简化设计：Manager 重启后不恢复旧终端连接。

- 客户端 QUIC 连接断开（Manager 重启导致）
- Session 进程存活 30 秒（等待重连），超时后自动关闭
- 客户端重连后开新终端（新 Session 进程，新代码）

用户知道：关闭重开就是最新的。

### 5.2 Session 进程超时清理

```
Manager 重启时序:
1. Manager 被 kill
2. Session 进程检测 socket 断开
3. Session 进程重新 listen + accept
4. 30 秒内无 Manager 连接 → close(master_fd) → bash 收 SIGHUP 退出
5. Session 进程 waitpid 后 exit
```

### 5.3 Worker 重连

Worker 现有机制：IPC 连接断开 → Worker 进程退出 → 新 Manager 启动后 spawn 新 Worker → 新 Worker 连接新 Manager。

新 Worker 不持有任何旧 Session 信息（Session 与 Worker 无父子关系）。

## 6. 改动范围

### 6.1 必须修改的文件

| 文件 | 改动类型 | 说明 |
|------|----------|------|
| `agent/src/worker/pty_factory.rs` | **重写** | forkpty → openpty+fork+fork；Session 进程纯同步 I/O 逻辑 |
| `agent/src/worker/session_manager.rs` | 修改 | waitpid 对象从 bash 变成 Session 进程；SessionInfo 新增 socket_name 字段 |
| `agent/src/worker/handlers/session.rs` | 修改 | handle_create_session 返回 socket_name 而非发送 FD |
| `agent/src/worker/handlers/shutdown.rs` | 修改 | 注释更新（Session 进程独立，不依赖 Manager/Worker） |
| `agent/src/manager/pty_registry.rs` | **重写** | master_fd → UnixSocket 连接；read/write/resize 改为通过 socket |
| `agent/src/manager/pty_output.rs` | **重写** | 直读 master_fd → 读 Session socket 的 PtyOutput 消息 |
| `agent/src/manager/ipc_server.rs` | 修改 | 移除 SCM_RIGHTS FD 接收；create_pty_session 改为接收 socket_name |
| `agent/src/manager/mod.rs` | 修改 | resize/kill 改为通过 Session socket；移除 set_window_size |
| `agent/src/manager/connection.rs` | 修改 | unregister 改为发 Close 给 Session |
| `agent/src/manager/orphan_reaper.rs` | 修改 | 回收 Session 进程（如果需要）；或简化为只清理 PtyRegistry |
| `agent/src/manager/hot_update_coordinator.rs` | 修改 | 注释更新（Session 进程独立存活） |
| `agent/src/server/quic.rs` | 修改 | PTY 读写改为通过 PtyRegistry（内部走 socket） |
| `agent/protocol/agent.proto` | 修改 | SessionCreated 新增 socket_name 字段 |

### 6.2 可移除的代码

| 文件/功能 | 原因 |
|-----------|------|
| `worker/ipc_client.rs` 的 `send_fd()` | 不再需要 SCM_RIGHTS FD 传递 |
| `manager/ipc_server.rs` 的 `receive_fd` / `receive_fd_from_stream` | 同上 |
| `manager/mod.rs` 的 `set_window_size` | 改为 Session 进程内 ioctl |
| `PtySession.master_fd` 字段 | 替换为 socket 连接 |

### 6.3 测试文件

| 文件 | 说明 |
|------|------|
| `agent/tests/phase2_integration_test.rs` | 适配新架构 |
| `agent/tests/integration_test.rs` | 适配新架构 |
| `agent/tests/ipc_integration_test.rs` | 适配新架构 |
| 新增 `agent/tests/session_process_test.rs` | Session 进程功能测试 |

## 7. 数据流对比

### 7.1 当前架构

```
Client → QUIC → Manager → PtyRegistry.write(master_fd) → bash
bash → master_fd → PtyRegistry.read() → Manager → QUIC → Client
```
1 跳（Manager 直读直写 master_fd）

### 7.2 方案 A

```
Client → QUIC → Manager → UnixSocket → Session.write(master_fd) → bash
bash → master_fd → Session.read() → UnixSocket → Manager → QUIC → Client
```
3 跳（Client ↔ Manager ↔ Session ↔ bash）

### 7.3 性能影响

| 维度 | 当前 | 方案 A | 差异 |
|------|------|--------|------|
| 延迟 | ~10μs | ~30μs | +20μs（UnixSocket 两次拷贝） |
| 吞吐 | ~100MB/s | ~80MB/s | -20%（序列化开销） |
| 用户感知 | 无 | 无 | 终端交互延迟 <1ms，不可感知 |

UnixSocket 在本机通过内核缓冲区直接拷贝，延迟微秒级。PTY 数据量小（终端文本），吞吐完全够用。

## 8. 安全分析

### 8.1 认证不变

认证仍在 Manager 层（PAM + 公钥），Session 进程不参与认证。客户端必须先通过 Manager 认证才能访问 Session。

### 8.2 UnixSocket 权限

使用 abstract socket，不创建文件：
- 无符号链接攻击
- 只有知道 session_id 的进程能连接
- session_id 是随机生成的，不可猜测

### 8.3 进程降权

Session 进程 setuid 到目标用户后持有 master_fd，比当前架构（root 的 Manager 持有 master_fd）更安全。

### 8.4 隔离性

Session 进程是独立进程，崩溃不影响 Manager/Worker/其他 Session。Manager/Worker 崩溃不影响 Session（bash 继续运行）。

## 9. 更新场景

### 9.1 通过 agent 终端更新

```
1. 用户在 agent 终端执行: sudo bash update.sh
2. update.sh: rm 二进制 + cp 新二进制 + systemctl restart
3. systemctl restart → Manager 被 kill
4. Session 进程存活（bash 继续运行）
5. Worker 存活（孤儿进程）
6. 新 Manager 启动 → spawn 新 Worker
7. 客户端 QUIC 断开 → 重连
8. 用户开新终端 → 新 Session 进程（新代码）
```

### 9.2 通过文件管理器更新

```
1. 客户端文件管理器上传新二进制（走 Worker 文件操作）
2. 客户端发 ExecuteCommand: sudo bash update.sh
3. 同上流程
```

### 9.3 通过客户端内置按钮更新

```
1. 客户端发 ExecuteCommand: sudo bash update.sh
2. 同上流程
```

## 10. 实施阶段

### Phase 1: openpty+fork 替换 forkpty
- pty_factory.rs 重写
- Session 进程纯同步 I/O 中继
- abstract UnixSocket 通信

### Phase 2: Manager 改为路由模式
- pty_registry.rs 重写（master_fd → UnixSocket 连接）
- pty_output.rs 重写（读 Session socket）
- 移除 SCM_RIGHTS FD 传递

### Phase 3: 适配与清理
- quic.rs PTY 读写改为通过 PtyRegistry（内部走 socket）
- connection.rs / orphan_reaper.rs 适配
- hot_update_coordinator.rs 注释更新
- 移除 send_fd / receive_fd

### Phase 4: 测试与验证
- 现有测试适配
- 新增 Session 进程测试
- 更新场景验证（Manager restart、Worker 热更新）

## 11. 风险与缓解

| 风险 | 严重度 | 缓解 |
|------|--------|------|
| fork 后同步代码 bug | 中 | 参考 auth/executor.rs 已有模式；充分测试 |
| Session 进程泄漏 | 低 | 30 秒超时自动退出；Manager 启动时清理残留 |
| 协议不兼容 | 低 | 版本号字段；协议极简，基本不变 |
| 性能下降 | 低 | UnixSocket 微秒级延迟；PTY 数据量小 |
| abstract socket 不可移植 | 低 | Linux 专属，项目只支持 Linux |

## 12. 约束遵循

- `KillMode=process`：systemctl restart 只杀 Manager，Session/Worker 存活 ✓
- `ExecReload`：SIGHUP 触发 Worker 热更新，Session 不受影响 ✓
- 用户隔离：Session 进程 setuid/setgid，与当前架构一致 ✓
- 开箱即用：无额外配置，Session 进程自动管理 ✓
- 代码零错误编译：所有改动必须通过 `cargo build --release` ✓
