# PAM 配置说明

本目录包含 GNOME Remote Agent 的 PAM（Pluggable Authentication Modules）配置文件。

## 文件说明

- `gnome-remote` - Agent 的 PAM 配置文件

## 安装

### 手动安装

```bash
# 复制配置文件到系统目录
sudo cp pam.d/gnome-remote /etc/pam.d/

# 验证权限
ls -l /etc/pam.d/gnome-remote
# 应该显示: -rw-r--r-- 1 root root ... /etc/pam.d/gnome-remote
```

### 通过安装脚本

```bash
# 使用项目根目录的安装脚本
sudo ./install.sh
```

## 配置说明

### 认证模块（auth）

```pam
auth        required      pam_env.so
auth        required      pam_unix.so nullok try_first_pass
auth        requisite     pam_succeed_if.so uid >= 1000 quiet_success
auth        required      pam_deny.so
```

- `pam_env.so` - 设置环境变量
- `pam_unix.so` - 标准 Unix 密码认证（允许空密码）
- `pam_succeed_if.so` - 限制 UID >= 1000（普通用户）
- `pam_deny.so` - 默认拒绝策略

### 账户模块（account）

```pam
account     required      pam_unix.so
account     sufficient    pam_localuser.so
account     sufficient    pam_succeed_if.so uid < 1000 quiet
account     required      pam_permit.so
```

- `pam_unix.so` - 验证账户有效性
- `pam_localuser.so` - 允许本地用户
- `pam_succeed_if.so` - 允许系统用户（UID < 1000）
- `pam_permit.so` - 默认允许策略

### 密码模块（password）

```pam
password    requisite     pam_pwquality.so try_first_pass local_users_only retry=3 authtok_type=
password    sufficient    pam_unix.so sha512 shadow nullok try_first_pass use_authtok
password    required      pam_deny.so
```

- `pam_pwquality.so` - 密码质量检查（重试 3 次）
- `pam_unix.so` - 密码更新（使用 SHA512）

### 会话模块（session）

```pam
session     optional      pam_keyinit.so revoke
session     required      pam_limits.so
-session    optional      pam_systemd.so
session     required      pam_unix.so
```

- `pam_keyinit.so` - 会话密钥管理
- `pam_limits.so` - 资源限制
- `pam_systemd.so` - systemd 会话集成
- `pam_unix.so` - 会话日志

## 安全特性

### 用户限制

- **UID >= 1000** - 仅允许普通用户认证
- **本地用户** - 仅允许本地账户
- **系统用户** - 允许系统服务（UID < 1000）

### 密码策略

- **密码质量** - 强制密码复杂度检查
- **SHA512** - 使用安全的哈希算法
- **重试限制** - 认证失败重试 3 次

### 会话管理

- **资源限制** - 通过 limits.conf 控制资源
- **会话日志** - 记录会话创建和关闭
- **密钥撤销** - 会话结束时撤销密钥

## 测试 PAM 配置

### 使用 pamtester

```bash
# 安装 pamtester
sudo apt install libpam-dotfile  # Debian/Ubuntu
sudo yum install pamtester       # RHEL/CentOS

# 测试认证
pamtester gnome-remote <username> authenticate
```

### 手动测试

```bash
# 启动 Agent
sudo ./agent --config agent.toml

# 使用客户端连接
gnome-remote-client connect --host localhost --port 8443 --user <username>
```

## 自定义配置

### 允许 root 用户

编辑 `/etc/pam.d/gnome-remote`，修改以下行：

```pam
# 移除 UID 限制
# auth        requisite     pam_succeed_if.so uid >= 1000 quiet_success
```

### 添加二次认证

```pam
# 在 pam_unix.so 之后添加
auth        required      pam_google_authenticator.so
```

### 限制登录时间

```pam
# 添加时间限制
account     required      pam_time.so
```

## 故障排查

### 查看认证日志

```bash
# 实时查看认证日志
sudo tail -f /var/log/auth.log        # Debian/Ubuntu
sudo tail -f /var/log/secure          # RHEL/CentOS

# 或使用 journalctl
sudo journalctl -u gnome-remote-agent -f
```

### 常见错误

#### 认证失败 - 用户不在允许范围

```
pam_succeed_if(gnome-remote:auth): requirement "uid >= 1000" not met by user "root"
```

**解决方案**：使用普通用户或修改 PAM 配置。

#### 认证失败 - 密码错误

```
pam_unix(gnome-remote:auth): authentication failure; logname=...
```

**解决方案**：检查用户密码或使用正确的认证方式。

#### PAM 配置文件不存在

```
PAM unable to dlopen(/lib/security/pam_xxx.so)
```

**解决方案**：确保 PAM 配置文件已正确安装。

## 相关文档

- [Agent 部署指南](../agent/README.md)
- [认证设计文档](../docs/superpowers/specs/2026-07-24-ssh-compatible-authentication-design.md)
- [Linux PAM 官方文档](http://www.linux-pam.org/Linux-PAM-html/)

## 参考

- `/etc/pam.d/sshd` - OpenSSH 的 PAM 配置参考
- `/etc/pam.d/login` - 系统登录的 PAM 配置参考
- `man pam.conf` - PAM 配置手册