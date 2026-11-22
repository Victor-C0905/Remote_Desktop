# quireld 自动安装与更新客户端化 — 实施计划

## Context

Quirel 客户端目前仅能通过 QUIC 连接已安装 quireld 的服务器，首次部署完全依赖手动流程（build.sh 打包 → scp → ssh → install.sh），更新同样手动，对最终用户门槛过高。本计划实现：

1. **客户端内置 SSH 全自动安装**：添加服务器向导一键"自动安装"，客户端用 russh 以用户已填凭据 SSH 到服务器，从官网静态源下载对应架构 quireld 二进制（SHA256 校验，GitHub Releases 兜底）→ SFTP 推送 → 远程安装 → 证书指纹预钉扎 → 直接 QUIC 连接（无 TOFU 弹窗）。
2. **客户端驱动的 quireld 更新**：认证响应携带版本 → 客户端检查新版本 → 下载校验 → 经 QUIC 推送 staging → 协议命令触发 Manager 备份/原子替换/热更新（不中断会话）→ 崩溃自动回滚。设置页版本管理 UI，默认仅手动。
3. **发布链路**：本地脚本构建 linux amd64/arm64 + SHA256SUMS → 生成官网静态目录（`publish-quireld.mjs`，rsync 到 Nginx）→ 可选上传 GitHub Releases 作兜底。

## 已确认决策

- 分支：**单分支 `feature/agent-provisioning`**（从 dev 拉出），三阶段独立提交、独立 verify:full
- 安装主路径：直接做 SSH 全自动；curl|bash 命令保留为降级路径（仅密钥+sudo 需密码场景）
- 分发渠道：GitHub Releases（origin = github.com/Victor-C0905/Remote_Desktop）
- 上传执行：AI 负责构建与 dry-run，`npm run publish:quireld` 真实上传由用户执行
- 工作区存在无关未跟踪文件 `docs/testing/virtual-display-plan-2026-11-16.md`，不动它

## 四个关键问题的设计回应

### Q1 如何确认是否需要安装/更新 quireld

版本来源链（三层，编译期注入不可伪造）：
1. **Manager 进程版本**：认证响应 `version` 字段 = `env!("CARGO_PKG_VERSION")`（agent/Cargo.toml 编译期注入）
2. **双版本完整性**：`agent_update_status` 返回 manager_version + worker_version + `manager_lags_worker` 布尔，暴露"Worker 新/Manager 旧"半更新状态
3. **最新版本**：官网 `latest.json` 优先，GitHub Releases latest API 兜底，与当前版本比对 → UI 提示

### Q2 如何判定目标服务器是否已安装 quireld（安装前三态判定）

DetectEnv 探测命令模板扩展（客户端固定，远端不接任意参数）：

```
uname -m; id -u; id -un; systemctl is-system-running 2>&1;
sudo -n true 2>&1;
command -v quireld 2>/dev/null;
systemctl list-unit-files quireld.service --no-legend 2>/dev/null;
/usr/local/bin/quireld --version 2>/dev/null;
cat /etc/quireld/quireld.toml 2>/dev/null
```

判定三态：
- **NotInstalled**：二进制与服务单元均不存在 → 全新安装
- **Installed{version, quic_port, cert_path}**：读已装配置 → 向导展示"检测到已安装 quireld vX.Y.Z"：
  - 版本 ≥ 客户端要求 → **跳过安装**，复用已装端口/证书直接配置连接
  - 版本旧 → 升级式安装：仅覆盖二进制，**保留已有配置与证书**（不重新生成）
  - 端口冲突：已装 quic_port 与向导所选不一致 → 提示并默认复用已装端口
- **Broken**：服务注册但二进制损坏/版本探测失败 → 重装

检测结果先呈现给用户确认，再执行后续步骤。

### Q3 SSH 登录凭据复用于 quirel 登录

