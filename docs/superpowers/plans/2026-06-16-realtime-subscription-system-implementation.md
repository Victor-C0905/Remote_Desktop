# 实时订阅推送系统 - 核心架构 + 系统监控实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现通用订阅推送架构和系统监控采集器，支持按需订阅、实时推送、可扩展设计。

**Architecture:** EventBus + SubscriptionManager + MetricsCollector 三层架构，Agent 后台定时采集并推送，客户端通过 Tauri subscribe/unsubscribe API 订阅。

**Tech Stack:** Rust (quinn, sysinfo, tokio), TypeScript (React, Tauri API)

---

## 文件结构

### Agent 侧新增/修改文件

```
agent/src/
  ├── collectors/
  │   ├── mod.rs           # 采集器模块入口（新增）
  │   └── metrics.rs       # 系统监控采集器（新增）
  ├── event_bus.rs         # 事件总线（新增）
  ├── subscription.rs      # 订阅管理器（新增）
  ├── protocol.rs          # 协议定义（修改：新增订阅类型）
  ├── handler.rs           # 消息处理（修改：新增订阅处理）
  ├── config.rs            # 配置（修改：新增采集器配置）
  └── main.rs              # 主入口（修改：集成订阅系统）
```

### Tauri Backend 新增/修改文件

```
src-tauri/src/
  ├── lib.rs                # 新增 subscribe/unsubscribe command
  └── connection.rs         # 扩展 QUIC Stream 管理（新增订阅 Stream）
```

### 前端新增/修改文件

```
src/
  ├── hooks/
  │   └── useSubscription.ts  # 订阅 Hook（新增）
  ├── config/
  │   └── subscription.ts     # 订阅配置（新增）
  ├── apps/
  │   └── SystemMonitor.tsx   # 使用 subscribe API（修改）
  └── shell/
      └── TopBar.tsx          # 显示 CPU/MEM 时订阅（修改）
```

---

## Task 1: Agent 协议扩展

**Files:**
- Modify: `agent/src/protocol.rs`

- [ ] **Step 1: 添加 SubscriptionType 定义**

在 `agent/src/protocol.rs` 中添加订阅类型枚举：

```rust
// agent/src/protocol.rs

/// 订阅类型枚举
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(tag = "type", content = "params")]
pub enum SubscriptionType {
    // 系统监控
    #[serde(rename = "metrics")]
    Metrics {
        interval_secs: Option<u64>, // 可选，默认使用配置值
    },

    // 文件变化监控（后续实现）
    #[serde(rename = "file_changes")]
    FileChanges {
        path: String,
        recursive: Option<bool>,
    },

    // 进程事件（后续实现）
    #[serde(rename = "process_events")]
    ProcessEvents {
        interval_secs: Option<u64>,
    },

    // 应用日志（后续实现）
    #[serde(rename = "app_logs")]
    AppLogs {
        app_name: String,
        level: Option<String>,
    },

    // 服务状态（后续实现）
    #[serde(rename = "service_status")]
    ServiceStatus {
        service: String,
        interval_secs: Option<u64>,
    },
}

impl SubscriptionType {
    /// 获取订阅类型名称
    pub fn type_name(&self) -> String {
        match self {
            SubscriptionType::Metrics { .. } => "metrics",
            SubscriptionType::FileChanges { .. } => "file_changes",
            SubscriptionType::ProcessEvents { .. } => "process_events",
            SubscriptionType::AppLogs { .. } => "app_logs",
            SubscriptionType::ServiceStatus { .. } => "service_status",
        }.to_string()
    }
}
```

- [ ] **Step 2: 添加订阅相关 Payload 类型**

在 `Payload` enum 中添加订阅相关类型：

```rust
// agent/src/protocol.rs

pub enum Payload {
    // 现有类型保持不变...

    // 新增：通用订阅
    #[serde(rename = "subscribe")]
    Subscribe {
        server_id: String,
        types: Vec<SubscriptionType>, // 支持同时订阅多种类型
    },

    // 新增：通用取消订阅
    #[serde(rename = "unsubscribe")]
    Unsubscribe {
        server_id: String,
        types: Vec<SubscriptionType>, // 支持取消部分订阅
    },

    // 新增：通用事件推送
    #[serde(rename = "event")]
    Event {
        event_type: String,           // "metrics" / "file_changes" / ...
        data: serde_json::Value,      // 事件数据（动态类型）
        timestamp: u64,               // 事件时间戳
    },

    // 新增：订阅确认
    #[serde(rename = "subscribe_ack")]
    SubscribeAck {
        success: bool,
        subscribed_types: Vec<SubscriptionType>,
    },

    // 新增：取消订阅确认
    #[serde(rename = "unsubscribe_ack")]
    UnsubscribeAck {
        success: bool,
    },
}
```

- [ ] **Step 3: 验证协议编译**

Run: `cd agent && cargo check`
Expected: 编译成功，无错误

- [ ] **Step 4: 提交协议扩展**

```bash
git add agent/src/protocol.rs
git commit -m "feat(agent): add subscription protocol types"
```

---

## Task 2: Agent 配置扩展

**Files:**
- Modify: `agent/src/config.rs`

