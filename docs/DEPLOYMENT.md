# Agent 部署说明

## 核心思想

将程序安装到系统 PATH，配置文件放在标准位置，使用 systemd 实现开机自启，达到像系统自带的 SSH 工具一样的效果。

## 下载源与发布（官网为主，GitHub 兜底）

客户端自动安装、在线更新与 `install-quireld.sh` 都按以下顺序取包：

1. 官网静态源主域：`https://quireld.cn/downloads/quireld`
2. 官网静态源备用域：`https://quireld.icu/downloads/quireld`（同一 quirel_web 应用，产物只存一份）
3. GitHub Releases（tag 与客户端版本一致）

两个官网域名都不可达/404 时才回落 GitHub；任一源返回的 SHA-256 不匹配立即中断（不换源）。
环境变量 `QUIREL_RELEASE_BASE` 可覆盖官网源：支持逗号或空格分隔多个源，置空则禁用官网源（只走 GitHub）。

静态目录布局（`npm run publish:quireld` 生成 `agent/dist/publish/`）：

```
{base}/latest.json                                   # {"version":"x.y.z"}，更新检查用
{base}/install-quireld.sh                            # curl|bash 引导脚本
{base}/SHA256SUMS                                    # 稳定别名 + 版本名清单
{base}/quireld-linux-{arch}.tar.gz                   # 稳定别名（引导脚本用）
{base}/{version}/quireld-{version}-release-linux-{arch}.tar.gz
{base}/{version}/SHA256SUMS                          # 客户端按版本名精确匹配
```

发布步骤（版本由 `npm run release` 锁定，tag 必须已存在）：

```bash
# WSL 内构建双架构产物
bash agent/build.sh release amd64
bash agent/build.sh release arm64

# 生成官网静态目录（默认不联网）
npm run publish:quireld

# 上传到官网（quirel_web 的 /downloads/** → data/downloads/**）——顺序敏感：
# 1) 先版本目录（客户端按版本精确取包）
rsync -av agent/dist/publish/<version>/ <user>@<官网服务器>:/opt/quirel-web/data/downloads/quireld/<version>/
# 2) 再根文件（稳定别名 / 引导脚本 / 根清单）
rsync -av --exclude '<version>' --exclude 'latest.json' agent/dist/publish/ <user>@<官网服务器>:/opt/quirel-web/data/downloads/quireld/
# 3) latest.json 最后（更新检查开关，避免客户端在产物就绪前发现新版本）
rsync -av agent/dist/publish/latest.json <user>@<官网服务器>:/opt/quirel-web/data/downloads/quireld/latest.json

# 可选：同时上传 GitHub Releases 作为兜底
npm run publish:quireld -- --github
```

校验行为：主域/备用域 404/不可达时按序换源，两个官网域名都失败才回落 GitHub；任一来源返回的 SHA-256 不匹配都会立即中断，不回落（避免掩盖源站投毒）。

下载鲁棒性：HTTP 客户端带连接超时（10s）与逐块读取空闲超时（30s）；传输层失败自动重试一次（完整性错误绝不重试）；官网主域 → 备用域 → GitHub 逐级回落；校验成功的包按版本缓存在客户端本地（app cache 目录 `releases/{version}/`），命中时按旁挂哈希复验后免下载，旧版本缓存自动清理。

官网侧说明：`quireld.cn` / `quireld.icu` 由隔壁 `quirel_web`（Spring Boot）提供，`/downloads/**` 直接映射服务器上的 `data/downloads/**`，因此本目录产物上传到 `/opt/quirel-web/data/downloads/quireld/` 即可被两个域名同时访问。旧版本目录必须保留（老客户端按自身版本号取包）；两个域名的 Nginx/证书配置见 `quirel_web/deploy/nginx-quireld-domains.conf`。

## 环境配置

| 环境 | 命令 | QUIC 端口 | WS 端口 | 配置文件 |
|------|------|-----------|---------|----------|
| 生产 | `bash build.sh release` | 9443 | 9444 | agent.prod.toml |
| 测试 | `bash build.sh test` | 8443 | 8444 | agent.test.toml |
| 开发 | `bash build.sh dev` | 8443 | 8444 | agent.dev.toml |

## 快速部署

### 1. 打包（开发机器）
```bash
cd /path/to/quirel/agent

# 打包生产版本
bash build.sh release

# 输出: dist/quireld-{version}-release-linux-amd64.tar.gz
```

### 2. 上传到服务器
```bash
scp dist/quireld-*.tar.gz user@server:/tmp/
```

### 3. 安装
```bash
ssh user@server
cd /tmp
tar xzf quireld-*.tar.gz
cd quireld-*
sudo bash install.sh
```

**安装后效果：**
- ✅ 程序路径：`/usr/local/bin/quireld`
- ✅ 配置路径：`/etc/quireld/quireld.toml`
- ✅ 证书路径：`/etc/quireld/cert.pem`（首次启动自动生成）
- ✅ 日志路径：`/var/log/quireld/`
- ✅ 服务名称：`quireld`
- ✅ 开机自启：已启用

## 安全与运维（安装脚本自动处理）