天然同源，零额外配置：quireld 认证就是 PAM（pam_service=sshd）+ authorized_keys，与 SSH 同一套系统用户凭据。
- SSH 密码登录 → 同密码走 quireld AuthPasswordRequest，PAM 验证通过
- SSH 公钥登录 → 公钥已在 authorized_keys（能 SSH 登录即已授权），quireld AuthPubKeyRequest 直接通过
- 实施落点：provision_server 完成后，前端将**同一套 auth 凭据**（用户名/密码/私钥/passphrase，已在 keyring）保存为 ServerConfig.auth → remote_connect 直接认证，用户无需二次输入；向导明示"安装后将以 {username} 身份登录 quireld，凭据与 SSH 相同"

### Q4 非 root 用户安装的权限边界（不会混乱）

quireld 权限模型与 SSH daemon 一致：**服务以 root 运行 ≠ 会话获得 root 权限**。
- 机制保障（已有代码）：agent/src/auth/executor.rs `execute_as_user` 通过 fork + setgid/setuid，所有文件操作在认证用户 uid/gid 上下文降权执行；UserSession 携带 uid/gid/home/shell（auth/mod.rs）
- 非 root 用户（sudo 装 quireld）通过客户端登录后，权限边界与其 SSH 登录**完全一致**：只能访问自己有权限的文件，系统目录被 OS 权限拒绝
- sudo 只在安装/重装时一次性使用；**日常更新走 QUIC 通道，由 root Manager 自行替换二进制，不需要 SSH/sudo**
- 实施补充：验证降权路径覆盖全部文件操作入口并补集成测试；向导 UI 明示"所有操作将以 {username} 身份执行，权限与 SSH 登录一致"

## 阶段一：发布链路（本地脚本 + gh CLI）

| 文件 | 动作 | 内容 |
|---|---|---|
| `agent/build.sh` | 修改 | 新增 ARCH 参数（amd64 默认/arm64，arm64 走 aarch64-unknown-linux-gnu）；包名 `quireld-{ver}-{env}-linux-{amd64\|arm64}.tar.gz`；打包后生成 `dist/SHA256SUMS` |
| `scripts/publish-quireld.mjs` | 新建 | 读 agent/Cargo.toml 版本 → 校验 tag 已存在 → `gh release create/upload` 上传 tarball + SHA256SUMS + install-quireld.sh；支持 `--dry-run`；零版本参数（版本由 `npm run release` 锁定） |
| `package.json` | 修改 | scripts 增加 `publish:quireld` |
| `agent/deploy/install.sh` | 修改 | 参数化（见下方"安装脚本策略"） |
| `agent/deploy/install-quireld.sh` | 新建 | curl\|bash 降级引导脚本（发布资产） |

架构命名映射统一收口常量表：`x86_64 → amd64`、`aarch64 → arm64`（uname -m → 包名后缀），阶段二/三复用。

## 阶段二：客户端内置 SSH 全自动安装

### 依赖（src-tauri/Cargo.toml）

```toml
russh = "0.49"          # 自带加密栈，不依赖 rustls，无双编译
russh-sftp = "2"        # 与 russh 0.49 配对
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls-ring", "json", "stream"] }
tar = "0.4"; flate2 = "1.0"; zeroize = "1"
```

加依赖后必跑 `cargo tree -d`（查双份编译）与 `cargo audit`。

### 新建 `src-tauri/src/ssh.rs`（russh 封装）

- 类型：`SshTarget` / `SshAuthData`（Password/Key）/ `SudoMode`（None/Passwordless/Password(Zeroizing<String>)/Unsupported）/ `ExecResult`
- API：`connect`（返回会话句柄 + 观测到的 host key 指纹，TOFU 比对不在 Handler 内决断，交 provision 层处理）/ `exec`（sudo 模板 `sudo -S -p '' -k bash -c <cmd>`，密码写 stdin 后立即关闭）/ `upload_file`（SFTP）/ `download_file`
- 错误类型 `SshError` 用 thiserror；密码用 `Zeroizing<String>`，任何日志/Display/argv 不得含密码

### 新建 `src-tauri/src/provision.rs`（安装编排）

进度事件 `app.emit("provision-progress", ProvisionProgress{ server_id, step, detail, percent })`，步骤枚举：`DetectEnv → Download → Verify → Upload → Install → ReadCert → Connect`。

返回 `ProvisionOutcome`：`Installed { fingerprint, host_key_fp, quireld_port }` 或 `Degraded { install_command, reason }`。

