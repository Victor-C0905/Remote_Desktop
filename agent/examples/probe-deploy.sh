#!/bin/bash
# agent/examples/probe-deploy.sh — WSL 内执行：搭建探针测试部署
# （用户/密码/PAM/配置），并输出启动命令。重复执行幂等。
set -e

echo "=== 探针测试部署（WSL）==="

# 生产机防呆：本脚本会创建已知密码的测试用户并覆盖 /etc/pam.d/quireld，
# 仅允许在专用测试环境运行。检测到生产部署痕迹（生产数据目录/服务单元
# 存在）时拒绝执行，除非显式 --force
if [[ "$1" != "--force" ]]; then
    if [[ -d /var/lib/quireld ]] || [[ -f /etc/systemd/system/quireld.service ]]; then
        echo "错误：检测到生产部署痕迹（/var/lib/quireld 或 quireld.service 存在）。"
        echo "本脚本会创建已知密码的测试用户并覆盖 /etc/pam.d/quireld，"
        echo "禁止在生产环境执行。如确需覆盖请显式传入 --force。"
        exit 1
    fi
fi

# 1. 测试用户（密码认证用）
if ! id probeuser &>/dev/null; then
    useradd -m -s /bin/bash probeuser
    echo "已创建用户 probeuser"
fi
echo "probeuser:probe-pass-2026" | chpasswd
echo "密码已设置: probe-pass-2026"

# 2. PAM 服务（密码认证走 common-auth）
cat > /etc/pam.d/quireld << 'PAM'
# Quireld 探针测试 PAM 配置
@include common-auth
PAM
echo "PAM 服务已安装: /etc/pam.d/quireld"

# 3. 部署目录与配置
mkdir -p /var/lib/quireld-probe/logs
cat > /var/lib/quireld-probe/quireld-probe.toml << 'TOML'
# 探针测试配置（bind 0.0.0.0：Windows 宿主经 localhost 转发或 WSL IP 访问）
[server]
bind = "0.0.0.0"
quic_port = 8443
cert_path = "./cert.pem"
key_path = "./key.pem"

[auth.ssh]
enable_pubkey = true
enable_password = true
pam_service = "quireld"

[limits]
max_terminal_sessions = 5
max_file_transfer_mb = 100
connection_idle_timeout_secs = 300

[collectors]
metrics_interval_secs = 5
process_scan_interval_secs = 5

[audit]
log_path = "/var/lib/quireld-probe/logs/audit.log"

[log]
level = "info"
dir = "/var/lib/quireld-probe/logs"
TOML
echo "配置已写入: /var/lib/quireld-probe/quireld-probe.toml"

echo ""
echo "启动命令（在 WSL 内执行）:"
echo "  cd /mnt/e/MyWork/Quirel/agent && nohup /mnt/e/MyWork/Quirel/target/debug/quireld \\"
echo "    --config /var/lib/quireld-probe/quireld-probe.toml \\"
echo "    --log-dir /var/lib/quireld-probe/logs > /var/lib/quireld-probe/logs/quireld.out 2>&1 &"
