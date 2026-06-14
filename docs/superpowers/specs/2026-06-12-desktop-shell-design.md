# GNOME 远程控制客户端 - 桌面外壳设计文档

> 版本: v1.0 | 日期: 2026-06-12
>
> 目标: 实现完整的 GNOME 桌面外壳,支持远程 Ubuntu 服务器连接和数据集成

---

## 一、项目概述

### 1.1 目标

实现 GNOME 远程控制客户端的第一阶段:桌面外壳基础(项目规范阶段 0)。

**核心功能:**
- TopBar: 显示连接状态、远程时间、系统指标、通知
- Desktop: 应用图标网格,双击打开应用
- Dock: 底部应用栏,点击打开应用
- Overview: 全屏活动概览,Super 键触发
- 连接管理: 单服务器连接,QUIC/WebSocket 协议
- 远程数据集成: 服务器时间、系统指标(CPU、内存、磁盘、网络)

### 1.2 开发策略

**客户端和 Agent 分步开发:**
- 通过 API 文档作为契约,前后端独立开发
- Agent 先实现协议,客户端根据 API 文档开发
- 最终集成测试验证是否符合 API 文档

---

## 二、整体架构

### 2.1 系统架构

```
客户端 (Tauri + React)
    ├── ShellContext (全局状态管理)
    │   ├── connectionState (连接状态)
    │   ├── remoteTime (远程时间)
    │   ├── metrics (系统指标)
    │   ├── appWindows (应用窗口列表)
    │   └── desktopState (桌面状态)
    │
    ├── Shell Components
    │   ├── TopBar (顶部面板)
    │   ├── Desktop (桌面区域)
    │   ├── Dock (底部应用栏)
    │   └── Overview (活动概览)
    │
    └── Tauri Backend
        ├── ConnectionManager (连接管理)
        ├── WindowManager (窗口管理)
        └── Event System (事件监听)

Agent (Rust + Quinn)
    ├── QUIC Server (监听 UDP 8443)
    ├── WebSocket Server (监听 WSS /ws)
    ├── Protocol Handler (消息处理)
    ├── File System Operations (文件操作)
    ├── PTY Manager (终端管理)
    └── Metrics Collector (系统指标采集)
```

### 2.2 数据流

```
用户操作 → ShellContext 更新 → 组件重新渲染
    ↓
定时器(每 1s) → invoke("remote_ping") → 更新 remoteTime + rtt_ms
    ↓
定时器(每 2s) → invoke("remote_get_metrics") → 更新 metrics
    ↓
ConnectionManager → QUIC/WebSocket → Agent → 返回数据
    ↓
Agent → 文件系统/PTY/系统指标 → 返回结果
```

---

## 三、API 文档(协议定义)

### 3.1 消息协议

所有消息使用 JSON 格式,通过 QUIC Stream 或 WebSocket 传输。

**消息结构:**

```json
{
  "request_id": 123,
  "payload": {
    "type": "<消息类型>",
    "data": { <消息数据> }
  }
}
```

### 3.2 心跳协议

**Ping (客户端 → Agent):**

```json
{
  "request_id": 0,
  "payload": {
    "type": "ping",
    "data": {
      "timestamp": 1718234567890
    }
  }
}
```

**Pong (Agent → 客户端):**

```json
{
  "request_id": 0,
  "payload": {
    "type": "pong",
    "data": {
      "timestamp": 1718234567890,
      "server_time": 1718234567895
    }
  }
}
```

**用途:**
- 获取远程服务器时间(server_time)
- 测量连接延迟(RTT = 客户端接收时间 - timestamp)
- 保持连接活跃(心跳检测)

---

### 3.3 认证协议

**AuthRequest (客户端 → Agent):**

```json
{
  "request_id": 1,
  "payload": {
    "type": "auth_request",
    "data": {
      "token": "gmr_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
    }
  }
}
```

**AuthResponse (Agent → 客户端):**

```json
{
  "request_id": 1,
  "payload": {
    "type": "auth_response",
    "data": {
      "success": true,
      "error": null
    }
  }
}
```

**用途:**
- 验证客户端身份
- Token 格式: `gmr_<32位随机字符串>`
- 认证失败返回 `success: false` + `error: "Token 无效"`

---

### 3.4 系统指标协议

**MetricsSubscribeRequest (客户端 → Agent):**