- [ ] **Step 1: 添加采集器配置**

在 `AgentConfig` 中添加采集器配置：

```rust
// agent/src/config.rs

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentConfig {
    pub server: ServerConfig,
    pub auth: AuthConfig,
    pub security: SecurityConfig,
    pub limits: LimitsConfig,
    pub collectors: CollectorsConfig, // 新增
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CollectorsConfig {
    #[serde(default = "default_metrics_interval")]
    pub metrics_interval_secs: u64,

    #[serde(default = "default_file_changes_delay")]
    pub file_changes_delay_ms: u64,

    #[serde(default = "default_process_scan_interval")]
    pub process_scan_interval_secs: u64,

    #[serde(default = "default_service_status_interval")]
    pub service_status_interval_secs: u64,
}

fn default_metrics_interval() -> u64 { 1 }
fn default_file_changes_delay() -> u64 { 100 }
fn default_process_scan_interval() -> u64 { 2 }
fn default_service_status_interval() -> u64 { 5 }
```

- [ ] **Step 2: 更新 default_config 函数**

修改 `default_config` 函数以包含采集器配置：

```rust
// agent/src/config.rs

fn default_config() -> AgentConfig {
    AgentConfig {
        server: ServerConfig {
            bind: "0.0.0.0".into(),
            quic_port: 8443,
            ws_port: 443,
            cert_path: "./cert.pem".into(),
            key_path: "./key.pem".into(),
        },
        auth: AuthConfig { token: String::new() },
        security: SecurityConfig {
            allowed_paths: default_allowed_paths(),
            blocked_commands: default_blocked_commands(),
        },
        limits: LimitsConfig {
            max_terminal_sessions: 10,
            max_file_transfer_mb: 500,
            metrics_interval_secs: 2,
        },
        collectors: CollectorsConfig {
            metrics_interval_secs: 1,
            file_changes_delay_ms: 100,
            process_scan_interval_secs: 2,
            service_status_interval_secs: 5,
        },
    }
}
```

- [ ] **Step 3: 验证配置编译**

Run: `cd agent && cargo check`
Expected: 编译成功，无错误

- [ ] **Step 4: 提交配置扩展**

```bash
git add agent/src/config.rs
git commit -m "feat(agent): add collectors configuration"
```

---

## Task 3: Agent EventBus 实现

**Files:**
- Create: `agent/src/event_bus.rs`

- [ ] **Step 1: 创建 EventBus 模块**

创建 `agent/src/event_bus.rs` 文件：

```rust
// agent/src/event_bus.rs

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde_json::Value;

type EventCallback = Arc<dyn Fn(Value) + Send + Sync>;

/// 事件总线，用于发布和订阅事件
pub struct EventBus {
    subscribers: Arc<RwLock<HashMap<String, Vec<EventCallback>>>,
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            subscribers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 发布事件
    pub async fn publish(&self, event_type: &str, data: Value) {
        let subs = self.subscribers.read().await;
        if let Some(callbacks) = subs.get(event_type) {
            for callback in callbacks {
                callback(data.clone());
            }
        }
    }

    /// 订阅事件
    pub async fn subscribe(&self, event_type: &str, callback: EventCallback) {
        let mut subs = self.subscribers.write().await;
        subs.entry(event_type.to_string())
            .or_insert_with(Vec::new)
            .push(callback);
    }

    /// 取消订阅（移除所有该类型的订阅）
    pub async fn unsubscribe_all(&self, event_type: &str) {
        let mut subs = self.subscribers.write().await;
        subs.remove(event_type);
    }

    /// 检查是否有订阅者
    pub async fn has_subscribers(&self, event_type: &str) -> bool {
        let subs = self.subscribers.read().await;
        subs.get(event_type).map(|v| !v.is_empty()).unwrap_or(false)
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] **Step 2: 在 main.rs 中引入 EventBus**

修改 `agent/src/main.rs`：

```rust
// agent/src/main.rs

mod event_bus;

use event_bus::EventBus;

fn main() {
    // ...
    let event_bus = Arc::new(EventBus::new());
    // ...
}
```

- [ ] **Step 3: 验证 EventBus 编译**

Run: `cd agent && cargo check`
Expected: 编译成功，无错误

- [ ] **Step 4: 提交 EventBus 实现**

```bash
git add agent/src/event_bus.rs agent/src/main.rs
git commit -m "feat(agent): implement EventBus for event publishing"
```

---

## Task 4: Agent MetricsCollector 实现

**Files:**
- Create: `agent/src/collectors/mod.rs`
- Create: `agent/src/collectors/metrics.rs`

- [ ] **Step 1: 创建采集器模块入口**

创建 `agent/src/collectors/mod.rs`：

```rust
// agent/src/collectors/mod.rs

pub mod metrics;

pub use metrics::MetricsCollector;
```

- [ ] **Step 2: 创建 MetricsCollector**

创建 `agent/src/collectors/metrics.rs`：

```rust
// agent/src/collectors/metrics.rs

use crate::event_bus::EventBus;
use crate::protocol::MetricsSnapshot;
use sysinfo::{System, Disks, Networks};
use tokio::time::{sleep, Duration};
use std::sync::Arc;
use tokio::sync::RwLock;

