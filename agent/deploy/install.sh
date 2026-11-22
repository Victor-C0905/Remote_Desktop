#!/bin/bash
# Quireld 安装脚本
# 实现系统级集成和开机自启
#
# 两种模式:
#   交互安装（默认）: 面向人工部署，read -p 确认后安装
#   自动安装（--yes）: 面向客户端 SSH 自动供应，无交互 stdin，机器可读输出
#
# 升级模式（--upgrade）: 仅替换二进制，不动 /etc 配置与证书（客户端升级式安装）

set -e

# 显示帮助信息
show_help() {
    cat << EOF
Quireld 安装工具

用法:
    $0 [选项]

选项:
    --dir DIR           安装目录 (默认: /usr/local/bin)
    --name NAME         服务名称 (默认: quireld)
    --yes               免交互确认（自动安装模式，成功时输出机器可读结果行）
    --port PORT         覆盖 QUIC 端口（写盘后 sed 修改配置中的 quic_port）
    --stagedir DIR      从该目录读取 quireld/quireld.toml（优先于打包/源码定位）
    --binary-hash HASH  校验 staged 二进制的 SHA-256（需配合 --stagedir）
    --upgrade           升级模式: 仅替换二进制并重启服务，保留配置/证书/systemd 单元
    --help              显示此帮助信息

示例:
    # 默认安装（推荐）
    sudo $0

    # 自定义安装
    sudo $0 --name my-remote

    # 客户端自动安装（无交互）
    sudo $0 --yes --stagedir /home/u/.quirel-provision --port 9443 --binary-hash ab12...

    # 升级式安装（仅换二进制）
    sudo $0 --yes --upgrade --stagedir /home/u/.quirel-provision --binary-hash ab12...

安装后：
    - 程序路径: /usr/local/bin/quireld
    - 配置路径: /etc/quireld/quireld.toml (首次启动自动生成)
    - 证书路径: /etc/quireld/cert.pem (首次启动自动生成)
    - 服务名称: quireld
    - 管理命令: systemctl status quireld

EOF
    exit 0
}

# 解析命令行参数
INSTALL_DIR="/usr/local/bin"
SERVICE_NAME="quireld"
ASSUME_YES=0
OVERRIDE_PORT=""
STAGE_DIR=""
BINARY_HASH=""
UPGRADE_MODE=0

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
        --yes)
            ASSUME_YES=1
            shift
            ;;
        --port)
            OVERRIDE_PORT="$2"
            shift 2
            ;;
        --stagedir)
            STAGE_DIR="$2"
            shift 2
            ;;
        --binary-hash)
            BINARY_HASH="$2"
            shift 2
            ;;
        --upgrade)
            UPGRADE_MODE=1
            shift
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

# 参数约束校验
if [ -n "$BINARY_HASH" ] && [ -z "$STAGE_DIR" ]; then
    echo "错误: --binary-hash 必须配合 --stagedir 使用"
    exit 1
fi
if [ -n "$OVERRIDE_PORT" ] && [ "$UPGRADE_MODE" = "1" ]; then
    echo "错误: --upgrade 模式保留已有配置，不允许 --port"
    exit 1
fi

# 检查 Root 权限
if [[ $EUID -ne 0 ]]; then
    echo "错误: 需要 root 权限执行"
    echo "请使用: sudo $0"
    exit 1
fi

# 检查文件（支持 stagedir / 打包部署 / 源码部署三种方式）
# SCRIPT_DIR = deploy/ 目录;PARENT_DIR = agent/ 根目录(源码部署时二进制在 ../target/release/)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PARENT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

if [ -n "$STAGE_DIR" ]; then
    # 客户端自动安装: 从 stagedir 读取（SFTP 推送目录）
    if [ ! -f "$STAGE_DIR/quireld" ]; then
        echo "错误: stagedir 中未找到 quireld 二进制: $STAGE_DIR/quireld"
        exit 1
    fi
    if [ "$UPGRADE_MODE" = "0" ] && [ ! -f "$STAGE_DIR/quireld.toml" ]; then
        echo "错误: stagedir 中未找到配置文件: $STAGE_DIR/quireld.toml"
        exit 1
    fi
    BINARY_FILE="$STAGE_DIR/quireld"
    CONFIG_FILE="$STAGE_DIR/quireld.toml"
