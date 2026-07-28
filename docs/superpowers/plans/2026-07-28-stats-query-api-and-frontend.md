# 统计查询API和前端展示实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现统计查询API和前端展示面板，让root用户可以查看全局统计，普通用户可以查看个人连接统计。

**Architecture:** 采用分层架构：后端提供统计查询API（基于用户权限过滤数据），前端根据用户类型显示不同内容。root用户可查看认证统计、全局连接统计和性能指标；普通用户仅可查看个人连接统计。

**Tech Stack:** Rust (Agent), TypeScript/React (Client), JSON-RPC over QUIC

---

## 文件结构

**后端（Agent）：**
- `agent/src/protocol.rs` - 添加GetStats/StatsResponse Payload类型
- `agent/src/auth/stats.rs` - 添加权限检查方法（已存在）
- `agent/src/handler.rs` - 添加统计查询处理逻辑（修改）
- `agent/src/server/quic.rs` - 传递stats_manager到handle_stream（修改）

**前端（Client）：**
- `src/components/StatsPanel.tsx` - 统计展示面板（新建）
- `src/hooks/useStats.ts` - 统计数据获取hook（新建）
- `src/apps/Settings.tsx` - 集成统计面板到设置页面（修改）

---

## Task 1: 添加统计查询Payload类型

**Files:**
- Modify: `agent/src/protocol.rs`

- [ ] **Step 1: 添加GetStats Payload类型**

在Payload枚举中添加请求类型：

```rust
/// 统计查询请求
#[serde(rename = "get_stats")]
GetStats {
    /// 统计类型: "auth" | "connection" | "performance" | "all"
    stats_type: String,
},

/// 统计查询响应
#[serde(rename = "stats_response")]
StatsResponse {
    /// 认证统计（仅root可见）
    auth: Option<AuthStatsSnapshot>,
    /// 连接统计（root看全局，普通用户看个人）
    connection: ConnectionStatsSnapshot,
    /// 性能指标（仅root可见）
    performance: Option<PerformanceStatsSnapshot>,
},
```

- [ ] **Step 2: 导入统计快照类型**

在protocol.rs顶部添加导入：

```rust
use crate::auth::stats::{AuthStatsSnapshot, ConnectionStatsSnapshot, PerformanceStatsSnapshot};
```

- [ ] **Step 3: 运行编译验证**

Run: `cargo check --manifest-path agent/Cargo.toml`

Expected: 编译成功，可能有一些警告（未使用的类型）

- [ ] **Step 4: 提交变更**

```bash
git add agent/src/protocol.rs
git commit -m "feat(protocol): add GetStats and StatsResponse payload types"
```

---

## Task 2: 添加权限检查方法

**Files:**
- Modify: `agent/src/auth/stats.rs`

- [ ] **Step 1: 添加权限检查方法**

在StatsManager impl中添加方法：

```rust
/// 检查用户是否有权限查看指定统计类型
///
/// # 参数
/// - `session`: 用户会话信息
/// - `stats_type`: 统计类型 ("auth" | "connection" | "performance" | "all")
///
/// # 返回
/// - `Ok(true)`: 有权限
/// - `Ok(false)`: 无权限
pub fn check_permission(&self, session: &UserSession, stats_type: &str) -> bool {
    match stats_type {
        "auth" | "performance" => {
            // 只有 root 用户可以查看认证统计和性能指标
            session.uid == 0
        }
        "connection" | "all" => {
            // root用户可以查看，普通用户也可以查看（但数据会过滤）
            true
        }
        _ => false,
    }
}

/// 获取连接统计（根据用户权限过滤）
///
/// # 参数
/// - `session`: 用户会话信息
///
/// # 返回
/// - root用户: 返回全局统计
/// - 普通用户: 返回个人统计（TODO: 未来实现）
pub fn get_connection_stats_for_user(&self, session: &UserSession) -> ConnectionStatsSnapshot {
    if session.uid == 0 {
        // root用户：返回全局统计
        self.get_connection_stats()
    } else {
        // 普通用户：返回个人统计（当前实现为空数据）
        // TODO: 未来需要追踪每个用户的连接数
        ConnectionStatsSnapshot {
            active_connections: 0,
            total_connections: 0,
            normal_disconnects: 0,
            timeout_disconnects: 0,
            error_disconnects: 0,
        }
    }
}
```

