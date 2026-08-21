# Agent 部署说明

## 核心思想

将程序安装到系统 PATH，配置文件放在标准位置，使用 systemd 实现开机自启，达到像系统自带的 SSH 工具一样的效果。

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