elif [ -f "$SCRIPT_DIR/quireld" ] && [ -f "$SCRIPT_DIR/quireld.toml" ]; then
    # 打包部署：quireld 和 quireld.toml 在 deploy/ 同目录
    BINARY_FILE="$SCRIPT_DIR/quireld"
    CONFIG_FILE="$SCRIPT_DIR/quireld.toml"
elif [ -f "$PARENT_DIR/target/release/quireld" ]; then
    # 源码部署：quireld 在 ../target/release/
    BINARY_FILE="$PARENT_DIR/target/release/quireld"
    CONFIG_FILE="$PARENT_DIR/quireld.prod.toml"
elif [ -f "$PARENT_DIR/target/debug/quireld" ]; then
    BINARY_FILE="$PARENT_DIR/target/debug/quireld"
    CONFIG_FILE="$PARENT_DIR/quireld.dev.toml"
else
    echo "错误: 未找到 quireld 二进制文件"
    echo "已查找: stagedir($STAGE_DIR), $SCRIPT_DIR/quireld, $PARENT_DIR/target/release/quireld, $PARENT_DIR/target/debug/quireld"
    echo "请先运行: bash build.sh release"
    echo "或手动编译: cargo build --release"
    exit 1
fi

# 配置文件校验（升级模式不需要 stagedir 配置，从已装配置读端口）
if [ "$UPGRADE_MODE" = "0" ] && [ ! -f "$CONFIG_FILE" ]; then
    echo "错误: 配置文件不存在"
    exit 1
fi

# 二进制哈希校验（自动安装模式端到端防替换的最后一环）
if [ -n "$BINARY_HASH" ]; then
    ACTUAL_HASH=$(sha256sum "$BINARY_FILE" | awk '{print $1}')
    if [ "$ACTUAL_HASH" != "$BINARY_HASH" ]; then
        echo "错误: 二进制 SHA-256 校验失败"
        echo "  期望: $BINARY_HASH"
        echo "  实际: $ACTUAL_HASH"
        exit 1
    fi
fi

# 读取端口:
# - --port 显式覆盖优先
# - 其余从配置文件读取（升级模式读已装配置）
if [ -n "$OVERRIDE_PORT" ]; then
    QUIC_PORT="$OVERRIDE_PORT"
elif [ "$UPGRADE_MODE" = "1" ]; then
    INSTALLED_CONFIG="/etc/$SERVICE_NAME/quireld.toml"
    if [ ! -f "$INSTALLED_CONFIG" ]; then
        echo "错误: 升级模式下未找到已装配置: $INSTALLED_CONFIG"
        exit 1
    fi
    QUIC_PORT=$(grep 'quic_port' "$INSTALLED_CONFIG" | head -n1 | sed 's/[^0-9]//g')
else
    QUIC_PORT=$(grep 'quic_port' "$CONFIG_FILE" | head -n1 | sed 's/[^0-9]//g')
fi

# 确认交互（--yes 跳过: russh exec 无交互 stdin, EOF 会破坏 read -p）
if [ "$ASSUME_YES" = "0" ]; then
    echo ""
    echo "================================"
    echo "  Quireld 安装配置"
    echo "================================"
    if [ "$UPGRADE_MODE" = "1" ]; then
        echo "  模式: 升级 (仅替换二进制)"
    else
        echo "  模式: 全新安装"
    fi
    echo "  程序路径: $INSTALL_DIR/$SERVICE_NAME"
    echo "  配置路径: /etc/$SERVICE_NAME/quireld.toml"
    echo "  服务名称: $SERVICE_NAME"
    echo "  QUIC 端口: ${QUIC_PORT:-未知}/udp"
    echo "================================"
    echo ""

    read -p "确认安装? (y/n) " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        echo "安装已取消"
        exit 0
    fi