```json
{
  "request_id": 2,
  "payload": {
    "type": "metrics_subscribe",
    "data": {}
  }
}
```

**MetricsData (Agent → 客户端):**

```json
{
  "request_id": 2,
  "payload": {
    "type": "metrics_data",
    "data": {
      "cpu_percent": 23.5,
      "mem_used_bytes": 4026531840,
      "mem_total_bytes": 8589934592,
      "swap_used_bytes": 0,
      "disks": [
        {
          "mount_point": "/",
          "total_bytes": 107374182400,
          "used_bytes": 72477573120
        },
        {
          "mount_point": "/var",
          "total_bytes": 128849018880,
          "used_bytes": 41231686604
        }
      ],
      "network_rx_bytes": 1234567,
      "network_tx_bytes": 340567,
      "uptime_secs": 86400
    }
  }
}
```

**用途:**
- 获取远程服务器实时系统指标
- CPU 使用率百分比
- 内存使用情况(字节)
- 磁盘使用情况(挂载点、总大小、已用大小)
- 网络流量(接收/发送字节)
- 系统运行时长(秒)

---

### 3.5 文件操作协议

**ReadDirRequest (客户端 → Agent):**

```json
{
  "request_id": 3,
  "payload": {
    "type": "read_dir",
    "data": {
      "path": "/home/user"
    }
  }
}
```

**ReadDirResponse (Agent → 客户端):**

```json
{
  "request_id": 3,
  "payload": {
    "type": "read_dir_resp",
    "data": {
      "path": "/home/user",
      "entries": [
        {
          "name": "Documents",
          "is_dir": true,
          "size": 0,
          "mtime": "2026-05-28T09:30:00Z",
          "permissions": "rwxr-xr-x"
        },
        {
          "name": ".bashrc",
          "is_dir": false,
          "size": 3771,
          "mtime": "2026-04-10T08:30:00Z",
          "permissions": "rw-r--r--"
        }
      ]
    }
  }
}
```

**用途:**
- 浏览远程服务器文件系统
- 返回文件列表(名称、类型、大小、修改时间、权限)

---

### 3.6 错误协议

**Error (Agent → 客户端):**

```json
{
  "request_id": 3,
  "payload": {
    "type": "error",
    "data": {
      "code": -1,
      "message": "访问被拒绝: 不在允许的路径列表中 (/root)"
    }
  }
}
```

**用途:**
- 返回操作失败信息
- 错误码: -1(通用错误), -2(权限错误), -3(文件不存在)

---

### 3.7 协议实现要求

**Agent 实现:**
- 必须支持所有协议消息类型
- 必须验证 Token(认证协议)
- 必须限制文件访问路径(allowed_paths 配置)
- 必须每 2 秒采集系统指标(MetricsSnapshot)
- 必须返回错误信息(Error 协议)

**客户端实现:**
- 必须处理所有协议响应类型
- 必须处理错误响应(Error 协议)
- 必须实现自动重连机制(连接断开时)
- 必须实现静默重试机制(网络波动时)

---

## 四、客户端实现方案

### 4.1 ShellContext 设计

**状态结构:**

```typescript
interface ShellState {
  // 连接状态
  connection: {
    host: string;
    port: number;
    status: 'connected' | 'disconnected' | 'connecting';
    rtt_ms: number;
    transport: 'quic' | 'websocket';
    connectedAt: number;
  } | null;

  // 远程数据
  remoteTime: Date | null;
  metrics: MetricsSnapshot | null;

  // 桌面状态
  overviewVisible: boolean;
  activeApp: AppType | null;
  appWindows: AppWindow[];
}

interface MetricsSnapshot {
  cpu_percent: number;
  mem_used_bytes: number;
  mem_total_bytes: number;
  swap_used_bytes: number;
  disks: DiskInfo[];
  network_rx_bytes: number;
  network_tx_bytes: number;
  uptime_secs: number;
}

interface AppWindow {
  id: string;
  type: AppType;
  label: string;
  tauriWindow: string;
  isActive: boolean;
}

type AppType = 'file-manager' | 'terminal' | 'system-monitor' | 'settings';
```

**Context Provider:**

