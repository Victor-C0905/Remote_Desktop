#!/bin/bash
# install.sh - GNOME Remote Agent安装脚本

set -e

echo "=== GNOME Remote Agent 安装脚本 ==="

# 1. 创建目录
echo "创建目录..."
mkdir -p /etc/gnome-remote
mkdir -p /var/log/gnome-remote
mkdir -p /var/lib/gnome-remote

# 2. 复制二进制文件
echo "安装二进制文件..."
cp target/release/gnome-remote-agent /usr/local/bin/
chmod 755 /usr/local/bin/gnome-remote-agent

# 3. 复制配置文件
echo "安装配置文件..."
cp agent.toml /etc/gnome-remote/
chmod 644 /etc/gnome-remote/agent.toml

# 4. 生成自签名证书
if [ ! -f /etc/gnome-remote/cert.pem ]; then
    echo "生成TLS证书..."
    openssl req -x509 -newkey rsa:4096 -keyout /etc/gnome-remote/key.pem \
        -out /etc/gnome-remote/cert.pem -days 365 -nodes \
        -subj "/CN=gnome-remote-agent"
    chmod 600 /etc/gnome-remote/key.pem
    chmod 644 /etc/gnome-remote/cert.pem
fi

# 5. 安装PAM配置
echo "配置PAM..."
cp pam.d/gnome-remote /etc/pam.d/

# 6. 安装systemd服务
echo "安装systemd服务..."
cp systemd/gnome-remote-agent.service /etc/systemd/system/
systemctl daemon-reload
systemctl enable gnome-remote-agent.service

# 7. 启动服务
echo "启动服务..."
systemctl start gnome-remote-agent.service

# 8. 检查状态
systemctl status gnome-remote-agent.service --no-pager

echo "=== 安装完成 ==="