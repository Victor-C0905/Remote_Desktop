//! Session ↔ Manager 通信协议
//!
//! 二进制帧格式: [1字节类型][4字节长度(big-endian)][数据]
//!
//! 协议不依赖 protobuf，因为 Session 进程是纯同步代码（fork 后 tokio 不可用），
//! 避免引入异步依赖。帧格式极简，由 Session 进程的 libc read/write 直接操作。

/// 消息类型（高 4 位是版本号 v1，低 4 位是类型）
pub mod msg_type {
    /// Manager → Session: 键盘输入字节流
    pub const PTY_INPUT: u8 = 0x01;
    /// Session → Manager: 终端输出字节流
    pub const PTY_OUTPUT: u8 = 0x02;
    /// Manager → Session: 窗口大小调整 (data: 4字节cols + 4字节rows)
    pub const RESIZE: u8 = 0x03;
    /// Session → Manager: PTY EOF (bash 退出)
    pub const EOF: u8 = 0x04;
    /// Manager → Session: 关闭会话
    pub const CLOSE: u8 = 0x05;
    /// 双向: 首次连接验证 (data: session_id 字符串)
    pub const HELLO: u8 = 0x06;
}

/// 帧头大小: 1字节类型 + 4字节长度
pub const FRAME_HEADER_SIZE: usize = 5;

/// 最大帧数据大小（防止异常大帧导致内存问题）
pub const MAX_FRAME_DATA_SIZE: usize = 64 * 1024;

/// Manager 断开后 Session 等待重连的超时（秒）
pub const RECONNECT_TIMEOUT_SECS: u32 = 30;

/// Abstract socket 名称前缀（不含 \0 前缀，nix::UnixAddr::new_abstract 会自动添加）
pub const SOCKET_PREFIX: &str = "gnome-remote-session-";

/// 生成 abstract socket 名称（不含 \0 前缀）
pub fn generate_socket_name(session_id: &str) -> String {
    format!("{}{}", SOCKET_PREFIX, session_id)
}

/// 验证 abstract socket 名称是否有效
pub fn is_valid_socket_name(name: &str) -> bool {
    name.starts_with(SOCKET_PREFIX) && name.len() > SOCKET_PREFIX.len()
}
