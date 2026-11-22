# Quirel 第三轮全项目安全与鲁棒性审计报告

日期：2026-11-20 · 分支：feature/agent-provisioning (5fcc926) · 工作区：干净
方式：只读审计（Read/Grep/Glob + 只读 git），三个并行分区子代理全量扫描 +
主代理对全部高危与关键中危发现逐行复核证据。**未修改任何源码**，修复另行走分支。
本报告供其他 AI 参考修改：每条发现含文件/行号证据、严重级、影响、最小修复建议。

> 对照基线：`docs/audits/2026-11-01-bug-audit-report.md`（第一轮）、
> `docs/audits/2026-11-08-audit-report.md`（第二轮）、`docs/PROGRESS.md` 修复映射。
> 本轮重点：① 新增 SSH 自动安装 / 客户端驱动更新链（provision/ssh/agent_update/
> update_manager）这一最大新攻击面；② 历史修复回归核实；③ 全项目鲁棒性兜底。

## 执行摘要

| 级别 | 数量 | 说明 |
|---|---|---|
| 严重 | 0 | 未发现未认证 RCE / 完全 root 接管级问题 |
| 高 | 4 | 降权路径缺 setgroups（垂直提权）、stream_count 无上限 OOM、同步管道 IO 冻结 runtime、SSH host key 校验晚于认证（MITM 截密码） |
| 中 | 8 | 全局锁跨阻塞 finish、管道帧歧义 + 2GB 分配、PAM 同步阻塞、SSRF 漏云元数据、Copy 无上限、SSH/SFTP 无界读、重连覆盖竞态、缓存信任锚自洽 |
| 低 | 23 | 超时/上限缺失、死代码、日志红线违例、CSP 残留、脚本健壮性、门禁启发式局限等 |

**Top 4 风险（建议优先修复）**：
1. **A-01** 用户 PTY 会话与隔离子进程降权时未 `setgroups`，继承 root 附加组（至少 gid 0）——跨越用户→root 组边界。第一轮 T2-04 修复仅覆盖 executor.rs 文件操作路径，此两处遗漏。
2. **B-01** provision 的 SSH host key「强制比对」发生在**认证之后**，握手期 `check_server_key` 恒放行——install 阶段的主动 MITM 可在指纹拒绝前截获 SSH/sudo 密码。
3. **A-02** 传输 `stream_count` 仅有下限 `.max(1)` 无上限，认证用户发 `u32::MAX` → ~96GB Vec 分配 → alloc-abort 杀死共享 Manager（跨用户 DoS）。
4. **A-03** 下载/上传 chunk IO 同步阻塞在 tokio worker，FIFO/SIGSTOP 可永久占满 worker 线程池 → 整个 runtime 冻结（跨用户 DoS）。

**总体结论**：第二轮审计加固绝大多数回归在位（S1/H1/H2/H5/H6/M1-M19/T2-02/T2-03/T3-01/T6-01/协议基线/供应链），前端 XSS 面干净、凭据信封加密只加强未降级。本轮新发现集中于**新增 provision/update 链的认证时序与信任锚**、**降权覆盖不全**、**认证后跨用户 DoS 兜底**。

## 审计范围与方法