- **更新权限**：quireld 上传/提交/回滚仅允许 root 会话；普通用户只能查询版本，客户端会收到明确拒绝
- **PAM**：安装时创建 `/etc/pam.d/quireld`（`@include common-auth/account/session`），默认不复用 `sshd`，避免被 sshd 的 2FA/访问控制策略耦合。升级安装保留现有配置；如需切换，把 `/etc/quireld/quireld.toml` 的 `pam_service` 改为 `quireld`。本地开发未执行 `install.sh` 时没有该文件，需手动创建，或临时把 `pam_service` 改回 `sshd`
- **StrictModes**：家目录、`.ssh`、`authorized_keys` 属主必须是用户或 root，且不得对组/其他用户可写；不符合时公钥认证拒绝并记录原因（与 sshd 行为一致）。可用 `strict_modes = false` 关闭
- **日志轮转**：`/etc/logrotate.d/quireld`，保留 14 天（`copytruncate`）
- **防火墙**：安装脚本不自动修改，按输出提示执行 `sudo ufw allow <quic_port>/udp`（启用 WS 时再放行对应 TCP 端口）
- **目录权限**：`/var/lib/quireld` 0700、`/var/log/quireld` 0750、配置文件 0600、证书私钥 0600

## 服务管理

```bash
# 查看状态
systemctl status quireld

# 启动/停止/重启
systemctl start quireld
systemctl stop quireld
systemctl restart quireld

# 查看日志
journalctl -u quireld -f
```

## 更新流程

### 快速更新
```bash
# 1. 打包新版本
bash build.sh release

# 2. 上传到服务器
scp dist/quireld-*.tar.gz user@server:/tmp/

# 3. 执行更新（自动备份+替换+重启）
ssh user@server
cd /tmp
tar xzf quireld-*.tar.gz
cd quireld-*
sudo bash update.sh
```

### 更新脚本特性
- ✅ 自动备份旧版本（`/var/backups/quireld/`）
- ✅ 自动停止和启动服务
- ✅ 失败时自动回滚
- ✅ 保留最近3个备份
- ✅ 验证服务状态
- ✅ `install.sh --upgrade`（客户端升级式安装）同样：新版本启动失败时自动恢复最近备份并复检，仍失败则退出交人工处理

### 手动回滚
```bash
# 查看备份列表
ls -l /var/backups/quireld/

# 回滚到指定版本
sudo systemctl stop quireld
sudo cp /var/backups/quireld/quireld_* /usr/local/bin/quireld
sudo systemctl start quireld
```

## 增量部署（按改动范围选择最快路径）

全量打包（`bash build.sh release` + `update.sh`）会备份、替换二进制、重启服务，流程较重。
如果本次改动**只涉及 agent 二进制**，没有改 deploy 脚本、systemd service、配置文件、proto 等外围资源，可以跳过打包流程，直接上传二进制并重启服务。

### 判断依据

部署前先确认本次改动的范围，对照下表选择部署方式：

| 改动范围 | 部署方式 | 操作 |
|----------|----------|------|
| 仅 `agent/src/` 下 Rust 代码 | **增量部署（仅二进制）** | 编译 → 上传二进制 → systemctl restart |
| `agent/deploy/` 脚本（install.sh/update.sh/uninstall.sh） | 全量部署 | `bash build.sh release` → `update.sh` |
| `systemd/quireld.service` | 全量部署 | `bash build.sh release` → `update.sh` |
| `agent/agent.*.toml` 配置模板 | 全量部署 | `bash build.sh release` → `update.sh` |
| `protocol/agent.proto` 或 `agent/src/protocol/` | 全量部署 | `bash build.sh release` → `update.sh` |
| 前端 `src/` 或 `src-tauri/` | 与 agent 无关，单独打包客户端 | `npm run tauri build` |

### 增量部署步骤（仅二进制）

适用条件：本次提交只改了 `agent/src/` 下的 Rust 代码，没有动 deploy 脚本、systemd service、配置文件、proto。

```bash
# 1. 编译 release 二进制（开发机器，WSL 环境，跳过 build.sh 打包流程）
cd /mnt/e/MyWork/quirel/agent
cargo build --release

# 2. 直接上传二进制到服务器
scp target/release/quireld user@server:/tmp/quireld

# 3. 在服务器上替换二进制并重启
ssh user@server << 'EOF'
sudo cp /tmp/quireld /usr/local/bin/quireld
sudo chmod +x /usr/local/bin/quireld
sudo systemctl restart quireld
sudo systemctl status quireld --no-pager
EOF
```

**注意：** 增量部署不会创建版本备份。如果需要保留回滚能力，先用 `update.sh` 全量部署，或手动备份：

```bash
sudo cp /usr/local/bin/quireld /var/backups/quireld/quireld_$(date +%Y%m%d_%H%M%S)
```

## 卸载

```bash
sudo bash uninstall.sh
```

## 自定义安装

```bash
# 自定义服务名称
sudo bash install.sh --name my-remote
```

## 标准化路径

| 内容 | 路径 | 说明 |
|------|------|------|
| 程序 | `/usr/local/bin/quireld` | 可全局执行 |
| 配置 | `/etc/quireld/quireld.toml` | 标准配置目录 |
| 证书 | `/etc/quireld/cert.pem` | TLS 证书（自动生成） |
| 私钥 | `/etc/quireld/key.pem` | TLS 私钥（自动生成） |
| 日志 | `/var/log/quireld/` | 标准日志目录 |
| 服务 | `/etc/systemd/system/quireld.service` | systemd 服务 |
| 备份 | `/var/backups/quireld/` | 版本备份 |

## 常见问题

**Q: 如何修改配置？**
```bash
sudo vim /etc/quireld/quireld.toml
sudo systemctl restart quireld
```

**Q: 如何查看日志？**
```bash
# 实时查看
sudo journalctl -u quireld -f

# 查看最近100行
sudo journalctl -u quireld -n 100

# 查看今天的日志
sudo journalctl -u quireld --since today
```

**Q: 如何查看历史版本？**
```bash
# 查看所有备份
ls -l /var/backups/quireld/
```
