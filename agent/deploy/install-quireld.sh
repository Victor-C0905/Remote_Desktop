#!/bin/bash
# install-quireld.sh - Quireld 一键安装引导脚本（降级路径）
#
# 用途: 客户端无法自动安装时的手动安装入口（复制粘贴到服务器终端执行）
#   curl -fsSL https://quireld.cn/downloads/quireld/install-quireld.sh | sudo bash
#   可选: | sudo bash -s -- --port 9443
#   备用域名: https://quireld.icu/downloads/quireld/install-quireld.sh
#
# 安全说明:
#   - 官网静态源优先，不可用时回落 GitHub Releases；两处都必须通过 SHA256SUMS 校验才执行
#   - 脚本本体不含任何密钥材料
#   - 证书由 quireld 首次启动自动生成；客户端首次连接走指纹确认（TOFU）

set -euo pipefail

# ── 配置 ──────────────────────────────────────────────────
# 官网静态源（主源 + 备用源，按序尝试；可用 QUIREL_BASE_URL 覆盖，便于镜像/测试，
# 支持逗号或空格分隔多个源；显式置空则跳过官网源直接走 GitHub）
SITE_BASE_URLS="${QUIREL_BASE_URL:-https://quireld.cn/downloads/quireld https://quireld.icu/downloads/quireld}"
SITE_BASE_URLS="${SITE_BASE_URLS//,/ }"
# GitHub Releases 兜底（可用 QUIREL_REPO 覆盖）
QUIREL_REPO="${QUIREL_REPO:-Victor-C0905/Remote_Desktop}"
GITHUB_BASE_URL="https://github.com/${QUIREL_REPO}/releases/latest/download"
INSTALL_DIR="/usr/local/bin"
SERVICE_NAME="quireld"
OVERRIDE_PORT=""
# 下载参数：连接/总超时 + 瞬时失败重试（仅传输层；SHA-256 校验失败由 fetch_from 返回 2 中断，绝不重试）
CURL_OPTS=(-fsSL --connect-timeout 10 --max-time 600 --retry 2 --retry-delay 1 --retry-connrefused)

# ── 参数解析 ──────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
    case $1 in
        --port)
            OVERRIDE_PORT="$2"
            shift 2
            ;;
        --name)
            SERVICE_NAME="$2"
            shift 2
            ;;
        *)
            echo "错误: 未知参数 '$1'" >&2
            exit 1
            ;;
    esac
done

# ── 工具函数 ──────────────────────────────────────────────
log_info()    { echo "[INFO] $1"; }
log_success() { echo "[OK]   $1"; }
log_error()   { echo "[ERROR] $1" >&2; }

# ── 前置检查 ──────────────────────────────────────────────
# root 权限
if [[ $EUID -ne 0 ]]; then
    log_error "需要 root 权限执行: curl ... | sudo bash"
    exit 1
fi

# 依赖检查
for cmd in curl tar sha256sum systemctl; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        log_error "缺少依赖命令: $cmd"
        exit 1
    fi
done

# systemd 可用性
if ! systemctl is-system-running >/dev/null 2>&1 \
    && [ "$(systemctl is-system-running 2>/dev/null || echo)" != "degraded" ]; then
    log_error "系统未运行 systemd（quireld 需要 systemd 管理）"
    exit 1
fi

# ── 架构检测（白名单映射, 与客户端/构建脚本同一映射表）─────
ARCH_RAW="$(uname -m)"
case "$ARCH_RAW" in
    x86_64)  ARCH="amd64" ;;
    aarch64) ARCH="arm64" ;;
    *)
        log_error "不支持的架构: $ARCH_RAW (仅支持 x86_64/aarch64)"
        exit 1
        ;;
esac
log_info "检测到架构: $ARCH_RAW → $ARCH"

# ── 下载与校验 ────────────────────────────────────────────
WORK_DIR="$(mktemp -d /tmp/quireld-install.XXXXXX)"
trap 'rm -rf "$WORK_DIR"' EXIT

