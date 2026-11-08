# Quirel 第二轮全项目安全审计报告

日期：2026-11-08 · 分支：dev (8a90f22) · 方式：协议层/agent 认证传输链路逐行精读 +
三个并行全量扫描（src-tauri / agent 基础设施 / 前端），只读审计，修复另行走分支

> **修复状态（2026-11-08 同日更新）**：严重 1/1、高危 6/7、中危 25/28 已在
> `fix/audit-round1` 修复并独立提交（0bc96ee…5a47fab，共 33 个提交，一个提交
> 一个修复），三套测试全绿（agent 166 / src-tauri 73 / 前端 tsc+vitest 529）。
> 分支待 squash 合入 dev。未修：H4（历史密钥泄漏，需产品决策）、M10（订阅
> 计数泄漏）、M12（SOCKS5 无认证，文档化建议）、M25（列表虚拟化，性能
> 立项）、低危 39 项。逐项映射与剩余清单见 `docs/PROGRESS.md` 2026-11-08 节。
> 注：修复提交信息中的「H4」指本报告的 H7（审计日志注入），编号映射见
> PROGRESS.md。

## 执行摘要

| 级别 | 数量 | 已修 | 说明 |
|---|---|---|---|
| 严重 | 1 | 1 | TLS 服务器认证完全失效（中间人可复制证书绕过指纹钉扎） |
| 高危 | 7 | 6 | 认证前 DoS、本地攻击链三件套、ProxyOpen SSRF、历史密钥泄漏、下载损坏、私钥入日志、审计注入 |
| 中危 | 28 | 25 | agent 生命周期/并发/配置、客户端传输健壮性、前端竞态 |
| 低危 | 39 | 0 | 死代码群、日志权限、整数溢出、i18n/locale 等（全清单见 PROGRESS.md） |

## 严重（S1）

**客户端 TLS 验证器跳过握手签名校验**（`src-tauri/src/connection.rs:2120-2171`）
`PinningCertVerifier` 对 `verify_tls12/13_signature` 与 `verify_server_cert` 无条件
放行——TLS 1.3 的 CertificateVerify（对端持有证书私钥的唯一证明）从未被验证。
中间人向真实服务器录一份证书后原样转发即可完成握手：指纹钉扎比对通过、首次
连接确认框展示的也是同一份被转发的证书，TOFU 模型整体失效，PAM 密码/终端/
文件流量可被完整截获。修复保留指纹钉扎，签名验证改用 ring provider 的
`WebPkiSupportedAlgorithms` 真实验证。

## 高危

- **H1 认证前 QUIC 无超时**（`agent/src/server/quic.rs`）：应用层超时检查仅在
  认证成功后启动且 `max_idle_timeout(None)`——握手后挂起认证流即可永久占用
  连接，accept 无并发上限。
- **H2 本地攻击链三件套**（quireld 以 root 运行）：
  ① Session 进程 abstract socket（`session_process.rs`）无对端认证，socket 名
  可经 /proc/net/unix 枚举、accept 先到先得，30s 重连窗口可劫持终端 I/O；
  ② Manager IPC（`ipc_server.rs` + `config.rs:229`）socket 落 /tmp、权限纯靠
  umask、首连接无条件当 Worker，而 IPC 消息可指挥任意用户上下文执行
  （uid==0 跳过降权）——本地伪装即提权入口；③ Session 进程 fork 后不清理
  继承 fd，降权后持 Worker 侧 root 描述符，yama=0 可被同 uid 用户 ptrace。
- **H3 ProxyOpen SSRF**（`agent/src/server/proxy.rs`）：以 quireld（root）身份
  连接任意目标无限制，低权用户可借道云元数据（169.254.169.254）窃取凭据。
- **H4 历史密钥泄漏**：`agent/key.pem` 曾提交（blob 4feda9a0，提交 4f8c994），
  git 历史可恢复；`cert.rs` 对已存在密钥原样加载。当前工作区密钥已轮换
  （哈希比对确认），但仍有历史版本部署的服务器等同私钥公开。
- **H5 下载静默损坏**（`src-tauri/src/transfer.rs`）：Agent 端不支持偏移续传
  （从字节 0 重发），客户端临时文件名为确定性哈希、对残留文件按偏移追加——
  取消后重下产出「旧前缀+完整新文件」，且 `create_fresh_temp_file` 存在即
  不截断为同型第二来源。
- **H6 私钥片段入日志**（`connection.rs:2232-2236`）：私钥缺 PEM 头时 error
  日志输出前 50 字符（≈37 字节密钥材料）落盘 client.log。
- **H7 审计日志注入**（`agent/src/audit.rs`）：user/path 未转义直接拼行，
  认证前即可伪造审计记录。

