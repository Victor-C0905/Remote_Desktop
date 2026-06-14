#!/usr/bin/env python3
"""
简单的 Agent Ping 测试
只测试 Ping/Pong 协议
"""

import websocket
import ssl
import json
import time

# Agent 配置
AGENT_URL = "wss://localhost:8444"

def test_ping():
    """测试 Ping/Pong"""
    print("=" * 60)
    print("Agent Ping 测试")
    print("=" * 60)
    print(f"连接地址: {AGENT_URL}")
    
    # 创建连接（禁用证书验证）
    sslopt = {"cert_reqs": ssl.CERT_NONE}
    
    try:
        ws = websocket.create_connection(AGENT_URL, sslopt=sslopt)
        print("✅ 连接成功")
        
        # 发送 Ping 消息（Binary）
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
        print(f"\n发送 Ping: {message}")
        
        # 发送 Binary 消息
        ws.send(message.encode(), opcode=websocket.ABNF.OPCODE_BINARY)
        
        # 接收响应
        print("\n等待响应...")
        response = ws.recv()
        
        print(f"\n收到响应: {response}")
        
        # 解析响应
        try:
            resp_data = json.loads(response)
            print(f"\n解析响应: {json.dumps(resp_data, indent=2)}")
            
            if resp_data.get("payload", {}).get("type") == "pong":
                print("\n✅ Ping/Pong 测试成功！")
            else:
                print("\n❌ Ping/Pong 测试失败")
        except Exception as e:
            print(f"\n解析失败: {e}")
        
        ws.close()
        print("\n✅ 连接已关闭")
        
    except Exception as e:
        print(f"❌ 连接失败: {e}")
        import traceback
        traceback.print_exc()

if __name__ == "__main__":
    test_ping()