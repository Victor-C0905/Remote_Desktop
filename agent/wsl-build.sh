#!/bin/bash
# WSL 编译验证脚本

set -e

# 确保 PATH 包含 rustup 安装的 Rust
export PATH="$HOME/.cargo/bin:$PATH"

echo "========================================="
echo "WSL 编译验证"
echo "========================================="

# 显示 Rust 版本
echo "Rust 版本信息:"
cargo --version
rustc --version
echo ""

# 进入项目目录
cd /mnt/e/MyWork/quirel/agent

# 删除旧的 Cargo.lock（如果存在）
if [ -f "Cargo.lock" ]; then
    echo "删除旧的 Cargo.lock..."
    rm -f Cargo.lock
fi

# 更新依赖
echo "更新依赖..."
cargo update

# 编译检查
echo "开始编译检查..."
cargo check

# 如果编译成功，尝试构建
echo "开始构建..."
cargo build --release

echo "========================================="
echo "✅ 编译验证完成"
echo "========================================="