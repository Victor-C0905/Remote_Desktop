# 部署与更新流程设计

**创建日期**: 2026-08-08
**状态**: 设计文档（当前阶段已实现权宜方案，apt 标准发布为未来目标）
**关联文件**: [install.sh](file:///e:/MyWork/gnome-remote/agent/deploy/install.sh), [update.sh](file:///e:/MyWork/gnome-remote/agent/deploy/update.sh), [uninstall.sh](file:///e:/MyWork/gnome-remote/agent/deploy/uninstall.sh), [systemd/gnome-remote-agent.service](file:///e:/MyWork/gnome-remote/systemd/gnome-remote-agent.service)

## 1. 设计目标

### 1.1 核心原则

- **开箱即用**：Agent 二进制本身不依赖任何外部脚本即可正常运行（证书、配置、端口重试都内建）
- **环境感知**：脚本能自动检测执行环境（SSH vs agent 终端），选择安全的操作方式
- **平滑过渡**：权宜脚本与未来 apt 标准发布职责边界清晰，过渡时只需替换脚本层

### 1.2 分层架构

```
┌─────────────────────────────────────────────────┐
│  用户操作层（apt install / bash install.sh）        │
├─────────────────────────────────────────────────┤
│  脚本层（install.sh / update.sh / uninstall.sh）   │ ← 未来由 apt 接管
├─────────────────────────────────────────────────┤
│  systemd 服务层（service 文件 + systemctl）        │
├─────────────────────────────────────────────────┤
│  Agent 二进制层（Manager + Worker + 业务逻辑）     │ ← 已开箱即用
└─────────────────────────────────────────────────┘
```

## 2. 职责划分

### 2.1 Agent 二进制内建能力（不依赖脚本）

| 能力 | 实现位置 | 说明 |
|------|---------|------|
| 证书自动生成 | [cert.rs](file:///e:/MyWork/gnome-remote/agent/src/cert.rs) | 启动时自签证书，无需用户配置 |
| 配置文件自动生成 | [config.rs:230-243](file:///e:/MyWork/gnome-remote/agent/src/config.rs#L230-L243) | 首次启动生成默认配置 |
| 端口占用重试 | [quic.rs:242-295](file:///e:/MyWork/gnome-remote/agent/src/server/quic.rs#L242-L295) | SO_REUSEADDR + 5 次重试（每次 1 秒） |
| TCP SO_REUSEADDR | [websocket.rs:17-30](file:///e:/MyWork/gnome-remote/agent/src/server/websocket.rs#L17-L30) | TCP 端口快速复用 |
| IPC 就绪同步 | [mod.rs:218-229](file:///e:/MyWork/gnome-remote/agent/src/manager/mod.rs#L218-L229) | oneshot 信号确保 bind 完成后再启动 Worker |
| Worker 崩溃自动重启 | worker_manager | 崩溃检测器自动重启 |
| 热更新（SIGHUP） | hot_update_coordinator | `systemctl reload` 触发 Worker 优雅重启 |
| Worker 路径自动检测 | [config.rs:220-226](file:///e:/MyWork/gnome-remote/agent/src/config.rs#L220-L226) | 通过 `/proc/self/exe` 获取，无需配置 |

### 2.2 脚本层职责（未来由 apt 接管）

| 工作 | install.sh | update.sh | uninstall.sh | 未来 apt 机制 |
|------|-----------|-----------|--------------|--------------|
| 二进制安装到 `/usr/local/bin/` | ✓ | ✓（rm+cp） | ✓（rm） | dpkg 自动 |
| 配置文件到 `/etc/gnome-remote-agent/` | ✓ | ✓（备份+覆盖） | ✓（rm） | conffiles 机制 |
| 创建 `/var/log/gnome-remote/` | ✓ | - | ✓ | dpkg 自动 |
| 创建 `/var/lib/gnome-remote/` | ✓ | - | ✓ | dpkg 自动 |
| 注册 systemd service | ✓ | - | ✓ | systemctl daemon-reload 自动 |
| 启动/重启服务 | ✓（restart） | ✓（环境感知） | ✓（stop） | postinst/prerm 脚本 |
| 备份旧版本 | - | ✓ | - | apt 不备份（用户需手动） |
| 失败回滚 | - | ✓（自动） | - | apt 不自动回滚 |

## 3. 当前阶段：权宜脚本设计

### 3.1 install.sh（首次安装）

**使用场景**：通过 SSH 登录服务器后手动执行

**流程**：
1. 检查 root 权限
2. 检测二进制文件位置（打包部署 / 源码部署）
3. 创建目录：`/usr/local/bin`、`/etc/gnome-remote-agent`、`/var/log/gnome-remote`、`/var/lib/gnome-remote`
4. **rm + cp 替换二进制**（避免 ETXTBSY）
5. 安装配置文件（已存在则跳过）
6. 安装 systemd service 文件（支持 sed 替换路径和服务名）
7. `reset-failed` + `daemon-reload` + `enable` + `restart`

**关键设计**：
- 使用 `rm + cp` 而非直接 `cp`：`rm(unlink)` 只删除目录项，运行中进程持有的 inode 不受影响，避免 ETXTBSY 错误
- `reset-failed` 清除可能的 StartLimitBurst 触发状态
- 配置文件已存在时跳过，保护用户修改

### 3.2 update.sh（升级更新）

**使用场景**：可通过 SSH 或 agent 终端执行

**核心问题：agent 终端执行的 SIGHUP 陷阱**

```
systemctl restart → systemd stop agent → agent 退出 → PTY 关闭
→ bash 收到 SIGHUP → systemctl 进程被杀 → start 不执行 → 服务没了
```

**解决方案：环境感知**

```bash
is_under_agent() {
    local pid=$$
    while [ "$pid" != "1" ] && [ -n "$pid" ]; do
        local cmdline
        cmdline=$(cat /proc/$pid/cmdline 2>/dev/null | tr '\0' ' ')
        if echo "$cmdline" | grep -q "$SERVICE_NAME"; then
            return 0
        fi
        pid=$(awk '/^PPid:/{print $2}' /proc/$pid/status 2>/dev/null)
    done
    return 1
}
```

**两种执行路径**：

| 场景 | 检测结果 | 处理方式 |
|------|---------|---------|
| SSH 终端 | `is_under_agent` 返回 false | 直接 `systemctl restart`，等待完成并验证，失败自动回滚 |
| agent 终端 | `is_under_agent` 返回 true | `nohup` 后台执行 restart，提示用户等待 5-10 秒重连 |

**agent 终端路径的 nohup 机制**：
```bash
nohup bash -c "systemctl restart $SERVICE_NAME ..." > "$UPDATE_LOG" 2>&1 &
disown
```
- `nohup`：子进程忽略 SIGHUP
- `disown`：从 shell 作业表移除，防止 shell 退出时发送 SIGHUP
- systemd(PID 1) 完成 stop + start 全流程，不依赖发起请求的终端

**流程**：
1. 检查 root 权限 + 旧版本存在
2. 备份旧二进制到 `/var/backups/$SERVICE_NAME/`
3. rm + cp 替换二进制
4. 备份旧配置 + 覆盖新配置
5. 环境感知重启
6. SSH 路径：验证 + 失败回滚 + 清理旧备份

### 3.3 uninstall.sh（卸载）

**流程**：
1. 停止服务
2. 删除二进制、配置目录、日志目录、WorkingDirectory、备份
3. `daemon-reload`

## 4. 开发者与用户必做步骤

### 4.1 开发者必做

**一次性准备**：
- 维护 `build.sh`（打包脚本）
- 维护 `agent.prod.toml`（生产配置模板）
- 维护 `systemd/gnome-remote-agent.service`（服务模板）
- 维护 `deploy/install.sh`、`update.sh`、`uninstall.sh`（权宜脚本）

**每次发布**：
```bash
cd agent
bash build.sh release
# 生成 dist/gnome-remote-agent-{version}-release-linux-amd64.tar.gz
# 分发给用户（scp / 网盘 / 下载链接）
```

### 4.2 用户必做

**首次安装**（通过 SSH）：
```bash
scp gnome-remote-agent-*.tar.gz user@server:/tmp/
ssh user@server
cd /tmp && tar xzf gnome-remote-agent-*.tar.gz
cd gnome-remote-agent-*
sudo bash install.sh
```

**升级更新**：
```bash
# 方式 A：通过 SSH（推荐，可看到验证结果）
scp gnome-remote-agent-*.tar.gz user@server:/tmp/
ssh user@server
cd /tmp && tar xzf gnome-remote-agent-*.tar.gz
cd gnome-remote-agent-*
sudo bash update.sh

# 方式 B：通过 agent 终端（脚本自动检测，nohup 后台 restart）
# 把新包传到服务器任意目录，解压后执行
sudo bash update.sh
# 终端会断开，等待 5-10 秒后重连
```

**卸载**：
```bash
sudo bash uninstall.sh
```

## 5. 已解决的技术问题

### 5.1 ETXTBSY（文件忙）

**问题**：直接 `cp` 覆盖运行中的二进制会失败
**解决**：`rm + cp`，rm 只 unlink，运行中进程的 inode 不受影响

### 5.2 端口占用（Address already in use）

**问题**：`systemctl restart` 时旧进程端口释放有延迟
**解决**：
- 代码层：`SO_REUSEADDR` + 5 次重试（每次 1 秒）
- 不在脚本层 stop，避免 agent 终端断连

### 5.3 SIGHUP 陷阱（agent 终端执行 update）

**问题**：systemctl restart 的 stop 杀掉 agent → PTY 关闭 → bash 收到 SIGHUP → systemctl 被杀 → start 不执行
**解决**：`is_under_agent` 检测 + `nohup` 后台执行 restart

### 5.4 IPC 启动竞态

**问题**：Manager 用 `sleep(100ms)` 等 IPC server bind，不可靠
**解决**：`oneshot` 信号确保 bind + subscribe 完成后再启动 Worker

### 5.5 StartLimitBurst 限制

**问题**：systemd 60 秒内重启超过 3 次后拒绝启动
**解决**：`systemctl reset-failed` 清除限制状态

## 6. 未来工作：过渡到 apt 标准发布

### 6.1 需要准备的工件

1. **`.deb` 包结构**：
   ```
   gnome-remote-agent_{version}_amd64.deb
   ├── DEBIAN/
   │   ├── control          # 包元数据（依赖、版本、描述）
   │   ├── conffiles        # 配置文件列表（升级时保留）
   │   ├── postinst         # 安装后脚本（daemon-reload + enable + start）
   │   ├── prerm            # 卸载前脚本（stop）
   │   ├── postrm           # 卸载后脚本（daemon-reload）
   │   └── maintainer_scripts
   ├── usr/local/bin/
   │   └── gnome-remote-agent
   ├── etc/gnome-remote-agent/
   │   └── agent.toml
   ├── etc/systemd/system/
   │   └── gnome-remote-agent.service
   ├── var/log/gnome-remote/
   └── var/lib/gnome-remote/
   ```

2. **CI/CD 流水线**（GitHub Actions）：
   - 触发：tag push（如 v0.2.0）
   - 步骤：cargo build --release → 构建 .deb → 签名 → 发布到 apt 仓库
   - apt 仓库：[cloudsmith](https://cloudsmith.io) 或自建 reprepro

3. **版本管理**：`Cargo.toml` 版本号与 `.deb` 版本号同步

4. **GPG 签名**：签名 `.deb` 包，apt 仓库添加公钥

### 6.2 用户操作变化

| 操作 | 当前（权宜脚本） | 未来（apt） |
|------|----------------|------------|
| 安装 | `sudo bash install.sh` | `sudo apt install gnome-remote-agent` |
| 更新 | `sudo bash update.sh` | `sudo apt update && sudo apt upgrade gnome-remote-agent` |
| 卸载 | `sudo bash uninstall.sh` | `sudo apt remove gnome-remote-agent` |
| 配置保留 | 脚本备份 `.bak` | apt conffiles 机制自动提示 |

### 6.3 过渡策略

1. **阶段 1（当前）**：权宜脚本 + tar.gz 分发
2. **阶段 2**：提供 `.deb` 包，用户手动 `dpkg -i` 安装
3. **阶段 3**：搭建 apt 仓库，用户添加源后 `apt install` 升级
4. **阶段 4**：权宜脚本退役，仅保留 `build.sh` 供开发者使用

## 7. 关键约束（来自项目规范）

- Agent 必须使用 systemd 进行进程管理，确保崩溃自动重启
- Agent 必须使用系统现有的 `/etc/pam.d/sshd` PAM 服务进行密码认证
- Agent 必须维持无状态设计，客户端计算 diff 优化网络流量
- 证书钉扎（SSH known_hosts 模式）：首次连接确认，后续自动校验
- `rm + cp` 是替换运行中二进制的正确方法，避免 ETXTBSY
- `systemctl restart` 由 systemd 原子操作，独立于调用终端
