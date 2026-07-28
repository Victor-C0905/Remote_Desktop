#!/bin/bash
# GNOME Remote Agent 更新脚本

set -e

# 显示帮助信息
show_help() {
    cat << EOF
GNOME Remote Agent 更新工具

用法:
    $0 [选项]

选项:
    --name NAME     服务名称 (默认: gnome-remote-agent)
    --help          显示此帮助信息

示例:
    # 默认更新
    sudo $0

    # 更新自定义服务
    sudo $0 --name my-remote

更新流程:
    1. 检查新版本二进制文件
    2. 停止旧服务
    3. 备份旧版本（可选）
    4. 替换二进制文件
    5. 重启服务
    6. 验证服务状态

EOF
    exit 0
}

# 解析命令行参数
SERVICE_NAME="gnome-remote-agent"

while [[ $# -gt 0 ]]; do
    case $1 in
        --name)
            SERVICE_NAME="$2"
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

# 检查文件（支持打包部署和源码部署两种方式）
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [ -f "$SCRIPT_DIR/agent" ] && [ -f "$SCRIPT_DIR/agent.toml" ]; then
    NEW_BINARY="$SCRIPT_DIR/agent"
    NEW_CONFIG="$SCRIPT_DIR/agent.toml"
elif [ -f "$SCRIPT_DIR/target/release/agent" ]; then
    NEW_BINARY="$SCRIPT_DIR/target/release/agent"
    NEW_CONFIG="$SCRIPT_DIR/agent.prod.toml"
elif [ -f "$SCRIPT_DIR/target/debug/agent" ]; then
    NEW_BINARY="$SCRIPT_DIR/target/debug/agent"
    NEW_CONFIG="$SCRIPT_DIR/agent.dev.toml"
else
    echo "错误: 未找到新版本二进制文件"
    echo "请先运行: bash build.sh release"
    exit 1
fi

if [ ! -f "$NEW_BINARY" ]; then
    echo "错误: 新版本二进制文件不存在"
    exit 1
fi

# 检查旧版本是否存在
INSTALL_DIR="/usr/local/bin"
OLD_BINARY="$INSTALL_DIR/$SERVICE_NAME"

if [ ! -f "$OLD_BINARY" ]; then
    echo "错误: 未找到已安装的服务"
    echo "请先运行: sudo bash install.sh"
    exit 1
fi

# 显示更新信息
echo ""
echo "================================"
echo "  GNOME Remote Agent 更新"
echo "================================"
echo "  服务名称: $SERVICE_NAME"
echo "  旧版本: $OLD_BINARY"
echo "  新版本: $NEW_BINARY"
echo "================================"
echo ""

read -p "确认更新? (y/n) " -n 1 -r
echo
if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    echo "更新已取消"
    exit 0
fi

# 1/5 备份旧版本
echo ">>> [1/5] 备份旧版本..."
BACKUP_DIR="/var/backups/$SERVICE_NAME"
mkdir -p "$BACKUP_DIR"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
BACKUP_FILE="$BACKUP_DIR/${SERVICE_NAME}_$TIMESTAMP"
cp "$OLD_BINARY" "$BACKUP_FILE"
echo "备份位置: $BACKUP_FILE"

# 2/5 停止服务
echo ">>> [2/5] 停止服务..."
if systemctl is-active --quiet $SERVICE_NAME; then
    systemctl stop $SERVICE_NAME
    echo "服务已停止"
else
    echo "服务未运行"
fi

# 3/5 替换二进制文件
echo ">>> [3/5] 更新程序..."
cp "$NEW_BINARY" "$OLD_BINARY"
chmod +x "$OLD_BINARY"
# 更新生产配置（备份旧配置，但不覆盖用户修改）
if [ -f "$NEW_CONFIG" ]; then
    CONFIG_PATH="/etc/$SERVICE_NAME/agent.toml"
    if [ -f "$CONFIG_PATH" ]; then
        cp "$CONFIG_PATH" "$CONFIG_PATH.bak"
    fi
    cp "$NEW_CONFIG" "$CONFIG_PATH"
    echo "  配置已更新（旧配置备份为 agent.toml.bak）"
fi
echo "程序已更新"

# 4/5 启动服务
echo ">>> [4/5] 启动服务..."
systemctl start $SERVICE_NAME

# 5/5 验证状态
echo ">>> [5/5] 验证服务..."
sleep 3
if systemctl is-active --quiet $SERVICE_NAME; then
    echo ""
    echo "✓ 更新成功！"
    echo ""
    echo "服务状态:"
    systemctl status $SERVICE_NAME --no-pager | head -n 10
    echo ""
    echo "如需回滚:"
    echo "  sudo systemctl stop $SERVICE_NAME"
    echo "  sudo cp $BACKUP_FILE $OLD_BINARY"
    echo "  sudo systemctl start $SERVICE_NAME"
    echo ""
else
    echo ""
    echo "✗ 服务启动失败"
    echo ""
    echo "自动回滚..."
    systemctl stop $SERVICE_NAME 2>/dev/null || true
    cp "$BACKUP_FILE" "$OLD_BINARY"
    systemctl start $SERVICE_NAME
    echo "已回滚到旧版本"
    journalctl -u $SERVICE_NAME -n 50 --no-pager
    exit 1
fi

# 清理旧备份（保留最近3个）
echo "清理旧备份..."
ls -t "$BACKUP_DIR" | tail -n +4 | xargs -I {} rm -f "$BACKUP_DIR/{}" 2>/dev/null || true
echo ""