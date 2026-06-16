# 实时订阅推送系统设计

> 版本: v1.0 | 日期: 2026-06-16
>
> 核心理念：按需订阅，实时推送，可扩展架构

---

## 一、设计目标

### 1.1 核心需求

| 需求 | 说明 |
|------|------|
| **按需订阅** | 打开窗口时订阅，关闭时取消，无订阅者时停止采集 |
| **实时推送** | Agent 主动推送数据，客户端无需轮询 |
| **可扩展** | 支持多种数据类型：系统监控、文件变化、进程事件、应用日志等 |
| **统一配置** | 推送间隔、订阅数量等参数统一管理，可快速调整 |
| **服务器负担可控** | 无订阅者时负担为 0，有订阅者时共享采集结果 |

### 1.2 适用场景

| 场景 | 订阅类型 | 推送频率 |
|------|---------|---------|
| **系统监控窗口** | `metrics` | 1 秒间隔 |
| **右上角 TopBar 显示** | `metrics` | 1 秒间隔 |
| **文件管理器** | `file_changes` | 实时（文件修改时） |
| **进程监控** | `process_events` | 2 秒间隔 |
| **应用日志查看** | `app_logs` | 实时（日志追加时） |
| **服务状态监控** | `service_status` | 5 秒间隔 |

---

## 二、架构设计

### 2.1 整体架构

```
┌─ 客户端 (React + Tauri) ─────────────────────────────────────┐
│                                                               │
│  ┌─ UI 组件 ───────────────────────────────────────────────┐ │
│  │  onMount: invoke("subscribe", { types })                │ │
│  │  onUnmount: invoke("unsubscribe", { types })            │ │
│  │  listen("event"): 更新 UI                               │ │
│  └──────────────────────────────────────────────────────────┘ │
│                                                               │
└───────────────────────────────────────────────────────────────┘
                              │
                      Tauri invoke / Event
                              │
┌─ Tauri Backend (Rust) ────────────────────────────────────────┐
│                                                               │
│  ┌─ subscribe command ──────────────────────────────────────┐ │
│  │  发送 Subscribe payload 到 Agent                         │ │
│  │  监听 Agent 推送，转发到前端 Tauri Event                 │ │
│  └───────────────────────────────────────────────────────────┘ │
│                                                               │
└───────────────────────────────────────────────────────────────┘
                              │
                      QUIC Stream (持久连接)
                              │
┌─ Agent (Rust) ────────────────────────────────────────────────┐
│                                                               │
│  ┌─ EventBus (事件总线) ────────────────────────────────────┐ │
│  │  publish(event_type, data)                               │ │
│  │  subscribe(event_type, callback)                         │ │
│  │  unsubscribe(event_type)                                 │ │
│  └───────────────────────────────────────────────────────────┘ │
│                                                               │
│  ┌─ SubscriptionManager (订阅管理) ─────────────────────────┐ │
│  │  subscribers: HashMap<StreamId, Vec<SubscriptionType>>   │ │
│  │  subscribe(stream_id, types): 添加订阅                   │ │
│  │  unsubscribe(stream_id, types): 移除订阅                 │ │
│  │  route(event): 根据事件类型推送到订阅者                   │ │
│  └───────────────────────────────────────────────────────────┘ │
│                                                               │
│  ┌─ DataCollectors (数据采集器) ────────────────────────────┐ │
│  │                                                           │ │
│  │  ┌─ MetricsCollector ──────────────────────────────────┐ │ │
│  │  │  定时采集系统指标 → EventBus.publish("metrics")    │ │ │
│  │  │  interval: 1 秒（可配置）                           │ │ │
│  │  │  无订阅者时停止采集                                 │ │ │
│  │  └─────────────────────────────────────────────────────┘ │ │
│  │                                                           │ │
│  │  ┌─ FileWatcher ──────────────────────────────────────┐ │ │
│  │  │  监听文件变化 → EventBus.publish("file_changes")   │ │ │
│  │  │  使用 notify crate (inotify/kqueue)                │ │ │
│  │  │  支持递归监听目录                                   │ │ │
│  │  └─────────────────────────────────────────────────────┘ │ │
│  │                                                           │ │
│  │  ┌─ ProcessMonitor ───────────────────────────────────┐ │ │
│  │  │  监听进程事件 → EventBus.publish("process_events") │ │ │
│  │  │  使用 sysinfo + 定时扫描                            │ │ │
│  │  │  interval: 2 秒（可配置）                           │ │ │
│  │  └─────────────────────────────────────────────────────┘ │ │
│  │                                                           │ │
│  │  ┌─ AppLogCollector ──────────────────────────────────┐ │ │
│  │  │  读取应用日志 → EventBus.publish("app_logs")       │ │ │
│  │  │  tail -f 模式                                       │ │ │
│  │  │  支持日志级别过滤                                   │ │ │
│  │  └─────────────────────────────────────────────────────┘ │ │
│  │                                                           │ │
│  │  ┌─ ServiceMonitor ───────────────────────────────────┐ │ │
│  │  │  systemctl status → EventBus.publish("service_status") │ │
│  │  │  interval: 5 秒（可配置）                           │ │ │
│  │  └─────────────────────────────────────────────────────┘ │ │
│  │                                                           │ │
│  └───────────────────────────────────────────────────────────┘ │
│                                                               │
└───────────────────────────────────────────────────────────────┘
```