```typescript
// src/context/ShellContext.tsx
export function ShellProvider({ children }: { children: React.ReactNode }) {
  const [state, setState] = useState<ShellState>({
    connection: null,
    remoteTime: null,
    metrics: null,
    overviewVisible: false,
    activeApp: null,
    appWindows: [],
  });

  // 定时获取远程数据
  useEffect(() => {
    if (!state.connection) return;

    const timeInterval = setInterval(async () => {
      try {
        const pingResult = await invoke('remote_ping', { serverId: 'default' });
        setState(prev => ({
          ...prev,
          remoteTime: new Date(pingResult.server_time),
          connection: prev.connection ? {
            ...prev.connection,
            rtt_ms: pingResult.latency_ms,
          } : null,
        }));
      } catch (error) {
        // 静默重试,不显示错误
      }
    }, 1000);

    const metricsInterval = setInterval(async () => {
      try {
        const metrics = await invoke('remote_get_metrics', { serverId: 'default' });
        setState(prev => ({ ...prev, metrics }));
      } catch (error) {
        // 静默重试,不显示错误
      }
    }, 2000);

    return () => {
      clearInterval(timeInterval);
      clearInterval(metricsInterval);
    };
  }, [state.connection]);

  return (
    <ShellContext.Provider value={{ state, setState }}>
      {children}
    </ShellContext.Provider>
  );
}
```

---

### 4.2 TopBar 组件

**设计规范:**
- 高度: 32px
- 背景: `var(--headerbar-bg)`
- 显示: 连接状态、远程时间、系统指标、通知

**组件实现:**

```tsx
// src/shell/TopBar.tsx
export function TopBar() {
  const { state, setState } = useShell();
  const { isReconnecting } = useAutoReconnect();

  const getStatusIcon = () => {
    if (!state.connection) return '⚫';
    if (isReconnecting) return '🔄';
    if (state.connection.status === 'connected') return '🟢';
    return '⚫';
  };

  return (
    <div className="top-bar" style={{
      height: 32,
      background: 'var(--headerbar-bg)',
      borderBottom: '1px solid var(--border-color)',
      display: 'flex',
      alignItems: 'center',
      padding: '0 12px',
      gap: 16,
    }}>
      {/* 活动按钮 */}
      <Button flat onClick={() => setState(prev => ({ ...prev, overviewVisible: true }))}>
        活动
      </Button>

      {/* 连接状态 */}
      <span>{getStatusIcon()}</span>
      <span>{state.connection?.host || '未连接'}</span>

      {/* 系统指标 */}
      {state.metrics && (
        <>
          <span>CPU {state.metrics.cpu_percent.toFixed(1)}%</span>
          <span>MEM {(state.metrics.mem_used_bytes / 1024**3).toFixed(1)}G</span>
        </>
      )}

      {/* 远程时钟 */}
      {state.remoteTime && <RemoteClock time={state.remoteTime} />}

      {/* 通知 */}
      <NotificationButton />
    </div>
  );
}
```

---

### 4.3 Desktop 组件

