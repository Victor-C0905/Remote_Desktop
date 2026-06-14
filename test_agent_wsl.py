#!/usr/bin/env python3
"""
Agent 功能测试脚本（WSL 内部测试）
直接在 WSL 中运行，避免网络问题

运行：
wsl -d Ubuntu-22.04 -- python3 test_agent_wsl.py
"""

import socket
import json
import time

# Agent 配置（WSL 内部）
AGENT_HOST = "localhost"
AGENT_PORT = 8444

def test_ping_pong():
    """测试 Ping/Pong 协议"""
    print("\n=== 测试 Ping/Pong ===")
    
    # 创建 TCP Socket
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.connect((AGENT_HOST, AGENT_PORT))
    
    # 发送 WebSocket 握手
    handshake = (
        "GET / HTTP/1.1\r\n"
        f"Host: {AGENT_HOST}:{AGENT_PORT}\r\n"
        "Upgrade: websocket\r\n"
        "Connection: Upgrade\r\n"
        "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n"
        "Sec-WebSocket-Version: 13\r\n"
        "\r\n"
    )
    
    sock.send(handshake.encode())
    
    # 接收握手响应
    response = sock.recv(1024).decode()
    print(f"握手响应:\n{response}")
    
    if "101 Switching Protocols" in response:
        print("✅ WebSocket 握手成功")
    else:
        print("❌ WebSocket 握手失败")
        sock.close()
        return False
    
    # 发送 Ping 消息（WebSocket 帧）
    # 简化版：直接发送 JSON（实际需要 WebSocket 帧封装）
    envelope = {
        "request_id": 1,
        "payload": {
            "type": "ping",
            "data": {
                "timestamp": int(time.time() * 1000)
            }
        }
    }
    
    message = json.dumps(envelope)
    
    # WebSocket 文本帧封装（简化版）
    # FIN=1, RSV1=0, RSV2=0, RSV3=0, Opcode=1 (text)
    frame = bytearray()
    frame.append(0x81)  # FIN + text frame
    frame.append(len(message))  # Payload length
    frame.extend(message.encode())
    
    sock.send(frame)
    print(f"发送 Ping: {envelope}")
    
    # 接收响应（简化版，不解析 WebSocket 帧）
    response_data = sock.recv(1024)
    print(f"收到响应（原始数据）: {response_data[:50]}...")
    
    sock.close()
    print("✅ Ping/Pong 测试完成")
    return True

def main():
    """主函数"""
    print("=" * 60)
    print("Agent 功能测试（WSL 内部）")
    print("=" * 60)
    print(f"Agent 地址: {AGENT_HOST}:{AGENT_PORT}")
    print("=" * 60)
    
    test_ping_pong()

if __name__ == "__main__":
    main()