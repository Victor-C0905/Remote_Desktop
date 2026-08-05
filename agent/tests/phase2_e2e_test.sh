#!/bin/bash
# 阶段 2 端到端验证脚本
cd /mnt/e/MyWork/gnome-remote/agent

echo "=== 1. 启动 agent ==="
./target/release/agent --config agent.toml --log-dir off > /tmp/agent-phase2-e2e.log 2>&1 &
AGENT_PID=$!
echo "Agent PID: $AGENT_PID"

sleep 3

echo "=== 2. 检查进程 ==="
ps aux | grep 'release/agent' | grep -v grep || echo "No agent processes found!"

echo "=== 3. 日志 ==="
cat /tmp/agent-phase2-e2e.log | head -20

echo "=== 4. 清理 ==="
kill $AGENT_PID 2>/dev/null
sleep 1
ps aux | grep 'release/agent' | grep -v grep || echo "All agent processes stopped"

echo "=== 完成 ==="
