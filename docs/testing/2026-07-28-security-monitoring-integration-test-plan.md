# 安全与监控功能集成测试计划

## 测试概述

**测试目标：** 验证会话超时、性能监控、统计查询API三个功能模块的正确性、安全性和性能

**测试范围：**
- 会话超时检查功能
- 性能监控埋点功能
- 统计查询API功能
- 权限控制机制
- 性能和稳定性

---

## 一、环境准备

### 1.1 测试环境配置

**服务端（Agent）：**
```bash
# 进入 agent 目录
cd e:\MyWork\gnome-remote\agent

# 编译 release 版本
cargo build --release

# 配置日志级别（可选，用于调试）
export RUST_LOG=debug
# 或 Windows PowerShell:
$env:RUST_LOG="debug"
```

**客户端（Client）：**
```bash
# 进入项目根目录
cd e:\MyWork\gnome-remote

# 编译前端
npm run build

# 编译 Tauri 客户端
npm run tauri build
```

### 1.2 测试数据准备

**创建测试用户：**
```bash
# 在 WSL/Linux 环境中
sudo useradd -m testuser1
sudo passwd testuser1
# 设置密码：testpass123

sudo useradd -m testuser2
sudo passwd testuser2
# 设置密码：testpass456
```

**准备测试文件：**
```bash
# 创建测试目录
mkdir -p /tmp/test-files

# 创建不同大小的测试文件
dd if=/dev/urandom of=/tmp/test-files/small.txt bs=1K count=100
dd if=/dev/urandom of=/tmp/test-files/medium.txt bs=1M count=10
dd if=/dev/urandom of=/tmp/test-files/large.txt bs=10M count=5
```

---

## 二、功能测试

### 2.1 会话超时功能测试

#### 测试用例 1：正常会话不超时

**目的：** 验证活跃会话不会被误判为超时

**步骤：**
1. 使用 root 用户登录
2. 每隔10秒发送一次请求（持续5分钟）
3. 观察日志，确认无超时警告

**预期结果：**
- ✅ 所有请求正常处理
- ✅ 日志中无"会话已超时"警告
- ✅ 会话活动时间持续更新

**验证命令：**
```bash
# 查看会话活动时间更新日志
journalctl -u gnome-remote-agent -f | grep "会话活动时间"
```

---

#### 测试用例 2：会话超时拒绝

**目的：** 验证超时会话被正确拒绝

**步骤：**
1. **临时修改超时时间（测试环境）：**
   ```rust
   // agent/src/auth/session.rs
   const SESSION_TIMEOUT_SECS: u64 = 60; // 临时改为60秒
   ```
   或使用环境变量：
   ```bash
   export SESSION_TIMEOUT_SECS=60
   ```

2. 重新编译并启动 Agent：
   ```bash
   cargo build --release
   cargo run --release
   ```

3. 使用 root 用户登录
4. 等待70秒（不发送任何请求）
5. 发送一个文件列表请求

**预期结果：**
- ✅ 返回错误响应（错误码 401）
- ✅ 错误消息："会话已过期，请重新登录"
- ✅ 日志显示："会话已超时: username=root, idle_time=70s"

**验证脚本：**
```python
# test_session_timeout.py
import time
import requests

# 登录
login_response = requests.post('http://localhost:8443/auth', json={
    'username': 'root',
    'password': 'your_password'
})
print(f"登录成功: {login_response.status_code}")

# 等待超时
print("等待70秒...")
time.sleep(70)

# 发送请求
try:
    response = requests.get('http://localhost:8443/files', headers={
        'Authorization': f'Bearer {login_response.json()["token"]}'
    })
    print(f"响应状态码: {response.status_code}")
    print(f"响应内容: {response.json()}")
except Exception as e:
    print(f"错误: {e}")
```

---

#### 测试用例 3：会话超时统计记录

**目的：** 验证会话超时被正确统计

**步骤：**
1. 触发多次会话超时（重复测试用例2）
2. 使用 root 用户查询统计数据：
   ```json
   {
     "stats_type": "all"
   }
   ```
3. 检查统计响应

**预期结果：**
- ✅ 统计数据中包含超时次数
- ✅ 超时次数与实际触发次数一致

**验证脚本：**
```bash
# 查询统计数据
curl -X POST http://localhost:8443/api \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"stats_type": "all"}'
```

---

### 2.2 性能监控功能测试

#### 测试用例 4：API 响应时间记录

**目的：** 验证 API 响应时间被正确记录

