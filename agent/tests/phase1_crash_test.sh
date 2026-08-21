#!/bin/bash
# 阶段 1 崩溃检测验证脚本
set -e

echo "=== 1. 清理旧进程 ==="
pkill -f 'target/release/quireld' 2>/dev/null || true
rm -f /tmp/quireld-worker.sock
sleep 1

echo "=== 2. 启动 quireld(后台) ==="
cd /mnt/e/MyWork/quirel/agent
./target/release/quireld --config quireld.toml --log-dir off > /tmp/quireld-phase1-test.log 2>&1 &
MAIN_PID=$!
echo "主进程 PID: $MAIN_PID"
sleep 3

echo "=== 3. 检查进程 ==="
ps aux | grep '[q]uireld' || true

echo "=== 4. 获取 Worker PID ==="
WORKER_PID=$(ps aux | grep '[q]uireld --worker' | awk '{print $2}' | head -1)
echo "Worker PID: $WORKER_PID"

if [ -z "$WORKER_PID" ]; then
    echo "ERROR: Worker 进程未找到"
    cat /tmp/quireld-phase1-test.log
    exit 1
fi

echo "=== 5. 杀死 Worker 进程 ==="
kill -9 $WORKER_PID
echo "已杀死 Worker PID: $WORKER_PID"

echo "=== 6. 等待自动重启(3秒) ==="
sleep 3

echo "=== 7. 检查新 Worker 进程 ==="
NEW_WORKER_PID=$(ps aux | grep '[q]uireld --worker' | awk '{print $2}' | head -1)
echo "新 Worker PID: $NEW_WORKER_PID"

if [ -z "$NEW_WORKER_PID" ]; then
    echo "WARNING: 新 Worker 进程未找到(可能崩溃检测器未重启)"
elif [ "$NEW_WORKER_PID" = "$WORKER_PID" ]; then
    echo "ERROR: Worker PID 未变化"
else
    echo "SUCCESS: Worker 已自动重启 (旧: $WORKER_PID -> 新: $NEW_WORKER_PID)"
fi

echo "=== 8. 日志尾部 ==="
tail -20 /tmp/quireld-phase1-test.log

echo "=== 9. 清理 ==="
kill $MAIN_PID 2>/dev/null || true
pkill -f 'target/release/quireld' 2>/dev/null || true
rm -f /tmp/quireld-worker.sock

echo "=== 完成 ==="
