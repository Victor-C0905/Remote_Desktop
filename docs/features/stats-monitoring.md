# 统计监控功能

## 功能概述

系统提供运行时统计监控功能，包括：
- 认证统计（成功/失败/锁定/速率限制）
- 连接统计（活跃连接/历史连接/断开原因）
- 性能指标（响应时间/吞吐量）

## 权限控制

### root用户 (uid=0)
- ✅ 认证统计（全局）
  - 总认证尝试次数
  - 成功次数
  - 失败次数
  - 账户锁定次数
  - IP速率限制触发次数
  - 会话超时次数
  - 密码认证次数
  - 公钥认证次数

- ✅ 连接统计（全局）
  - 当前活跃连接数
  - 历史总连接数
  - 正常断开次数
  - 超时断开次数
  - 错误断开次数

- ✅ 性能指标（全局）
  - API响应时间（p50/p95/p99）
  - 文件传输总字节数
  - 终端输出总字节数

### 普通用户 (uid≠0)
- ❌ 认证统计（不可见）
  - 安全考虑：防止普通用户推测其他用户的登录模式

- ✅ 连接统计（个人）
  - 当前活跃连接数
  - 历史连接数
  - 断开原因分布

- ❌ 性能指标（不可见）
  - 安全考虑：防止普通用户了解系统负载，避免潜在攻击

## 使用方式

### 前端界面
打开 **设置 -> 系统监控** 查看统计面板

### API调用
```typescript
const response = await invoke('get_stats', {
  serverId: 'your-server-id',
  statsType: 'all'  // "auth" | "connection" | "performance" | "all"
});
```

**响应格式：**
```typescript
interface StatsResponse {
  auth?: {
    total_attempts: number;
    successful: number;
    failed: number;
    locked: number;
    rate_limited: number;
    session_timeout: number;
    password_attempts: number;
    pubkey_attempts: number;
  };
  connection: {
    active_connections: number;
    total_connections: number;
    normal_disconnects: number;
    timeout_disconnects: number;
    error_disconnects: number;
  };
  performance?: {
    response_times: {
      p50: number;
      p95: number;
      p99: number;
      min: number;
      max: number;
      count: number;
    };
    total_bytes_transferred: number;
    total_terminal_bytes: number;
  };
}
```

## 数据刷新
- 自动刷新：每30秒
- 手动刷新：点击刷新按钮

## 安全考虑
- 普通用户无法查看其他用户的登录模式
- 普通用户无法了解系统负载
- 防止通过统计数据推测系统弱点
- 所有统计查询都经过权限验证

## 实现细节

### 后端（Agent）
- **文件：** `agent/src/auth/stats.rs`
- **统计管理器：** `StatsManager` 负责收集和存储统计数据
- **权限检查：** `check_permission()` 方法验证用户权限
- **数据过滤：** `get_connection_stats_for_user()` 方法根据用户权限过滤数据

### 前端（Client）
- **文件：** `src/components/StatsPanel.tsx`
- **组件：** React 组件，使用 Tauri API 获取统计数据
- **样式：** `src/components/StatsPanel.css` 提供响应式布局和状态颜色

### 协议（Protocol）
- **文件：** `agent/src/protocol.rs`
- **Payload类型：**
  - `GetStats { stats_type: String }` - 统计查询请求
  - `StatsResponse { auth, connection, performance }` - 统计查询响应

## 故障排查

### 问题：统计面板无法加载数据
**可能原因：**
1. 后端服务未启动
2. 网络连接中断
3. 用户权限不足

**解决方案：**
1. 检查 Agent 服务状态：`systemctl status gnome-remote-agent`
2. 检查网络连接
3. 确认用户登录状态

### 问题：普通用户看到空数据
**这是正常现象：**
- 当前实现中，普通用户的个人连接统计为空（需要后续实现追踪每个用户的连接数）
- 认证统计和性能指标对普通用户不可见

## 未来增强
- 持久化统计数据到数据库
- 添加历史趋势图表
- 实现普通用户的个人连接统计
- 添加更多统计类型（如文件操作统计）
- 集成告警系统（如连接数异常、认证失败率过高自动告警）