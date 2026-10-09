# 文件传输性能与安全重构(B+ 方案)设计

> spec 日期: 2026-08-14
> 状态: 待实现
> 关联根因分析: 见对话历史(根因链已核实代码)

## 目标

三条硬性目标(用户提出):

1. **性能**: 上传/下载吞吐相对当前 5-15x;大文件多流并行 10-30x;**不劣于 SSH 传输**(含 1Gbps 局域网单文件)。
2. **安全**: 真正解决传输安全性——补齐用户隔离、路径校验、审计、临时文件防护四大缺口。
3. **复用**: 协议帧 / IO 模型 / 安全层抽象统一,使传输改造成果复用到 PTY、订阅等功能,功能对等。

## 背景与根因(已核实)

### 根因链

顶层根因: **数据平面与控制平面未分离**——握手/心跳与 64KB 数据块共用同一套 `Envelope{request_id, payload}` JSON 信封 + 同一 async handler + 单流模型。

派生三个直接根因:

- **R1 数据帧 = JSON + base64**: `protocol/serde.rs:80-86` `Envelope::encode = serde_json::to_vec`;`serde.rs:283-289` `FileChunk.data: Vec<u8>` 被强制 base64。64KB → ~85KB,带宽膨胀 33%,CPU 成吞吐天花板。
- **R2 同步 IO 在 async 上下文**: `file_stream.rs:246` `BufWriter<File>`(同步);`file_stream.rs:481-508` `write_chunk` 直接在 async handler 调用,无 `spawn_blocking`,阻塞 tokio worker。
- **R3 QUIC 未调参 + 单流顺序**: `server/quic.rs:378-382` 仅设 idle/keep_alive,未设窗口/拥塞控制;客户端单 `open_bi` 顺序发完。

### 独立安全缺口(与 R1-R3 正交,但必须补)

- **用户隔离未生效(危急)**: `auth/executor.rs:232-270` `execute_as_user_unchecked` 注释明写"不 fork 不降权",非 root 用户也以 root 父进程执行;`auth/namespace.rs`(User Namespace)实现完整但生产路径不调用。文件操作实际以 root 身份,完全无隔离,违背项目硬约束。
- **路径校验缺失**: `handler.rs:347` `handle_file_transfer_request` 无目录穿越/家目录限制;`file_stream.rs:70-77` 注释"调用方负责"但无人做。
- **审计缺失**: `audit.rs:155` `log_file_operation` 标 `#[allow(dead_code)]`,传输全程无审计。
- **临时文件可预测 + symlink 无防护**: `file_stream.rs:17-33` `generate_temp_path` 用 `DefaultHasher` 确定性哈希(非文档声称的 UUID);`file_stream.rs:452-471` `create_fresh_temp_file` 无 `O_NOFOLLOW`,TOCTOU 可被符号链接攻击。

### SSH 性能基线(对比)

- 加密层持平: ring AES-NI(rustls)≈ OpenSSH AES-NI,1Gbps 下两者 CPU 均非瓶颈。
- 方案 B+ 优于 SSH: 高延迟(QUIC 无队头阻塞 + BBR)、丢包无线网、多文件并行、连接迁移。
- 方案 B+ 不如 SSH: 1Gbps 局域网单文件——SSH AES-NI 已饱和链路,而单流方案受单拥塞窗口 + 单 stream 窗口制约。**必须加大文件多流并行**才能不劣于 SSH。

## 架构: 三层并做

```
┌─────────────────────────────────────────────┐
│ 复用层: TransportFrame / Payload 按域拆分   │  阶段3
│   / AsyncFileIO trait / PathGuard            │
├─────────────────────────────────────────────┤
│ 性能层: 裸帧 + spawn_blocking + BBR + 8MB   │  阶段0,2,4
│   窗口 + 256KB chunk + 大文件多流并行        │
├─────────────────────────────────────────────┤
│ 安全层: 路径校验 + 用户隔离(fork+setuid     │  阶段1
│   +namespace) + 审计 + 随机临时名 + O_NOFOLLOW│
└─────────────────────────────────────────────┘
```

## 详细设计