命令拆分两个（向导状态机更清晰）：`provision_detect`（SSH 探测 → 返回 `DetectResult { install_state: not_installed|installed{version,port,cert_path}|broken, sudo_mode, arch }`，前端展示判定结果，用户点"开始安装"确认）与 `provision_server`（执行安装编排，按 detect 结果走对应分支）。

1. **DetectEnv（三态判定 + sudo 归类）**：探测命令见「Q2」，同时归出 sudo 四模式（root 直登/免密/密码复用走 sudo -S/仅密钥+sudo 需密码 → 返回 Degraded）。按「Q2」三态分流：NotInstalled → 全新安装；Installed 版本够新 → 跳到 ReadCert/Connect（跳过下载与安装）；Installed 版本旧 → 升级式安装（保留配置证书）；Broken → 重装。判定结果 emit 给前端呈现确认后再继续
2. **Download**：官网静态源优先（`{base}/{version}/`，主域 `quireld.cn` → 备用域 `quireld.icu`，可用 `QUIREL_RELEASE_BASE` 覆盖为逗号/空格分隔的多个源）→ 均不可达时回落 GitHub tag（版本钉扎）→ 匹配资产 tarball + SHA256SUMS；传输层失败重试一次（完整性错误不重试）；校验成功的包按版本本地缓存（旁挂哈希复验，旧版本自动清理）；所有源都不可用时报专用错误码
3. **Verify**：严格解析 SHA256SUMS（`^([0-9a-f]{64})\s{2}(.+)$`）→ 校验 → 本地解压（条目校验：拒绝绝对路径/../非常规文件，大小上限，剥离 setuid/setgid）→ 计算二进制 sha256
4. **Upload**：SFTP 推 quireld/install.sh/quireld.toml/systemd/quireld.service 到远端 `$HOME/.quirel-provision/`（家目录无需 sudo；二进制 0755）
5. **Install**：`sudo bash <stagedir>/install.sh --yes --port {quireld_port} --stagedir <stagedir> --binary-hash {sha256}`；升级式安装时追加 `--upgrade`（install.sh 只替换二进制，不动 /etc 配置与证书）；解析末行 `QUIRELD_INSTALL_OK port=…`
6. **ReadCert**：读 /etc/quireld/quireld.toml 的 cert_path（缺省回退 /var/lib/quireld/cert.pem）→ SFTP 下载 → PEM→DER→SHA-256 小写 hex，**格式与 `PinningCertVerifier::compute_fingerprint`（connection.rs L2145）完全一致，该函数提为 pub(crate) 复用**；指纹不写日志
7. **Connect**：清理远端 stagedir → 返回 Installed；前端保存 ServerConfig（**同一套 SSH 凭据直接作为 auth 保存，见「Q3」**；quireldPort/provisionedAt/provisionMethod='auto'/sshHostKeyFingerprint/certFingerprint）后走**既有 remote_connect**（指纹预存 → 无 TOFU 弹窗），不在 provision.rs 复制连接逻辑

`errors.rs` codes 追加：`ssh_connect_failed / ssh_auth_failed / ssh_exec_failed / provision_unsupported_arch / provision_github_unavailable / provision_sha256_mismatch / provision_install_failed / provision_cert_read_failed / provision_degraded`

### 前端

| 文件 | 改动 |
|---|---|
| `src/types/server.ts` | ServerConfig 加 optional 字段：sshPort/quireldPort/provisionedAt/provisionMethod/quireldVersion/sshHostKeyFingerprint（加字段非破坏性，**不升 servers.bin schema 版本**） |
| `src/apps/Settings.tsx` | 向导加「自动安装 quireld」区块：安装方式 radio（自动/手动命令）+ SSH 端口（默认22）+ QUIC 端口（默认9443）；onAdd 分流：自动 → invoke("provision_server") + 监听进度条；Degraded → 展示 install_command + 复制按钮 + "已完成安装，立即连接" |
| `src/context/ServerManager.tsx` | provision 进度事件监听（仿 cert-trusted 监听模式）；成功/失败 pushNotification；成功后自动触发连接 |
| `src/i18n/locales/zh-CN.ts` + `en-US.ts` | 添加服务器命名空间追加词条，双语言同步 |
| 图标 | C+B 双档主题化图标体系，禁 lucide |