---

## 三、协议设计

### 3.1 订阅类型定义

```rust
// agent/src/protocol.rs

/// 订阅类型枚举
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "params")]
pub enum SubscriptionType {
    // 系统监控
    #[serde(rename = "metrics")]
    Metrics {
        interval_secs: Option<u64>, // 可选，默认使用配置值
    },

    // 文件变化监控
    #[serde(rename = "file_changes")]
    FileChanges {
        path: String,              // 监听路径
        recursive: Option<bool>,   // 是否递归监听
    },

    // 进程事件（启动/停止）
    #[serde(rename = "process_events")]
    ProcessEvents {
        interval_secs: Option<u64>,
    },

    // 应用日志
    #[serde(rename = "app_logs")]
    AppLogs {
        app_name: String,          // 应用名称
        level: Option<String>,     // 日志级别过滤
    },

    // 系统服务状态
    #[serde(rename = "service_status")]
    ServiceStatus {
        service: String,           // 服务名称
        interval_secs: Option<u64>,
    },

    // 未来扩展...
}
```

### 3.2 Payload 类型

```rust
// agent/src/protocol.rs

pub enum Payload {
    // 通用订阅
    #[serde(rename = "subscribe")]
    Subscribe {
        server_id: String,
        types: Vec<SubscriptionType>, // 支持同时订阅多种类型
    },

    // 通用取消订阅
    #[serde(rename = "unsubscribe")]
    Unsubscribe {
        server_id: String,
        types: Vec<SubscriptionType>, // 支持取消部分订阅
    },

    // 通用事件推送
    #[serde(rename = "event")]
    Event {
        event_type: String,           // "metrics" / "file_changes" / ...
        data: serde_json::Value,      // 事件数据（动态类型）
        timestamp: u64,               // 事件时间戳
    },

    // 订阅确认
    #[serde(rename = "subscribe_ack")]
    SubscribeAck {
        success: bool,
        subscribed_types: Vec<SubscriptionType>,
    },

    // 取消订阅确认
    #[serde(rename = "unsubscribe_ack")]
    UnsubscribeAck {
        success: bool,
    },

    // 现有类型保持不变...
}
```

---

## 四、数据流设计

### 4.1 订阅流程

```
1. 客户端打开窗口
   → invoke("subscribe", { serverId: "prod-server", types: [...] })

2. Tauri Backend
   → 通过 QUIC Stream 发送 Subscribe payload
   → 记录 Stream ID，监听 Agent 推送

3. Agent SubscriptionManager
   → 收到 Subscribe payload
   → 记录订阅者（StreamId + SubscriptionType）
   → 启动对应的 DataCollector（如果未启动）

4. Agent DataCollector
   → 后台定时采集数据
   → EventBus.publish(event_type, data)

5. Agent SubscriptionManager
   → EventBus 收到事件
   → 根据事件类型查找订阅者
   → 推送 Event payload 到订阅者的 QUIC Stream

6. Tauri Backend
   → 收到 Event payload
   → 转发到前端 Tauri Event

7. 客户端
   → listen("event", callback)
   → 更新 UI（图表、列表等）
```