**步骤：**
1. 使用 root 用户登录
2. 连续发送 20 个文件列表请求：
   ```bash
   for i in {1..20}; do
     curl -X GET http://localhost:8443/files \
       -H "Authorization: Bearer $TOKEN"
     sleep 0.5
   done
   ```
3. 查询性能统计：
   ```json
   {
     "stats_type": "performance"
   }
   ```

**预期结果：**
- ✅ 响应时间数据存在
- ✅ P50/P95/P99 值合理
- ✅ 响应时间分布正常

**验证脚本：**
```python
# test_api_response_time.py
import requests
import time

BASE_URL = "http://localhost:8443"
TOKEN = "your_token_here"

# 发送多个请求
response_times = []
for i in range(20):
    start = time.time()
    response = requests.get(f"{BASE_URL}/files", headers={
        'Authorization': f'Bearer {TOKEN}'
    })
    elapsed = time.time() - start
    response_times.append(elapsed)
    print(f"请求 {i+1}: {elapsed*1000:.2f}ms")

# 查询统计
stats_response = requests.post(f"{BASE_URL}/api", headers={
    'Authorization': f'Bearer {TOKEN}'
}, json={'stats_type': 'performance'})

print(f"\n性能统计: {stats_response.json()}")
```

---

#### 测试用例 5：文件传输统计

**目的：** 验证文件传输字节数被正确记录

**步骤：**
1. 准备测试文件（见环境准备）
2. 上传文件：
   ```bash
   curl -X POST http://localhost:8443/files/upload \
     -H "Authorization: Bearer $TOKEN" \
     -F "file=@/tmp/test-files/medium.txt"
   ```
3. 下载文件：
   ```bash
   curl -X GET http://localhost:8443/files/download/medium.txt \
     -H "Authorization: Bearer $TOKEN" \
     -o /tmp/downloaded-medium.txt
   ```
4. 查询统计：
   ```json
   {
     "stats_type": "performance"
   }
   ```

**预期结果：**
- ✅ 文件传输字节数正确（约10MB上传 + 10MB下载）
- ✅ 统计数据准确反映传输量

---

#### 测试用例 6：终端输出统计

**目的：** 验证终端输出字节数被正确记录

**步骤：**
1. 打开终端窗口
2. 执行多个命令：
   ```bash
   ls -la /usr
   find /home -name "*.txt"
   cat /etc/passwd
   ```
3. 查询统计：
   ```json
   {
     "stats_type": "performance"
   }
   ```

**预期结果：**
- ✅ 终端输出字节数 > 0
- ✅ 字节数与实际输出量匹配

---

### 2.3 统计查询 API 测试

#### 测试用例 7：root 用户查询所有统计

**目的：** 验证 root 用户可以查询所有统计数据

**步骤：**
1. 使用 root 用户登录
2. 发送查询请求：
   ```json
   {
     "stats_type": "all"
   }
   ```
3. 检查响应内容

**预期结果：**
- ✅ 返回状态码 200
- ✅ 响应包含 auth、connection、performance 三个字段
- ✅ 所有字段都有数据

**验证脚本：**
```python
# test_root_stats_query.py
import requests

TOKEN = "root_user_token"

response = requests.post('http://localhost:8443/api', headers={
    'Authorization': f'Bearer {TOKEN}'
}, json={'stats_type': 'all'})

print(f"状态码: {response.status_code}")
print(f"响应内容: {response.json()}")

# 验证响应结构
data = response.json()
assert 'auth' in data['payload']
assert 'connection' in data['payload']
assert 'performance' in data['payload']
assert data['payload']['auth'] is not None
assert data['payload']['performance'] is not None
print("✅ root 用户可以查询所有统计")
```

---

#### 测试用例 8：普通用户查询连接统计

**目的：** 验证普通用户可以查询自己的连接统计

**步骤：**
1. 使用 testuser1 登录
2. 发送查询请求：
   ```json
   {
     "stats_type": "connection"
   }
   ```
3. 检查响应内容

**预期结果：**
- ✅ 返回状态码 200
- ✅ 响应只包含 connection 字段
- ✅ auth 和 performance 为 null

**验证脚本：**
```python
# test_normal_user_connection_stats.py
import requests

TOKEN = "testuser1_token"

response = requests.post('http://localhost:8443/api', headers={
    'Authorization': f'Bearer {TOKEN}'
}, json={'stats_type': 'connection'})

print(f"状态码: {response.status_code}")
print(f"响应内容: {response.json()}")

data = response.json()
assert data['payload']['auth'] is None
assert data['payload']['performance'] is None
print("✅ 普通用户可以查询连接统计")
```

---