### lib.rs

`pub mod ssh; pub mod provision;` + invoke_handler 追加 `provision_server`。

## 阶段三：客户端驱动的 quireld 更新

### 协议扩展（quirel-protocol/src/，先跑 wire_compat 红测试；全部为新增变体 + serde default 可选字段 → 兼容性变更，无需升次版本；实施时加载 quirel-protocol-change skill）

- `AuthResponse` 加 `version: Option<String>`（agent 填 Manager `env!("CARGO_PKG_VERSION")`）
- capabilities 加 `"agent_update"`（旧 Agent 收新 payload 会断流，门控是硬前提）
- 新变体：`agent_update_status` / `agent_update_status_resp`（manager_version/worker_version/manager_lags_worker/is_reloading/staged/last_result）/ `agent_update_upload_request`（file_size/sha256/version）/ `agent_update_upload_accept`（复用 FileChunk 流式帧）/ `agent_update_upload_resp` / `agent_update_commit`（expected_sha256）/ `agent_update_rollback` / `agent_update_action_resp`
- 辅助类型 `StagedUpdate` / `AgentUpdateResult` 入 types.rs；同步 docs/IPC_PROTOCOL.md

### agent 侧

| 文件 | 改动 |
|---|---|
| `agent/src/manager/update_manager.rs` | 新建：status / receive_upload（staging `/var/lib/quireld/updates/staging/quireld.new`，逐级 create_dir + 0700/0600）/ verify_staged（sha256 比对 + ELF e_machine 校验 x86-64=0x3E aarch64=0xB7 + 大小上限 + `--version` 子进程探测）/ commit（备份 current_exe → /var/backups/quireld/ 保留3份 → rm+copy+chmod（ETXTBSY 方案）→ reload_tx.send(ReloadTrigger::ClientCommand{operator}) → 轮询 is_reloading 直至 false（超时30s）→ 确认新 Worker 存活；失败自动从备份恢复再 reload）/ rollback |
| `agent/src/manager/mod.rs` | 注册模块；Manager 持有 reload_tx clone（与 watch_sighup 共用通道） |
| `agent/src/server/quic.rs` | L1272 match 追加更新命令分支（本地处理不路由 Worker）；send_auth_response 补 version 字段 |
| `agent/src/config.rs` + quireld.toml | 新增 `[updates]` 节（enabled/staging_dir/max_binary_mb/keep_backups，全 serde default，旧配置兼容） |
| `agent/src/manager/crash_detector.rs` | commit 后观察窗口（10分钟）内重启数超 max_restarts → 自动回滚 |

worker_version 获取：优先 Worker IPC 首帧追加 version；成本高则回退 staging `--version` 探测值 + reload 后存活校验；`manager_lags_worker` 必须能暴露"Worker 新/Manager 旧"半更新状态（UI 明示需 systemctl restart，不自动执行）。

### 客户端

新建 `src-tauri/src/agent_update.rs`：复用 `remote_send`（connection.rs L838）+ 与 provision.rs 共享下载/解压/SHA256SUMS 逻辑（抽 pub fn）。三个命令：`agent_update_check` / `agent_update_apply`（commit 放宽超时60s）/ `agent_update_rollback`；进度事件 `agent-update-progress`；无 agent_update 能力返回 `update_not_supported`。errors codes 追加：`update_not_supported / update_staging_rejected / update_commit_failed / update_rollback_failed`。lib.rs 注册。

### 设置页版本管理 UI

Settings.tsx 服务器详情区「quireld 版本」卡片：当前版本（manager_lags_worker 显示警告条）、手动"检查更新"、更新（二次确认+进度+失败展示 last_result）、回滚（有备份时启用）。默认仅手动；连接后被动版本比对仅 pushNotification 提醒。i18n 双语言。

## 安装脚本策略