### 4.2 取消订阅流程

```
1. 客户端关闭窗口
   → invoke("unsubscribe", { serverId: "prod-server", types: [...] })

2. Tauri Backend
   → 发送 Unsubscribe payload 到 Agent

3. Agent SubscriptionManager
   → 移除订阅者
   → 检查是否还有其他订阅者订阅该类型
   → 无订阅者时停止对应的 DataCollector

4. Agent DataCollector
   → 停止后台采集
   → 释放资源
```

---

## 五、配置设计

### 5.1 Agent 配置

```toml
# agent.toml

[collectors]
# 系统监控采集间隔（秒）
metrics_interval_secs = 1

# 文件变化监听延迟（毫秒，避免频繁触发）
file_changes_delay_ms = 100

# 进程扫描间隔（秒）
process_scan_interval_secs = 2

# 服务状态检查间隔（秒）
service_status_interval_secs = 5

[limits]
# 最大订阅者数量（防止资源耗尽）
max_subscribers = 50

# 每个订阅者最大订阅类型数量
max_subscription_types = 10

# 事件推送批量大小（减少网络开销）
event_batch_size = 10
```

### 5.2 客户端配置

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
```

---

## 六、实现细节

### 6.1 Agent 侧目录结构

```
agent/src/
  ├── collectors/
  │   ├── mod.rs           # 采集器模块入口
  │   ├── metrics.rs       # 系统监控采集器
  │   ├── file_watcher.rs  # 文件变化监听器
  │   ├── process.rs       # 进程事件监听器
  │   ├── logs.rs          # 应用日志采集器
  │   └── service.rs       # 服务状态监控器
  ├── event_bus.rs         # 事件总线
  ├── subscription.rs      # 订阅管理器
  ├── protocol.rs          # 协议定义（扩展）
  ├── handler.rs           # 消息处理（扩展）
  └── main.rs              # 主入口（集成）
```

### 6.2 核心模块实现

**EventBus（事件总线）：**

```rust
// agent/src/event_bus.rs

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

type EventCallback = Arc<dyn Fn(serde_json::Value) + Send + Sync>;

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
    pub async fn publish(&self, event_type: &str, data: serde_json::Value) {
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

    /// 取消订阅
    pub async fn unsubscribe(&self, event_type: &str, callback: &EventCallback) {
        let mut subs = self.subscribers.write().await;
        if let Some(callbacks) = subs.get_mut(event_type) {
            callbacks.retain(|cb| cb != callback);
        }
    }
}
```

**SubscriptionManager（订阅管理器）：**

```rust
// agent/src/subscription.rs

use std::collections::HashMap;
use quinn::SendStream;
use crate::protocol::{SubscriptionType, Payload, Envelope};

pub struct SubscriptionManager {
    // StreamId -> 订阅类型列表
    subscribers: HashMap<u64, Vec<SubscriptionType>>,

    // StreamId -> QUIC Stream 发送器
    stream_senders: HashMap<u64, SendStream>,

    // 订阅类型 -> 订阅者数量
    type_counts: HashMap<String, usize>,

    event_bus: Arc<EventBus>,
}

impl SubscriptionManager {
    /// 添加订阅
    pub async fn subscribe(
        &mut self,
        stream_id: u64,
        types: Vec<SubscriptionType>,
        stream: SendStream,
    ) -> Result<(), String> {
        // 记录订阅者
        self.subscribers.insert(stream_id, types.clone());
        self.stream_senders.insert(stream_id, stream);

        // 更新订阅类型计数
        for t in &types {
            let type_name = t.type_name();
            let count = self.type_counts.entry(type_name).or_insert(0);
            *count += 1;

            // 如果是第一个订阅者，启动对应的采集器
            if *count == 1 {
                self.start_collector(t)?;
            }
        }

        // 发送订阅确认
        self.send_ack(stream_id, true, types.clone()).await?;

        Ok(())
    }