### 1. 协议分层(裸帧,阶段0+2)

控制面(低吞吐,保留 JSON): `Envelope{request_id, payload}` 不变,`read_message/write_message`(4B LE len + JSON)保留。`FileTransferRequest/Accept/Complete/Progress` 仍走 JSON。

数据面(高吞吐,裸二进制帧): 仅 `FileChunk` 数据改走裸帧,不经 Envelope。帧格式:

```
[4B length LE][1B type][body]
  length = 1 + body.len()
  type = 0x01 → body 是 JSON(Envelope,控制消息)
  type = 0x02 → body = [4B seq][4B size][raw bytes](数据块)
```

统一帧格式使一条 stream 一种读法,无歧义区分控制/数据。

**协商降级**: `FileTransferAccept`(`serde.rs:275-280`)增 `frame_mode: String` 字段,**必须加 `#[serde(default = "default_json")]`**(核查发现 serde 对 String 字段默认必需,不加会导致旧客户端不带该字段时反序列化失败、硬断裂)。旧客户端连新服务端 → 回退 JSON 模式,避免硬断裂(回滚安全网)。

### 2. IO 异步化(阶段2)

- `file_stream.rs:481-508` `write_chunk` / `171-206` `read_next_chunk`: 用 `tokio::task::spawn_blocking` 包装同步 `std::io`。保留同步 `BufWriter` 语义(Drop/abort/finish 不变),只在外层包 spawn_blocking。
- `BufWriter` 容量 → 256KB(≥ chunk,减少 syscall)。
- chunk_size 64KB → 256KB(`file_stream.rs:48` + `handler.rs:378,448,506` 三处)。

### 3. QUIC 调优(阶段2)

- 服务端 `server/quic.rs:378-382` `build_server_config` + 客户端 `src-tauri/src/connection.rs`: `stream_receive_window(8MB)` + `receive_window(64MB)` + `congestion_controller(quinn::congestion::Bbr::default_factory())`。
- 两端 `Cargo.toml`: 启用 quinn `bbr` feature。
- 8MB 覆盖千兆×50ms RTT 的 6.25MB BDP 并留余量。

### 4. 大文件多流并行(阶段4,达成不劣于 SSH)

- 触发: 单文件 >100MB。
- 客户端 `src-tauri/src/transfer.rs`: `open_bi` 开 N 个 stream(N 默认 4,可配),每 stream 负责一段 offset。
- 服务端 `file_stream.rs`: 新增 `FileStreamWriter::write_range(offset, data)`,预分配文件 + `pwrite` 各 offset(无需临时文件合并)。
- 复用 `ConnectionContext`(`quic.rs:39-150`)注册/清理多 stream;**`transfer_session_ids` 是 `Vec<String>`(核查发现),多 stream 并发需防重复 push(改去重或 HashSet)**。多 stream 写同一 writer 需 offset 锁(按 offset 分段无冲突,无需 Mutex);方案 ii' 下 writer 持 pipe,多 stream 共享 pipe 需 Mutex 串行化或每 stream 独立子进程(实现时定)。

### 5. 安全补齐(阶段1)

- **路径校验**: 新增 `auth/path_guard.rs`,`validate_path(path: &str, home_dir: &Path, uid: u32)`(核查发现 `UserSession.home_dir` 是 `PathBuf`,签名须用 `&Path` 而非 `&str`);`canonicalize` + 家目录范围检查 + `..` 拒绝;root 特例(uid=0 放开但记审计);返回 `SafePath` newtype。`UserSession` 的 `home_dir/uid/username` 字段已就位(`auth/session.rs:14-31`),无需补字段。在 `handler.rs:347` 入口 + `worker/handlers/file.rs` 各 handler 入口统一调用。
- **审计贯穿**: `audit.rs:155` 去掉 `#[allow(dead_code)]`。**需逐层下传 `Arc<AuditLogger>` 参数**(核查发现 `handler.rs` 完全无 audit_log,`AuditLogger` 实例仅在 `quic.rs:392` `handle_connection` 层有):`handle_envelope`(`handler.rs:22`)→ `handle_file_transfer_request`(`handler.rs:347`)→ `handle_file_upload_stream`/`handle_file_download_stream`(`quic.rs:1681/1752`)逐层加 `audit_log: Arc<AuditLogger>` 参数,调用点同步传入。然后在 `handler.rs:347/555/620` + `quic.rs:1681/1752` 入口/完成/中断处调 `log_file_operation(uid, op, path, size)`。
- **临时文件安全**: `file_stream.rs:17-33` `generate_temp_path` 改 `Uuid::new_v4()`(保留 find_existing_temp_file 逻辑,但断点续传改为查 session 状态而非确定性文件名);`file_stream.rs:452-471` `create_fresh_temp_file` unix 加 `O_NOFOLLOW`(先 `symlink_metadata` 拒绝 symlink)。