**设计规范:**
- 背景: Adwaita 蓝色 (#3584e4)
- 应用图标网格: 4x2 布局
- 双击打开应用窗口

**组件实现:**

```tsx
// src/shell/Desktop.tsx
export function Desktop() {
  const { setState } = useShell();

  const apps: AppDefinition[] = [
    { type: 'file-manager', icon: '📁', label: '文件' },
    { type: 'terminal', icon: '🖥️', label: '终端' },
    { type: 'system-monitor', icon: '📊', label: '监控' },
    { type: 'settings', icon: '⚙️', label: '设置' },
  ];

  const openApp = async (appType: AppType) => {
    const windowLabel = `${appType}-${Date.now()}`;
    await invoke('create_app_window', {
      label: windowLabel,
      appType,
      url: `/app/${appType}`,
    });

    setState(prev => ({
      ...prev,
      appWindows: [...prev.appWindows, {
        id: windowLabel,
        type: appType,
        label: apps.find(a => a.type === appType)?.label || appType,
        tauriWindow: windowLabel,
        isActive: true,
      }],
    }));
  };

  return (
    <div className="desktop" style={{
      flex: 1,
      background: 'var(--accent-bg)',
      display: 'flex',
      justifyContent: 'center',
      alignItems: 'center',
      padding: 48,
    }}>
      <div className="app-grid" style={{
        display: 'grid',
        gridTemplateColumns: 'repeat(4, 96px)',
        gridTemplateRows: 'repeat(2, 96px)',
        gap: 24,
      }}>
        {apps.map(app => (
          <div key={app.type} className="app-icon" onDoubleClick={() => openApp(app.type)}>
            <div className="icon-image">{app.icon}</div>
            <div className="icon-label">{app.label}</div>
          </div>
        ))}
      </div>
    </div>
  );
}
```

---

### 4.4 Dock 组件

**设计规范:**
- 高度: 64px
- 背景: `var(--card-bg)`
- 圆角: `var(--radius-lg)` (18px)
- 点击打开应用

**组件实现:**

```tsx
// src/shell/Dock.tsx
export function Dock() {
  const { setState } = useShell();

  const dockApps: AppDefinition[] = [
    { type: 'file-manager', icon: '📁', label: '文件' },
    { type: 'terminal', icon: '🖥️', label: '终端' },
    { type: 'system-monitor', icon: '📊', label: '监控' },
    { type: 'settings', icon: '⚙️', label: '设置' },
    { type: 'all-apps', icon: '⋮', label: '所有应用' },
  ];

  const openApp = async (appType: AppType | 'all-apps') => {
    if (appType === 'all-apps') {
      setState(prev => ({ ...prev, overviewVisible: true }));
    } else {
      // 同 Desktop 的 openApp 逻辑
    }
  };

  return (
    <div className="dock" style={{
      height: 64,
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'center',
      padding: '8px 12px',
      background: 'var(--card-bg)',
      borderRadius: 'var(--radius-lg)',
      border: '1px solid var(--border-color)',
      gap: 4,
    }}>
      {dockApps.map(app => (
        <div key={app.type} className="dock-item" onClick={() => openApp(app.type)}>
          <div className="dock-icon">{app.icon}</div>
          <div className="dock-label">{app.label}</div>
        </div>
      ))}
    </div>
  );
}
```

---

### 4.5 Overview 组件

**设计规范:**
- 全屏覆盖,z-index 最高
- 半透明背景模糊
- Super 键触发,Esc 退出

**组件实现:**

```tsx
// src/shell/Overview.tsx
export function Overview() {
  const { state, setState } = useShell();
  const [searchQuery, setSearchQuery] = useState('');

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Super') {
        setState(prev => ({ ...prev, overviewVisible: !prev.overviewVisible }));
      }
      if (e.key === 'Escape' && state.overviewVisible) {
        setState(prev => ({ ...prev, overviewVisible: false }));
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [state.overviewVisible]);

  if (!state.overviewVisible) return null;

  return (
    <div className="overview" style={{
      position: 'fixed',
      top: 0, left: 0, right: 0, bottom: 0,
      zIndex: 9999,
      background: 'rgba(0, 0, 0, 0.5)',
      backdropFilter: 'blur(10px)',
      display: 'flex',
      flexDirection: 'column',
      padding: 32,
    }}>
      {/* 搜索栏 */}
      <input type="text" placeholder="搜索..." value={searchQuery} />

      {/* 窗口网格 */}
      <div className="window-grid">
        {state.appWindows.map(window => (
          <div key={window.id} className="window-thumbnail">
            <div className="window-title">{window.label}</div>
          </div>
        ))}
      </div>

      {/* Dock */}
      <Dock />
    </div>
  );
}
```

---

### 4.6 自动重连机制

**实现策略:**
- 首次连接失败 → 显示错误提示
- 后续网络波动 → 完全静默处理
- 真正无法连接 → 温和提示 "重新连接中..."

**实现代码:**

```typescript
// src/hooks/useAutoReconnect.ts
export function useAutoReconnect() {
  const { state, setState } = useShell();
  const [reconnectAttempts, setReconnectAttempts] = useState(0);
  const [isReconnecting, setIsReconnecting] = useState(false);

  const MAX_RECONNECT_ATTEMPTS = 10;
  const RECONNECT_INTERVALS = [0, 1, 2, 5, 10, 15, 30, 60, 120, 300];

  const attemptReconnect = async () => {
    if (reconnectAttempts >= MAX_RECONNECT_ATTEMPTS) {
      setState(prev => ({
        ...prev,
        connection: prev.connection ? {
          ...prev.connection,
          status: 'disconnected',
        } : null,
      }));
      return;
    }

    setIsReconnecting(true);

    try {
      const result = await invoke('remote_connect', {
        serverId: 'default',
        host: state.connection?.host || '',
        port: state.connection?.port || 8443,
        token: localStorage.getItem('auth_token'),
      });

      setState(prev => ({ ...prev, connection: result }));
      setReconnectAttempts(0);
      setIsReconnecting(false);

    } catch (error) {
      setReconnectAttempts(prev => prev + 1);
      const delay = RECONNECT_INTERVALS[reconnectAttempts] || 300;
      setTimeout(() => attemptReconnect(), delay * 1000);
    }
  };

  useEffect(() => {
    const unlisten = listen('connection-lost', () => {
      attemptReconnect();
    });
    return () => { unlisten.then(f => f()); };
  }, []);

  return { isReconnecting, reconnectAttempts };
}
```

---

## 五、Agent 实现方案

### 5.1 Agent 架构

**模块结构:**

```rust
agent/src/
  ├── main.rs           // 主入口
  ├── config.rs         // 配置管理
  ├── server/
  │   ├── mod.rs        // 服务器模块
  │   ├── quic.rs       // QUIC 服务器
  │   └── websocket.rs  // WebSocket 服务器
  ├── handler.rs        // 消息处理
  ├── protocol.rs       // 协议定义
  └── cert.rs           // 证书生成
```

---

### 5.2 协议实现

**消息定义:**

```rust
// agent/src/protocol.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub request_id: u32,
    pub payload: Payload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum Payload {
    #[serde(rename = "ping")]
    Ping { timestamp: u64 },
    #[serde(rename = "pong")]
    Pong { timestamp: u64, server_time: u64 },
    #[serde(rename = "auth_request")]
    AuthRequest { token: String },
    #[serde(rename = "auth_response")]
    AuthResponse { success: bool, error: Option<String> },
    #[serde(rename = "metrics_subscribe")]
    MetricsSubscribeRequest {},
    #[serde(rename = "metrics_data")]
    MetricsData(MetricsSnapshot),
    #[serde(rename = "error")]
    Error { code: i32, message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    pub cpu_percent: f32,
    pub mem_used_bytes: u64,
    pub mem_total_bytes: u64,
    pub swap_used_bytes: u64,
    pub disks: Vec<DiskInfo>,
    pub network_rx_bytes: u64,
    pub network_tx_bytes: u64,
    pub uptime_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskInfo {
    pub mount_point: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
}
```

---

### 5.3 消息处理

**Handler 实现:**

```rust
// agent/src/handler.rs
use crate::protocol::{Envelope, Payload, MetricsSnapshot};
use sysinfo::{System, Disks, Networks};

pub fn handle_envelope(envelope: &Envelope, cfg: &AgentConfig) -> Envelope {
    match &envelope.payload {
        Payload::Ping { timestamp } => {
            let server_time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;

            Envelope::new(
                envelope.request_id,
                Payload::Pong {
                    timestamp: *timestamp,
                    server_time,
                },
            )
        }

        Payload::AuthRequest { token } => {
            let success = token == &cfg.auth.token;
            Envelope::new(
                envelope.request_id,
                Payload::AuthResponse {
                    success,
                    error: if success { None } else { Some("Token 无效".into()) },
                },
            )
        }

        Payload::MetricsSubscribeRequest {} => {
            match collect_metrics() {
                Ok(metrics) => Envelope::new(
                    envelope.request_id,
                    Payload::MetricsData(metrics),
                ),
                Err(e) => error_response(envelope.request_id, &e),
            }
        }

        _ => error_response(envelope.request_id, "未知的消息类型"),
    }
}

fn collect_metrics() -> Result<MetricsSnapshot, String> {
    let mut sys = System::new_all();
    sys.refresh_all();

    let disks_obj = Disks::new_with_refreshed_list();
    let disks: Vec<_> = disks_obj.iter()
        .map(|d| DiskInfo {
            mount_point: d.mount_point().to_string_lossy().to_string(),
            total_bytes: d.total_space(),
            used_bytes: d.total_space() - d.available_space(),
        })
        .collect();

    let networks = Networks::new_with_refreshed_list();
    let mut network_rx = 0u64;
    let mut network_tx = 0u64;
    for (_, data) in &networks {
        network_rx += data.received();
        network_tx += data.transmitted();
    }

    Ok(MetricsSnapshot {
        cpu_percent: sys.global_cpu_usage(),
        mem_used_bytes: sys.used_memory(),
        mem_total_bytes: sys.total_memory(),
        swap_used_bytes: sys.used_swap(),
        disks,
        network_rx_bytes: network_rx,
        network_tx_bytes: network_tx,
        uptime_secs: System::uptime() as u64,
    })
}
```

---

### 5.4 QUIC 服务器

**服务器实现:**

```rust
// agent/src/server/quic.rs
use quinn::{Endpoint, ServerConfig};
use std::net::SocketAddr;

pub async fn run_quic_server(config: &AgentConfig) -> Result<(), String> {
    let addr: SocketAddr = config.server.listen.parse()
        .map_err(|e| format!("地址解析失败: {}", e))?;

    let server_config = build_server_config(config)?;

    let mut endpoint = Endpoint::server(server_config, addr)
        .map_err(|e| format!("创建 Endpoint 失败: {}", e))?;

    tracing::info!("QUIC 服务器启动: {}", addr);

    while let Some(conn) = endpoint.accept().await {
        let connection = conn.await
            .map_err(|e| format!("连接失败: {}", e))?;

        tokio::spawn(handle_connection(connection));
    }

    Ok(())
}

async fn handle_connection(conn: quinn::Connection) {
    while let Ok(stream) = conn.accept_bi().await {
        let (mut send, mut recv) = stream;
        tokio::spawn(async move {
            // 读取消息
            let mut len_buf = [0u8; 4];
            recv.read_exact(&mut len_buf).await.ok();
            let len = u32::from_le_bytes(len_buf) as usize;

            let mut data = vec![0u8; len];
            recv.read_exact(&mut data).await.ok();

            // 处理消息
            let envelope = Envelope::decode(&data).ok();
            if let Some(env) = envelope {
                let response = handle_envelope(&env, &config);

                // 发送响应
                let bytes = response.encode().ok();
                if let Some(b) = bytes {
                    let resp_len = (b.len() as u32).to_le_bytes();
                    send.write_all(&resp_len).await.ok();
                    send.write_all(&b).await.ok();
                }
            }
        });
    }
}
```

---

### 5.5 WebSocket 服务器

**服务器实现:**

```rust
// agent/src/server/websocket.rs
use tokio_tungstenite::accept_hdr_async;
use futures_util::{SinkExt, StreamExt};

pub async fn run_ws_server(config: &AgentConfig) -> Result<(), String> {
    let addr: SocketAddr = config.server.listen.parse()
        .map_err(|e| format!("地址解析失败: {}", e))?;

    let listener = TcpListener::bind(addr)
        .await
        .map_err(|e| format!("绑定端口失败: {}", e))?;

    tracing::info!("WebSocket 服务器启动: {}", addr);

    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(handle_ws_connection(stream));
    }

    Ok(())
}

async fn handle_ws_connection(stream: TcpStream) {
    let ws_stream = accept_hdr_async(stream, |_, _| Ok(None))
        .await
        .ok();

    if let Some(ws) = ws_stream {
        let (mut write, mut read) = ws.split();

        while let Some(msg) = read.next().await {
            if let Ok(WsMessage::Binary(data)) = msg {
                let envelope = Envelope::decode(&data).ok();
                if let Some(env) = envelope {
                    let response = handle_envelope(&env, &config);
                    let bytes = response.encode().ok();
                    if let Some(b) = bytes {
                        write.send(WsMessage::Binary(b)).await.ok();
                    }
                }
            }
        }
    }
}
```

---

## 六、测试策略

### 6.1 客户端测试

**单元测试:**
- ShellContext 状态更新逻辑
- TopBar 组件渲染(连接状态、远程时间、指标)
- Desktop/Dock 应用图标点击
- Overview 窗口管理

**集成测试:**
- 连接流程:连接 → 获取数据 → 显示状态
- 断开连接:断开 → 清空状态 → 显示提示
- 窗口生命周期:创建 → 聚焦 → 关闭

**端到端测试:**
- 用户完整流程:连接服务器 → 打开文件管理器 → 浏览文件
- Overview 流程:按 Super 键 → 打开 Overview → 点击窗口
- 错误恢复流程:连接失败 → 重试 → 成功连接

---

### 6.2 Agent 测试

**单元测试:**
- 协议编解码(Envelope encode/decode)
- 消息处理(handle_envelope)
- 系统指标采集(collect_metrics)

**集成测试:**
- QUIC 服务器连接测试
- WebSocket 服务器连接测试
- 认证流程测试(Token 验证)

**端到端测试:**
- 客户端连接 Agent → 获取数据 → 验证响应
- 文件操作测试:浏览目录 → 验证文件列表
- 系统指标测试:获取指标 → 验证数据格式

---

## 七、部署方案

### 7.1 Agent 部署

**编译:**

```bash
cargo build --release --target x86_64-unknown-linux-musl
# → agent (静态链接, ~8MB)
```

**部署:**

```bash
scp target/x86_64-unknown-linux-musl/release/agent user@server:/opt/gnome-remote/
scp agent.toml user@server:/opt/gnome-remote/
```

**配置:**

```toml
# agent.toml
[server]
listen = "0.0.0.0:8443"
cert_path = "/opt/gnome-remote/cert.pem"
key_path = "/opt/gnome-remote/key.pem"

[auth]
token = "gmr_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"

[limits]
max_file_transfer_mb = 1000
max_terminal_sessions = 10
metrics_interval_secs = 2

[security]
allowed_paths = ["/home", "/etc", "/var/log", "/opt"]
blocked_commands = ["rm -rf /", "dd if=", "mkfs."]
```

**Systemd 服务:**

```bash
systemctl enable --now gnome-remote-agent
```

---

### 7.2 客户端打包

**Windows:**

```bash
cargo tauri build --target x86_64-pc-windows-msvc
# → gnome-remote.exe + .msi
```

**macOS:**

```bash
cargo tauri build --target x86_64-apple-darwin
# → gnome-remote.app + .dmg
```

**Linux:**

```bash
cargo tauri build --target x86_64-unknown-linux-gnu
# → gnome-remote.AppImage
```

---

## 八、开发进度交接

### 8.1 开发顺序

**第一阶段: Agent 开发(根据 API 文档)**
1. 实现协议定义(protocol.rs)
2. 实现消息处理(handler.rs)
3. 实现 QUIC 服务器(server/quic.rs)
4. 实现 WebSocket 服务器(server/websocket.rs)
5. 实现系统指标采集(collect_metrics)
6. 测试 Agent 功能(单元测试 + 集成测试)

**第二阶段: 客户端开发(根据 API 文档)**
1. 实现 ShellContext(状态管理)
2. 实现 TopBar(连接状态、远程时间、指标)
3. 实现 Desktop(应用图标网格)
4. 实现 Dock(底部应用栏)
5. 实现 Overview(活动概览)
6. 实现自动重连机制
7. 测试客户端功能(单元测试 + 集成测试)

**第三阶段: 集成测试**
1. 客户端连接 Agent
2. 验证协议实现
3. 验证数据格式
4. 验证错误处理
5. 端到端测试

---

### 8.2 API 文档作为契约

**契约内容:**
- 消息协议定义(第三章)
- 数据格式定义(MetricsSnapshot, FileEntry)
- 错误处理规范(Error 协议)
- 实现要求(Agent 和客户端必须实现的功能)

**契约验证:**
- Agent 实现必须符合 API 文档
- 客户端实现必须符合 API 文档
- 集成测试验证前后端是否符合契约

---

## 九、后续阶段规划

### 9.1 第二阶段: 文件管理器

- 远程文件浏览
- 文件搜索
- 文件预览、下载、上传
- 文件操作(新建、删除、重命名)

### 9.2 第三阶段: 终端控制

- 远程终端会话
- PTY 管理
- 实时输入输出
- 多标签页、分屏

### 9.3 第四阶段: 系统监控

- 实时系统指标图表
- 进程管理
- 服务控制
- 系统日志查看

---

## 十、总结

本设计文档定义了 GNOME 远程控制客户端桌面外壳的完整实现方案,包括:

1. **整体架构:** ShellContext + 组件 + Tauri Backend + Agent
2. **API 文档:** 协议定义作为前后端契约
3. **客户端实现:** TopBar + Desktop + Dock + Overview + 自动重连
4. **Agent 实现:** QUIC/WebSocket 服务器 + 协议处理 + 系统指标采集
5. **测试策略:** 单元测试 + 集成测试 + 端到端测试
6. **部署方案:** Agent 部署 + 客户端打包
7. **开发进度:** Agent 先开发,客户端后开发,通过 API 文档交接

通过分步开发和 API 文档契约,可以确保前后端独立开发,降低耦合风险,最终实现完整的 GNOME 桌面外壳功能。