    /// 移除订阅
    pub async fn unsubscribe(
        &mut self,
        stream_id: u64,
        types: Vec<SubscriptionType>,
    ) -> Result<(), String> {
        // 移除订阅者
        if let Some(subscribed_types) = self.subscribers.get_mut(&stream_id) {
            for t in &types {
                subscribed_types.retain(|st| st != t);

                // 更新订阅类型计数
                let type_name = t.type_name();
                if let Some(count) = self.type_counts.get_mut(&type_name) {
                    *count -= 1;

                    // 如果无订阅者，停止对应的采集器
                    if *count == 0 {
                        self.stop_collector(t)?;
                    }
                }
            }
        }

        // 发送取消订阅确认
        self.send_ack(stream_id, false, types).await?;

        Ok(())
    }

    /// 推送事件到订阅者
    pub async fn route_event(
        &self,
        event_type: &str,
        data: serde_json::Value,
    ) {
        for (stream_id, types) in &self.subscribers {
            // 检查订阅者是否订阅了该事件类型
            if types.iter().any(|t| t.type_name() == event_type) {
                // 推送事件
                let payload = Payload::Event {
                    event_type: event_type.to_string(),
                    data,
                    timestamp: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as u64,
                };

                if let Some(stream) = self.stream_senders.get(stream_id) {
                    let envelope = Envelope::new(0, payload);
                    if let Ok(encoded) = envelope.encode() {
                        // 发送数据到 QUIC Stream
                        stream.write_all(&encoded).await.ok();
                    }
                }
            }
        }
    }

    /// 启动采集器
    fn start_collector(&self, type: &SubscriptionType) -> Result<(), String> {
        // 根据订阅类型启动对应的采集器
        // ...
    }

    /// 停止采集器
    fn stop_collector(&self, type: &SubscriptionType) -> Result<(), String> {
        // 根据订阅类型停止对应的采集器
        // ...
    }
}
```

**MetricsCollector（系统监控采集器）：**

```rust
// agent/src/collectors/metrics.rs

use sysinfo::{System, Disks, Networks};
use tokio::time::{sleep, Duration};

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
        let mut running = self.running.write().await;
        *running = true;

        tokio::spawn(async move {
            loop {
                // 检查是否有订阅者
                if !*running {
                    break;
                }

                // 采集数据
                let metrics = self.collect();

                // 发布事件
                self.event_bus.publish("metrics", metrics).await;

                // 等待下一次采集
                sleep(Duration::from_secs(self.interval_secs)).await;
            }
        });
    }

    /// 停止采集
    pub async fn stop(&self) {
        let mut running = self.running.write().await;
        *running = false;
    }

    /// 采集系统指标
    fn collect(&self) -> serde_json::Value {
        let mut sys = System::new_all();
        sys.refresh_all();

        serde_json::json!({
            "cpu_percent": sys.global_cpu_usage(),
            "mem_used_bytes": sys.used_memory(),
            "mem_total_bytes": sys.total_memory(),
            "swap_used_bytes": sys.used_swap(),
            "uptime_secs": System::uptime(),
            // ...
        })
    }
}
```

**FileWatcher（文件变化监听器）：**

```rust
// agent/src/collectors/file_watcher.rs

use notify::{RecommendedWatcher, RecursiveMode, Watcher, Event};
use std::path::Path;

pub struct FileWatcher {
    watcher: RecommendedWatcher,
    event_bus: Arc<EventBus>,
}

impl FileWatcher {
    pub fn new(event_bus: Arc<EventBus>) -> Self {
        // 创建文件监听器
        let watcher = notify::recommended_watcher(|event: Result<Event, notify::Error>| {
            if let Ok(event) = event {
                // 发布文件变化事件
                event_bus.publish("file_changes", serde_json::json!({
                    "path": event.paths[0].to_string_lossy(),
                    "kind": event.kind.to_string(),
                })).await;
            }
        }).unwrap();

        Self { watcher, event_bus }
    }

    /// 监听路径
    pub fn watch(&mut self, path: &str, recursive: bool) -> Result<(), String> {
        let mode = if recursive {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };

        self.watcher.watch(Path::new(path), mode)
            .map_err(|e| e.to_string())
    }