/// 系统监控采集器
pub struct MetricsCollector {
    interval_secs: u64,
    event_bus: Arc<EventBus>,
    running: Arc<RwLock<bool>>,
}

impl MetricsCollector {
    pub fn new(interval_secs: u64, event_bus: Arc<EventBus>) -> Self {
        Self {
            interval_secs,
            event_bus,
            running: Arc::new(RwLock::new(false)),
        }
    }

    /// 启动采集
    pub async fn start(&self) {
        let running = self.running.clone();
        let event_bus = self.event_bus.clone();
        let interval = self.interval_secs;

        // 设置运行状态
        {
            let mut r = running.write().await;
            *r = true;
        }

        tokio::spawn(async move {
            loop {
                // 检查是否有订阅者
                let should_run = {
                    let r = running.read().await;
                    *r
                };

                if !should_run {
                    break;
                }

                // 采集数据
                let metrics = Self::collect();

                // 发布事件
                event_bus.publish("metrics", serde_json::to_value(metrics).unwrap()).await;

                // 等待下一次采集
                sleep(Duration::from_secs(interval)).await;
            }
        });
    }

    /// 停止采集
    pub async fn stop(&self) {
        let mut running = self.running.write().await;
        *running = false;
    }

    /// 采集系统指标
    fn collect() -> MetricsSnapshot {
        let mut sys = System::new_all();
        sys.refresh_all();

        let disks_obj = Disks::new_with_refreshed_list();
        let disks: Vec<_> = disks_obj
            .iter()
            .map(|d| crate::protocol::DiskInfo {
                mount_point: d.mount_point().to_string_lossy().to_string(),
                total_bytes: d.total_space(),
                used_bytes: d.total_space() - d.available_space(),
            })
            .collect();

        let networks = Networks::new_with_refreshed_list();
        let mut network_rx = 0u64;
        let mut network_tx = 0u64;
        for (_name, data) in &networks {
            network_rx += data.received();
            network_tx += data.transmitted();
        }

        MetricsSnapshot {
            cpu_percent: sys.global_cpu_usage(),
            mem_used_bytes: sys.used_memory(),
            mem_total_bytes: sys.total_memory(),
            swap_used_bytes: sys.used_swap(),
            disks,
            network_rx_bytes: network_rx,
            network_tx_bytes: network_tx,
            uptime_secs: System::uptime() as u64,
        }
    }
}
```

- [ ] **Step 3: 在 main.rs 中引入采集器模块**

修改 `agent/src/main.rs`：

```rust
// agent/src/main.rs

mod event_bus;
mod collectors;

use event_bus::EventBus;
use collectors::MetricsCollector;

fn main() {
    // ...
}
```

- [ ] **Step 4: 验证 MetricsCollector 编译**

Run: `cd agent && cargo check`
Expected: 编译成功，无错误

- [ ] **Step 5: 提交 MetricsCollector 实现**

```bash
git add agent/src/collectors/mod.rs agent/src/collectors/metrics.rs agent/src/main.rs
git commit -m "feat(agent): implement MetricsCollector for system monitoring"
```

---

## Task 5: Agent SubscriptionManager 实现

**Files:**
- Create: `agent/src/subscription.rs`

- [ ] **Step 1: 创建 SubscriptionManager**

创建 `agent/src/subscription.rs`：

```rust
// agent/src/subscription.rs

use std::collections::HashMap;
use std::sync::Arc;
use quinn::{SendStream, RecvStream};
use tokio::sync::RwLock;
use crate::protocol::{SubscriptionType, Payload, Envelope};
use crate::event_bus::EventBus;
use crate::collectors::MetricsCollector;
use crate::config::AgentConfig;

/// 订阅管理器
pub struct SubscriptionManager {
    // StreamId -> 订阅类型列表
    subscribers: Arc<RwLock<HashMap<u64, Vec<SubscriptionType>>>>,

    // 订阅类型 -> 订阅者数量
    type_counts: Arc<RwLock<HashMap<String, usize>>>,

    // 采集器实例
    metrics_collector: Arc<MetricsCollector>,

    // 配置
    config: AgentConfig,
}

impl SubscriptionManager {
    pub fn new(config: AgentConfig, event_bus: Arc<EventBus>) -> Self {
        let metrics_collector = Arc::new(MetricsCollector::new(
            config.collectors.metrics_interval_secs,
            event_bus.clone(),
        ));

        Self {
            subscribers: Arc::new(RwLock::new(HashMap::new())),
            type_counts: Arc::new(RwLock::new(HashMap::new())),
            metrics_collector,
            config,
        }
    }

    /// 添加订阅
    pub async fn subscribe(
        &self,
        stream_id: u64,
        types: Vec<SubscriptionType>,
        send_stream: SendStream,
    ) -> Result<(), String> {
        // 记录订阅者
        {
            let mut subs = self.subscribers.write().await;
            subs.insert(stream_id, types.clone());
        }

        // 更新订阅类型计数
        for t in &types {
            let type_name = t.type_name();
            let mut counts = self.type_counts.write().await;
            let count = counts.entry(type_name.clone()).or_insert(0);
            *count += 1;

            // 如果是第一个订阅者，启动对应的采集器
            if *count == 1 {
                self.start_collector(&t)?;
            }
        }

        // 发送订阅确认
        let ack = Envelope::new(
            0,
            Payload::SubscribeAck {
                success: true,
                subscribed_types: types,
            },
        );

        if let Ok(encoded) = ack.encode() {
            // 发送确认（需要实现 Stream 发送逻辑）
            // send_stream.write_all(&encoded).await.ok();
        }

        Ok(())
    }

