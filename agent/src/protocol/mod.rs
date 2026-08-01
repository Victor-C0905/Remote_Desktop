//! IPC 协议模块
//!
//! 该模块封装了 Protobuf 生成的消息类型，提供易用的 Rust API。
//!
//! ## 模块结构
//!
//! - `generated`: Protobuf 自动生成的代码（基于 `agent.proto`）
//! - `serde`: 基于 Serde 的旧实现（用于 JSON 序列化）
//!
//! ## 使用方式
//!
//! ```rust
//! use protocol::{ManagerRequest, WorkerResponse};
//! ```

// 导入自动生成的 Protobuf 代码
pub mod generated;

// 保留旧的 Serde 实现（向后兼容）
pub mod serde;

// 默认导出 Serde 类型（当前实现）
pub use serde::{
    Envelope, Payload, SubscriptionType, MetricsSnapshot,
    MountInfo, FileEntry, DiskInfo,
};

/// 辅助函数：创建错误 Payload
///
/// # 参数
///
/// - `code`: 错误码
/// - `message`: 错误信息
///
/// # 返回值
///
/// 返回一个 `Payload::Error`
#[allow(dead_code)]
pub fn create_error_payload(code: i32, message: String) -> Payload {
    Payload::Error { code, message }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_error_payload() {
        let payload = create_error_payload(404, "Not found".to_string());

        if let Payload::Error { code, message } = payload {
            assert_eq!(code, 404);
            assert_eq!(message, "Not found");
        } else {
            panic!("Expected Error payload");
        }
    }
}