- **分区 A（agent 服务端）**：agent/src/ 全部 + deploy/install.sh、install-quireld.sh、build.sh、systemd 单元。
- **分区 B（src-tauri 客户端）**：src-tauri/src/ 全部 21 文件 + tauri.conf.json（CSP）+ capabilities。
- **分区 C（前端+协议+脚本）**：src/、quirel-protocol/src/ + tests/wire_compat.rs、scripts/*.mjs、package.json、vite.config.ts、index.html。

**威胁模型与严重级校准**：
- 未认证网络攻击者打 QUIC 端口（认证前 DoS / 认证绕过）。
- 已认证低权用户试图提权——**PTY 按设计授予该用户完整 shell，"用户能以自己身份执行任意代码"不计为发现**；必须跨越信任边界（提权到 root/跨用户/认证前影响/本地提权/客户端被恶意服务器攻陷/供应链）。
- 恶意/被控服务器：客户端解析远端响应必须鲁棒；远端可控串进入 webview innerHTML 即严重（webview 持完整 IPC 权限）。
- 鲁棒性计入：不可信输入上的 unwrap/expect/panic、缺失超时、无界内存、死锁、竞态、资源泄漏、静默吞错致数据损坏。

**证据可靠性**：全部高危（A-01/02/03、B-01）与关键中危（A-05、B-04、B-13）已由主代理实际 Read 复核行号与代码片段（下文标 ✓）。其余发现行号来自分区子代理实际 Read，修复时请就近复核。对依赖部署环境或触发时序的发现已标注「需验证」。

## 已加固基线（回归确认在位，勿重复修复）

- **TLS/钉扎**：S1 真实签名校验在位（connection.rs:2322-2342 verify_tls12/13_signature + 回归测试）；QUIC TOFU 顺序正确（指纹确认先于认证）；证书变更弹窗新旧指纹并列、日志去指纹（debug 仅 8 字符）。
- **认证前防线**：H1 未认证闸 64 + AUTH_IO_TIMEOUT 30s + idle 120s（quic.rs:46,52,254-264）；挑战单次消费 + TTL60s + 公钥绑定（challenge.rs:96-127）；StrictModes（ssh.rs:118-165）；IP 限流 + 用户名锁定双保险。
- **本地攻击链 H2**：IPC socket 0600 + peer_cred 同 uid（ipc_server.rs:592-595）；Session abstract socket verify_peer_is_manager + close_inherited_fds + PR_SET_DUMPABLE（session_process.rs:246-252,398-409）；帧 64KB。
- **降权（部分）**：executor.rs 文件操作路径 setgroups→setgid→setuid→验证 + /etc/group fail-closed（188-230）✓；path_guard canonicalize + 系统前缀 + TOCTOU 复查（29-114）；uid 服务端盖章（protocol_adapter.rs:38-47）。**注意：PTY/隔离路径遗漏 setgroups，见 A-01。**
- **传输/文件**：H5 临时文件恒截断 + finish 硬校验 transferred==file_size（transfer.rs:1236-1241,1274-1281）；M13 FILE_CHUNK_TIMEOUT、M14 abort_spawned、M15 resize 按 server_id 精确取连接；帧 10MB 上限 + chunk size 校验（raw_frame.rs:16,34,72）；临时文件 O_NOFOLLOW+0600+UUID；命令白名单 canonical + spawn_blocking + timeout kill（command.rs:44-66,137-198）；file handler spawn_blocking（T3-01）。
- **更新链**：更新三命令 root 门控（quic.rs:417-419）；staging 0700/0600 + sha256 + ELF e_machine 校验 + commit 强制备份/restore + 崩溃观察窗 600s/第 3 次回滚（update_manager.rs / crash_detector.rs）。**状态查询遗漏 root 门控，见 A-14。**
- **凭据**：keyring 32B 主密钥 + AES-256-GCM，nonce 每次 SystemRandom 12B；密码/私钥 Zeroizing；SshAuthData Debug 脱敏；serversStore partialize **无条件** stripSecrets（M18，serversStore.ts:150-157）；credentials.ts 禁明文回退。
- **前端 XSS**：唯一运行时 innerHTML（Terminal.tsx:501）经 escapeHtml 五实体转义（T6-01 在位）；AppIcon/AppBadge dangerouslySetInnerHTML 为内部常量 SVG；零 eval/new Function；远端错误串经 React 文本渲染。
- **浏览器红线**：会话销毁唯一入口 init.ts:99-113；全部 body portal 带 data-browser-overlay（22 处）；心跳 1s + listen disposed 防护（M22）；normalize_url 中和 javascript:/data:（browser_window.rs:678-701）；capabilities 仅 main 窗口有 IPC，浏览器 webview 无 IPC。
- **协议兼容性**：Envelope/Payload 后加字段全带 #[serde(default)]（含 agent_update 全系）；wire_compat 40+ golden 锁死字节格式；新 payload 凭 capabilities 门控防旧 Agent 断流。
- **供应链**：install-quireld.sh 三级源 quireld.cn→quireld.icu→GitHub，SHA256 不匹配 exit 1 **不换源**，精确文件名 sha256sum -c，curl 超时 + mktemp/trap + 架构白名单；install.sh 升级失败自动回滚 + 备份 glob 限定前缀 + --binary-hash 不匹配 exit 1（5fcc926）；tar 解压防线（拒绝对/../符号链接/setuid，200MB 上限，unpack_in）；SHA256SUMS 严格解析；压缩包子系统固定 argv 模板 + 条目校验无命令注入；publish latest.json 最后上传；release.mjs main 分支 + 干净树 + semver + lockstep + tag 纪律。
- **存储/日志**：cert.rs 私钥 0600+O_NOFOLLOW+旧文件收紧（T2-02）；audit.rs 字段转义防注入（H7）+ 0600；前端 logger 生产 console 仅 warn+、落盘 info 起、1s 节流 + 2000 字符截断；存储版本契约阻断机制（T-03）。

## 发现清单

### 🔴 高危（4）

#### A-01 ✓ 降权路径缺 setgroups，用户 shell 继承 root 附加组（垂直提权）
- 位置：`agent/src/worker/session_process.rs:235,240`（PTY 会话 fork）；`agent/src/main.rs:430,433`（隔离子命令）；对照 `agent/src/auth/executor.rs:190`（文件操作路径**有** setgroups）
- 证据：session_process.rs 与 main.rs 两处降权仅 `setgid`→`setuid`，全程无 `setgroups/initgroups`；全仓 setgroups 仅 executor.rs:190 与 namespace.rs（UserNamespace 已导出但无调用方）。`setuid/setgid` 不清除附加组。
- 影响：quireld 以 root 运行（systemd User=root），用户 PTY bash 与隔离 reader/writer/merger 子进程保留 root 附加组（至少 gid 0），获得超出自身组的组级文件访问；若 root 属 adm/disk/shadow/sudo 等组则可直接读敏感文件——跨越用户→root 组边界。**最大危害取决于部署的 root 附加组与目标文件组权限（需验证），但"保留 root 组"这一边界突破确凿。**
- 修复：两条路径在 setgid 之前补 setgroups（复用 executor.rs 的附加组解析 + /etc/group fail-closed），与 executor fork 路径对齐；或接入 UserNamespace 统一降权。
- 备注：第一轮 T2-04（initgroups）修复仅覆盖 executor.rs，此两处遗漏——属修复覆盖不全。

#### A-02 ✓ stream_count 无上限 → 认证用户触发 OOM/alloc-abort，Manager 全体断线
- 位置：`agent/src/handler.rs:591`（仅下限）；`agent/src/transfer_session.rs:185`（无界分配）；`agent/src/server/quic.rs:1691`（直传 *stream_count）
- 证据：
  ```rust
  // handler.rs:591
  let stream_count = stream_count.unwrap_or(1).max(1);   // 只有下限，无上限
  // transfer_session.rs:185
  let part_paths = vec![String::new(); stream_count as usize];
  ```
- 影响：认证用户发 `stream_count=u32::MAX` → ~96GB Vec 分配 → alloc-abort/OOM 杀死共享 Manager，所有用户连接中断（跨用户 DoS）。
- 修复：入口钳制上限（如 `stream_count.min(16)`），并在 compute_part_offsets/new_multi_stream 前校验。

#### A-03 ✓ 同步管道 IO 占用 tokio worker，FIFO/SIGSTOP 可永久阻塞 → runtime 冻结
- 位置：`agent/src/server/quic.rs:2259`（下载 reader.read_next_chunk 同步）、`2411,2462`（上传 writer.write_chunk 同步）；`agent/src/file_stream.rs:906-926,731-752`（pipe 读写）；`agent/src/main.rs:206`（裸 #[tokio::main]，worker=num_cpus）、`634-655`（run_metadata 仅拒 is_dir，FIFO size=0 通过）、`519-540`（run_isolated_reader 任意类型 File::open）
- 证据：
  ```rust
  // quic.rs:2249-2259  reader 已 take 出 session、锁已 drop，随后同步读
  let mut reader = session.reader.take()...;
  drop(sessions);
  loop { let chunk = match reader.read_next_chunk() { ... }   // 同步阻塞在 async worker
  ```
  path_guard.rs 无文件类型检查（grep is_file/file_type 空）。
- 影响：用户 `mkfifo ~/f` 后请求下载 → 隔离子进程 open FIFO 阻塞 → 父进程 pipe read 永久阻塞 → worker 线程永久泄漏（reader 已 take，cleanup 无法 abort）；或用户 PTY `SIGSTOP` 自家隔离子进程同效；占满 num_cpus 个 worker 即冻结整个 runtime（跨用户 DoS）。
- 修复：chunk IO 包 spawn_blocking（finish/abort 已这样做，quic.rs:2419,2433,2467,2483 ✓）；下载/读取拒绝非常规文件；pipe 读加超时。

#### B-01 ✓ SSH host key 指纹比对晚于认证，MITM 可先截获密码再被中止
- 位置：`src-tauri/src/provision.rs:251-258`；`src-tauri/src/ssh.rs:129-139`
- 证据：
  ```rust
  // provision.rs:251  先连接（握手 + 密码认证一体）
  let mut session = connect_session(&params, &ctx).await?;
  // provision.rs:254-255  认证完成后才比对指纹
  let observed = session.host_key_fingerprint().to_string();
  if observed != options.host_key_fingerprint { return Err(...SSH_HOST_KEY_CHANGED...); }
  // ssh.rs:138-139  check_server_key 恒放行，握手期不校验
  // 接受握手继续；是否信任由调用方比对 known host 指纹后决定
  Ok(true)
  ```
- 影响：detect 与 install 之间若存在主动 MITM，SSH 密码（及复用的 sudo 密码）在指纹比对拒绝之前已发送给对端，"强制比对"无法防止凭据泄露。
- 修复：把用户确认的指纹传入 SshSession::connect，在 check_server_key（握手期、认证前）比对并拒绝；凭据保护只加强不降级。

### 🟠 中危（8）

#### A-04 全局 TRANSFER_SESSIONS 锁跨阻塞 finish/abort（waitpid 无超时）→ 跨用户串行/挂起
- 位置：`agent/src/handler.rs:1014-1032`（complete 持锁调 writer.finish）、`1101-1115`（cancel 持锁 writer.abort）；`agent/src/auth/executor.rs:500-554`（wait_isolated_child 阻塞 waitpid 无超时 502）
- 影响：NFS hard/FUSE 挂起使子进程进 D 态 → waitpid 永久 → 全局锁永久持有 → 全体用户传输挂起；正常大文件 fsync 也串行化所有人。
- 修复：take writer 后**释放锁**再 spawn_blocking 执行 finish/abort（仿 handler.rs:930-967 chunk 路径）。

#### A-05 ✓ Manager↔隔离子进程管道帧歧义 + 错误分支无界分配（~2GB）
- 位置：`agent/src/auth/executor.rs:242-245`（Ok 写 [4B LE len][JSON]）、`716-731`（Err 写 [0x00][4B len][JSON]）、`744-761`（read_result_from_pipe 首字节 0x00 当错误标记，错误分支 vec![0u8;msg_len] 无上限）、`789`（正常分支 100MB 上限）
- 证据：成功帧首字节是 LE 长度的**低字节**；当 `data.len()%256==0` 时为 0x00，被误判为错误帧，随后 4B msg_len 含 JSON 首字节 '{'(0x7b) → ≈2.06GB 零页分配。代码注释 778 行误称"第一个字节是长度的高位字节"（实为低位），印证帧格式认知错误。
- 影响：文件 handler 全走 execute_as_user_blocking（非 root fork），认证用户构造目录使列表 JSON 长度 %256==0 → 共享 root Worker 伪失败 + ~2GB 分配；严格 overcommit/cgroup 下 Worker abort，全体文件操作受影响。**易触发**（任意长度恰为 256 倍数的 JSON）。
- 修复：错误帧用独立 magic（非单字节 0x00，如 4 字节魔数）；错误分支 msg_len 加上限（与正常分支一致）；修正注释。

#### A-06 同步 PAM 认证运行在 async worker（无 spawn_blocking）
- 位置：`agent/src/server/quic.rs:624`（authenticate_password 直调）；`agent/src/auth/pam.rs`（do_pam_authenticate 同步）
- 影响：网络 PAM（LDAP/SSSD）慢时，未认证闸 64 连接 × 多用户名可占满 num_cpus worker → 全服停摆。
- 修复：PAM 调用包 spawn_blocking。
- 备注：默认安装 /etc/pam.d/quireld = @include common-auth（install.sh:218-227），标准 Ubuntu 为本地 pam_unix，影响较低；**需验证部署 PAM 后端**。

#### A-07 代理 SSRF 过滤不全（漏云元数据段）
- 位置：`agent/src/server/proxy.rs:162-170`（仅拦 169.254/16 + fe80::/10 + v4-mapped）
- 影响：漏阿里云 100.100.100.200、AWS IPv6 fd00:ec2::254，认证用户经代理读取云实例元数据（含临时凭据）。DNS rebinding 不可行（connect 用已过滤的解析地址 43-81 ✓）。
- 修复：补云元数据地址或 100.64/10 + fc00::/7。
- 备注：H3 SSRF 防护已在位但覆盖面不全；**需验证云部署**。

#### A-08 Copy 递归无大小/数量上限 + 共享 Worker 串行 → 阻塞全体文件操作
- 位置：`agent/src/worker/handlers/file.rs:662-677`（copy_path_recursive 无限制）；`agent/src/worker/mod.rs:75-126`（串行循环）
- 影响：认证用户复制巨树长时间独占共享 Worker，全体用户文件操作排队阻塞（ReadFile 有 10MB + 常规限制 file.rs:200-210 ✓，Copy 无）。
- 修复：Copy 加总量/数量上限或超时。

#### B-02 SSH 通道输出与 SFTP 下载无界读取，恶意服务器可致客户端 OOM
- 位置：`src-tauri/src/ssh.rs:252`（run_channel）、`363,378,393`（download_file）
- 证据：download_file 限额仅依赖服务器自报 meta.size（378），size 为 None/谎报时 `.read_to_end(&mut buf)`（393）无界读尽；run_channel stdout/stderr Vec 在 120s 超时内无累计上限。
- 影响：恶意 SFTP/SSH 服务器灌入 GB 级输出即可耗尽客户端内存（provision 探测/安装均经此路径）。
- 修复：run_channel 对 stdout/stderr 设累计上限（如 1MB）超限断流；download_file 用 take(size_limit) 包裹读取，不信任 meta.size。

#### B-03 同 server_id 重连覆盖旧条目，旧连接清理竞态可摧毁新连接（需验证）
- 位置：`src-tauri/src/connection.rs:480`（insert）、`694-709`（统一清理）
- 证据：conns.insert(server_id, ...) 直接覆盖同 key 旧条目未先断开；旧主循环因 heartbeat/status 持 tx clone 继续存活，死后清理按 server_id remove/abort subscriptions（705）。
- 影响：旧连接延迟死亡时，其清理可能 remove 新条目、取消新传输/订阅并 emit connection-lost，竞态摧毁刚建立的新连接。
- 修复：insert 前先断开并清理同 server_id 旧条目（或拒绝重复 connect）；清理按连接身份（generation/id）而非仅 server_id 匹配。**需验证实际触发时序。**

#### B-04 ✓ release 本地缓存仅信本地 sidecar，信任锚自洽（设计已声明接受同信任域风险）
- 位置：`src-tauri/src/provision.rs:851-861`（cache_entry_valid）、`904-912`（缓存命中跳过下载）、`849-850`（设计注释）
- 证据：
  ```rust
  // provision.rs:852-858  仅比对本地 .sha256 sidecar
  let Ok(expected) = std::fs::read_to_string(sidecar) else { return false };
  ... match sha256_hex_file(tarball) { Ok(actual) => actual == expected, ... }
  // 849-850 注释：缓存与客户端同信任域…复验目标是损坏而非本地攻击者；
  // 二进制端到端完整性仍由 install.sh --binary-hash / 服务端 staging 校验兜底。
  ```
- 影响：具同用户写权限的本地攻击者替换 tarball+sidecar 后，客户端把恶意二进制经 sudo 安装为服务器 root 守护进程。**关键：注释声称的 install.sh --binary-hash 兜底实为自洽校验**——binary_hash 由（可能被篡改的）tarball 自身计算，install.sh 与服务端 staging 均比对客户端发来的同一哈希，无独立信任锚。
- 修复：缓存命中仍从源拉取 SHA256SUMS 比对（独立锚），或缓存仅存源 manifest 期望哈希用于复验；至少修正注释中"兜底"论证。
- 备注：前提为同用户本地写权限（与 keyring 同信任域，边际风险有限——同用户已可读 keyring 主密钥）；代码注释已显式接受该威胁模型。本条主要指出其兜底论证缺陷，属纵深防御/文档修正。

### 🟡 低危（23）

**agent 服务端（A-09 ~ A-17）**

| ID | 位置 | 问题 | 修复 |
|---|---|---|---|
| A-09 | update_manager.rs:574-589,167-205；quic.rs:1329 | probe_version 注释称 1.5s 实际 .output() 无超时无输出上限；finish_upload 同步阻塞 async（root-only，自伤为主） | timeout + spawn_blocking + 输出上限 |
| A-10 | update_manager.rs:592-597 | replace_binary 注释称原子实为 unlink+fs::copy，中途崩溃/盘满致二进制缺失（有 backup+restore 兜底可恢复） | 同目录 staging + fsync + rename |
| A-11 | auth/ssh.rs:540-613；executor.rs:650-687；worker_manager.rs:487；namespace.rs | 危险死代码：authenticate_pubkey signature=None 仅公钥匹配即 Success（网络路径不调用，quic.rs:745→790→902 随机挑战+验签 ✓）；spawn_process pre_exec 无 setgroups；UserNamespace/wait_for_exit 无调用方 | 删除或加固（接入前补 setgroups/验签） |
| A-12 | quic.rs:1310；update_manager.rs:156-164 | 上传计数用客户端声明 size 而非实际字节（root-only；finish_upload:169-173 有 actual_size 复核兜底） | received += data.len() + 累计上限 |
| A-13 | auth/rate_limiter.rs:153-156 | 每次失败刷新 locked_until → 攻击者持续对受害用户名失败认证可永久锁定该账户（锁定 DoS） | 锁定期内不刷新或设封顶 |
| A-14 | quic.rs:1266 | AgentUpdateStatus 无 root 门控，任何认证用户可查版本/staged/last_result（更新三命令本身已 root 门控 ✓，状态查询遗漏） | 对齐 root 门控 |
| A-15 | quic.rs:2538-2566,218-231 | 多流合并等待 100ms 轮询不查 is_timeout + 清理仅机会式（连接 cleanup 时）→ 保活连接不完成其余流可无限泄漏 task/session/段文件 | 轮询加 deadline + 周期清理 |
| A-16 | manager/session_manager.rs:231-264；executor.rs:278-281 | monitor waitpid(-1,WNOHANG) 抢先回收 → 父 waitpid ECHILD 被忽略 status=0 → 退出检查静默跳过，子进程异常退出误判成功 | monitor 仅回收已知 session pid，不用 -1 |
| A-17 | handler.rs:872 | 多流重复 join：contains_key 仅在 writer 仍在 map 时有效，take 后同 index 再 join 生成新 writer 覆盖 part_paths[idx] → 段文件泄漏 + 合并错段（自伤为主） | 用独立 joined 索引集去重 |

**src-tauri 客户端（B-05 ~ B-13）**

| ID | 位置 | 问题 | 修复 |
|---|---|---|---|
| B-05 | connection.rs:1420,1441,1464,1473-1474；前端 openRemoteFile.ts:127-128 | remote_open_locally Rust 侧无扩展名白名单仅靠前端路由门控（当前 .exe/.hta 不路由到此命令，不可直接利用）；临时名可预测有同名覆盖竞态 | Rust 侧加扩展名白名单（仅 .html/.htm）；临时文件随机名+独占创建 |
| B-06 | terminal.rs:184-201,299-317 | 终端输出读循环无超时且 close 仅 remove_session 不 abort 阻塞 recv 的输出任务 → 恶意服务器半帧挂起泄漏僵尸任务 | 输出读加空闲超时；close 经 JoinHandle abort 或取消信号 |
| B-07 | proxy/mod.rs:186-187,291-304；socks5.rs:32,72 | SOCKS5 握手读无超时 + accept 无并发上限 → 本地进程半开连接耗尽任务/FD | 握手读加超时（10s）；accept 加 Semaphore 上限 |
| B-08 | provision.rs:1022-1029 | tarball 流式下载无总大小上限（read_timeout 仅空闲超时，慢速无限流不触发）→ 被控源磁盘填充 | 下载流加累计字节上限（如 2×EXTRACT_TOTAL_LIMIT）超限中断清理 |
| B-09 | credentials.rs:50-52,89,108,70-73,123 | 主密钥/明文缓冲未 zeroize（密码/私钥已 Zeroizing）；account 消毒把 a/b 与 a_b 映射同一文件（碰撞覆盖）；fs::write 非原子 | 主密钥/明文缓冲 Zeroizing；account 用 hex 无碰撞编码；临时文件+rename 原子替换（nonce 每次随机 12B 正确无需改） |
| B-10 | connection.rs:829-833 | remote_ping 延迟用服务器回显 timestamp 计算，恶意 Pong 回显未来时间致 u64 下溢回绕（release 未开 overflow-checks，仅展示层无内存安全） | checked_sub/saturating_sub 或改客户端往返计时 |
| B-11 | connection.rs:1924-1937,1946-1952,2060 | subscribe 锁仅保护 check 不保护 await，并发同 key 均通过 check 各自 spawn，第二次 insert 覆盖第一个 JoinHandle → 首任务持写端泄漏（注释"锁保护整个订阅流程"与实际不符） | check+insert 同锁内完成，或 await 前 entry 占位防重复 spawn |
| B-12 | tauri.conf.json:18 | CSP connect-src 含开发残留 ws://localhost:1420；缺 base-uri/form-action；style-src unsafe-inline（script 回落 default-src 'self' 无 unsafe-inline/eval、object-src 'none' 正确） | 移除 ws://localhost:1420；补 base-uri 'self'、form-action 'self' |
| B-13 ✓ | store_maintenance.rs:94；proxy/browser.rs:205；proxy/browser_window.rs:1145 | 生产 info 日志输出真实用户路径（path.display）与完整 URL（可能含 token/query），违反日志红线（check:sensitive-logs 门禁未拦，见 C-05） | 路径打码（basename/哈希）、URL 去 query 或截断；补门禁覆盖 |

**前端 + 协议 + 脚本（C-01 ~ C-05）**

| ID | 位置 | 问题 | 修复 |
|---|---|---|---|
| C-01 | src/apps/browser/BrowserNavbar.tsx:119-122,367；BookmarksBar.tsx:32；BrowserApp.tsx:425,447；historyStore.ts:57 | 地址栏危险 scheme 正则仅 trim 不去内嵌控制字符，`java\tscript:` 可绕过前端正则（Rust normalize_url 已中和 javascript:/data:，browser_window.rs:678-701 ✓，故实际不可利用）；多个 browser_create_tab 入口无前端 scheme 校验；历史 record 仅排除 about: | 正则先剥离 [\x00-\x1f\x7f] 再匹配或用 new URL() 按 protocol 白名单；historyStore.record 同步过滤；Rust 侧最终强制（已在位）。脚本在被浏览页面上下文执行，浏览器 webview 无应用 IPC，非应用级 RCE |
| C-02 | scripts/publish-quireld.mjs:76-78,85,192-219,67-72,154 | 版本串仅正则取值无 semver 复验即插值 gh/git shell（release.mjs 已强制 semver，现实风险低，仅 Cargo.toml 被手改时注入）；--out 可递归删除任意目录（操作者即用户，脚枪） | publish 读 VERSION 后复用 semver 正则复验非法即 die；--out 限制在 agent/dist 下或删除前要求确认标记 |
| C-03 | scripts/verify.mjs:114 | WSL 路径单引号包裹未转义 `cd '${repo}'`，仓库克隆到含单引号路径时命令断裂（路径由本机用户控制，仅开发门禁不影响产物；其余 shell:true cmdStr 全为内部常量、MODE 白名单校验无注入） | repo.replace(/'/g, "'\\''") 后再包裹 |
| C-04 | quirel-protocol/src/subscription.rs:52-62 | SubscriptionType::type_name 与 serde rename 手工双写无一致性测试（T1-01 残留；定义已单源化 agent serde.rs:5 re-export，但 type_name 用于订阅路由/日志，新增变体漏改一边致订阅静默失配；wire_compat 未覆盖 SubscriptionType） | wire_compat 增加「每变体序列化 type 字段 == type_name()」遍历断言 |
| C-05 | scripts/check-sensitive-logs.mjs:29-32,92 | 门禁 R3 允许清单过宽（日志块内任意位置出现 null/false/undefined/!!/Exists 即整块放行，如 log.info('x',{password:p,ok:false}) 漏报）；TS_LOG 漏 log.trace（Rust R1/R2 与自测夹具完备） | 允许清单改为「敏感属性自身值形态」判定而非整块关键词；TS_LOG 补 trace |

## 遗留 / 接受风险项状态（核实，不计新发现）

| 项 | 当前状态 | 建议 |
|---|---|---|
| H4 历史密钥泄漏 | agent/key.pem 当前已 gitignore 且未跟踪，但仍存在于历史提交 384727a、4f8c994；dist/ 产物未携带 pem。**未根治** | 轮换该密钥 + git filter-repo 清除历史 + 保持 gitignore（等产品确认无历史版本部署） |
| agent L7 fork 后 malloc 死锁窗口 | session_process.rs:181-189 仍在 fork 子进程内 Command::new(shell).output()。**接受现状仍在** | 修复需重构登录环境加载为预 spawn 方案，风险大于收益，记录待议 |
| T3-02 waitpid 无超时 | executor.rs:278-281 父 waitpid 仍无超时。**接受现状仍在**（与 A-04/A-16 相关） | 合法大文件操作无上界，加超时会误杀，需产品决策 |
| M12 SOCKS5 本地监听无认证 | socks5.rs:22-28 信任模型注释 + mod.rs:285 绑 127.0.0.1 + 默认随机端口。**文档化接受风险**（B-07 补充握手超时/并发上限） | 维持随机端口 + 风险提示；补 B-07 超时与并发上限 |
| T1-01 订阅 type_name 双写 | 定义已单源化，type_name 手工同步无测试 → 见 C-04 | 补 wire_compat 一致性断言 |

## 修复优先级与验证指引（供其他 AI 执行）

**建议修复顺序**（一个提交一个修复，单分支顺序提交，禁止多分支 cherry-pick）：
1. **A-01**（补 setgroups，垂直提权）→ 2. **B-01**（host key 校验前移到握手期）→
3. **A-02**（stream_count 上限）→ 4. **A-03**（chunk IO spawn_blocking + 拒非常规文件）→
5. **A-05**（管道帧 magic + 错误分支上限）→ 6. **A-04**（锁外 finish/abort）→
7. 其余中危（A-06/07/08、B-02/03/04）→ 8. 低危按模块批量。

**红线约束（修复时必须遵守，触碰前加载对应技能）**：
- 凭据加密只许加强不许降级（keyring 主密钥 + AES-GCM 信封，禁改回明文、禁绕过 isKeyringReady）——B-01/B-09 修复不得削弱。触碰 credentials.rs / agent/src/auth/ / path_guard / socks5 / 新增日志前必读 `quirel-safety` 技能 + docs/audits/ 对应文档。
- 日志禁含密钥/私钥片段/口令/token/证书指纹全文/真实用户主目录路径——B-13 修复须打码；改完跑 `npm run check:sensitive-logs`（并按 C-05 补门禁覆盖）。
- 浏览器会话销毁唯一入口（React effect/cleanup 禁止销毁会话，body portal 须带 data-browser-overlay）——触碰前读 `quirel-redlines-browser`，回归 `src/window-system/core/WindowManager.session.test.ts`。
- 窗口坏帧防闪三件套（WM_ERASEBKGND 填色/子窗口链不擦/transparent(false)）——B-12/CSP 与窗口相关改动触碰前读 `quirel-redlines-window`，dupcap 探针回归。
- 协议破坏性变更须双端同发并升次版本（0.x）；新增变体 + serde default 可选字段为兼容性变更——C-04 触碰 quirel-protocol 前读 `quirel-protocol-change`，回归 `quirel-protocol/tests/wire_compat.rs`。
- 版本号唯一入口 scripts/release.mjs，禁手改版本文件/手动 tag；dev 测试部署不改版本字段。
- 新增/修改错误类型、日志、async 生命周期遵循 `quirel-rust-idioms`。

**验证命令（AGENTS.md §6 / quirel-verify 技能）**：
| 改动面 | 必跑 |
|---|---|
| 前端 src/ | `npm run verify:fe` |
| Rust 任一 crate | `npm run verify:rust`（agent 经 WSL） |
| 触碰红线 | 红线表对应回归（WindowManager.session.test.ts / dupcap / wire_compat.rs / check:sensitive-logs） |
| 完成声明 / 请求合并前 | `npm run verify:full` |
| 依赖升级后 | `cargo audit`（需 cargo install cargo-audit，联网） |

**bug 修复黄金流程（强制）**：先写复现测试（此刻必须 FAIL—红）→ 根因修复（systematic-debugging，禁症状压制）→ 复现测试转 PASS（绿）+ verify:full 全绿 → 复现测试永久入库（禁删/ignore）。A-02/A-03/A-05/B-01 均应先写复现测试。

**完成摘要四件套**：变更摘要 / 影响面（含受影响现有测试文件名）/ 测试证据（命令+结果，含红→绿记录）/ 建议 squash 提交信息 `type(scope): 中文简述`。

## 覆盖边界（诚实声明）

- **分区 A**：quic.rs/handler.rs/main.rs/file_stream.rs/transfer_session.rs/executor.rs/manager 全 11 文件/auth 全 8 文件/proxy.rs/archive 全 3 文件/config/cert/audit/subscription/protocol 帧层/deploy 脚本已逐行或段读。**仅 grep 抽查未逐行**：collectors/{metrics,processes,file_watcher}.rs（危险模式 grep 见 mutex unwrap/expect 均 poison-only 非直接可触发）、worker/handlers/{system,shutdown}.rs、worker/ipc_client.rs、auth/stats.rs、diff.rs、event_bus.rs、manager/{hot_update_coordinator,signal_handler}.rs、deploy/{uninstall.sh,build.sh}、examples/probe-deploy.sh。
- **分区 B**：connection.rs（3080 行）精读 TLS/TOFU/remote_send/open_locally/ping/subscribe/清理，未逐行通读全部命令编排；browser_window.rs（1715 行）仅读 normalize_url/历史/导航/心跳，标签状态机/睡眠/bounds 未逐行（窗口红线区安全相关性低）；suggest.rs（988 行）grep 确认固定 host/percent_encode/上限/socks_port 过滤，请求构造/响应解析未逐行；transfer.rs（2323 行）精读临时文件/续传/多流 abort/超时，单流逐字节循环未全读；fullscreen.rs 未读；lib.rs 读日志/本地文件命令/wipe/handler 注册，setup/壁纸/窗口子类未逐行。
- **分区 C**：Rust 强制层（normalize_url/path_guard/credentials/socks5）属 A/B 范围已交叉确认；pdfjs-dist/html2canvas 具体消费点、i18n locale 文案内容未逐条审阅；只读审计未执行 verify/测试，golden 与单测通过状态以 CI 为准。
- **需验证项**：A-01 最大危害（root 附加组/文件组权限）、A-06（PAM 后端）、A-07（云部署）、B-03（重连竞态时序）均依赖部署环境或触发时序，已在条目内标注。
- 本轮未跑构建/测试（只读审计）；上一批加固验证基线为 verify:full 12/12 PASS（5fcc926）。发现数量不应被解读为"覆盖盲区已审干净"。