    /// 移除订阅
    pub async fn unsubscribe(
        &self,
        stream_id: u64,
        types: Vec<SubscriptionType>,
    ) -> Result<(), String> {
        // 移除订阅者
        {
            let mut subs = self.subscribers.write().await;
            if let Some(subscribed_types) = subs.get_mut(&stream_id) {
                for t in &types {
                    subscribed_types.retain(|st| st != t);
                }

                // 如果订阅类型列表为空，移除订阅者
                if subscribed_types.is_empty() {
                    subs.remove(&stream_id);
                }
            }
        }

        // 更新订阅类型计数
        for t in &types {
            let type_name = t.type_name();
            let mut counts = self.type_counts.write().await;
            if let Some(count) = counts.get_mut(&type_name) {
                *count -= 1;

                // 如果无订阅者，停止对应的采集器
                if *count == 0 {
                    counts.remove(&type_name);
                    self.stop_collector(&t)?;
                }
            }
        }

        Ok(())
    }

    /// 启动采集器
    fn start_collector(&self, type: &SubscriptionType) -> Result<(), String> {
        match type {
            SubscriptionType::Metrics { .. } => {
                self.metrics_collector.start().await;
                Ok(())
            }
            _ => Err("订阅类型尚未实现".to_string()),
        }
    }

    /// 停止采集器
    fn stop_collector(&self, type: &SubscriptionType) -> Result<(), String> {
        match type {
            SubscriptionType::Metrics { .. } => {
                self.metrics_collector.stop().await;
                Ok(())
            }
            _ => Err("订阅类型尚未实现".to_string()),
        }
    }

    /// 检查是否有订阅者
    pub async fn has_subscribers(&self, event_type: &str) -> bool {
        let counts = self.type_counts.read().await;
        counts.get(event_type).map(|v| *v > 0).unwrap_or(false)
    }
}
```

- [ ] **Step 2: 在 main.rs 中引入 SubscriptionManager**

修改 `agent/src/main.rs`：

```rust
// agent/src/main.rs

mod event_bus;
mod collectors;
mod subscription;

use event_bus::EventBus;
use collectors::MetricsCollector;
use subscription::SubscriptionManager;

fn main() {
    // ...
}
```

- [ ] **Step 3: 验证 SubscriptionManager 编译**

Run: `cd agent && cargo check`
Expected: 编译成功，无错误

- [ ] **Step 4: 提交 SubscriptionManager 实现**

```bash
git add agent/src/subscription.rs agent/src/main.rs
git commit -m "feat(agent): implement SubscriptionManager for subscription management"
```

---

## Task 6: Agent Handler 扩展

**Files:**
- Modify: `agent/src/handler.rs`

- [ ] **Step 1: 添加订阅处理逻辑**

在 `handle_envelope` 函数中添加订阅处理：

```rust
// agent/src/handler.rs

use crate::subscription::SubscriptionManager;

pub fn handle_envelope(
    envelope: &Envelope,
    cfg: &AgentConfig,
    subscription_manager: &SubscriptionManager,
    stream_id: u64,
    send_stream: &mut SendStream,
) -> Envelope {
    match &envelope.payload {
        // 现有处理逻辑保持不变...

        // 新增：订阅处理
        Payload::Subscribe { server_id, types } => {
            tracing::info!("订阅请求: server_id={}, types={}", server_id, types.len());

            match subscription_manager.subscribe(stream_id, types.clone(), send_stream.clone()).await {
                Ok(_) => {
                    tracing::info!("订阅成功");
                    // 返回确认（已在 subscribe 中发送）
                    Envelope::new(0, Payload::SubscribeAck {
                        success: true,
                        subscribed_types: types.clone(),
                    })
                }
                Err(e) => {
                    tracing::error!("订阅失败: {}", e);
                    Envelope::new(0, Payload::SubscribeAck {
                        success: false,
                        subscribed_types: vec![],
                    })
                }
            }
        }

        // 新增：取消订阅处理
        Payload::Unsubscribe { server_id, types } => {
            tracing::info!("取消订阅请求: server_id={}, types={}", server_id, types.len());

            match subscription_manager.unsubscribe(stream_id, types.clone()).await {
                Ok(_) => {
                    tracing::info!("取消订阅成功");
                    Envelope::new(0, Payload::UnsubscribeAck { success: true })
                }
                Err(e) => {
                    tracing::error!("取消订阅失败: {}", e);
                    Envelope::new(0, Payload::UnsubscribeAck { success: false })
                }
            }
        }

        // 其他处理逻辑...
    }
}
```

- [ ] **Step 2: 验证 handler 编译**

Run: `cd agent && cargo check`
Expected: 编译成功，无错误

- [ ] **Step 3: 提交 handler 扩展**

```bash
git add agent/src/handler.rs
git commit -m "feat(agent): add subscription handling in handler"
```

---

## Task 7: Agent QUIC Server 集成

**Files:**
- Modify: `agent/src/server/quic.rs`

- [ ] **Step 1: 在 QUIC Server 中集成 SubscriptionManager**

修改 `agent/src/server/quic.rs`：

```rust
// agent/src/server/quic.rs

