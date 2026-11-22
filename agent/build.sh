#!/bin/bash
# Quireld 构建打包脚本
# 根据环境自动选择配置文件，打包成独立的 tar.gz

set -e

# 显示帮助信息
show_help() {
    cat << EOF
Quireld 构建打包工具

用法:
    $0 <环境> [架构] [选项]

环境:
    release     生产环境 (使用 quireld.prod.toml, 端口 9443/9444)
    test        测试环境 (使用 quireld.test.toml, 端口 8443/8444)
    dev         开发环境 (使用 quireld.dev.toml, 端口 8443/8444)

架构:
    amd64       x86_64 (默认)
    arm64       aarch64 (需已安装 aarch64-unknown-linux-gnu target
                与 aarch64-linux-gnu-gcc 交叉链接器)

选项:
    --output DIR    输出目录 (默认: dist)
    --help          显示此帮助信息

示例:
    # 打包生产版本 (amd64)
    $0 release

    # 打包生产版本 (arm64)
    $0 release arm64

    # 指定输出目录
    $0 release amd64 --output /tmp/packages

打包结果:
    dist/quireld-{version}-{env}-linux-{amd64|arm64}.tar.gz
    dist/SHA256SUMS              (dist 内全部 tarball 的校验清单, 每次构建后重建)

解压后包含:
    - quireld            可执行文件
    - quireld.toml         对应环境的配置文件
    - install.sh         安装脚本
    - update.sh          更新脚本
    - uninstall.sh       卸载脚本

部署:
    tar xzf quireld-*.tar.gz
    cd quireld-*
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

# 架构参数：第一个非选项位置参数（默认 amd64）
# 命名映射统一收口：uname -m 输出 → 包名后缀（客户端/引导脚本复用同一映射）
ARCH="amd64"

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
        -*)
            echo "错误: 未知参数 '$1'"
            exit 1
            ;;
        *)
            # 第一个位置参数为架构
            if [ "$ARCH" = "amd64" ] && [ "$ARCH_SET" != "1" ]; then
                ARCH="$1"
                ARCH_SET=1
                shift
            else
                echo "错误: 多余参数 '$1'"
                exit 1
            fi
            ;;
    esac
done

# 架构配置映射
case "$ARCH" in
    amd64)
        RUST_TARGET="x86_64-unknown-linux-gnu"
        ARCH_DESC="x86_64"
        ;;
    arm64)
        RUST_TARGET="aarch64-unknown-linux-gnu"
        ARCH_DESC="aarch64"
        ;;
    *)
        echo "错误: 未知架构 '$ARCH'"
        echo "可选: amd64, arm64"
        exit 1
        ;;
esac

# 环境配置映射
case "$ENV" in
    release)
        CONFIG_FILE="quireld.prod.toml"
        ENV_DESC="生产环境"
        ;;
    test)
        CONFIG_FILE="quireld.test.toml"
        ENV_DESC="测试环境"
        ;;
    dev)
        CONFIG_FILE="quireld.dev.toml"
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
log_info "  架构: $ARCH_DESC ($ARCH)"
log_info "==============================="

# arm64 交叉编译前置检查：rustup target 与交叉链接器必须就绪
if [ "$ARCH" = "arm64" ]; then
    if ! rustup target list --installed 2>/dev/null | grep -q '^aarch64-unknown-linux-gnu$'; then
        log_error "缺少 rust target: 执行 rustup target add aarch64-unknown-linux-gnu"
        exit 1
    fi
    if ! command -v aarch64-linux-gnu-gcc >/dev/null 2>&1; then
        log_error "缺少交叉链接器 aarch64-linux-gnu-gcc (apt install gcc-aarch64-linux-gnu)"
        exit 1
    fi
fi

# 1/4 编译
log_info "[1/4] 编译 Quireld..."
cd "$AGENT_DIR"

# 根据环境选择编译参数；arm64 追加 --target 走交叉编译
TARGET_ARGS=""
if [ "$ARCH" = "arm64" ]; then
    TARGET_ARGS="--target $RUST_TARGET"
fi

case "$ENV" in
    release)
        cargo build --release $TARGET_ARGS
        # 工作区共享 target-dir（.cargo/config.toml → 项目根 target/）：
        # amd64 不带 --target 产物在 ../target/release；arm64 交叉产物在 ../target/<triple>/release
        if [ "$ARCH" = "arm64" ]; then
            BINARY_PATH="../target/$RUST_TARGET/release/quireld"
        else
            BINARY_PATH="../target/release/quireld"
        fi
        ;;
    test)
        cargo build --release $TARGET_ARGS
        if [ "$ARCH" = "arm64" ]; then
            BINARY_PATH="../target/$RUST_TARGET/release/quireld"
        else
            BINARY_PATH="../target/release/quireld"
        fi
        ;;
    dev)
        # dev 环境保持本机调试构建，不支持交叉（调试产物无需跨架构）
        if [ "$ARCH" = "arm64" ]; then
            log_error "dev 环境不支持 arm64 交叉构建（调试构建仅限本机架构）"
            exit 1
        fi
        cargo build
        BINARY_PATH="target/debug/quireld"
        ;;
esac

if [ ! -f "$AGENT_DIR/$BINARY_PATH" ]; then
    log_error "编译失败: 二进制文件不存在"
    exit 1
fi

log_success "编译完成: $BINARY_PATH"

# 2/4 准备打包目录
log_info "[2/4] 准备打包..."
PACKAGE_NAME="quireld-${VERSION}-${ENV}-linux-${ARCH}"
PACKAGE_DIR="$OUTPUT_DIR/$PACKAGE_NAME"

# 清理旧文件
rm -rf "$PACKAGE_DIR"
mkdir -p "$PACKAGE_DIR"

# 复制文件
cp "$AGENT_DIR/$BINARY_PATH" "$PACKAGE_DIR/quireld"
chmod +x "$PACKAGE_DIR/quireld"

# 复制配置文件（统一命名为 quireld.toml）
cp "$AGENT_DIR/$CONFIG_FILE" "$PACKAGE_DIR/quireld.toml"

# 复制部署脚本
cp "$AGENT_DIR/deploy/install.sh" "$PACKAGE_DIR/"
cp "$AGENT_DIR/deploy/update.sh" "$PACKAGE_DIR/"
cp "$AGENT_DIR/deploy/uninstall.sh" "$PACKAGE_DIR/"
chmod +x "$PACKAGE_DIR"/*.sh

# 复制 systemd service 模板（install.sh 安装时使用）
# 路径：项目根目录/systemd/quireld.service
SYSTEMD_TEMPLATE="$AGENT_DIR/../systemd/quireld.service"
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

# 重建 dist/SHA256SUMS：覆盖 dist 内全部 quireld-*.tar.gz 的校验清单
# （客户端与 install-quireld.sh 按"文件名 → 哈希"精确匹配消费）
log_info "[4/4] 重建 SHA256SUMS..."
cd "$OUTPUT_DIR"
SHA256_FILES=$(ls -1 quireld-*.tar.gz 2>/dev/null || true)
if [ -z "$SHA256_FILES" ]; then
    log_error "dist 内没有 tarball，无法生成 SHA256SUMS"
    exit 1
fi
: > SHA256SUMS
for f in $SHA256_FILES; do
    sha256sum "$f" >> SHA256SUMS
done
cd "$AGENT_DIR"
log_success "SHA256SUMS 已重建 ($(wc -l < "$OUTPUT_DIR/SHA256SUMS") 个条目)"

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