    /// 停止监听
    pub fn unwatch(&mut self, path: &str) -> Result<(), String> {
        self.watcher.unwatch(Path::new(path))
            .map_err(|e| e.to_string())
    }
}
```

---

## 七、性能优化

### 7.1 采集器优化

| 优化项 | 说明 |
|-------|------|
| **无订阅者时停止采集** | 服务器负担为 0 |
| **共享采集结果** | 多订阅者共享，不重复采集 |
| **缓存推送** | 避免每次请求都采集 |
| **增量更新** | 只推送变化的数据 |

### 7.2 推送优化

| 优化项 | 说明 |
|-------|------|
| **批量推送** | 合并多个事件，减少网络开销 |
| **压缩传输** | 使用 gzip 压缩 JSON 数据 |
| **Stream 多路复用** | 一个 QUIC 连接承载多个订阅 |

### 7.3 性能指标

| 场景 | CPU 开销 | 内存开销 | 网络带宽 |
|------|---------|---------|---------|
| **无订阅者** | 0% | 0 MB | 0 KB/s |
| **1 个订阅者（metrics）** | ~0.5% | ~1 MB | ~2 KB/s |
| **10 个订阅者（metrics）** | ~0.5% | ~1 MB | ~20 KB/s |
| **文件监听（1 个路径）** | ~0.1% | ~0.5 MB | ~0.1 KB/s |

---

## 八、错误处理

### 8.1 错误场景

| 错误场景 | 处理方式 |
|---------|---------|
| **网络断开** | QUIC Stream 自动关闭，Agent 清理订阅者 |
| **订阅失败** | 返回 `SubscribeAck { success: false }`，客户端显示错误提示 |
| **采集失败** | Agent 记录日志，推送空数据或上次缓存 |
| **客户端超时** | Tauri Event 超时，客户端显示"连接中断" |
| **多客户端冲突** | 共享采集结果，不重复采集 |
| **订阅数量超限** | 返回错误，拒绝订阅 |

### 8.2 错误恢复

```
网络断开 → QUIC Stream 关闭 → Agent 清理订阅者
客户端重连 → 重新订阅 → Agent 启动采集器
```

---

## 九、测试策略

### 9.1 单元测试

| 测试内容 | 测试方法 |
|---------|---------|
| **MetricsCollector 采集准确性** | Mock sysinfo，验证数据格式 |
| **FileWatcher 监听准确性** | 创建临时文件，验证事件触发 |
| **SubscriptionManager 订阅管理** | 添加/移除订阅者，验证计数 |

### 9.2 集成测试

| 测试内容 | 测试方法 |
|---------|---------|
| **订阅/取消订阅流程** | 客户端订阅，验证 Agent 推送 |
| **多订阅者同步** | 多客户端订阅，验证共享采集 |
| **网络断开恢复** | 断开 QUIC 连接，验证订阅清理 |

### 9.3 性能测试

| 测试内容 | 测试方法 |
|---------|---------|
| **服务器负担** | 监控 CPU、内存、带宽 |
| **推送延迟** | 测量 Agent 推送到客户端接收的时间 |
| **并发订阅** | 50 个订阅者同时订阅，验证稳定性 |

---

## 十、扩展性设计

### 10.1 添加新订阅类型

**步骤：**

1. **定义 SubscriptionType**：在 `protocol.rs` 中添加新类型
2. **实现 DataCollector**：创建新的采集器，发布事件到 EventBus
3. **客户端订阅**：前端调用 `subscribe` API
4. **无需修改核心架构**：SubscriptionManager 和 EventBus 自动处理

**示例：添加网络流量监控**

```rust
// 1. 定义订阅类型
#[serde(rename = "network_traffic")]
NetworkTraffic {
    interface: String,
    interval_secs: Option<u64>,
}

// 2. 实现采集器
pub struct NetworkTrafficCollector {
    interface: String,
    event_bus: Arc<EventBus>,
}

impl NetworkTrafficCollector {
    fn collect(&self) -> serde_json::Value {
        // 采集网络流量...
    }
}

// 3. 客户端订阅
invoke("subscribe", {
  types: [{ type: "network_traffic", interface: "eth0" }]
});
```

---

## 十一、与现有架构的集成

### 11.1 与 QUIC Server 的集成

```rust
// agent/src/server/quic.rs

impl QuicServer {
    async fn handle_stream(&self, stream: quinn::NewStream) {
        match stream {
            quinn::NewStream::Bi(send, recv) => {
                // 处理双向 Stream（订阅/取消订阅）
                self.handle_bi_stream(send, recv).await;
            }
            quinn::NewStream::Uni(recv) => {
                // 处理单向 Stream（事件推送）
                self.handle_uni_stream(recv).await;
            }
        }
    }

