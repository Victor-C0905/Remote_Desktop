# Agent 部署说明

## 核心思想

将程序安装到系统 PATH，配置文件放在标准位置，使用 systemd 实现开机自启，达到像系统自带的 SSH 工具一样的效果。

## 快速部署

### 1. 编译程序
```bash
cd /path/to/gnome-remote/agent
cargo build --release
```

### 2. 上传到服务器
```bash
scp -r agent/ user@server:/tmp/
```

### 3. 执行安装
```bash
ssh user@server
cd /tmp/agent/deploy
sudo bash install.sh
```

**安装后效果：**
- ✅ 程序路径：`/usr/local/bin/gnome-remote-agent`
- ✅ 配置路径：`/etc/gnome-remote-agent/config.toml`
- ✅ 日志路径：`/var/log/gnome-remote-agent/`
- ✅ 服务名称：`gnome-remote-agent`
- ✅ 开机自启：已启用
- ✅ 系统命令：`systemctl status gnome-remote-agent`

## 自定义安装

```bash
# 自定义服务名称和端口
sudo bash install.sh --name my-remote --port 9443

# 完整自定义
sudo bash install.sh --dir /usr/local/bin --name my-remote --port 9443
```

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
# 1. 编译新版本
cd /path/to/gnome-remote/agent
cargo build --release

# 2. 上传到服务器
scp -r agent/ user@server:/tmp/

# 3. 执行更新（自动备份+替换+重启）
ssh user@server
cd /tmp/agent/deploy
sudo bash update.sh
```

### 更新脚本特性
- ✅ 自动备份旧版本（`/var/backups/gnome-remote-agent/`）
- ✅ 自动停止和启动服务
- ✅ 失败时自动回滚
- ✅ 保留最近5个备份
- ✅ 验证服务状态

### 手动回滚
```bash
# 查看备份列表
ls -l /var/backups/gnome-remote-agent/

# 回滚到指定版本
sudo systemctl stop gnome-remote-agent
sudo cp /var/backups/gnome-remote-agent/gnome-remote-agent_20260128_143522 /usr/local/bin/gnome-remote-agent
sudo systemctl start gnome-remote-agent
```

## 卸载

```bash
sudo bash uninstall.sh
```

## 客户端连接

```
地址：<服务器IP>:8443
用户：root
密码：系统密码
```

## 技术原理

### 1. 系统集成
- 安装到 `/usr/local/bin`（系统 PATH），可直接执行命令
- 配置文件放在 `/etc/`（标准配置目录）
- 日志文件放在 `/var/log/`（标准日志目录）

### 2. 自动启动
- 使用 systemd 管理服务
- 配置 `After=network.target` 确保网络就绪后启动
- 配置 `WantedBy=multi-user.target` 实现开机自启
- 配置 `Restart=on-failure` 自动重启

### 3. 标准化路径

| 内容 | 路径 | 说明 |
|------|------|------|
| 程序 | `/usr/local/bin/gnome-remote-agent` | 可全局执行 |
| 配置 | `/etc/gnome-remote-agent/config.toml` | 标准配置目录 |
| 日志 | `/var/log/gnome-remote-agent/` | 标准日志目录 |
| 服务 | `/etc/systemd/system/gnome-remote-agent.service` | systemd 服务 |

## 部署脚本特点

1. **无依赖安装**：不需要安装额外软件包
2. **交互式确认**：安装前显示配置，避免误操作
3. **自动检测**：自动检查二进制文件是否存在
4. **动态生成**：根据参数动态生成 systemd 服务文件
5. **错误处理**：服务启动失败时显示详细日志

## 常见问题

**Q: 如何更新版本？**
```bash
# 方法一：使用更新脚本（推荐）
sudo bash update.sh

# 方法二：手动更新
sudo systemctl stop gnome-remote-agent
sudo cp target/release/agent /usr/local/bin/gnome-remote-agent
sudo systemctl start gnome-remote-agent
```

**Q: 更新失败怎么办？**
```bash
# 更新脚本会自动回滚
# 如果手动更新失败，可以恢复备份
sudo systemctl stop gnome-remote-agent
sudo cp /var/backups/gnome-remote-agent/gnome-remote-agent_* /usr/local/bin/gnome-remote-agent
sudo systemctl start gnome-remote-agent
```

**Q: 如何查看历史版本？**
```bash
# 查看所有备份
ls -l /var/backups/gnome-remote-agent/

# 查看备份时间
ls -lt /var/backups/gnome-remote-agent/
```

**Q: 如何修改配置？**
```bash
sudo vim /etc/gnome-remote-agent/config.toml
sudo systemctl restart gnome-remote-agent
```

**Q: 如何查看历史版本？**
```bash
# 查看所有备份
ls -l /var/backups/gnome-remote-agent/

# 查看备份时间
ls -lt /var/backups/gnome-remote-agent/
```

**Q: 如何修改配置？**
```bash
sudo vim /etc/gnome-remote-agent/config.toml
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