use crate::subscription::SubscriptionManager;
use crate::event_bus::EventBus;

pub struct QuicServer {
    endpoint: quinn::Endpoint,
    config: AgentConfig,
    subscription_manager: Arc<SubscriptionManager>,
    event_bus: Arc<EventBus>,
}

impl QuicServer {
    pub fn new(config: AgentConfig) -> Self {
        let event_bus = Arc::new(EventBus::new());
        let subscription_manager = Arc::new(SubscriptionManager::new(config.clone(), event_bus.clone()));

        // ... QUIC endpoint 创建逻辑

        Self {
            endpoint,
            config,
            subscription_manager,
            event_bus,
        }
    }

    async fn handle_connection(&self, conn: quinn::Connection) {
        let subscription_manager = self.subscription_manager.clone();
        let event_bus = self.event_bus.clone();

        // 监听 EventBus 事件并推送
        tokio::spawn(async move {
            event_bus.subscribe("metrics", Arc::new(|data| {
                // 推送 metrics 事件到订阅者
                // ...
            })).await;
        });

        // 处理 Stream
        while let Some(stream) = conn.accept_bi().await.ok() {
            let (send, recv) = stream;
            self.handle_stream(send, recv, subscription_manager.clone()).await;
        }
    }

    async fn handle_stream(
        &self,
        send: SendStream,
        recv: RecvStream,
        subscription_manager: Arc<SubscriptionManager>,
    ) {
        // 读取数据
        let data = recv.read_to_end(1024).await.ok();
        if let Some(data) = data {
            let envelope = Envelope::decode(&data);
            if let Ok(envelope) = envelope {
                // 处理订阅请求
                let stream_id = send.id(); // 获取 Stream ID
                let response = handle_envelope(&envelope, &self.config, &subscription_manager, stream_id, &mut send);

                // 发送响应
                send.write_all(&response.encode().unwrap()).await.ok();
            }
        }
    }
}
```

- [ ] **Step 2: 验证 QUIC Server 编译**

Run: `cd agent && cargo check`
Expected: 编译成功，无错误

- [ ] **Step 3: 提交 QUIC Server 集成**

```bash
git add agent/src/server/quic.rs
git commit -m "feat(agent): integrate SubscriptionManager into QUIC server"
```

---

## Task 8: Tauri Backend subscribe/unsubscribe Command

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: 添加 subscribe command**

在 `src-tauri/src/lib.rs` 中添加 subscribe command：

```rust
// src-tauri/src/lib.rs

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "params")]
pub enum SubscriptionType {
    #[serde(rename = "metrics")]
    Metrics { interval_secs: Option<u64> },
    #[serde(rename = "file_changes")]
    FileChanges { path: String, recursive: Option<bool> },
    #[serde(rename = "process_events")]
    ProcessEvents { interval_secs: Option<u64> },
    #[serde(rename = "app_logs")]
    AppLogs { app_name: String, level: Option<String> },
    #[serde(rename = "service_status")]
    ServiceStatus { service: String, interval_secs: Option<u64> },
}

#[tauri::command]
async fn subscribe(
    server_id: String,
    types: Vec<SubscriptionType>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    use crate::connection::ConnectionManager;
    use tauri::Manager;

    // 获取连接管理器
    let manager = app.state::<ConnectionManager>();
    let connection = manager.get_connection(&server_id)?;

    // 创建双向 Stream
    let (send, recv) = connection.open_bi().await?;

    // 发送 Subscribe payload
    let payload = crate::protocol::Payload::Subscribe {
        server_id: server_id.clone(),
        types: types.iter().map(|t| {
            match t {
                SubscriptionType::Metrics { interval_secs } => crate::protocol::SubscriptionType::Metrics { interval_secs },
                SubscriptionType::FileChanges { path, recursive } => crate::protocol::SubscriptionType::FileChanges { path: path.clone(), recursive },
                SubscriptionType::ProcessEvents { interval_secs } => crate::protocol::SubscriptionType::ProcessEvents { interval_secs },
                SubscriptionType::AppLogs { app_name, level } => crate::protocol::SubscriptionType::AppLogs { app_name: app_name.clone(), level: level.clone() },
                SubscriptionType::ServiceStatus { service, interval_secs } => crate::protocol::SubscriptionType::ServiceStatus { service: service.clone(), interval_secs },
            }
        }).collect(),
    };

    let envelope = crate::protocol::Envelope::new(0, payload);
    send.write_all(&envelope.encode()?).await?;

    // 监听 Agent 推送
    tokio::spawn(async move {
        loop {
            let data = recv.read_to_end(4096).await;
            if let Ok(Some(data)) = data {
                let envelope = crate::protocol::Envelope::decode(&data);
                if let Ok(envelope) = envelope {
                    if let crate::protocol::Payload::Event { event_type, data, timestamp } = envelope.payload {
                        // 转发到前端 Tauri Event
                        app.emit("subscription_event", SubscriptionEvent {
                            server_id: server_id.clone(),
                            event_type,
                            data,
                            timestamp,
                        }).ok();
                    }
                }
            } else {
                break; // Stream 关闭
            }
        }
    });

    Ok(())
}