## 中危（agent M1-M10 / 客户端 M11-M19 / 前端 M20-M28）

| # | 位置 | 问题 |
|---|------|------|
| M1 | `agent/src/auth/ssh.rs` | RSA 签名硬编码 SHA-256，rsa-sha2-512 客户端认证恒败 |
| M2 | `handler.rs`/`pty_registry.rs` | 传输/PTY 会话无属主绑定，凭 session_id 可跨用户操纵 |
| M3 | `manager/pty_registry.rs:88-98` | unregister 写锁内 await send_close，单会话冻结全部终端 |
| M4 | `manager/worker_manager.rs` | stop 同步 wait 无超时持双锁，Worker 卡死即热更新半瘫 |
| M5 | `manager/crash_detector.rs` | 热更新走 graceful 路径使检测器 break，新 Worker 崩溃无人重启 |
| M6 | `agent/src/main.rs` | 主路径无 SIGTERM 处理，systemctl stop 直接杀进程 |
| M7 | `manager/protocol_adapter.rs:141` | WriteFile base64 解码失败静默回退，文件无声损坏 |
| M8 | `agent/src/config.rs` | blocked_commands 死配置；metrics_interval_secs=0 忙循环 |
| M9 | `collectors/processes.rs` | 全量进程扫描在 async 上下文同步执行 |
| M10 | `agent/src/subscription.rs:46` | 订阅计数泄漏 + 采集器双启动竞态（未修） |
| M11 | 客户端 JSON/代理/订阅 10 处 | 长度前缀无上限，对端发 0xFFFFFFFF 即分配 ~4GB |
| M12 | `src-tauri/src/proxy/socks5.rs` | SOCKS5 本地监听无认证、支持固定端口（未修，文档化建议） |
| M13 | `src-tauri/src/transfer.rs` | 多流 ACK/merge 读取无超时，任务永久挂死不可取消 |
| M14 | `src-tauri/src/transfer.rs` | 多流循环失败路径泄漏已 spawn 的上传任务 |
| M15 | `src-tauri/src/terminal.rs:296` | resize 取「第一个可用连接」，多服务器发错目标 |
| M16 | `proxy/webview_host.rs:174` | WebView2 创建无期限等待，挂死即会话僵尸化 |
| M17 | `connection.rs:607` | 命令通道串行队头阻塞，慢请求拖住全部远端操作 |
| M18 | `src/stores/serversStore.ts` | keyring 未就绪时凭据明文落盘 servers.bin |
| M19 | `manager/worker_manager.rs` | restart_count 只增不清，累计 3 次永久放弃重启 |
| M20 | `src/apps/FileManager.tsx:467` | loadDir 无竞态守卫，旧响应覆盖新目录 |
| M21 | `FileManager.tsx:1154`+`fileMutations.ts` | 增量补丁按发起时目录过滤，切换目录误伤同名条目 |
| M22 | 三处 | 异步 listen 缺 disposed 防护，监听器永久泄漏 |
| M23 | `useFileManager.ts:415` | 全量保存 mtime 置 0，差异保存 baseMtime=0 |
| M24 | `src/apps/Terminal.tsx:489` | effect 缺 workingDirectory 依赖，默认目录设置失效 |
| M25 | `FileManager.tsx:2457+` | 列表无虚拟化 + O(n·m) 选中判定（未修，性能立项） |
| M26 | `FileManager.tsx` 四处 portal | 缺 data-browser-overlay（§5.1 自约定） |
| M27 | `useTransferProgress.ts:141` | 每进度事件全量写盘 + 终态任务无界增长 |
| M28 | i18n | ctxOpenLinkNewTab 缺失；keyring 报错键错放命名空间 |

## 低危（39 项，全清单见 PROGRESS.md 2026-11-08 节）

agent 基础设施 12（L1-L12）、src-tauri 14（低-1..14）、前端 11（L3-L13，L1/L2
为正面确认）、部署 2（install.sh 配置 0644、systemd 无 UMask/PrivateTmp）。

## 正面确认（排查过、无需返工）

协议 crate 兼容性纪律（golden 测试）、命令白名单（裸命令名防同名绕过）、文件
传输大小限制闭环 + O_NOFOLLOW 0600 临时文件、压缩包解压防线（zip-slip/链接
逃逸/setuid 清理）、get_user_info 的 /etc/passwd TOCTOU 防护、挑战-响应防重放、
客户端凭据信封加密（keyring 主密钥 + AES-256-GCM）、前端 XSS 面干净
（escapeHtml 全覆盖、零 eval）、AGENTS.md §5.1/§6 两条红线合规。