# 资产命名与 build.sh 产出一致（稳定别名，无需本地拼接版本号）
TARBALL="quireld-linux-${ARCH}.tar.gz"

# 从指定基础地址下载 tarball + SHA256SUMS 并校验
# 返回: 0=成功；1=源不可用（可换源）；2=校验失败（必须中断，不得换源掩盖）
fetch_from() {
    local base="${1%/}"
    rm -f "$WORK_DIR/$TARBALL" "$WORK_DIR/SHA256SUMS"
    curl "${CURL_OPTS[@]}" -o "$WORK_DIR/$TARBALL" "$base/$TARBALL" || return 1
    curl "${CURL_OPTS[@]}" -o "$WORK_DIR/SHA256SUMS" "$base/SHA256SUMS" || return 1
    # 严格校验: 只取精确匹配该文件名的行做校验（拒绝模糊匹配与空结果）
    if ! grep -E "(^|[[:space:]])${TARBALL}$" "$WORK_DIR/SHA256SUMS" | sha256sum -c --status -; then
        return 2
    fi
    return 0
}

# 官网源按序尝试：任一源校验失败立即中断（不得换源掩盖投毒），
# 仅"源不可达"才继续下一个来源；全部不可用再回落 GitHub。
FETCH_OK=0
for base in $SITE_BASE_URLS; do
    # 空值意味着用户显式禁用了官网源（循环体自然跳过）
    [ -n "$base" ] || continue
    log_info "从官网下载源获取 $TARBALL ($base) ..."
    FETCH_RC=0
    fetch_from "$base" || FETCH_RC=$?
    if [[ $FETCH_RC -eq 0 ]]; then
        log_success "SHA-256 校验通过（$base）"
        FETCH_OK=1
        break
    elif [[ $FETCH_RC -eq 2 ]]; then
        log_error "SHA-256 校验失败: $TARBALL（$base，已中断，不回退其他源）"
        exit 1
    fi
    log_info "源不可用，尝试下一个来源 ..."
done

if [[ $FETCH_OK -eq 0 ]]; then
    log_info "官网源均不可用，回退 GitHub Releases ..."
    FETCH_RC=0
    fetch_from "$GITHUB_BASE_URL" || FETCH_RC=$?
    if [[ $FETCH_RC -eq 0 ]]; then
        log_success "SHA-256 校验通过（GitHub 源）"
    elif [[ $FETCH_RC -eq 2 ]]; then
        log_error "SHA-256 校验失败: $TARBALL（GitHub 源）"
        exit 1
    else
        log_error "下载失败: 官网源与 GitHub 源均不可用"
        exit 1
    fi
fi

# ── 解压与安装 ────────────────────────────────────────────
tar xzf "$WORK_DIR/$TARBALL" -C "$WORK_DIR"
PKG_DIR="$(find "$WORK_DIR" -maxdepth 1 -type d -name 'quireld-*' | head -n1)"
if [ -z "$PKG_DIR" ] || [ ! -f "$PKG_DIR/install.sh" ]; then
    log_error "压缩包内容异常: 未找到 install.sh"
    exit 1
fi

INSTALL_ARGS=(--yes)
if [ -n "$OVERRIDE_PORT" ]; then
    INSTALL_ARGS+=(--port "$OVERRIDE_PORT")
fi
if [ "$SERVICE_NAME" != "quireld" ]; then
    INSTALL_ARGS+=(--name "$SERVICE_NAME")
fi

log_info "执行安装 (端口: ${OVERRIDE_PORT:-包内默认}) ..."
bash "$PKG_DIR/install.sh" "${INSTALL_ARGS[@]}"

# ── 完成提示 ──────────────────────────────────────────────
echo ""
log_success "Quireld 安装完成"
echo ""
echo "服务状态:   systemctl status $SERVICE_NAME"
echo "实时日志:   journalctl -u $SERVICE_NAME -f"
echo "配置文件:   /etc/$SERVICE_NAME/quireld.toml"
echo ""
echo "现在可以在 Quirel 客户端中连接本服务器。"