#### 测试用例 9：权限拒绝测试

**目的：** 验证普通用户无法查询敏感统计

**步骤：**
1. 使用 testuser1 登录
2. 尝试查询认证统计：
   ```json
   {
     "stats_type": "auth"
   }
   ```
3. 尝试查询性能统计：
   ```json
   {
     "stats_type": "performance"
   }
   ```
4. 尝试查询所有统计：
   ```json
   {
     "stats_type": "all"
   }
   ```

**预期结果：**
- ✅ 所有请求返回 403 错误
- ✅ 错误消息："权限不足：只有root用户可以查看此统计"
- ✅ 日志记录权限拒绝事件

**验证脚本：**
```python
# test_permission_denied.py
import requests

TOKEN = "testuser1_token"

for stats_type in ['auth', 'performance', 'all']:
    response = requests.post('http://localhost:8443/api', headers={
        'Authorization': f'Bearer {TOKEN}'
    }, json={'stats_type': stats_type})

    print(f"\n查询 {stats_type}:")
    print(f"状态码: {response.status_code}")

    data = response.json()
    if response.status_code == 403:
        print(f"✅ 正确拒绝访问: {data['payload']['message']}")
    else:
        print(f"❌ 权限检查失败: 应返回403，实际返回{response.status_code}")
```

---

## 三、安全测试

### 3.1 权限边界测试

#### 测试用例 10：越权访问测试

**目的：** 验证权限检查无漏洞

**步骤：**
1. 使用 testuser1 登录
2. 尝试各种绕过权限检查的方法：
   ```python
   # 测试各种可能的绕过方式
   payloads = [
       {'stats_type': 'auth'},
       {'stats_type': 'performance'},
       {'stats_type': 'all'},
       {'stats_type': 'AUTH'},  # 大小写绕过
       {'stats_type': 'auth '},  # 空格绕过
       {'stats_type': 'auth\x00'},  # 空字节绕过
   ]
   ```

**预期结果：**
- ✅ 所有尝试都返回 403 错误
- ✅ 无权限绕过漏洞

---

### 3.2 会话安全测试

#### 测试用例 11：并发会话测试

**目的：** 验证会话超时检查在高并发下的正确性

**步骤：**
1. 使用同一用户创建多个并发会话
2. 部分会话保持活跃，部分会话超时
3. 验证超时检查不会影响活跃会话

**验证脚本：**
```python
# test_concurrent_sessions.py
import threading
import time
import requests

def session_worker(session_id, keep_alive):
    token = login()  # 登录获取 token

    if keep_alive:
        # 保持活跃
        for i in range(10):
            requests.get('http://localhost:8443/files', headers={
                'Authorization': f'Bearer {token}'
            })
            time.sleep(5)
    else:
        # 等待超时
        time.sleep(70)

    # 发送最终请求
    response = requests.get('http://localhost:8443/files', headers={
        'Authorization': f'Bearer {token}'
    })

    print(f"会话 {session_id}: 状态码 {response.status_code}")

# 创建并发会话
threads = []
for i in range(5):
    keep_alive = i % 2 == 0
    t = threading.Thread(target=session_worker, args=(i, keep_alive))
    threads.append(t)
    t.start()

for t in threads:
    t.join()
```

---

## 四、性能测试

### 4.1 响应时间测试

#### 测试用例 12：统计查询性能

**目的：** 验证统计查询 API 的性能

**步骤：**
1. 使用 Apache Bench 或 wrk 进行压力测试：
   ```bash
   # 使用 Apache Bench
   ab -n 1000 -c 10 -H "Authorization: Bearer $TOKEN" \
     -p stats_query.json -T application/json \
     http://localhost:8443/api

   # stats_query.json 内容：
   # {"stats_type": "all"}
   ```

2. 记录响应时间分布

**预期结果：**
- ✅ 平均响应时间 < 100ms
- ✅ P95 响应时间 < 200ms
- ✅ 无明显性能瓶颈

---

### 4.2 并发性能测试

#### 测试用例 13：高并发统计查询

**目的：** 验证系统在高并发下的稳定性

**步骤：**
1. 使用 wrk 进行高并发测试：
   ```bash
   wrk -t10 -c100 -d30s \
     -H "Authorization: Bearer $TOKEN" \
     -s stats_query.lua \
     http://localhost:8443/api
   ```
   stats_query.lua:
   ```lua
   wrk.method = "POST"
   wrk.body = '{"stats_type": "all"}'
   wrk.headers["Content-Type"] = "application/json"
   ```

