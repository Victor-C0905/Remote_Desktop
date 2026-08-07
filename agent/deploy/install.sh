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
    --help          显示此帮助信息

示例:
    # 默认安装（推荐）
    sudo $0

    # 自定义安装
    sudo $0 --name my-remote

安装后：
    - 程序路径: /usr/local/bin/gnome-remote-agent
    - 配置路径: /etc/gnome-remote-agent/agent.toml (首次启动自动生成)
    - 证书路径: /etc/gnome-remote-agent/cert.pem (首次启动自动生成)
    - 服务名称: gnome-remote-agent
    - 管理命令: systemctl status gnome-remote-agent

EOF
    exit 0
}

# 解析命令行参数
INSTALL_DIR="/usr/local/bin"
SERVICE_NAME="gnome-remote-agent"

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

# 检查文件（支持打包部署和源码部署两种方式）
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# 打包部署：agent 和 agent.toml 在同目录
# 源码部署：agent 在 target/release/ 或 target/debug/，agent.toml 在上级目录
if [ -f "$SCRIPT_DIR/agent" ] && [ -f "$SCRIPT_DIR/agent.toml" ]; then
    BINARY_FILE="$SCRIPT_DIR/agent"
    CONFIG_FILE="$SCRIPT_DIR/agent.toml"
elif [ -f "$SCRIPT_DIR/target/release/agent" ]; then
    BINARY_FILE="$SCRIPT_DIR/target/release/agent"
    CONFIG_FILE="$SCRIPT_DIR/agent.prod.toml"
elif [ -f "$SCRIPT_DIR/target/debug/agent" ]; then
    BINARY_FILE="$SCRIPT_DIR/target/debug/agent"
    CONFIG_FILE="$SCRIPT_DIR/agent.dev.toml"
else
    echo "错误: 未找到 agent 二进制文件"
    echo "请先运行: bash build.sh release"
    echo "或手动编译: cargo build --release"
    exit 1
fi

if [ ! -f "$CONFIG_FILE" ]; then
    echo "错误: 配置文件不存在"
    exit 1
fi

# 从配置文件读取端口信息
QUIC_PORT=$(grep 'quic_port' "$CONFIG_FILE" | head -n1 | sed 's/[^0-9]//g')
WS_PORT=$(grep 'ws_port' "$CONFIG_FILE" | head -n1 | sed 's/[^0-9]//g')

# 显示安装信息
echo ""
echo "================================"
echo "  GNOME Remote Agent 安装配置"
echo "================================"
echo "  程序路径: $INSTALL_DIR/$SERVICE_NAME"
echo "  配置路径: /etc/$SERVICE_NAME/agent.toml"
echo "  服务名称: $SERVICE_NAME"
echo "  QUIC 端口: ${QUIC_PORT:-未知}/udp"
echo "  WebSocket 端口: ${WS_PORT:-未知}/tcp"
echo "================================"
echo ""

read -p "确认安装? (y/n) " -n 1 -r
echo
if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    echo "安装已取消"
    exit 0
fi

# 1/3 安装程序和配置
echo ">>> [1/3] 安装程序..."
mkdir -p "$INSTALL_DIR"
mkdir -p "/etc/$SERVICE_NAME"
mkdir -p "/var/log/gnome-remote"
mkdir -p "/var/lib/gnome-remote"  # WorkingDirectory 目录
# 注意: 不能直接 cp 覆盖正在运行的二进制文件
# Linux 对运行中的可执行文件有 ETXTBSY(Text file busy) 保护
# 解决方案: 先 rm(unlink),再 cp
# rm 只是减少文件链接数,运行中进程持有的 inode 不受影响
rm -f "$INSTALL_DIR/$SERVICE_NAME"
cp "$BINARY_FILE" "$INSTALL_DIR/$SERVICE_NAME"
chmod +x "$INSTALL_DIR/$SERVICE_NAME"
# 安装配置（如果目标配置不存在）
if [ ! -f "/etc/$SERVICE_NAME/agent.toml" ]; then
    cp "$CONFIG_FILE" "/etc/$SERVICE_NAME/agent.toml"
    echo "  配置已安装"
else
    echo "  配置已存在，跳过（如需更新请手动修改）"
fi

# 2/3 配置 systemd 服务
echo ">>> [2/3] 配置系统服务..."

# 查找 systemd service 模板文件
# 优先使用项目根目录的 systemd/gnome-remote-agent.service（包含 Phase 4 热更新配置）
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SERVICE_TEMPLATE=""

# 尝试多个可能的路径（支持打包部署和源码部署）
# 打包部署：systemd/gnome-remote-agent.service 在子目录
# 源码部署：systemd/ 在项目根目录
if [ -f "$SCRIPT_DIR/systemd/gnome-remote-agent.service" ]; then
    SERVICE_TEMPLATE="$SCRIPT_DIR/systemd/gnome-remote-agent.service"
elif [ -f "$SCRIPT_DIR/../systemd/gnome-remote-agent.service" ]; then
    SERVICE_TEMPLATE="$SCRIPT_DIR/../systemd/gnome-remote-agent.service"
elif [ -f "$SCRIPT_DIR/../../systemd/gnome-remote-agent.service" ]; then
    SERVICE_TEMPLATE="$SCRIPT_DIR/../../systemd/gnome-remote-agent.service"
fi

if [ -n "$SERVICE_TEMPLATE" ] && [ -f "$SERVICE_TEMPLATE" ]; then
    echo "  使用 service 模板: $SERVICE_TEMPLATE"

    # 复制 service 文件，替换二进制路径和服务名称
    sed \
        -e "s|/usr/local/bin/gnome-remote-agent|$INSTALL_DIR/$SERVICE_NAME|g" \
        -e "s|gnome-remote-agent|$SERVICE_NAME|g" \
        "$SERVICE_TEMPLATE" > /etc/systemd/system/$SERVICE_NAME.service
else
    echo "  警告: 未找到 service 模板，使用内联最小配置"
    cat > /etc/systemd/system/$SERVICE_NAME.service << EOF
[Unit]
Description=GNOME Remote Agent
After=network.target network-online.target
Wants=network-online.target
StartLimitBurst=3
StartLimitIntervalSec=60

[Service]
Type=simple
User=root
Group=root
WorkingDirectory=/var/lib/gnome-remote
ExecStart=$INSTALL_DIR/$SERVICE_NAME --config /etc/$SERVICE_NAME/agent.toml --log-dir /var/log/gnome-remote
KillMode=process
ExecReload=/bin/kill -HUP \$MAINPID
Restart=on-failure
RestartSec=5s
LimitNOFILE=65536
LimitNPROC=infinity
Environment="RUST_LOG=info"
Environment="HOME=/var/lib/gnome-remote"
StandardOutput=journal
StandardError=journal
SyslogIdentifier=$SERVICE_NAME

[Install]
WantedBy=multi-user.target
EOF
fi

# 3/3 启动服务
echo ">>> [3/3] 启动服务..."
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
echo "修改配置（首次启动后生成）:"
echo "  vim /etc/$SERVICE_NAME/agent.toml"
echo "  systemctl restart $SERVICE_NAME"
echo ""
else
    echo ""
    echo "✗ 服务启动失败"
    journalctl -u $SERVICE_NAME -n 50 --no-pager
    exit 1
fi