fi

# ── 安全/运维配套：PAM 独立服务与日志轮转（升级安装也补齐，已存在则保留定制）──
if [ ! -f "/etc/pam.d/$SERVICE_NAME" ]; then
    cat > "/etc/pam.d/$SERVICE_NAME" <<'PAMEOF'
# Quireld 独立 PAM 服务：不复用 sshd，避免被其 2FA/访问控制策略耦合
@include common-auth
@include common-account
@include common-session
PAMEOF
    chmod 644 "/etc/pam.d/$SERVICE_NAME"
    echo "  PAM 服务已创建: /etc/pam.d/$SERVICE_NAME"
fi

if [ ! -f "/etc/logrotate.d/$SERVICE_NAME" ]; then
    cat > "/etc/logrotate.d/$SERVICE_NAME" <<LOGROTATE_EOF
/var/log/$SERVICE_NAME/*.log {
    daily
    rotate 14
    compress
    delaycompress
    missingok
    notifempty
    copytruncate
    su root root
}
LOGROTATE_EOF
    chmod 644 "/etc/logrotate.d/$SERVICE_NAME"
    echo "  日志轮转已配置: /etc/logrotate.d/$SERVICE_NAME（保留 14 天）"
fi

# 升级/全新安装成功后的防火墙提示（不自动改防火墙，避免覆盖用户既有策略）
print_firewall_hint() {
    if ! command -v ufw >/dev/null 2>&1; then
        return
    fi
    local cfg="/etc/$SERVICE_NAME/quireld.toml"
    local ws=""
    if [ -f "$cfg" ]; then
        ws=$(grep '^ws_port' "$cfg" | head -n1 | sed 's/[^0-9]//g')
    fi
    echo ""
    echo "防火墙提示（安装脚本不自动修改防火墙）:"
    echo "  sudo ufw allow ${QUIC_PORT:-9443}/udp   # QUIC"
    if [ -n "$ws" ]; then
        echo "  sudo ufw allow ${ws}/tcp                # WebSocket（如启用）"
    fi
}

# ── 升级模式: 仅替换二进制并重启 ──────────────────────────
if [ "$UPGRADE_MODE" = "1" ]; then
    INSTALLED_BIN="$INSTALL_DIR/$SERVICE_NAME"
    if [ ! -f "$INSTALLED_BIN" ]; then
        echo "错误: 未找到已安装的二进制: $INSTALLED_BIN (升级模式要求 quireld 已安装)"
        exit 1
    fi

    # 备份旧版本（/var/backups/quireld/, 与 update.sh 惯例一致, 保留最近 3 份）
    BACKUP_DIR="/var/backups/quireld"
    mkdir -p "$BACKUP_DIR"
    cp "$INSTALLED_BIN" "$BACKUP_DIR/${SERVICE_NAME}_$(date +%Y%m%d_%H%M%S)"
    ls -1t "$BACKUP_DIR/${SERVICE_NAME}_"* 2>/dev/null | tail -n +4 | xargs -r rm -f

    # ETXTBSY: 不能 cp 覆盖正在运行的二进制, 先 rm(unlink) 再 cp
    # rm 只是减少文件链接数, 运行中进程持有的 inode 不受影响
    rm -f "$INSTALLED_BIN"
    cp "$BINARY_FILE" "$INSTALLED_BIN"
    chmod 755 "$INSTALLED_BIN"

    # restart 失败不直接退出（set -e），交由 is-active 复检判定，保证回滚分支可达
    systemctl restart "$SERVICE_NAME" || true
    sleep 2
    if systemctl is-active --quiet "$SERVICE_NAME"; then
        echo "✓ 升级成功！"
        print_firewall_hint
        echo "QUIRELD_INSTALL_OK port=${QUIC_PORT:-0} config=/etc/$SERVICE_NAME/quireld.toml mode=upgrade"
        exit 0
    fi

    # 升级失败自动回滚: 恢复最近备份 → 重启 → 复检（仍失败 exit 1 交人工处理）
    echo "✗ 新版本启动失败, 尝试自动回滚到最近备份..."
    journalctl -u "$SERVICE_NAME" -n 50 --no-pager
    LATEST_BACKUP=$(ls -1t "$BACKUP_DIR/${SERVICE_NAME}_"* 2>/dev/null | head -n1)
    if [ -z "$LATEST_BACKUP" ]; then
        echo "✗ 未找到可用备份, 无法自动回滚, 请人工处理: $BACKUP_DIR"
        exit 1
    fi
    rm -f "$INSTALLED_BIN"
    cp "$LATEST_BACKUP" "$INSTALLED_BIN"
    chmod 755 "$INSTALLED_BIN"
    systemctl reset-failed "$SERVICE_NAME" 2>/dev/null || true
    systemctl restart "$SERVICE_NAME" || true
    sleep 2
    if systemctl is-active --quiet "$SERVICE_NAME"; then
        echo "✓ 已回滚到备份: $LATEST_BACKUP（服务恢复正常）"
        echo "✗ 本次升级失败已回滚, 请检查日志后重试: journalctl -u $SERVICE_NAME -n 100 --no-pager"
        exit 1
    fi
    echo "✗ 回滚后服务仍无法启动, 请人工处理: journalctl -u $SERVICE_NAME -n 100 --no-pager"
    exit 1
fi

# ── 全新安装 ──────────────────────────────────────────────

# 1/3 安装程序和配置
echo ">>> [1/3] 安装程序..."
mkdir -p "$INSTALL_DIR"
mkdir -p "/etc/$SERVICE_NAME"
mkdir -p "/var/log/$SERVICE_NAME"
mkdir -p "/var/lib/$SERVICE_NAME"  # WorkingDirectory 目录（与 --name 改名后的 unit 保持一致）
chmod 700 "/var/lib/$SERVICE_NAME" 2>/dev/null || true
chmod 750 "/var/log/$SERVICE_NAME" 2>/dev/null || true
# 注意: 不能直接 cp 覆盖正在运行的二进制文件
# Linux 对运行中的可执行文件有 ETXTBSY(Text file busy) 保护
# 解决方案: 先 rm(unlink),再 cp
# rm 只是减少文件链接数,运行中进程持有的 inode 不受影响
rm -f "$INSTALL_DIR/$SERVICE_NAME"
cp "$BINARY_FILE" "$INSTALL_DIR/$SERVICE_NAME"
chmod 755 "$INSTALL_DIR/$SERVICE_NAME"
# 安装配置（如果目标配置不存在）
if [ ! -f "/etc/$SERVICE_NAME/quireld.toml" ]; then
    cp "$CONFIG_FILE" "/etc/$SERVICE_NAME/quireld.toml"
    chmod 600 "/etc/$SERVICE_NAME/quireld.toml"
    # PAM 服务名与安装服务名保持一致（--name 自定义时）
    sed -i "s/^pam_service[[:space:]]*=.*/pam_service = \"$SERVICE_NAME\"/" "/etc/$SERVICE_NAME/quireld.toml"
    echo "  配置已安装"
