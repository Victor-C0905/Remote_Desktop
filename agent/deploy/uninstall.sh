#!/bin/bash
# Quireld 卸载脚本

set -e

# 显示帮助信息
show_help() {
    cat << EOF
Quireld 卸载工具

用法:
    $0 [选项]

选项:
    --dir DIR       安装目录 (默认: /usr/local/bin)
    --name NAME     服务名称 (默认: quireld)
    --port PORT     监听端口 (默认: 8443)
    --help          显示此帮助信息

示例:
    # 默认卸载
    sudo $0

    # 卸载自定义安装（参数需与安装时一致）
    sudo $0 --name my-remote --port 9443

EOF
    exit 0
}

# 解析命令行参数
INSTALL_DIR="/usr/local/bin"
SERVICE_NAME="quireld"
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

# 显示卸载信息
echo ""
echo "================================"
echo "  Quireld 卸载配置"
echo "================================"
echo "  程序路径: $INSTALL_DIR/$SERVICE_NAME"
echo "  配置路径: /etc/$SERVICE_NAME"
echo "  服务名称: $SERVICE_NAME"
echo "================================"
echo ""

read -p "确认卸载? (y/n) " -n 1 -r
echo
if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    echo "卸载已取消"
    exit 0
fi

echo ">>> [1/4] 停止服务..."
if systemctl is-active --quiet $SERVICE_NAME; then
    systemctl stop $SERVICE_NAME
fi

echo ">>> [2/4] 禁用开机自启..."
if systemctl is-enabled --quiet $SERVICE_NAME; then
    systemctl disable $SERVICE_NAME
fi

echo ">>> [3/4] 删除服务文件..."
if [ -f /etc/systemd/system/$SERVICE_NAME.service ]; then
    rm -f /etc/systemd/system/$SERVICE_NAME.service
    systemctl daemon-reload
fi

echo ">>> [4/4] 删除文件..."
# 删除二进制文件
if [ -f "$INSTALL_DIR/$SERVICE_NAME" ]; then
    rm -f "$INSTALL_DIR/$SERVICE_NAME"
fi

# 删除配置目录
if [ -d "/etc/$SERVICE_NAME" ]; then
    rm -rf "/etc/$SERVICE_NAME"
fi

# 删除日志目录
if [ -d "/var/log/$SERVICE_NAME" ]; then
    rm -rf "/var/log/$SERVICE_NAME"
fi

# 删除防火墙规则（可选）
if command -v ufw &> /dev/null; then
    ufw delete allow $QUIC_PORT/udp 2>/dev/null || true
elif command -v firewall-cmd &> /dev/null; then
    firewall-cmd --permanent --remove-port=$QUIC_PORT/udp 2>/dev/null || true
    firewall-cmd --reload 2>/dev/null || true
fi

echo ""
echo "✓ 卸载完成！"
echo ""