- [ ] **Step 2: 运行编译验证**

Run: `cargo check --manifest-path agent/Cargo.toml`

Expected: 编译成功

- [ ] **Step 3: 提交变更**

```bash
git add agent/src/auth/stats.rs
git commit -m "feat(stats): add permission check methods"
```

---

## Task 3: 添加统计查询处理逻辑

**Files:**
- Modify: `agent/src/handler.rs`
- Modify: `agent/src/server/quic.rs`

- [ ] **Step 1: 在handler.rs中添加统计查询处理**

在handle_stream函数的match envelope.payload分支中添加：

```rust
// 统计查询请求
Payload::GetStats { stats_type } => {
    tracing::info!("统计查询请求: stats_type={}, user={}", stats_type, session.username);

    // 检查权限
    if !stats_manager.check_permission(session, &stats_type) {
        tracing::warn!("权限不足: user={}, stats_type={}", session.username, stats_type);

        let response = Envelope::new(
            envelope.request_id,
            Payload::StatsResponse {
                auth: None,
                connection: ConnectionStatsSnapshot::default(),
                performance: None,
            },
        );

        match response.encode() {
            Ok(resp_bytes) => {
                if let Err(e) = write_message(&mut send, &resp_bytes).await {
                    tracing::warn!("发送响应失败: {}", e);
                }
            }
            Err(e) => tracing::warn!("编码响应失败: {}", e),
        }
        return Ok(());
    }

    // 获取统计数据
    let auth_stats = if stats_type == "auth" || stats_type == "all" {
        if session.uid == 0 {
            Some(stats_manager.get_auth_stats())
        } else {
            None
        }
    } else {
        None
    };

    let connection_stats = if stats_type == "connection" || stats_type == "all" {
        stats_manager.get_connection_stats_for_user(session)
    } else {
        ConnectionStatsSnapshot::default()
    };

    let performance_stats = if stats_type == "performance" || stats_type == "all" {
        if session.uid == 0 {
            Some(stats_manager.get_performance_stats().await)
        } else {
            None
        }
    } else {
        None
    };

    // 发送响应
    let response = Envelope::new(
        envelope.request_id,
        Payload::StatsResponse {
            auth: auth_stats,
            connection: connection_stats,
            performance: performance_stats,
        },
    );

    match response.encode() {
        Ok(resp_bytes) => {
            if let Err(e) = write_message(&mut send, &resp_bytes).await {
                tracing::warn!("发送响应失败: {}", e);
            }
        }
        Err(e) => tracing::warn!("编码响应失败: {}", e),
    }
}
```

- [ ] **Step 2: 修改handle_stream函数签名**

添加stats_manager参数：

```rust
async fn handle_stream(
    stream: (SendStream, RecvStream),
    cfg: &AgentConfig,
    subscription_manager: Arc<SubscriptionManager>,
    event_bus: Arc<EventBus>,
    #[cfg(unix)] pty_manager: Arc<PtyManager>,
    #[cfg(not(unix))] _pty_manager: Arc<PtyManager>,
    ctx: Arc<ConnectionContext>,
    session: &UserSession,
    stats_manager: Arc<StatsManager>,  // 新增参数
) -> Result<()>
```

- [ ] **Step 3: 在quic.rs中传递stats_manager**

在主循环中修改handle_stream调用：

```rust
let stats_manager_inner = stats_manager.clone();
tokio::spawn(async move {
    if let Err(e) = handle_stream(
        stream,
        &cfg_inner,
        subscription_manager_inner,
        event_bus_inner,
        pty_manager_inner,
        ctx_inner,
        &session_inner,
        stats_manager_inner,  // 新增参数
    ).await {
        tracing::warn!("QUIC Stream 处理错误: {}", e);
    }
});
```