### 6. 用户隔离(决策点,阶段1.2)

三方案:

- **方案 i(移 Worker)**: 文件 IO 移到 Worker 进程(已有 `execute_as_user` 真 fork+setuid),FileChunk 经 IPC 流到 Worker 写盘。隔离最彻底,但需新设计 Worker 流式文件 IPC 协议(当前不存在),改动最大。
- **方案 ii'(推荐,Manager 内 fork + pipe 桥接)**: Manager 进程内 fork 子进程,子进程先 `setuid + setgid`,再 `UserNamespace::new(uid,gid).create_and_switch()`(复用 `namespace.rs`,**注意是两步调用**——核查发现 `create_and_switch` 无参,是 `&self` 方法)。子进程内完成全部文件 IO(open + read/write chunk),经标准 `os_pipe` 与父进程交换 chunk 字节;父进程持 pipe 做 `tokio::io::AsyncReadExt/AsyncWriteExt` 与 network 桥接。
  - **避免从零写 SCM_RIGHTS fd 转移**(核查发现 `manager/ipc_server.rs:7-9` 已移除 SCM_RIGHTS、无 send_fd/recv_fd,原方案 ii 的"复用基建"假设不成立)。
  - 子进程同步 IO 不阻塞父进程 tokio worker(天然隔离)。
  - 无需额外依赖(os_pipe 是标准 crate,或直接 std::process + pipe)。
  - 下载:子进程 read file→write pipe;上传:子进程 read pipe→write file。
- **方案 iii(最低,过渡)**: 保持 `execute_as_user_unchecked` 但加严格路径白名单(仅家目录)+ 审计。不真隔离,仅降风险。过渡方案,不达成"解决安全"目标。

**推荐方案 ii'**: 避免不存在的 SCM_RIGHTS 基建、不动 Worker 架构、真 fork+setuid+namespace 隔离、子进程 IO 不阻塞 async runtime、无新依赖。若选 i/iii 见本节替代,计划阶段1.2 需对应调整。

### 7. 复用层重构(阶段3)

- **TransportFrame 抽象**: 新增 `protocol/transport_frame.rs`,`enum Frame { Control(Envelope), Binary(RawChunk) }`,统一 `read_frame/write_frame`,QUIC/WS 复用。
- **Payload 按域拆分**: 拆 `protocol/{auth,file,transfer,terminal,subscription,system}.rs`;`serde.rs` 顶层 re-export。**57 变体**(核查修正,原写 55 有误)分 6 域;`#[serde(flatten)]` 保持线兼容;消除 protocol → 业务层反向依赖(`serde.rs:3-4` 当前引用 `crate::diff`、`crate::auth::stats`)。此任务为纯重构,工作量大且收益间接(编译加速/解耦),**可选延后**,不影响性能/安全目标。
- **AsyncFileIO trait**: 新增 `auth/async_file.rs`,`async fn read_chunk/write_chunk` 强制内部 spawn_blocking;`file_stream.rs` 实现;PTY master_fd 读写复用。
- **PTY 改裸帧**: `quic.rs` PTY 流 + `serde.rs:247` `TerminalData` 复用 TransportFrame Binary,PTY 流量同步去 base64 膨胀。

## 数据流(上传,改后)

