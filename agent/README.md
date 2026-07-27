# GNOME Remote Agent

远程桌面控制 Agent 端，支持 SSH 兼容的多用户认证系统。

## 功能特性

- ✅ **SSH 公钥认证** - 兼容 OpenSSH authorized_keys 格式
- ✅ **PAM 密码认证** - 支持系统用户密码认证
- ✅ **组合认证** - 同时支持公钥和密码认证
- ✅ **用户隔离** - 基于 Linux Namespace 的进程隔离
- ✅ **安全审计** - 完整的认证日志记录

## 快速开始

### 1. 构建

```bash
# 开发环境构建
cargo build

# 生产环境构建（优化体积）
cargo build --release
```

### 2. 安装 PAM 配置

```bash
# 复制 PAM 配置文件
sudo cp pam.d/gnome-remote /etc/pam.d/

# 验证配置
cat /etc/pam.d/gnome-remote
```

### 3. 创建配置文件

```bash
# 复制示例配置
cp agent.toml ~/.config/gnome-remote/

# 编辑配置
vim ~/.config/gnome-remote/agent.toml
```

### 4. 启动 Agent

```bash
# 直接启动
sudo ./target/release/agent --config ~/.config/gnome-remote/agent.toml

# 或使用 systemd 服务
sudo cp systemd/gnome-remote-agent.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl start gnome-remote-agent
```

## 配置说明

### agent.toml

```toml
[server]
# 监听地址
listen = "0.0.0.0:8443"

# 认证配置
[auth]
# 启用公钥认证
enable_pubkey = true

# 启用密码认证
enable_password = true

# PAM 服务名称
pam_service = "gnome-remote"

# 日志配置
[log]
level = "info"
```

## 认证方式

### SSH 公钥认证

1. 客户端提供公钥
2. Agent 验证 `~/.ssh/authorized_keys`
3. 返回用户身份信息

### PAM 密码认证

1. 客户端提供用户名和密码
2. Agent 通过 PAM 验证
3. 返回用户身份信息

### 组合认证

默认同时启用公钥和密码认证，客户端可选择任一方式。

## 测试

### 单元测试

```bash
# 运行所有单元测试
cargo test

# 运行特定测试
cargo test --test auth_test
```

### 集成测试

集成测试需要真实 Linux 环境：

```bash
# 运行所有测试（包括被忽略的）
cargo test -- --ignored

# 仅运行集成测试
cargo test --test auth_integration_test -- --ignored
```

## 部署检查清单

- [ ] 安装 PAM 配置到 `/etc/pam.d/gnome-remote`
- [ ] 创建配置文件 `agent.toml`
- [ ] 配置日志目录和权限
- [ ] 创建 systemd 服务（可选）
- [ ] 测试认证功能
- [ ] 配置防火墙规则（开放 8443 端口）

## 安全建议

1. **使用 systemd 服务** - 推荐 systemd 管理 Agent 进程
2. **限制用户范围** - PAM 配置默认限制 UID >= 1000
3. **启用审计日志** - 记录所有认证请求
4. **定期更新密钥** - 定期轮换 SSH 密钥
5. **网络隔离** - 建议使用 VPN 或内网访问

## 故障排查

### 认证失败

```bash
# 检查 PAM 配置
sudo cat /etc/pam.d/gnome-remote

# 查看 Agent 日志
journalctl -u gnome-remote-agent -f

# 测试 PAM 认证（需要安装 expect）
pamtester gnome-remote <username> authenticate
```

### 权限问题

```bash
# Agent 需要 root 权限
sudo ./agent --config agent.toml

# 或使用 sudo 运行
sudo systemctl start gnome-remote-agent
```

## 相关文档

- [PAM 配置说明](../pam.d/README.md)
- [认证设计文档](../docs/superpowers/specs/2026-07-24-ssh-compatible-authentication-design.md)
- [实现计划](../docs/superpowers/plans/2026-07-24-ssh-compatible-authentication-implementation.md)

## 支持的密钥格式

### 客户端支持

- ✅ **OpenSSH 格式**（推荐）
  - 现代Linux/Mac默认格式
  - 通常位于 `~/.ssh/id_ed25519` 或 `~/.ssh/id_rsa`

- ✅ **PEM 格式**
  - PKCS#1 RSA：阿里云、AWS等云服务商提供的密钥
  - PKCS#8：通用PEM格式

### 不支持的格式

- ❌ **PuTTY 格式（.ppk）**
  - Windows用户常用格式
  - 需要先转换为 OpenSSH 格式
  - 转换方法：使用 PuTTYgen 导出为 OpenSSH 格式

### 加密私钥处理

如果您的私钥有密码保护：

1. 在"私钥密码"字段输入密码
2. 如果是PEM格式加密私钥，建议先解密：
   ```bash
   ssh-keygen -p -f <私钥文件>
   ```

### 常见问题

**Q: 提示"无法解析私钥文件"？**
A: 检查以下几点：
- 私钥文件是否完整（包含 -----BEGIN 和 -----END）
- 是否是PuTTY格式（.ppk）- 需要转换
- 是否有密码保护 - 需要输入密码

**Q: 如何转换PuTTY格式？**
A: 使用PuTTYgen：
1. 打开PuTTYgen
2. 加载.ppk文件
3. 点击"Conversions" -> "Export OpenSSH key"
4. 保存新文件