#!/usr/bin/env python3
"""
Agent 完整功能测试脚本
测试认证、系统指标采集、文件操作

运行：
python test_agent_full.py
"""

import websocket
import ssl
import json
import time
from typing import Dict, Any

# Agent 配置
AGENT_URL = "wss://localhost:8444"
AGENT_TOKEN = "gmr_1ecb954c-9a8c-4a5e-a3b4-f9a77af0d036"

class AgentTester:
    """Agent 测试类"""
    
    def __init__(self):
        self.ws = None
        self.request_id = 0
        
    def connect(self):
        """连接 Agent"""
        print("\n" + "=" * 60)
        print("连接 Agent")
        print("=" * 60)
        print(f"地址: {AGENT_URL}")
        
        sslopt = {"cert_reqs": ssl.CERT_NONE}
        
        try:
            self.ws = websocket.create_connection(AGENT_URL, sslopt=sslopt)
            print("✅ 连接成功")
            return True
        except Exception as e:
            print(f"❌ 连接失败: {e}")
            return False
    
    def send_request(self, payload_type: str, data: Dict[Any, Any]) -> Dict[Any, Any]:
        """发送请求并接收响应"""
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
        
        # 发送 Binary 消息
        self.ws.send(message.encode(), opcode=websocket.ABNF.OPCODE_BINARY)
        
        # 接收响应
        response = self.ws.recv()
        
        # 解析响应
        try:
            resp_data = json.loads(response)
            return resp_data
        except Exception as e:
            print(f"解析失败: {e}")
            return {}
    
    def close(self):
        """关闭连接"""
        if self.ws:
            self.ws.close()
            print("\n✅ 连接已关闭")
    
    def test_ping(self):
        """测试 Ping/Pong"""
        print("\n" + "=" * 60)
        print("测试 Ping/Pong")
        print("=" * 60)
        
        response = self.send_request("ping", {
            "timestamp": int(time.time() * 1000)
        })
        
        payload = response.get("payload", {})
        if payload.get("type") == "pong":
            data = payload.get("data", {})
            print(f"✅ Ping/Pong 成功")
            print(f"  客户端时间: {data.get('timestamp')}")
            print(f"  服务器时间: {data.get('server_time')}")
            return True
        else:
            print(f"❌ Ping/Pong 失败")
            return False
    
    def test_auth(self):
        """测试认证"""
        print("\n" + "=" * 60)
        print("测试认证")
        print("=" * 60)
        
        response = self.send_request("auth_request", {
            "token": AGENT_TOKEN
        })
        
        payload = response.get("payload", {})
        if payload.get("type") == "auth_response":
            data = payload.get("data", {})
            success = data.get("success", False)
            
            if success:
                print(f"✅ 认证成功")
                return True
            else:
                error = data.get("error", "未知错误")
                print(f"❌ 认证失败: {error}")
                return False
        else:
            print(f"❌ 认证失败")
            return False
    
    def test_metrics(self):
        """测试系统指标采集"""
        print("\n" + "=" * 60)
        print("测试系统指标采集")
        print("=" * 60)
        
        response = self.send_request("metrics_subscribe", {})
        
        payload = response.get("payload", {})
        if payload.get("type") == "metrics_data":
            data = payload.get("data", {})
            
            print(f"✅ 系统指标采集成功")
            print(f"  CPU 使用率: {data.get('cpu_percent', 0):.2f}%")
            print(f"  内存使用: {data.get('mem_used_bytes', 0) / (1024**3):.2f} GB / {data.get('mem_total_bytes', 0) / (1024**3):.2f} GB")
            print(f"  磁盘数量: {len(data.get('disks', []))}")
            
            # 显示磁盘信息
            for disk in data.get('disks', []):
                mount = disk.get('mount', 'unknown')
                total = disk.get('total_bytes', 0) / (1024**3)
                used = disk.get('used_bytes', 0) / (1024**3)
                percent = (used / total) * 100 if total > 0 else 0
                print(f"    {mount}: {used:.2f} GB / {total:.2f} GB ({percent:.1f}%)")
            
            print(f"  网络接收: {data.get('network_rx_bytes', 0) / (1024**2):.2f} MB")
            print(f"  网络发送: {data.get('network_tx_bytes', 0) / (1024**2):.2f} MB")
            print(f"  系统运行时间: {data.get('uptime_secs', 0) / 3600:.2f} 小时")
            
            return True
        else:
            print(f"❌ 系统指标采集失败")
            return False
    
    def test_read_dir(self):
        """测试读取目录"""
        print("\n" + "=" * 60)
        print("测试读取目录")
        print("=" * 60)
        
        response = self.send_request("read_dir", {
            "path": "/home"
        })
        
        payload = response.get("payload", {})
        if payload.get("type") == "read_dir_resp":
            data = payload.get("data", {})
            entries = data.get("entries", [])
            
            print(f"✅ 读取目录成功")
            print(f"  目录: /home")
            print(f"  文件数量: {len(entries)}")
            
            # 显示前 10 个文件
            print(f"  文件列表:")
            for entry in entries[:10]:
                icon = "📁" if entry.get("is_dir") else "📄"
                name = entry.get("name", "unknown")
                size = entry.get("size", 0)
                perms = entry.get("permissions", "unknown")
                print(f"    {icon} {name} ({size} bytes, {perms})")
            
            return True
        else:
            print(f"❌ 读取目录失败")
            return False
    
    def test_read_file(self):
        """测试读取文件"""
        print("\n" + "=" * 60)
        print("测试读取文件")
        print("=" * 60)
        
        response = self.send_request("read_file", {
            "path": "/etc/hostname"
        })
        
        payload = response.get("payload", {})
        if payload.get("type") == "read_file_resp":
            data = payload.get("data", {})
            
            print(f"✅ 读取文件成功")
            print(f"  文件: /etc/hostname")
            print(f"  内容: {data.get('content', '').strip()}")
            print(f"  大小: {data.get('size', 0)} bytes")
            
            return True
        else:
            print(f"❌ 读取文件失败")
            return False
    
    def run_all_tests(self):
        """运行所有测试"""
        print("\n" + "=" * 60)
        print("Agent 完整功能测试")
        print("=" * 60)
        
        if not self.connect():
            return
        
        results = []
        
        try:
            results.append(("Ping/Pong", self.test_ping()))
            results.append(("认证", self.test_auth()))
            results.append(("系统指标", self.test_metrics()))
            results.append(("读取目录", self.test_read_dir()))
            results.append(("读取文件", self.test_read_file()))
        except Exception as e:
            print(f"\n❌ 测试过程中出错: {e}")
            import traceback
            traceback.print_exc()
        
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
    main()