else
    echo "  配置已存在，跳过（如需更新请手动修改）"
fi

# 端口覆盖（--port）: 配置写盘后、首次启动前 sed 修改 quic_port
if [ -n "$OVERRIDE_PORT" ]; then
    sed -i "s/^quic_port\s*=.*/quic_port = $OVERRIDE_PORT/" "/etc/$SERVICE_NAME/quireld.toml"
    echo "  QUIC 端口已设置为 $OVERRIDE_PORT"
fi

# 2/3 配置 systemd 服务
echo ">>> [2/3] 配置系统服务..."

# 查找 systemd service 模板文件
# 优先使用项目根目录的 systemd/quireld.service（包含热更新配置）
# 打包部署：systemd/quireld.service 在子目录；源码部署：systemd/ 在项目根目录
SERVICE_TEMPLATE=""
if [ -n "$STAGE_DIR" ] && [ -f "$STAGE_DIR/systemd/quireld.service" ]; then
    SERVICE_TEMPLATE="$STAGE_DIR/systemd/quireld.service"
elif [ -f "$SCRIPT_DIR/systemd/quireld.service" ]; then
    SERVICE_TEMPLATE="$SCRIPT_DIR/systemd/quireld.service"
elif [ -f "$SCRIPT_DIR/../systemd/quireld.service" ]; then
    SERVICE_TEMPLATE="$SCRIPT_DIR/../systemd/quireld.service"
