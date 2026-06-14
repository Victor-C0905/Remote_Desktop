#!/usr/bin/env python3
"""
QUIC 测试脚本（使用 Python + asyncio + aioquic）
测试 Agent 的 QUIC 连接功能

安装依赖：
pip3 install aioquic
"""

import asyncio
import json
import time
from aioquic.asyncio.client import connect
from aioquic.quic.configuration import QuicConfiguration
from aioquic.asyncio.protocol import QuicStreamProtocol

async def test_quic_connection():
    """测试 QUIC 连接"""
    print("=== QUIC 客户端测试 ===")
    print()
    
    # 配置 QUIC 客户端（跳过证书验证）
    configuration = QuicConfiguration(
        is_client=True,
        alpn_protocols=["h3"],
    )
    configuration.verify_mode = False  # 跳过证书验证
    
    print("🔗 连接 Agent (127.0.0.1:8443)...")
    
    try:
        # 连接服务器
        async with connect("127.0.0.1", 8443, configuration=configuration) as client:
            print("✅ QUIC 连接成功")
            print()
            
            # 创建 Stream
            stream_id = client._quic.get_next_available_stream_id()
            print(f"✅ Stream 创建成功 (id={stream_id})")
            print()
            
            # 发送 Ping 消息
            timestamp = int(time.time() * 1000)
            ping_msg = {
                "request_id": 1,
                "payload": {
                    "type": "ping",
                    "data": {
                        "timestamp": timestamp
                    }
                }
            }
            
            ping_bytes = json.dumps(ping_msg).encode('utf-8')
            
            # 发送消息长度（4 字节 LE）
            len_bytes = len(ping_bytes).to_bytes(4, byteorder='little')
            client._quic.send_stream_data(stream_id, len_bytes, end_stream=False)
            
            # 发送消息内容
            client._quic.send_stream_data(stream_id, ping_bytes, end_stream=False)
            print("✅ Ping 消息发送成功")
            print(f"   消息: {json.dumps(ping_msg, indent=2)}")
            print()
            
            # 等待响应
            print("⏳ 等待 Pong 响应...")
            
            # 创建 Stream Reader
            reader, writer = await client.create_stream()
            
            # 接收响应长度
            len_buf = await reader.readexactly(4)
            resp_len = int.from_bytes(len_buf, byteorder='little')
            
            # 接收响应内容
            resp_buf = await reader.readexactly(resp_len)
            pong_msg = json.loads(resp_buf.decode('utf-8'))
            
            print("✅ Pong 消息接收成功")
            print(f"   消息: {json.dumps(pong_msg, indent=2)}")
            print()
            
            # 关闭连接
            writer.close()
            print("✅ 连接关闭")
            print()
            
            print("=== 测试完成 ===")
            
    except Exception as e:
        print(f"❌ 测试失败: {e}")
        import traceback
        traceback.print_exc()

if __name__ == "__main__":
    asyncio.run(test_quic_connection())