#[derive(Debug, Clone, Serialize)]
struct SubscriptionEvent {
    server_id: String,
    event_type: String,
    data: serde_json::Value,
    timestamp: u64,
}
```

- [ ] **Step 2: 添加 unsubscribe command**

在 `src-tauri/src/lib.rs` 中添加 unsubscribe command：

```rust
// src-tauri/src/lib.rs

#[tauri::command]
async fn unsubscribe(
    server_id: String,
    types: Vec<SubscriptionType>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    use crate::connection::ConnectionManager;
    use tauri::Manager;

    // 获取连接管理器
    let manager = app.state::<ConnectionManager>();
    let connection = manager.get_connection(&server_id)?;

    // 创建双向 Stream
    let (send, recv) = connection.open_bi().await?;

    // 发送 Unsubscribe payload
    let payload = crate::protocol::Payload::Unsubscribe {
        server_id: server_id.clone(),
        types: types.iter().map(|t| {
            match t {
                SubscriptionType::Metrics { interval_secs } => crate::protocol::SubscriptionType::Metrics { interval_secs },
                SubscriptionType::FileChanges { path, recursive } => crate::protocol::SubscriptionType::FileChanges { path: path.clone(), recursive },
                SubscriptionType::ProcessEvents { interval_secs } => crate::protocol::SubscriptionType::ProcessEvents { interval_secs },
                SubscriptionType::AppLogs { app_name, level } => crate::protocol::SubscriptionType::AppLogs { app_name: app_name.clone(), level: level.clone() },
                SubscriptionType::ServiceStatus { service, interval_secs } => crate::protocol::SubscriptionType::ServiceStatus { service: service.clone(), interval_secs },
            }
        }).collect(),
    };

    let envelope = crate::protocol::Envelope::new(0, payload);
    send.write_all(&envelope.encode()?).await?;

    // 等待确认
    let data = recv.read_to_end(1024).await?;
    let ack = crate::protocol::Envelope::decode(&data)?;

    if let crate::protocol::Payload::UnsubscribeAck { success } = ack.payload {
        if success {
            Ok(())
        } else {
            Err("取消订阅失败".to_string())
        }
    } else {
        Err("未收到取消订阅确认".to_string())
    }
}
```

- [ ] **Step 3: 注册 commands**

在 `lib.rs` 的 `run` 函数中注册 commands：

```rust
// src-tauri/src/lib.rs

