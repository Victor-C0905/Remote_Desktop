// 订阅类型:通用订阅系统(Subscribe/Unsubscribe/SubscribeAck)使用的
// SubscriptionType 定义
//
// 搬运自 agent/src/protocol/serde.rs。客户端 src-tauri/src/connection.rs
// 中的副本字段与 serde 属性完全一致(tag="type"/content="params"),无差异。

use serde::{Deserialize, Serialize};

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
    //
    // C 档想法（暂不实现，仅记录）：基于本订阅由 Agent 主动推送目录变化事件，
    // 客户端据此同步“其他终端/会话”对同一目录做的新增/删除/改名，覆盖本机
    // 操作之外的外部变更；需要 Agent 侧 inotify 监听、事件去重与前端合并策略。
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
    // ⚠️ 此处的字符串必须与各变体的 serde rename 保持一致（手工同步，改动时务必两边同改）
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
