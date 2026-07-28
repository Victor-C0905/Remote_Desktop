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
cd /path/to/gnome-remote/agent

# 打包生产版本
bash build.sh release

# 输出: dist/gnome-remote-agent-{version}-release-linux-amd64.tar.gz
```

### 2. 上传到服务器
```bash
scp dist/gnome-remote-agent-*.tar.gz user@server:/tmp/
```

### 3. 安装
```bash
ssh user@server
cd /tmp
tar xzf gnome-remote-agent-*.tar.gz
cd gnome-remote-agent-*
sudo bash install.sh
```

**安装后效果：**
- ✅ 程序路径：`/usr/local/bin/gnome-remote-agent`
- ✅ 配置路径：`/etc/gnome-remote-agent/agent.toml`
- ✅ 证书路径：`/etc/gnome-remote-agent/cert.pem`（首次启动自动生成）
- ✅ 日志路径：`/var/log/gnome-remote/`
- ✅ 服务名称：`gnome-remote-agent`
- ✅ 开机自启：已启用

## 服务管理

```bash
# 查看状态
systemctl status gnome-remote-agent

# 启动/停止/重启
systemctl start gnome-remote-agent
systemctl stop gnome-remote-agent
systemctl restart gnome-remote-agent

# 查看日志
journalctl -u gnome-remote-agent -f
```

## 更新流程

### 快速更新
```bash
# 1. 打包新版本
bash build.sh release

# 2. 上传到服务器
scp dist/gnome-remote-agent-*.tar.gz user@server:/tmp/

# 3. 执行更新（自动备份+替换+重启）
ssh user@server
cd /tmp
tar xzf gnome-remote-agent-*.tar.gz
cd gnome-remote-agent-*
sudo bash update.sh
```

### 更新脚本特性
- ✅ 自动备份旧版本（`/var/backups/gnome-remote-agent/`）
- ✅ 自动停止和启动服务
- ✅ 失败时自动回滚
- ✅ 保留最近3个备份
- ✅ 验证服务状态

### 手动回滚
```bash
# 查看备份列表
ls -l /var/backups/gnome-remote-agent/

# 回滚到指定版本
sudo systemctl stop gnome-remote-agent
sudo cp /var/backups/gnome-remote-agent/gnome-remote-agent_* /usr/local/bin/gnome-remote-agent
sudo systemctl start gnome-remote-agent
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
| 程序 | `/usr/local/bin/gnome-remote-agent` | 可全局执行 |
| 配置 | `/etc/gnome-remote-agent/agent.toml` | 标准配置目录 |
| 证书 | `/etc/gnome-remote-agent/cert.pem` | TLS 证书（自动生成） |
| 私钥 | `/etc/gnome-remote-agent/key.pem` | TLS 私钥（自动生成） |
| 日志 | `/var/log/gnome-remote/` | 标准日志目录 |
| 服务 | `/etc/systemd/system/gnome-remote-agent.service` | systemd 服务 |
| 备份 | `/var/backups/gnome-remote-agent/` | 版本备份 |

## 常见问题

**Q: 如何修改配置？**
```bash
sudo vim /etc/gnome-remote-agent/agent.toml
sudo systemctl restart gnome-remote-agent
```

**Q: 如何查看日志？**
```bash
# 实时查看
sudo journalctl -u gnome-remote-agent -f

# 查看最近100行
sudo journalctl -u gnome-remote-agent -n 100

# 查看今天的日志
sudo journalctl -u gnome-remote-agent --since today
```

**Q: 如何查看历史版本？**
```bash
# 查看所有备份
ls -l /var/backups/gnome-remote-agent/
```