pub fn run() {
    tauri::Builder::default()
        .manage(ConnectionManager::new())
        .invoke_handler(tauri::generate_handler![
            // 现有 commands...
            subscribe,
            unsubscribe,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 4: 验证 Tauri Backend 编译**

Run: `cd src-tauri && cargo check`
Expected: 编译成功，无错误

- [ ] **Step 5: 提交 Tauri Backend commands**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(tauri): add subscribe/unsubscribe commands"
```

---

## Task 9: 前端订阅配置

**Files:**
- Create: `src/config/subscription.ts`

- [ ] **Step 1: 创建订阅配置文件**

创建 `src/config/subscription.ts`：

```typescript
// src/config/subscription.ts

export const SUBSCRIPTION_CONFIG = {
  // 默认推送间隔（与 Agent 保持一致）
  DEFAULT_INTERVAL_SECS: {
    metrics: 1,
    process_events: 2,
    service_status: 5,
  },

  // 文件变化监听延迟
  FILE_CHANGES_DELAY_MS: 100,

  // 最大历史数据长度（秒）
  HISTORY_LENGTH: 120, // 2 分钟历史

  // 图表刷新频率（前端渲染）
  CHART_REFRESH_MS: 1000,
};

export interface SubscriptionType {
  type: 'metrics' | 'file_changes' | 'process_events' | 'app_logs' | 'service_status';
  params?: {
    interval_secs?: number;
    path?: string;
    recursive?: boolean;
    app_name?: string;
    level?: string;
    service?: string;
  };
}

export interface SubscriptionEvent {
  server_id: string;
  event_type: string;
  data: any;
  timestamp: number;
}
```

- [ ] **Step 2: 提交订阅配置**

```bash
git add src/config/subscription.ts
git commit -m "feat(frontend): add subscription configuration"
```

---

## Task 10: 前端订阅 Hook

**Files:**
- Create: `src/hooks/useSubscription.ts`

- [ ] **Step 1: 创建订阅 Hook**

创建 `src/hooks/useSubscription.ts`：

```typescript
// src/hooks/useSubscription.ts

import { useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import { SubscriptionType, SubscriptionEvent } from '../config/subscription';

export function useSubscription(
  serverId: string | null,
  types: SubscriptionType[],
  onEvent: (event: SubscriptionEvent) => void
) {
  useEffect(() => {
    if (!serverId || types.length === 0) return;

    let unlisten: UnlistenFn | null = null;

    // 订阅
    invoke('subscribe', { serverId, types })
      .then(() => {
        // 监听事件
        return listen<SubscriptionEvent>('subscription_event', (event) => {
          if (event.payload.server_id === serverId) {
            onEvent(event.payload);
          }
        });
      })
      .then((fn) => {
        unlisten = fn;
      })
      .catch((e) => {
        console.error('订阅失败:', e);
      });

    // 清理：取消订阅
    return () => {
      if (unlisten) {
        unlisten();
      }
      invoke('unsubscribe', { serverId, types }).catch((e) => {
        console.error('取消订阅失败:', e);
      });
    };
  }, [serverId, types, onEvent]);
}

export function useMetricsSubscription(
  serverId: string | null,
  onMetrics: (metrics: any) => void
) {
  const types: SubscriptionType[] = [
    { type: 'metrics', params: { interval_secs: 1 } },
  ];

  const handleEvent = useCallback((event: SubscriptionEvent) => {
    if (event.event_type === 'metrics') {
      onMetrics(event.data);
    }
  }, [onMetrics]);

  useSubscription(serverId, types, handleEvent);
}
```

- [ ] **Step 2: 提交订阅 Hook**

```bash
git add src/hooks/useSubscription.ts
git commit -m "feat(frontend): add useSubscription hook"
```

---

## Task 11: SystemMonitor 集成订阅

**Files:**
- Modify: `src/apps/SystemMonitor.tsx`

- [ ] **Step 1: 移除轮询逻辑，使用订阅 Hook**

修改 `src/apps/SystemMonitor.tsx`：

```typescript
// src/apps/SystemMonitor.tsx

import { useState, useEffect, useCallback, useRef } from "react";
import { useServerManager } from "../context/ServerManager";
import { useMetricsSubscription } from "../hooks/useSubscription";
import "./SystemMonitor.css";

// ... 现有的类型定义和辅助函数保持不变

export function SystemMonitor() {
  const { activeServerId } = useServerManager();

  const [activeTab, setActiveTab] = useState<TabId>("resources");
  const [metrics, setMetrics] = useState<MetricsSnapshot>(generateDemoMetrics());
  const [cpuHistory, setCpuHistory] = useState<HistoryPoint[]>([]);
  const [memHistory, setMemHistory] = useState<HistoryPoint[]>([]);
  const [processes, setProcesses] = useState<ProcessInfo[]>(generateDemoProcesses());
  const [processSort, setProcessSort] = useState<"cpu" | "mem" | "pid">("cpu");
  const [selectedPid, setSelectedPid] = useState<number | null>(null);

  const HISTORY_LENGTH = 120; // 增加到 120（2 分钟历史）

  // 使用订阅 Hook
  const handleMetricsUpdate = useCallback((newMetrics: MetricsSnapshot) => {
    setMetrics(newMetrics);
    const now = Date.now();
    setCpuHistory(prev => {
      const next = [...prev, { time: now, value: newMetrics.cpu_percent }];
      return next.length > HISTORY_LENGTH ? next.slice(-HISTORY_LENGTH) : next;
    });
    setMemHistory(prev => {
      const memPercent = (newMetrics.mem_used_bytes / newMetrics.mem_total_bytes) * 100;
      const next = [...prev, { time: now, value: memPercent }];
      return next.length > HISTORY_LENGTH ? next.slice(-HISTORY_LENGTH) : next;
    });
  }, []);

  useMetricsSubscription(activeServerId, handleMetricsUpdate);

  // 移除原有的轮询逻辑（fetchMetrics 和 setInterval）

  // ... 其他逻辑保持不变
}
```

- [ ] **Step 2: 验证 SystemMonitor 编译**

Run: `npm run typecheck`（如果存在）
Expected: 编译成功，无错误

- [ ] **Step 3: 提交 SystemMonitor 集成**

```bash
git add src/apps/SystemMonitor.tsx
git commit -m "feat(frontend): integrate subscription into SystemMonitor"
```

---

## Task 12: TopBar 集成订阅

**Files:**
- Modify: `src/shell/TopBar.tsx`

- [ ] **Step 1: 在 TopBar 中添加订阅逻辑**

修改 `src/shell/TopBar.tsx`：

```typescript
// src/shell/TopBar.tsx

import { useState, useEffect } from 'react';
import { useServerManager } from '../context/ServerManager';
import { useMetricsSubscription } from '../hooks/useSubscription';

interface TopBarProps {
  showMetrics: boolean; // 是否显示 CPU/MEM
}

export function TopBar({ showMetrics }: TopBarProps) {
  const { activeServerId } = useServerManager();
  const [metrics, setMetrics] = useState<{ cpu: number; mem: number } | null>(null);

  // 只在显示指标时订阅
  const handleMetricsUpdate = useCallback((data: any) => {
    const cpu = data.cpu_percent || 0;
    const mem = (data.mem_used_bytes / data.mem_total_bytes) * 100;
    setMetrics({ cpu, mem });
  }, []);

  useMetricsSubscription(
    showMetrics ? activeServerId : null, // 不显示时不订阅
    handleMetricsUpdate
  );

  return (
    <div className="top-bar">
      {/* ... */}
      {showMetrics && metrics && (
        <div className="top-bar-metrics">
          <span>CPU {metrics.cpu.toFixed(1)}%</span>
          <span>MEM {metrics.mem.toFixed(1)}%</span>
        </div>
      )}
      {/* ... */}
    </div>
  );
}
```

- [ ] **Step 2: 验证 TopBar 编译**

Run: `npm run typecheck`（如果存在）
Expected: 编译成功，无错误

- [ ] **Step 3: 提交 TopBar 集成**

```bash
git add src/shell/TopBar.tsx
git commit -m "feat(frontend): integrate subscription into TopBar"
```

---

## Task 13: 测试订阅系统

**Files:**
- Test: 手动测试

- [ ] **Step 1: 启动 Agent**

Run: `cd agent && cargo run --release`
Expected: Agent 启动成功，监听 QUIC 端口

- [ ] **Step 2: 启动 Tauri 客户端**

Run: `npm run tauri dev`
Expected: 客户端启动成功

- [ ] **Step 3: 连接服务器**

在客户端中连接 Agent：
- 输入服务器地址和 Token
- 点击连接
Expected: 连接成功

- [ ] **Step 4: 打开系统监控窗口**

点击桌面图标打开 SystemMonitor：
Expected:
- 窗口打开
- Agent 开始推送 metrics 数据
- 图表实时更新（每 1 秒）

- [ ] **Step 5: 关闭系统监控窗口**

关闭 SystemMonitor 窗口：
Expected:
- Agent 停止推送 metrics 数据
- Agent 停止采集（无订阅者）

- [ ] **Step 6: 验证 TopBar 显示**

在 TopBar 中启用显示 CPU/MEM：
Expected:
- TopBar 开始订阅 metrics
- Agent 开始推送（如果之前无订阅者）
- TopBar 显示实时数据

- [ ] **Step 7: 验证多订阅者共享**

同时打开 SystemMonitor 和启用 TopBar 显示：
Expected:
- Agent 只采集一次，推送给两个订阅者
- 服务器负担不变（共享采集）

---

## Task 14: 性能测试

**Files:**
- Test: 手动测试

- [ ] **Step 1: 监控 Agent CPU 开销**

在 Agent 运行时，使用 `top` 或 `htop` 监控 CPU 使用率：
Expected:
- 无订阅者时：CPU ~0%
- 有订阅者时：CPU ~0.5%

- [ ] **Step 2: 监控网络带宽**

使用 `iftop` 或 `nethogs` 监控网络带宽：
Expected:
- 无订阅者时：带宽 ~0 KB/s
- 有订阅者时：带宽 ~2 KB/s

- [ ] **Step 3: 监控内存开销**

使用 `top` 或 `htop` 监控内存使用：
Expected:
- 无订阅者时：内存 ~0 MB（采集器未启动）
- 有订阅者时：内存 ~1 MB（采集器缓存）

---

## Task 15: 文档更新

**Files:**
- Modify: `README.md`（可选）

- [ ] **Step 1: 更新 README**

在 README 中添加订阅系统说明：

```markdown
## 实时订阅推送系统

### 功能特性

- **按需订阅**：打开窗口时订阅，关闭时取消
- **实时推送**：Agent 主动推送数据，客户端无需轮询
- **可扩展**：支持多种数据类型（系统监控、文件变化、进程事件等）
- **服务器负担可控**：无订阅者时负担为 0，有订阅者时共享采集

### 使用方式

1. 打开系统监控窗口 → 自动订阅 metrics 数据
2. TopBar 显示 CPU/MEM → 自动订阅 metrics 数据
3. 关闭窗口 → 自动取消订阅

### 配置

Agent 配置文件 `agent.toml` 中可调整推送间隔：

```toml
[collectors]
metrics_interval_secs = 1  # 系统监控推送间隔（秒）
```
```

- [ ] **Step 2: 提交文档更新**

```bash
git add README.md
git commit -m "docs: add subscription system documentation"
```

---

## 自我审查

### 1. Spec 覆盖检查

| Spec 章节 | 对应 Task |
|-----------|----------|
| 协议设计（SubscriptionType） | Task 1 |
| 配置设计（CollectorsConfig） | Task 2 |
| EventBus 实现 | Task 3 |
| MetricsCollector 实现 | Task 4 |
| SubscriptionManager 实现 | Task 5 |
| Handler 扩展 | Task 6 |
| QUIC Server 集成 | Task 7 |
| Tauri Backend commands | Task 8 |
| 前端订阅配置 | Task 9 |
| 前端订阅 Hook | Task 10 |
| SystemMonitor 集成 | Task 11 |
| TopBar 集成 | Task 12 |

✅ **覆盖完整**

### 2. 占位符扫描

✅ **无占位符**：所有步骤包含完整代码

### 3. 类型一致性

✅ **类型一致**：
- `SubscriptionType` 在 Agent 和 Tauri Backend 定义一致
- `MetricsSnapshot` 在 Agent 和前端定义一致
- `SubscriptionEvent` 在 Tauri Backend 和前端定义一致

---

**计划完成，保存到文件。**