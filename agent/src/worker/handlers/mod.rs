//! Handlers 模块 - 处理各类 Manager 请求
//!
//! 该模块包含各类业务逻辑处理器：
//! - `session`: 会话管理（创建 PTY、调整终端大小）
//! - `file`: 文件操作（读取目录、读写文件）
//! - `command`: 命令执行
//! - `system`: 系统信息查询

pub mod command;
pub mod file;
pub mod session;
pub mod system;