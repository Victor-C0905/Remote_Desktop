# PAM 配置说明

## 无需额外配置

GNOME Remote Agent 直接使用系统已有的 PAM 配置，用户无需手动配置。

默认使用 `sshd` PAM 服务，该服务在大多数 Linux 发行版中都已预配置。

## 认证流程

1. **密码认证**：使用系统的 SSH PAM 配置进行认证
2. **公钥认证**：使用用户的 `~/.ssh/authorized_keys` 进行认证

## 如果需要自定义

如果需要自定义 PAM 配置，可以在 `agent.toml` 中修改 `pam_service` 参数：

```toml
[auth.ssh]
pam_service = "login"  # 或其他PAM服务名
```

可选的 PAM 服务：
- `sshd` - SSH认证（默认，推荐）
- `login` - 控制台登录
- `system-auth` - 通用系统认证（Red Hat系列）
- `common-auth` - 通用认证配置（Debian系列）