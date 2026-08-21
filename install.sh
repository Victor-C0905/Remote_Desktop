#!/bin/bash
# install.sh - Quireld安装脚本

set -e

echo "=== Quireld 安装脚本 ==="

# 1. 创建目录
echo "创建目录..."
mkdir -p /etc/quireld
mkdir -p /var/log/quireld
mkdir -p /var/lib/quireld

# 2. 复制二进制文件
echo "安装二进制文件..."
cp target/release/quireld /usr/local/bin/
chmod 755 /usr/local/bin/quireld

# 3. 复制配置文件
echo "安装配置文件..."
cp quireld.toml /etc/quireld/
chmod 644 /etc/quireld/quireld.toml

# 4. 生成自签名证书
if [ ! -f /etc/quireld/cert.pem ]; then
    echo "生成TLS证书..."
    openssl req -x509 -newkey rsa:4096 -keyout /etc/quireld/key.pem \
        -out /etc/quireld/cert.pem -days 365 -nodes \
        -subj "/CN=quireld"
    chmod 600 /etc/quireld/key.pem
    chmod 644 /etc/quireld/cert.pem
fi

# 5. 安装PAM配置
echo "配置PAM..."
cp pam.d/quireld /etc/pam.d/

# 6. 安装systemd服务
echo "安装systemd服务..."
cp systemd/quireld.service /etc/systemd/system/
systemctl daemon-reload
systemctl enable quireld.service

# 7. 启动服务
echo "启动服务..."
systemctl start quireld.service

# 8. 检查状态
systemctl status quireld.service --no-pager

echo "=== 安装完成 ==="