**agent/deploy/install.sh 改造（向后兼容）**：加 `--yes`（跳过 read -p，russh exec 无交互 stdin）、`--port N`（sed 改 quic_port）、`--stagedir DIR`（置于现有三路检测链之前）、`--binary-hash`（校验 staged 二进制）、`--upgrade`（仅替换二进制，不动 /etc 配置与证书，供升级式安装）；成功末行固定输出 `QUIRELD_INSTALL_OK port=<n>`；无参数行为不变。

**install-quireld.sh（降级引导，发布资产）**：set -euo pipefail；依赖检查；root 检查；uname -m 白名单 → latest release 下载 tarball+SHA256SUMS → sha256sum -c → 解压执行包内 install.sh --yes。无密钥材料；降级路径证书信任由首连 TOFU 弹窗兜底。

## 安全设计

1. 下载校验三重：SHA256SUMS 严格解析 → tarball 校验 → Manager staging 侧比对（端到端防替换）；签名校验列为后续增强
2. SFTP 路径固定 `$HOME/.quirel-provision/<常量文件名>`，拒绝 `..`
3. sudo 密码 `Zeroizing<String>` 内存中转、stdin 即关；不进 tracing/Display/argv
4. staging 目录 0700、文件 0600；路径默认硬编码
5. 更新命令权限门控：QUIC 层认证前提 + `enabled` 配置开关 + operator 留痕（ReloadTrigger::ClientCommand{operator}）
6. 回滚三层：commit 内自动恢复 + 崩溃观察窗口自动回滚 + 手动 rollback；备份保留 3 份

## 验证方案

| 层 | 内容 | 命令 |
|---|---|---|
| 发布 | build.sh amd64/arm64 产物存在；sha256sum -c 通过；tar tzf 确认包内容；publish dry-run 输出正确 | WSL bash agent/build.sh |
| 协议 | wire_compat.rs：新变体编码往返、旧载荷（无 version）解码 | cargo test -p quirel-protocol |
| agent | verify_staged 假 ELF fixture（错 arch/hash/大小）；备份保留数；commit/rollback 端到端 `#[ignore]`（WSL 真机演练） | npm run verify:rust |
| 客户端 Rust | SHA256SUMS 解析、arch 映射、三态判定逻辑（fixture 命令输出）、sudo 命令模板、PEM→指纹（rcgen 现场生成）；russh 真连接集成测试 `#[ignore]` + QUIREL_SSH_TEST_TARGET 门控 | npm run verify:rust |
| agent 降权 | execute_as_user 降权路径覆盖验证：非 root 会话的文件操作/命令执行以会话 uid/gid 运行（呼应 Q4 权限边界承诺） | npm run verify:rust |
| 前端 | 向导三分支状态机（自动/手动/降级）、版本卡片 UI | npm run verify:fe |
| 门禁 | 每阶段完成前 | npm run verify:full + npm run check:sensitive-logs；依赖变更后 cargo audit |

## 风险与对策

1. russh 依赖冲突（tokio/rand/ed25519-dalek）→ russh 不依赖 rustls 冲突面小；cargo tree -d 检查
2. reqwest 拉 native-tls → default-features=false + rustls-tls-ring
3. ETXTBSY → rm(unlink)+cp 方案（install.sh 已验证）
4. 半更新状态（Worker 新/Manager 旧）→ 双版本上报 + manager_lags_worker；UI 明示 systemctl restart
5. 下载源不可用 → 官网静态源与 GitHub 双源互备；两源均失败时报专用错误码
6. 架构检测错 → uname -m 白名单 + ELF e_machine 双保险
7. Defaults requiretty → 归类降级手动安装；文档注明
8. commit 超时重复触发 → is_reloading 防重入已有；超时后先查 status 再重试
9. 旧 Agent 收新 payload 断流 → capabilities 严格门控
10. 凭据链 → 参数入 Zeroizing；走既有 keyring 链路不新增落盘点

## 提交节奏

每阶段一个规范提交（type(scope): 中文简述，body 每行 ≤100 字符）：
- `feat(release): 发布链路支持多架构产物与 SHA256SUMS`
- `feat(provision): 客户端 SSH 全自动安装 quireld`
- `feat(protocol): 客户端驱动的 quireld 更新链路`

发布执行节奏：AI 构建 + dry-run → 用户执行 `npm run publish:quireld` 上传 GitHub Releases。
