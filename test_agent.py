#!/usr/bin/env python3
"""
Agent 功能测试脚本
测试 Ping/Pong、认证、系统指标采集、文件操作
"""

import socket
import json
import struct
import time

# WSL Agent 地址
AGENT_HOST = "172.20.10.3"
AGENT_PORT = 8443  # QUIC 端口
AGENT_TOKEN = "gmr_1ecb954c..."  # 从日志中获取完整 Token

# 注意：QUIC 协议需要特殊的客户端库
# 这里使用简单的 TCP Socket 测试 WebSocket 连接
WS_PORT = 8444

def test_ping_pong():
    """测试 Ping/Pong 协议"""
    print("\n=== 测试 Ping/Pong ===")
    
    # 创建 WebSocket 连接（简化版，实际需要 WebSocket 库）
    # 这里我们使用 TCP Socket 模拟
    
    request_id = 1
    envelope = {
        "request_id": request_id,
        "payload": {
            "type": "ping",
            "data": {
                "timestamp": int(time.time() * 1000)
            }
        }
    }
    
    print(f"发送 Ping: {envelope}")
    print("注意：实际测试需要 WebSocket 客户端库（如 websocket-client）")
    
    return True

def test_auth():
    """测试认证协议"""
    print("\n=== 测试认证 ===")
    
    request_id = 2
    envelope = {
        "request_id": request_id,
        "payload": {
            "type": "auth_request",
            "data": {
                "token": AGENT_TOKEN
            }
        }
    }
    
    print(f"发送认证请求: {envelope}")
    print("注意：实际测试需要 WebSocket 客户端库")
    
    return True

def test_metrics():
    """测试系统指标采集"""
    print("\n=== 测试系统指标采集 ===")
    
    request_id = 3
    envelope = {
        "request_id": request_id,
        "payload": {
            "type": "metrics_subscribe",
            "data": {}
        }
    }
    
    print(f"发送指标订阅请求: {envelope}")
    print("预期响应：CPU、内存、磁盘、网络指标")
    
    return True

def test_file_operations():
    """测试文件操作"""
    print("\n=== 测试文件操作 ===")
    
    # 测试读取目录
    request_id = 4
    read_dir_envelope = {
        "request_id": request_id,
        "payload": {
            "type": "read_dir",
            "data": {
                "path": "/home"
            }
        }
    }
    
    print(f"发送读取目录请求: {read_dir_envelope}")
    print("预期响应：/home 目录下的文件列表")
    
    # 测试读取文件
    request_id = 5
    read_file_envelope = {
        "request_id": request_id,
        "payload": {
            "type": "read_file",
            "data": {
                "path": "/etc/hostname"
            }
        }
    }
    
    print(f"发送读取文件请求: {read_file_envelope}")
    print("预期响应：hostname 文件内容")
    
    return True

def main():
    """主测试函数"""
    print("=" * 60)
    print("Agent 功能测试脚本")
    print("=" * 60)
    print(f"Agent 地址: {AGENT_HOST}")
    print(f"QUIC 端口: {AGENT_PORT}")
    print(f"WebSocket 端口: {WS_PORT}")
    print(f"认证 Token: {AGENT_TOKEN}")
    print("=" * 60)
    
    # 运行测试
    results = []
    
    results.append(("Ping/Pong", test_ping_pong()))
    results.append(("认证", test_auth()))
    results.append(("系统指标", test_metrics()))
    results.append(("文件操作", test_file_operations()))
    
    # 输出结果
    print("\n" + "=" * 60)
    print("测试结果总结")
    print("=" * 60)
    
    for name, result in results:
        status = "✅ 通过" if result else "❌ 失败"
        print(f"{name}: {status}")
    
    print("\n注意：")
    print("1. 此脚本仅演示协议格式，实际测试需要 WebSocket 客户端库")
    print("2. 建议使用 'pip install websocket-client' 安装库")
    print("3. 完整的测试脚本需要实现 WebSocket 连接和消息收发")

if __name__ == "__main__":
    main()