    async fn handle_bi_stream(&self, send: SendStream, recv: RecvStream) {
        // 读取客户端请求
        let data = recv.read_to_end(1024).await.unwrap();
        let envelope = Envelope::decode(&data).unwrap();

        match envelope.payload {
            Payload::Subscribe { server_id, types } => {
                // 添加订阅
                self.subscription_manager.subscribe(
                    stream_id,
                    types,
                    send,
                ).await;
            }
            Payload::Unsubscribe { server_id, types } => {
                // 移除订阅
                self.subscription_manager.unsubscribe(
                    stream_id,
                    types,
                ).await;
            }
            // ...
        }
    }
}
```

### 11.2 与 Tauri Backend 的集成

```rust
// src-tauri/src/lib.rs

#[tauri::command]
async fn subscribe(
    server_id: String,
    types: Vec<SubscriptionType>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    // 获取 QUIC 连接
    let connection = get_connection(&server_id)?;

    // 创建双向 Stream
    let (send, recv) = connection.open_bi().await?;

    // 发送 Subscribe payload
    let payload = Payload::Subscribe { server_id, types };
    let envelope = Envelope::new(0, payload);
    send.write_all(&envelope.encode()?).await?;

    // 监听 Agent 推送
    tokio::spawn(async move {
        loop {
            let data = recv.read_to_end(1024).await?;
            let envelope = Envelope::decode(&data)?;

            if let Payload::Event { event_type, data, .. } = envelope.payload {
                // 转发到前端 Tauri Event
                app.emit("event", EventPayload {
                    event_type,
                    data,
                }).ok();
            }
        }
    });

    Ok(())
}

#[tauri::command]
async fn unsubscribe(
    server_id: String,
    types: Vec<SubscriptionType>,
) -> Result<(), String> {
    // 发送 Unsubscribe payload
    // ...
}
```

---

## 十二、实施路线

### 阶段 1：核心架构（1-2 周）

| 任务 | 产出 |
|------|------|
| EventBus 实现 | `agent/src/event_bus.rs` |
| SubscriptionManager 实现 | `agent/src/subscription.rs` |
| 协议扩展 | `agent/src/protocol.rs`（新增 Subscribe/Unsubscribe/Event） |
| Tauri Backend 集成 | `src-tauri/src/lib.rs`（subscribe/unsubscribe command） |

### 阶段 2：系统监控采集器（1 周）

| 任务 | 产出 |
|------|------|
| MetricsCollector 实现 | `agent/src/collectors/metrics.rs` |
| 客户端 SystemMonitor 集成 | `src/apps/SystemMonitor.tsx`（使用 subscribe API） |
| TopBar 集成 | `src/shell/TopBar.tsx`（显示 CPU/MEM 时订阅） |

### 阶段 3：文件变化监听器（1 周）

| 任务 | 产出 |
|------|------|
| FileWatcher 实现 | `agent/src/collectors/file_watcher.rs` |
| FileManager 集成 | `src/apps/FileManager.tsx`（自动刷新文件列表） |

### 阘段 4：其他采集器（按需）

| 任务 | 产出 |
|------|------|
| ProcessMonitor | 进程事件监听 |
| AppLogCollector | 应用日志采集 |
| ServiceMonitor | 服务状态监控 |

---

## 十三、总结

### 13.1 核心优势

| 优势 | 说明 |
|------|------|
| **按需订阅** | 无订阅者时负担为 0，有订阅者时共享采集 |
| **实时推送** | Agent 主动推送，客户端无需轮询 |
| **可扩展** | 添加新订阅类型无需修改核心架构 |
| **统一配置** | 推送间隔等参数统一管理，可快速调整 |
| **服务器负担可控** | 多订阅者共享采集结果，不增加负担 |

### 13.2 适用场景

- 系统监控窗口（CPU、内存、磁盘、网络）
- 右上角 TopBar 显示系统参数
- 文件管理器（自动刷新文件列表）
- 进程监控（进程启动/停止事件）
- 应用日志查看（实时日志流）
- 服务状态监控（systemctl status）

---

**设计完成，等待用户确认后进入实施阶段。**