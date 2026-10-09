#!/bin/bash
# 阶段 2 端到端验证脚本
cd /mnt/e/MyWork/quirel/agent

echo "=== 1. 启动 quireld ==="
./target/release/quireld --config quireld.toml --log-dir off > /tmp/quireld-phase2-e2e.log 2>&1 &
AGENT_PID=$!
echo "Quireld PID: $AGENT_PID"

sleep 3

echo "=== 2. 检查进程 ==="
ps aux | grep 'release/quireld' | grep -v grep || echo "No quireld processes found!"

echo "=== 3. 日志 ==="
cat /tmp/quireld-phase2-e2e.log | head -20

echo "=== 4. 清理 ==="
kill $AGENT_PID 2>/dev/null
sleep 1
ps aux | grep 'release/quireld' | grep -v grep || echo "All quireld processes stopped"

echo "=== 完成 ==="