- [ ] **Step 4: 添加必要的导入**

在handler.rs顶部添加：

```rust
use crate::auth::stats::{StatsManager, AuthStatsSnapshot, ConnectionStatsSnapshot, PerformanceStatsSnapshot};
use std::sync::Arc;
```

- [ ] **Step 5: 运行编译验证**

Run: `cargo check --manifest-path agent/Cargo.toml`

Expected: 编译成功

- [ ] **Step 6: 提交变更**

```bash
git add agent/src/handler.rs agent/src/server/quic.rs
git commit -m "feat(handler): add stats query handler"
```

---

## Task 4: 创建前端统计面板组件

**Files:**
- Create: `src/components/StatsPanel.tsx`

- [ ] **Step 1: 创建StatsPanel组件**

```typescript
import React, { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/tauri';
import './StatsPanel.css';

interface AuthStats {
  total_attempts: number;
  successful: number;
  failed: number;
  locked: number;
  rate_limited: number;
  session_timeout: number;
  password_attempts: number;
  pubkey_attempts: number;
}

interface ConnectionStats {
  active_connections: number;
  total_connections: number;
  normal_disconnects: number;
  timeout_disconnects: number;
  error_disconnects: number;
}

interface PerformanceStats {
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
}

interface StatsResponse {
  auth?: AuthStats;
  connection: ConnectionStats;
  performance?: PerformanceStats;
}

interface StatsPanelProps {
  serverId: string;
  isRoot: boolean;
}

export const StatsPanel: React.FC<StatsPanelProps> = ({ serverId, isRoot }) => {
  const [stats, setStats] = useState<StatsResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const fetchStats = async () => {
    setLoading(true);
    setError(null);
    try {
      const response = await invoke<StatsResponse>('get_stats', {
        serverId,
        statsType: 'all',
      });
      setStats(response);
    } catch (err) {
      setError(err instanceof Error ? err.message : '获取统计失败');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchStats();
    const interval = setInterval(fetchStats, 30000); // 每30秒刷新
    return () => clearInterval(interval);
  }, [serverId]);

  if (loading) {
    return <div className="stats-panel">加载中...</div>;
  }

  if (error) {
    return (
      <div className="stats-panel">
        <div className="error">{error}</div>
        <button onClick={fetchStats}>重试</button>
      </div>
    );
  }

  if (!stats) {
    return null;
  }

  return (
    <div className="stats-panel">
      <h2>📊 系统监控</h2>

      {/* 认证统计（仅root可见） */}
      {isRoot && stats.auth && (
        <div className="stats-section">
          <h3>认证统计</h3>
          <div className="stats-grid">
            <div className="stat-item">
              <span className="label">总尝试次数</span>
              <span className="value">{stats.auth.total_attempts}</span>
            </div>
            <div className="stat-item">
              <span className="label">成功次数</span>
              <span className="value success">{stats.auth.successful}</span>
            </div>
            <div className="stat-item">
              <span className="label">失败次数</span>
              <span className="value error">{stats.auth.failed}</span>
            </div>
            <div className="stat-item">
              <span className="label">锁定次数</span>
              <span className="value warning">{stats.auth.locked}</span>
            </div>
            <div className="stat-item">
              <span className="label">速率限制</span>
              <span className="value">{stats.auth.rate_limited}</span>
            </div>
            <div className="stat-item">
              <span className="label">会话超时</span>
              <span className="value">{stats.auth.session_timeout}</span>
            </div>
          </div>
        </div>
      )}

      {/* 连接统计（所有用户可见） */}
      <div className="stats-section">
        <h3>连接统计</h3>
        <div className="stats-grid">
          <div className="stat-item">
            <span className="label">活跃连接</span>
            <span className="value">{stats.connection.active_connections}</span>
          </div>
          <div className="stat-item">
            <span className="label">总连接数</span>
            <span className="value">{stats.connection.total_connections}</span>
          </div>
          <div className="stat-item">
            <span className="label">正常断开</span>
            <span className="value success">{stats.connection.normal_disconnects}</span>
          </div>
          <div className="stat-item">
            <span className="label">超时断开</span>
            <span className="value warning">{stats.connection.timeout_disconnects}</span>
          </div>
          <div className="stat-item">
            <span className="label">错误断开</span>
            <span className="value error">{stats.connection.error_disconnects}</span>
          </div>
        </div>
      </div>

      {/* 性能指标（仅root可见） */}
      {isRoot && stats.performance && (
        <div className="stats-section">
          <h3>性能指标</h3>
          <div className="stats-grid">
            <div className="stat-item">
              <span className="label">响应时间(p50)</span>
              <span className="value">{stats.performance.response_times.p50}ms</span>
            </div>
            <div className="stat-item">
              <span className="label">响应时间(p95)</span>
              <span className="value">{stats.performance.response_times.p95}ms</span>
            </div>
            <div className="stat-item">
              <span className="label">响应时间(p99)</span>
              <span className="value">{stats.performance.response_times.p99}ms</span>
            </div>
            <div className="stat-item">
              <span className="label">文件传输</span>
              <span className="value">{formatBytes(stats.performance.total_bytes_transferred)}</span>
            </div>
            <div className="stat-item">
              <span className="label">终端输出</span>
              <span className="value">{formatBytes(stats.performance.total_terminal_bytes)}</span>
            </div>
          </div>
        </div>
      )}

      <button onClick={fetchStats} className="refresh-button">
        刷新数据
      </button>
    </div>
  );
};

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
}
```

