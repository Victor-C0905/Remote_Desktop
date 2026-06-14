#!/usr/bin/env python3
"""
Agent 功能完整测试脚本
使用 WebSocket 连接测试 Agent 的所有功能

安装依赖：
pip install websocket-client

运行：
python test_agent_ws.py
"""

import websocket
import ssl
import json
import time
import threading
from typing import Dict, Any

# WSL Agent 配置
# 注意：Agent 使用 WSS（WebSocket Secure），需要 TLS 加密
AGENT_HOST = "localhost"
AGENT_PORT = 8444  # WebSocket 端口
AGENT_URL = f"wss://{AGENT_HOST}:{AGENT_PORT}"  # 使用 WSS

# 从 Agent 日志获取完整 Token
AGENT_TOKEN = "gmr_1ecb954c-9a8c-4a5e-a3b4-f9a77af0d036"

class AgentTester:
    """Agent 测试类"""
    
    def __init__(self):
        self.ws = None
        self.request_id = 0
        self.responses = {}
        self.connected = False
        
    def connect(self):
        """连接 Agent"""
        print(f"\n=== 连接 Agent ===")
        print(f"WebSocket URL: {AGENT_URL}")
        
        try:
            # 禁用证书验证（Agent 使用自签名证书）
            sslopt = {"cert_reqs": ssl.CERT_NONE}
            self.ws = websocket.create_connection(AGENT_URL, sslopt=sslopt)
            self.connected = True
            print("✅ 连接成功")
            return True
        except Exception as e:
            print(f"❌ 连接失败: {e}")
            return False
    
    def send_request(self, payload_type: str, data: Dict[Any, Any]) -> int:
        """发送请求"""
        self.request_id += 1
        
        envelope = {
            "request_id": self.request_id,
            "payload": {
                "type": payload_type,
                "data": data
            }
        }
        
        message = json.dumps(envelope)
        print(f"\n发送请求 #{self.request_id}: {payload_type}")
        print(f"数据: {json.dumps(data, indent=2)}")
        
        # 发送 Binary 消息（Agent 只处理 Binary 消息）
        self.ws.send(message, opcode=websocket.ABNF.OPCODE_BINARY)
        return self.request_id
    
    def receive_response(self, timeout: float = 5.0) -> Dict[Any, Any]:
        """接收响应"""
        try:
            result = self.ws.recv()
            response = json.loads(result)
            
            request_id = response.get("request_id")
            payload = response.get("payload", {})
            
            print(f"\n收到响应 #{request_id}:")
            print(f"类型: {payload.get('type')}")
            print(f"数据: {json.dumps(payload.get('data', {}), indent=2)}")
            
            return response
        except Exception as e:
            print(f"❌ 接收响应失败: {e}")
            return {}
    
    def close(self):
        """关闭连接"""
        if self.ws:
            self.ws.close()
            print("\n✅ 连接已关闭")
    
    def test_ping_pong(self):
        """测试 Ping/Pong 协议"""
        print("\n" + "=" * 60)
        print("测试 Ping/Pong 协议")
        print("=" * 60)
        
        request_id = self.send_request("ping", {
            "timestamp": int(time.time() * 1000)
        })
        
        response = self.receive_response()
        
        if response.get("payload", {}).get("type") == "pong":
            print("✅ Ping/Pong 测试成功")
            return True
        else:
            print("❌ Ping/Pong 测试失败")
            return False
    
    def test_auth(self):
        """测试认证协议"""
        print("\n" + "=" * 60)
        print("测试认证协议")
        print("=" * 60)
        
        request_id = self.send_request("auth_request", {
            "token": AGENT_TOKEN
        })
        
        response = self.receive_response()
        
        payload = response.get("payload", {})
        if payload.get("type") == "auth_response":
            success = payload.get("data", {}).get("success", False)
            if success:
                print("✅ 认证测试成功")
                return True
            else:
                print(f"❌ 认证测试失败: {payload.get('data', {}).get('error')}")
                return False
        else:
            print("❌ 认证测试失败")
            return False
    
    def test_metrics(self):
        """测试系统指标采集"""
        print("\n" + "=" * 60)
        print("测试系统指标采集")
        print("=" * 60)
        
        request_id = self.send_request("metrics_subscribe", {})
        
        response = self.receive_response()
        
        payload = response.get("payload", {})
        if payload.get("type") == "metrics_data":
            data = payload.get("data", {})
            
            print("\n系统指标:")
            print(f"  CPU 使用率: {data.get('cpu_percent', 0):.2f}%")
            print(f"  内存使用: {data.get('mem_used_bytes', 0) / (1024**3):.2f} GB / {data.get('mem_total_bytes', 0) / (1024**3):.2f} GB")
            print(f"  磁盘数量: {len(data.get('disks', []))}")
            print(f"  网络接收: {data.get('network_rx_bytes', 0) / (1024**2):.2f} MB")
            print(f"  网络发送: {data.get('network_tx_bytes', 0) / (1024**2):.2f} MB")
            print(f"  系统运行时间: {data.get('uptime_secs', 0) / 3600:.2f} 小时")
            
            print("✅ 系统指标测试成功")
            return True
        else:
            print("❌ 系统指标测试失败")
            return False
    
    def test_read_dir(self):
        """测试读取目录"""
        print("\n" + "=" * 60)
        print("测试读取目录")
        print("=" * 60)
        
        request_id = self.send_request("read_dir", {
            "path": "/home"
        })
        
        response = self.receive_response()
        
        payload = response.get("payload", {})
        if payload.get("type") == "read_dir_resp":
            entries = payload.get("data", {}).get("entries", [])
            
            print(f"\n目录内容 (/home):")
            print(f"  文件数量: {len(entries)}")
            
            for entry in entries[:5]:  # 只显示前 5 个
                icon = "📁" if entry.get("is_dir") else "📄"
                print(f"  {icon} {entry.get('name')} ({entry.get('size')} bytes)")
            
            print("✅ 读取目录测试成功")
            return True
        else:
            print("❌ 读取目录测试失败")
            return False
    
    def test_read_file(self):
        """测试读取文件"""
        print("\n" + "=" * 60)
        print("测试读取文件")
        print("=" * 60)
        
        request_id = self.send_request("read_file", {
            "path": "/etc/hostname"
        })
        
        response = self.receive_response()
        
        payload = response.get("payload", {})
        if payload.get("type") == "read_file_resp":
            data = payload.get("data", {})
            
            print(f"\n文件内容 (/etc/hostname):")
            print(f"  内容: {data.get('content', '').strip()}")
            print(f"  大小: {data.get('size', 0)} bytes")
            
            print("✅ 读取文件测试成功")
            return True
        else:
            print("❌ 读取文件测试失败")
            return False
    
    def run_all_tests(self):
        """运行所有测试"""
        print("\n" + "=" * 60)
        print("Agent 功能完整测试")
        print("=" * 60)
        
        if not self.connect():
            return
        
        results = []
        
        # 运行测试
        try:
            results.append(("Ping/Pong", self.test_ping_pong()))
            results.append(("认证", self.test_auth()))
            results.append(("系统指标", self.test_metrics()))
            results.append(("读取目录", self.test_read_dir()))
            results.append(("读取文件", self.test_read_file()))
        except Exception as e:
            print(f"\n❌ 测试过程中出错: {e}")
        
        # 输出结果
        print("\n" + "=" * 60)
        print("测试结果总结")
        print("=" * 60)
        
        passed = 0
        failed = 0
        
        for name, result in results:
            status = "✅ 通过" if result else "❌ 失败"
            print(f"{name}: {status}")
            
            if result:
                passed += 1
            else:
                failed += 1
        
        print(f"\n总计: {passed} 通过, {failed} 失败")
        
        self.close()

def main():
    """主函数"""
    tester = AgentTester()
    tester.run_all_tests()

if __name__ == "__main__":
    print("注意：")
    print("1. 需要安装 websocket-client: pip install websocket-client")
    print("2. 确保 Agent 正在运行: wsl -d Ubuntu-22.04 -- bash -c 'cd ~/gnome-remote/agent && ./target/release/agent'")
    print("3. 检查 Token 是否正确（从 Agent 日志获取完整 Token）")
    print("\n开始测试...")
    
    main()