**预期结果：**
- ✅ 系统稳定运行，无崩溃
- ✅ 无内存泄漏
- ✅ CPU 使用率合理

---

## 五、稳定性测试

### 5.1 长时间运行测试

#### 测试用例 14：长时间运行稳定性

**目的：** 验证系统长时间运行的稳定性

**步骤：**
1. 启动 Agent 服务
2. 持续运行 24 小时
3. 定期发送请求（每分钟一次）
4. 监控系统资源使用

**预期结果：**
- ✅ 系统持续稳定运行
- ✅ 无内存泄漏
- ✅ 统计数据持续记录
- ✅ 会话超时正常工作

**监控脚本：**
```bash
# monitor.sh
while true; do
    echo "=== $(date) ==="
    echo "进程信息:"
    ps aux | grep gnome-remote-agent

    echo "内存使用:"
    cat /proc/$(pgrep gnome-remote-agent)/status | grep -E 'VmSize|VmRSS'

    echo "统计查询:"
    curl -s -X POST http://localhost:8443/api \
      -H "Authorization: Bearer $TOKEN" \
      -H "Content-Type: application/json" \
      -d '{"stats_type": "performance"}' | jq '.'

    sleep 60
done
```

---

## 六、回归测试

### 6.1 功能回归测试

**目的：** 确保新功能不影响现有功能

**测试清单：**
- ✅ 文件管理器功能正常（上传、下载、删除、重命名）
- ✅ 终端功能正常（打开、关闭、输入、输出）
- ✅ 系统监控功能正常（CPU、内存、磁盘、网络）
- ✅ 认证功能正常（密码认证、公钥认证）
- ✅ 文件上传下载速度无影响
- ✅ 终端响应速度无影响

---

## 七、测试报告模板

### 7.1 测试执行记录

| 测试用例 | 执行时间 | 结果 | 备注 |
|---------|---------|------|------|
| 1. 正常会话不超时 | 2026-07-28 10:00 | ✅ 通过 | - |
| 2. 会话超时拒绝 | 2026-07-28 10:05 | ✅ 通过 | - |
| 3. 会话超时统计记录 | 2026-07-28 10:10 | ✅ 通过 | - |
| ... | ... | ... | ... |

### 7.2 问题跟踪表

| 问题编号 | 描述 | 严重性 | 状态 | 修复版本 |
|---------|------|--------|------|---------|
| BUG-001 | 示例问题描述 | 中 | 已修复 | v1.2.3 |

---

## 八、自动化测试脚本

### 8.1 完整测试套件

```bash
#!/bin/bash
# run_all_tests.sh

echo "=== 开始执行完整测试套件 ==="

# 功能测试
echo "\n1. 执行功能测试..."
python3 tests/test_session_timeout.py
python3 tests/test_performance_monitoring.py
python3 tests/test_stats_query_api.py

# 安全测试
echo "\n2. 执行安全测试..."
python3 tests/test_permission_boundary.py
python3 tests/test_concurrent_sessions.py

# 性能测试
echo "\n3. 执行性能测试..."
bash tests/performance_test.sh

# 稳定性测试
echo "\n4. 执行稳定性测试..."
bash tests/stability_test.sh &

echo "\n=== 测试套件执行完成 ==="
```

---

## 九、测试数据清理

### 9.1 清理测试环境

```bash
# 删除测试用户
sudo userdel -r testuser1
sudo userdel -r testuser2

# 删除测试文件
rm -rf /tmp/test-files

# 重置统计数据（如果需要）
# 方法1：重启服务
systemctl restart gnome-remote-agent

# 方法2：清空统计数据库（如果使用持久化存储）
# rm -f /var/lib/gnome-remote-agent/stats.db
```

---

## 十、验收标准

### 10.1 必须通过的测试

| 类别 | 测试项 | 通过标准 |
|------|--------|---------|
| 功能 | 会话超时检查 | 100% 通过 |
| 功能 | 性能监控记录 | 100% 通过 |
| 功能 | 统计查询API | 100% 通过 |
| 安全 | 权限控制 | 100% 通过 |
| 性能 | 响应时间 | < 100ms (平均) |
| 稳定性 | 长时间运行 | 无崩溃 |

### 10.2 发布前检查清单

- [ ] 所有测试用例执行完成
- [ ] 无 P0/P1 级别缺陷
- [ ] 性能指标达标
- [ ] 安全审查通过
- [ ] 文档更新完成
- [ ] 代码审查通过
- [ ] 部署文档更新

---

**测试计划文档版本：** 1.0
**最后更新时间：** 2026-07-28
**负责人：** 测试团队