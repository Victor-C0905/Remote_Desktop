#!/bin/bash
# GNOME Remote Agent 安装脚本
# 实现系统级集成和开机自启

set -e

# 显示帮助信息
show_help() {
    cat << EOF
GNOME Remote Agent 安装工具

用法:
    $0 [选项]

选项:
    --dir DIR       安装目录 (默认: /usr/local/bin)
    --name NAME     服务名称 (默认: gnome-remote-agent)
    --port PORT     监听端口 (默认: 8443)
    --help          显示此帮助信息

示例:
    # 默认安装（推荐）
    sudo $0

    # 自定义安装
    sudo $0 --name my-remote --port 9443

安装后：
    - 程序路径: /usr/local/bin/gnome-remote-agent
    - 配置路径: /etc/gnome-remote-agent/config.toml
    - 服务名称: gnome-remote-agent
    - 管理命令: systemctl status gnome-remote-agent

EOF
    exit 0
}

# 解析命令行参数
INSTALL_DIR="/usr/local/bin"
SERVICE_NAME="gnome-remote-agent"
QUIC_PORT="8443"

while [[ $# -gt 0 ]]; do
    case $1 in
        --dir)
            INSTALL_DIR="$2"
            shift 2
            ;;
        --name)
            SERVICE_NAME="$2"
            shift 2
            ;;
        --port)
            QUIC_PORT="$2"
            shift 2
            ;;
        --help)
            show_help
            ;;
        *)
            echo "错误: 未知参数 '$1'"
            echo "运行 '$0 --help' 查看帮助"
            exit 1
            ;;
    esac
done

# 检查 Root 权限
if [[ $EUID -ne 0 ]]; then
    echo "错误: 需要 root 权限执行"
    echo "请使用: sudo $0"
    exit 1
fi

# 检查二进制文件
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
AGENT_DIR="$(dirname "$SCRIPT_DIR")"
BINARY_FILE="$AGENT_DIR/target/release/agent"

if [ ! -f "$BINARY_FILE" ]; then
    echo "错误: 二进制文件不存在"
    echo "请先在开发环境中编译: cargo build --release"
    echo "文件位置: $BINARY_FILE"
    exit 1
fi

# 显示安装信息
echo ""
echo "================================"
echo "  GNOME Remote Agent 安装配置"
echo "================================"
echo "  程序路径: $INSTALL_DIR/$SERVICE_NAME"
echo "  配置路径: /etc/$SERVICE_NAME"
echo "  服务名称: $SERVICE_NAME"
echo "  监听端口: $QUIC_PORT/udp"
echo "================================"
echo ""

read -p "确认安装? (y/n) " -n 1 -r
echo
if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    echo "安装已取消"
    exit 0
fi

# 1/4 安装二进制文件
echo ">>> [1/4] 安装程序..."
mkdir -p "$INSTALL_DIR"
cp "$BINARY_FILE" "$INSTALL_DIR/$SERVICE_NAME"
chmod +x "$INSTALL_DIR/$SERVICE_NAME"

# 2/4 创建配置文件
echo ">>> [2/4] 创建配置文件..."
mkdir -p "/etc/$SERVICE_NAME"
cat > "/etc/$SERVICE_NAME/config.toml" << EOF
# GNOME Remote Agent 配置文件

[server]
bind = "0.0.0.0"
quic_port = $QUIC_PORT

[limits]
max_file_transfer_mb = 100
connection_idle_timeout_secs = 300

[security]
allowed_paths = ["/home", "/root", "/tmp", "/var"]
blocked_commands = ["rm -rf /", "dd if=/dev/zero"]

[logging]
level = "info"
file = "/var/log/$SERVICE_NAME/agent.log"
EOF

mkdir -p "/var/log/$SERVICE_NAME"

# 3/4 配置 systemd 服务
echo ">>> [3/4] 配置系统服务..."
cat > /etc/systemd/system/$SERVICE_NAME.service << EOF
[Unit]
Description=GNOME Remote Agent
After=network.target

[Service]
Type=simple
User=root
ExecStart=$INSTALL_DIR/$SERVICE_NAME
Restart=on-failure
RestartSec=5s
Environment="RUST_LOG=info"
Environment="AGENT_CONFIG=/etc/$SERVICE_NAME/config.toml"

[Install]
WantedBy=multi-user.target
EOF

# 4/4 启动服务
echo ">>> [4/4] 启动服务..."
systemctl daemon-reload
systemctl enable $SERVICE_NAME
systemctl start $SERVICE_NAME

# 检查状态
sleep 2
if systemctl is-active --quiet $SERVICE_NAME; then
    echo ""
    echo "✓ 安装成功！"
    echo ""
    echo "服务管理:"
    echo "  systemctl status $SERVICE_NAME"
    echo "  systemctl start $SERVICE_NAME"
    echo "  systemctl stop $SERVICE_NAME"
    echo "  systemctl restart $SERVICE_NAME"
    echo ""
    echo "查看日志:"
    echo "  journalctl -u $SERVICE_NAME -f"
    echo ""
    echo "客户端连接:"
    echo "  地址: <服务器IP>:$QUIC_PORT"
    echo "  用户: root"
    echo "  密码: 系统密码"
    echo ""
else
    echo ""
    echo "✗ 服务启动失败"
    journalctl -u $SERVICE_NAME -n 50 --no-pager
    exit 1
fi