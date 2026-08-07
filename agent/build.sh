#!/bin/bash
# GNOME Remote Agent 构建打包脚本
# 根据环境自动选择配置文件，打包成独立的 tar.gz

set -e

# 显示帮助信息
show_help() {
    cat << EOF
GNOME Remote Agent 构建打包工具

用法:
    $0 <环境> [选项]

环境:
    release     生产环境 (使用 agent.prod.toml, 端口 9443/9444)
    test        测试环境 (使用 agent.test.toml, 端口 8443/8444)
    dev         开发环境 (使用 agent.dev.toml, 端口 8443/8444)

选项:
    --output DIR    输出目录 (默认: dist)
    --help          显示此帮助信息

示例:
    # 打包生产版本
    $0 release

    # 打包测试版本
    $0 test

    # 打包开发版本
    $0 dev

    # 指定输出目录
    $0 release --output /tmp/packages

打包结果:
    dist/gnome-remote-agent-{version}-{env}-linux-amd64.tar.gz

解压后包含:
    - agent              可执行文件
    - agent.toml         对应环境的配置文件
    - install.sh         安装脚本
    - update.sh          更新脚本
    - uninstall.sh       卸载脚本

部署:
    tar xzf gnome-remote-agent-*.tar.gz
    cd gnome-remote-agent-*
    sudo bash install.sh

EOF
    exit 0
}

# 解析参数
if [[ $# -lt 1 ]]; then
    show_help
fi

# 处理 --help
if [[ "$1" == "--help" || "$1" == "-h" ]]; then
    show_help
fi

ENV="$1"
shift

OUTPUT_DIR="dist"

while [[ $# -gt 0 ]]; do
    case $1 in
        --output)
            OUTPUT_DIR="$2"
            shift 2
            ;;
        --help|-h)
            show_help
            ;;
        *)
            echo "错误: 未知参数 '$1'"
            exit 1
            ;;
    esac
done

# 环境配置映射
case "$ENV" in
    release)
        CONFIG_FILE="agent.prod.toml"
        ENV_DESC="生产环境"
        ;;
    test)
        CONFIG_FILE="agent.test.toml"
        ENV_DESC="测试环境"
        ;;
    dev)
        CONFIG_FILE="agent.dev.toml"
        ENV_DESC="开发环境"
        ;;
    *)
        echo "错误: 未知环境 '$ENV'"
        echo "可选: release, test, dev"
        exit 1
        ;;
esac

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m'

log_info()    { echo -e "${BLUE}[INFO]${NC} $1"; }
log_success() { echo -e "${GREEN}[OK]${NC} $1"; }
log_error()   { echo -e "${RED}[ERROR]${NC} $1"; }

# 获取项目根目录和版本号
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
AGENT_DIR="$SCRIPT_DIR"

# 从 Cargo.toml 读取版本号
VERSION=$(grep '^version' "$AGENT_DIR/Cargo.toml" | head -n1 | sed 's/.*"\(.*\)".*/\1/')
if [ -z "$VERSION" ]; then
    log_error "无法读取版本号"
    exit 1
fi

# 检查配置文件
if [ ! -f "$AGENT_DIR/$CONFIG_FILE" ]; then
    log_error "配置文件不存在: $CONFIG_FILE"
    exit 1
fi

log_info "========== 构建配置 =========="
log_info "  环境: $ENV_DESC ($ENV)"
log_info "  版本: $VERSION"
log_info "  配置: $CONFIG_FILE"
log_info "==============================="

# 1/4 编译
log_info "[1/4] 编译 Agent..."
cd "$AGENT_DIR"

# 根据环境选择编译参数
case "$ENV" in
    release)
        cargo build --release
        BINARY_PATH="target/release/agent"
        ;;
    test)
        cargo build --release
        BINARY_PATH="target/release/agent"
        ;;
    dev)
        cargo build
        BINARY_PATH="target/debug/agent"
        ;;
esac

if [ ! -f "$AGENT_DIR/$BINARY_PATH" ]; then
    log_error "编译失败: 二进制文件不存在"
    exit 1
fi

log_success "编译完成: $BINARY_PATH"

# 2/4 准备打包目录
log_info "[2/4] 准备打包..."
PACKAGE_NAME="gnome-remote-agent-${VERSION}-${ENV}-linux-amd64"
PACKAGE_DIR="$OUTPUT_DIR/$PACKAGE_NAME"

# 清理旧文件
rm -rf "$PACKAGE_DIR"
mkdir -p "$PACKAGE_DIR"

# 复制文件
cp "$AGENT_DIR/$BINARY_PATH" "$PACKAGE_DIR/agent"
chmod +x "$PACKAGE_DIR/agent"

# 复制配置文件（统一命名为 agent.toml）
cp "$AGENT_DIR/$CONFIG_FILE" "$PACKAGE_DIR/agent.toml"

# 复制部署脚本
cp "$AGENT_DIR/deploy/install.sh" "$PACKAGE_DIR/"
cp "$AGENT_DIR/deploy/update.sh" "$PACKAGE_DIR/"
cp "$AGENT_DIR/deploy/uninstall.sh" "$PACKAGE_DIR/"
chmod +x "$PACKAGE_DIR"/*.sh

# 复制 systemd service 模板（install.sh 安装时使用）
# 路径：项目根目录/systemd/gnome-remote-agent.service
SYSTEMD_TEMPLATE="$AGENT_DIR/../systemd/gnome-remote-agent.service"
if [ -f "$SYSTEMD_TEMPLATE" ]; then
    mkdir -p "$PACKAGE_DIR/systemd"
    cp "$SYSTEMD_TEMPLATE" "$PACKAGE_DIR/systemd/"
    log_info "  包含 systemd service 模板"
else
    log_info "  警告: 未找到 systemd service 模板,install.sh 将使用内联最小配置"
fi

log_success "打包目录准备完成"

# 3/4 生成 tar.gz
log_info "[3/4] 生成压缩包..."
mkdir -p "$OUTPUT_DIR"
tar czf "$OUTPUT_DIR/$PACKAGE_NAME.tar.gz" -C "$OUTPUT_DIR" "$PACKAGE_NAME"

# 显示包大小
PACKAGE_SIZE=$(du -h "$OUTPUT_DIR/$PACKAGE_NAME.tar.gz" | cut -f1)
log_success "压缩包生成完成: $PACKAGE_NAME.tar.gz ($PACKAGE_SIZE)"

# 4/4 清理临时目录
log_info "[4/4] 清理..."
rm -rf "$PACKAGE_DIR"

log_success "构建完成！"
echo ""
log_info "输出文件: $OUTPUT_DIR/$PACKAGE_NAME.tar.gz"
echo ""
echo "部署方法:"
echo "  # 上传到服务器"
echo "  scp $OUTPUT_DIR/$PACKAGE_NAME.tar.gz user@server:/tmp/"
echo ""
echo "  # 在服务器上安装"
echo "  ssh user@server"
echo "  cd /tmp"
echo "  tar xzf $PACKAGE_NAME.tar.gz"
echo "  cd $PACKAGE_NAME"
echo "  sudo bash install.sh"
echo ""