elif [ -f "$SCRIPT_DIR/../../systemd/quireld.service" ]; then
    SERVICE_TEMPLATE="$SCRIPT_DIR/../../systemd/quireld.service"
fi

if [ -n "$SERVICE_TEMPLATE" ] && [ -f "$SERVICE_TEMPLATE" ]; then
    echo "  使用 service 模板: $SERVICE_TEMPLATE"

    # 复制 service 文件，替换二进制路径和服务名称
    sed \
        -e "s|/usr/local/bin/quireld|$INSTALL_DIR/$SERVICE_NAME|g" \
        -e "s|quireld|$SERVICE_NAME|g" \
        "$SERVICE_TEMPLATE" > /etc/systemd/system/$SERVICE_NAME.service
else
    echo "  警告: 未找到 service 模板，使用内联最小配置"
    cat > /etc/systemd/system/$SERVICE_NAME.service << EOF
[Unit]
Description=Quireld
After=network.target network-online.target
Wants=network-online.target
StartLimitBurst=3
StartLimitIntervalSec=60

[Service]
Type=simple
User=root
Group=root
WorkingDirectory=/var/lib/$SERVICE_NAME
ExecStart=$INSTALL_DIR/$SERVICE_NAME --config /etc/$SERVICE_NAME/quireld.toml --log-dir /var/log/$SERVICE_NAME
KillMode=process
ExecReload=/bin/kill -HUP \$MAINPID
Restart=on-failure
RestartSec=5s
LimitNOFILE=65536
LimitNPROC=infinity
Environment="RUST_LOG=info"
Environment="HOME=/var/lib/quireld"
StandardOutput=journal
StandardError=journal
SyslogIdentifier=$SERVICE_NAME

[Install]
WantedBy=multi-user.target
EOF
fi

# 3/3 启动服务
echo ">>> [3/3] 启动服务..."

# 清除可能的 failed 状态（StartLimitBurst 触发后需要 reset）
systemctl reset-failed $SERVICE_NAME 2>/dev/null || true

systemctl daemon-reload
systemctl enable $SERVICE_NAME
systemctl restart $SERVICE_NAME

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
    print_firewall_hint
    # 机器可读结果行（自动安装模式客户端解析此行）
    echo "QUIRELD_INSTALL_OK port=${QUIC_PORT:-0} config=/etc/$SERVICE_NAME/quireld.toml mode=install"
else
    echo ""
    echo "✗ 服务启动失败"
    journalctl -u $SERVICE_NAME -n 50 --no-pager
    echo ""
    echo "排查与清理指引:"
    echo "  1. 查看完整日志: journalctl -u $SERVICE_NAME -n 100 --no-pager"
    echo "  2. 修复后重试:   systemctl restart $SERVICE_NAME"
    echo "  3. 如需彻底卸载（配置/证书/数据将一并删除）:"
    echo "     systemctl disable --now $SERVICE_NAME"
    echo "     rm -f /etc/systemd/system/$SERVICE_NAME.service $INSTALL_DIR/$SERVICE_NAME"
    echo "     rm -rf /etc/$SERVICE_NAME /var/lib/$SERVICE_NAME /var/log/$SERVICE_NAME"
    echo "     systemctl daemon-reload"
    exit 1
fi
