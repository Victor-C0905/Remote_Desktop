#!/bin/bash
# GNOME Remote Agent 更新脚本
#
# 更新流程:
#   1. 备份旧版本
#   2. rm + cp 替换二进制文件（不需要 stop，rm unlink 不影响运行中进程）
#   3. systemctl restart（systemd 原子操作，不受 PTY 关闭的 SIGHUP 影响）
#
# 关键原理:
#   - rm(unlink) 只删除目录项,运行中进程持有的 inode 不受影响
#   - cp 创建新文件,不会触发 ETXTBSY（因为是新 inode）
#   - systemctl restart 由 systemd(PID 1)执行 stop + start,
#     不依赖发起请求的终端是否存活
#   - 因此可以安全地通过 gnome-remote 终端执行本脚本
#
# 用法:
#   sudo bash update.sh                  # 默认更新
#   sudo bash update.sh --name my-remote # 自定义服务名

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
    1. 备份旧版本
    2. 替换二进制文件（rm + cp，不需要 stop）
    3. systemctl restart（systemd 原子操作）
    4. 验证服务状态

可通过 gnome-remote 终端安全执行:
    本脚本不依赖 stop + cp + start 三段式,
    而是 rm + cp 替换后直接 systemctl restart。
    systemctl restart 由 systemd 完成 stop + start,
    不受 PTY 关闭的 SIGHUP 影响。

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

# 1/4 备份旧版本
echo ">>> [1/4] 备份旧版本..."
BACKUP_DIR="/var/backups/$SERVICE_NAME"
mkdir -p "$BACKUP_DIR"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
BACKUP_FILE="$BACKUP_DIR/${SERVICE_NAME}_$TIMESTAMP"
cp "$OLD_BINARY" "$BACKUP_FILE"
echo "备份位置: $BACKUP_FILE"

# 2/4 替换二进制文件
echo ">>> [2/4] 替换二进制文件..."
# 关键: 用 rm + cp 替代直接 cp 覆盖
# - rm(unlink) 只删除目录项,运行中进程持有的 inode 不受影响
# - cp 创建新文件(新 inode),不会触发 ETXTBSY
# - 因此不需要 stop 服务即可替换二进制
rm -f "$OLD_BINARY"
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

# 3/4 重启服务
echo ">>> [3/4] 重启服务..."
# systemctl restart 是 systemd 的原子操作:
# - 由 systemd(PID 1)执行 stop + start
# - 不依赖当前终端是否存活
# - 即使 PTY 关闭(bash 收到 SIGHUP 退出),systemd 仍会完成 restart
# - 通过 gnome-remote 终端执行时,终端会断开,但服务会成功重启
systemctl restart $SERVICE_NAME

# 4/4 验证状态
echo ">>> [4/4] 验证服务..."
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
    echo "  sudo rm -f $OLD_BINARY"
    echo "  sudo cp $BACKUP_FILE $OLD_BINARY"
    echo "  sudo chmod +x $OLD_BINARY"
    echo "  sudo systemctl start $SERVICE_NAME"
    echo ""
else
    echo ""
    echo "✗ 服务启动失败"
    echo ""
    echo "自动回滚..."
    systemctl stop $SERVICE_NAME 2>/dev/null || true
    rm -f "$OLD_BINARY"
    cp "$BACKUP_FILE" "$OLD_BINARY"
    chmod +x "$OLD_BINARY"
    systemctl start $SERVICE_NAME
    echo "已回滚到旧版本"
    journalctl -u $SERVICE_NAME -n 50 --no-pager
    exit 1
fi

# 清理旧备份（保留最近3个）
echo "清理旧备份..."
ls -t "$BACKUP_DIR" | tail -n +4 | xargs -I {} rm -f "$BACKUP_DIR/{}" 2>/dev/null || true
echo ""