- [ ] **Step 2: 创建StatsPanel.css样式文件**

```css
.stats-panel {
  padding: 20px;
  max-width: 800px;
  margin: 0 auto;
}

.stats-section {
  margin-bottom: 30px;
}

.stats-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
  gap: 15px;
}

.stat-item {
  display: flex;
  flex-direction: column;
  padding: 15px;
  background: var(--background-color);
  border-radius: 8px;
  border: 1px solid var(--border-color);
}

.stat-item .label {
  font-size: 12px;
  color: var(--text-color-secondary);
  margin-bottom: 5px;
}

.stat-item .value {
  font-size: 24px;
  font-weight: bold;
}

.value.success {
  color: var(--success-color);
}

.value.warning {
  color: var(--warning-color);
}

.value.error {
  color: var(--error-color);
}

.refresh-button {
  padding: 10px 20px;
  background: var(--accent-color);
  color: white;
  border: none;
  border-radius: 4px;
  cursor: pointer;
}

.refresh-button:hover {
  background: var(--accent-color-hover);
}

.error {
  color: var(--error-color);
  padding: 10px;
  background: var(--error-background);
  border-radius: 4px;
  margin-bottom: 10px;
}
```

- [ ] **Step 3: 提交变更**

```bash
git add src/components/StatsPanel.tsx src/components/StatsPanel.css
git commit -m "feat(client): add StatsPanel component"
```

---

## Task 5: 集成统计面板到设置页面

**Files:**
- Modify: `src/apps/Settings.tsx`

- [ ] **Step 1: 在Settings.tsx中添加统计面板Tab**

```typescript
import { StatsPanel } from '../components/StatsPanel';
import { useServersStore } from '../stores/serversStore';

// 在Settings组件中添加新的Tab
const [activeTab, setActiveTab] = useState('connection');

const tabs = [
  { id: 'connection', label: '连接配置' },
  { id: 'appearance', label: '外观设置' },
  { id: 'security', label: '安全设置' },
  { id: 'stats', label: '系统监控' },  // 新增
];

// 在渲染部分添加
{activeTab === 'stats' && (
  <div className="tab-content">
    <StatsPanel
      serverId={currentServer?.id || ''}
      isRoot={currentServer?.user?.uid === 0}
    />
  </div>
)}
```

- [ ] **Step 2: 提交变更**

```bash
git add src/apps/Settings.tsx
git commit -m "feat(settings): integrate StatsPanel into settings page"
```

---

## Task 6: 端到端测试

