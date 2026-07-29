# Agent 配置文件说明

本目录包含三个环境的配置文件，用于不同场景的部署。

## 配置文件

### 1. `agent.test.toml` - 测试环境

**用途：** 本地开发、单元测试、功能验证

**特点：**
- 仅监听本地地址（127.0.0.1）
- 宽松的安全限制（允许访问 `/tmp`）
- 不阻止危险命令（仅测试用）
- 详细日志（debug 级别）
- 短超时时间（快速清理资源）

**启动方式：**
```bash
./agent --config agent.test.toml
```

---

### 2. `agent.dev.toml` - 开发环境

**用途：** 开发调试、集成测试、预发布验证

**特点：**
- 监听所有网络接口（0.0.0.0）
- 中等安全限制
- 阻止常见危险命令
- 详细日志（debug 级别）
- 中等超时时间（3分钟）

**启动方式：**
```bash
./agent --config agent.dev.toml
```

---

### 3. `agent.prod.toml` - 生产环境

**用途：** 生产部署、正式环境

**特点：**
- 监听所有网络接口（0.0.0.0）
- **权限控制完全依赖 Linux 文件系统权限（已重构）**
- 生产级别的证书路径（`/etc/gnome-remote/`）
- 信息级别日志（info）
- 长超时时间（5分钟）
- 完整审计日志

**启动方式：**
```bash
./agent --config agent.prod.toml
```

**生产部署建议：**
1. 使用 systemd 管理 Agent 服务
2. 日志文件使用 `logrotate` 自动轮转
3. 定期审计 `/var/log/gnome-remote/audit.log`
4. 根据实际需求调整 `blocked_commands`

---

## 配置差异对比

| 配置项 | 测试环境 | 开发环境 | 生产环境 |
|--------|---------|---------|---------|
| **绑定地址** | 127.0.0.1 | 0.0.0.0 | 0.0.0.0 |
| **QUIC 端口** | 8443 | 8443 | 9443 |
| **WS 端口** | 8444 | 8444 | 9444 |
| **证书路径** | ./cert.pem | ./cert.pem | /etc/gnome-remote/ |
| **日志级别** | debug | debug | info |
| **连接超时** | 60s | 180s | 300s |
| **最大终端会话** | 5 | 10 | 20 |
| **文件传输限制** | 100MB | 200MB | 500MB |
| **安全限制** | 宽松 | 中等 | 严格 |

> **证书路径说明**：生产环境证书路径默认使用 `./cert.pem`，实际部署时建议使用 `/etc/gnome-remote/`

---

## 环境变量覆盖

可以通过环境变量覆盖配置文件的值：

```bash
# 设置日志级别
export RUST_LOG=debug

# 启动 Agent（RUST_LOG 会覆盖配置文件中的日志级别）
RUST_LOG=trace ./agent --config agent.prod.toml
```

---

## 快速切换配置

创建软链接方便切换：

```bash
# 切换到测试环境
ln -sf agent.test.toml agent.toml

# 切换到开发环境
ln -sf agent.dev.toml agent.toml

# 切换到生产环境
ln -sf agent.prod.toml agent.toml
```

然后直接运行：

```bash
./agent  # 自动使用 agent.toml
```

---

## 安全建议

1. **生产环境必须：**
   - 使用防火墙限制访问端口
   - 定期更新证书（`cert.pem` 和 `key.pem`）
   - 监控审计日志异常行为
   - 使用非 root 用户运行 Agent

2. **证书管理：**
   - 测试/开发环境可使用自签名证书
   - 生产环境建议使用正规 CA 签发的证书

3. **密码认证：**
   - 生产环境可考虑禁用密码认证（`enable_password = false`）
   - 仅允许公钥认证以提高安全性

---

## 故障排查

### 查看日志

```bash
# 实时查看日志
tail -f logs/agent.log

# 查看最近100行
tail -n 100 logs/agent.log

# 搜索错误
grep -i error logs/agent.log
```

### 检查端口占用

```bash
# 检查 QUIC 端口
netstat -tuln | grep 8443

# 检查 WebSocket 端口
netstat -tuln | grep 8444
```

### 测试连接

```bash
# 使用 openssl 测试证书
openssl s_client -connect localhost:8443
```

---

## 更新配置

修改配置文件后需要重启 Agent：

```bash
# 查找进程
ps aux | grep agent

# 发送 SIGTERM 信号优雅关闭
kill -TERM <PID>

# 或者直接重启
systemctl restart gnome-remote-agent  # 如果使用 systemd
```