```
客户端 FileReader.read_next_chunk (spawn_blocking, 256KB)
 → RawChunk{seq, data}
 → TransportFrame::Binary → write_frame(send_stream)  [4B len][1B type=0x02][4B seq][4B size][raw]
 → QUIC stream (Bbr, 8MB 窗口; >100MB 时多 stream 并行)
 → 服务端 read_frame(recv_stream)
 → PathGuard 已在握手时校验路径
 → FileStreamWriter.write_chunk(data) → async 写 pipe → 子进程(fork+setuid+namespace)read pipe→write file [BufWriter 256KB,子进程同步 IO 不阻塞父进程 async runtime]
 → 循环至 FileTransferComplete (JSON, 走 Frame::Control)
 → audit_log.log_file_operation(...)  [审计贯穿]
```

## 错误处理

- **帧解析失败**(type 非法 / length 超 10MB): 关 stream,触发 `FileStreamWriter::abort`(删临时文件)。
- **IO 错误**(`file_stream.rs:494-502` `write_all` 失败): abort,回 `FileTransferComplete{success:false}`。
- **路径校验失败**: 握手阶段拒绝,回 `FileTransferAccept` 错误或 `Error` payload,记审计。
- **用户隔离失败**(fork/setuid 失败): 拒绝传输,记审计,回错误。
- **连接中断**: Drop 清理临时文件(现有机制 `file_stream.rs:240-242,617-643` 保留)。
- **协商回退**: `frame_mode=json` 时走旧路径,行为等同现状。
- `finish()` 的 `sync_all + rename` 保留(完整性保证)。

## 测试

- `raw_frame` 单测: round-trip、空 data、10MB 边界、截断帧、非法 type。
- `file_stream`: spawn_blocking 语义、超文件大小拒绝、BufWriter 刷新、write_range offset 正确性。
- `path_guard`: 目录穿越(`../`)、家目录越界、symlink、root 特例。
- `audit`: 文件操作审计记录正确。
- **端到端基准**: 本地 QUIC 回环传 100MB/1GB,断言字节完整 + 吞吐 > 当前 N 倍 + ≥ scp。
- 兼容: `frame_mode=json` 回退回归。

## 预期提升

- **5-15x**(消除 R1 CPU 天花板 + R2 IO 阻塞 + R3 流控): 吞吐上限从"CPU 处理速率"→ `min(带宽, 磁盘)`。千兆 ~125MB/s、SSD 200-500MB/s → 带宽先到顶;当前 CPU 受限典型 8-25MB/s → 60-120MB/s。
- **10-30x**(大文件多流并行,阶段4): 高 BDP 链路与 1Gbps 局域网单文件逼近/超过 SSH。
- **硬上限**: `min(带宽, 磁盘)`;千兆物理上限 ~125MB/s。

## 改动文件清单

### agent 侧
- 新增: `protocol/raw_frame.rs`、`protocol/transport_frame.rs`、`auth/path_guard.rs`、`auth/async_file.rs`
- 拆分: `protocol/{auth,file,transfer,terminal,subscription,system}.rs`(从 `serde.rs`)
- 修改: `protocol/serde.rs`、`protocol/mod.rs`、`file_stream.rs`、`server/quic.rs`、`handler.rs`、`auth/executor.rs`、`auth/namespace.rs`(启用)、`audit.rs`、`Cargo.toml`
- 修改: `protocol/transport_frame.rs`(PTY 复用)、`worker/handlers/file.rs`(路径校验复用)

### 客户端侧
- 修改: `src-tauri/src/transfer.rs`、`src-tauri/src/connection.rs`、`src-tauri/Cargo.toml`

## 跨阶段约束

- **零编译错误**(项目硬约束): 每阶段 `cargo build` 通过。
- **双平台编译**: Windows 验证 lib(unix stub 已就位),WSL 验证完整(用 `wsl -e bash -l -c`)。
- **向后兼容**: `frame_mode` 协商保证旧客户端不断。
- **注释保留**(用户规则): 改动处保留既有注释风格。
- **项目结构分类完整**(用户规则): 新增模块按职责归入 `protocol/`、`auth/`,分类清晰。
- **git 由用户操作**(用户规则): 每阶段 commit 由用户执行,实现方不自动 git。