**Files:**
- Test: 手动测试

- [ ] **Step 1: 启动Agent服务**

Run: `cargo run --manifest-path agent/Cargo.toml --release`

Expected: 服务成功启动，监听QUIC端口

- [ ] **Step 2: 启动Client前端**

Run: `npm run tauri dev`

Expected: 前端成功启动，显示登录界面

- [ ] **Step 3: 测试root用户统计**

1. 使用root账户登录
2. 打开设置 -> 系统监控
3. 验证：应显示认证统计、连接统计、性能指标
4. 验证：数据每30秒自动刷新
5. 点击刷新按钮，验证数据手动刷新

Expected: 所有统计数据显示正确

- [ ] **Step 4: 测试普通用户统计**

1. 使用普通用户账户登录
2. 打开设置 -> 系统监控
3. 验证：应仅显示连接统计（个人数据）
4. 验证：认证统计和性能指标不显示

Expected: 仅显示连接统计，无认证和性能数据

- [ ] **Step 5: 测试权限控制**

1. 使用普通用户账户登录
2. 尝试手动调用get_stats API（stats_type="auth"）
3. 验证：返回空数据或错误提示

Expected: 权限控制生效，普通用户无法获取敏感数据

- [ ] **Step 6: 提交测试报告**

记录测试结果到文档：

```markdown
## 测试报告

### root用户测试
- ✅ 认证统计显示正确
- ✅ 连接统计显示正确
- ✅ 性能指标显示正确
- ✅ 自动刷新功能正常
- ✅ 手动刷新功能正常

### 普通用户测试
- ✅ 仅显示连接统计
- ✅ 认证统计不显示
- ✅ 性能指标不显示
- ✅ 权限控制生效

### 问题记录
（如有）
```

---

## Task 7: 文档更新

**Files:**
- Modify: `README.md`
- Create: `docs/features/stats-monitoring.md`

- [ ] **Step 1: 创建功能文档**

创建 `docs/features/stats-monitoring.md`：

```markdown
# 统计监控功能

## 功能概述

系统提供运行时统计监控功能，包括：
- 认证统计（成功/失败/锁定/速率限制）
- 连接统计（活跃连接/历史连接/断开原因）
- 性能指标（响应时间/吞吐量）

## 权限控制

### root用户 (uid=0)
- ✅ 认证统计（全局）
- ✅ 连接统计（全局）
- ✅ 性能指标（全局）

### 普通用户 (uid≠0)
- ❌ 认证统计（不可见）
- ✅ 连接统计（个人）
- ❌ 性能指标（不可见）

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

## 数据刷新
- 自动刷新：每30秒
- 手动刷新：点击刷新按钮

## 安全考虑
- 普通用户无法查看其他用户的登录模式
- 普通用户无法了解系统负载
- 防止通过统计数据推测系统弱点
```

- [ ] **Step 2: 更新README.md**

在功能列表中添加：

```markdown
### 系统监控
- 认证统计（成功/失败/锁定/速率限制）
- 连接统计（活跃连接/历史连接/断开原因）
- 性能指标（响应时间/吞吐量）
- 权限控制（root用户看全局，普通用户看个人）
```

- [ ] **Step 3: 提交文档**

```bash
git add README.md docs/features/stats-monitoring.md
git commit -m "docs: add stats monitoring documentation"
```

---

## 完成检查清单

执行完所有任务后，验证以下内容：

- [ ] 编译成功（零错误）
- [ ] root用户可以查看所有统计数据
- [ ] 普通用户只能查看个人连接统计
- [ ] 统计数据每30秒自动刷新
- [ ] 手动刷新按钮工作正常
- [ ] 权限控制正确生效
- [ ] 文档已更新

---

## 注意事项

1. **性能考虑**：统计查询API应轻量级，避免频繁查询影响性能
2. **安全考虑**：严格验证用户权限，防止信息泄露
3. **扩展性**：未来可以添加更多统计类型（如文件操作统计）
4. **数据保留**：当前实现不持久化统计数据，未来可以考虑添加历史数据存储