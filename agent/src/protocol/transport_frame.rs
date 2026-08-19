//! TransportFrame 抽象(数据平面统一帧)
//!
//! 将 raw_frame 的裸二进制帧与 serde 的 JSON Envelope 统一为 `Frame` 枚举，
//! 使文件传输/PTY/订阅等高频数据流复用同一帧编解码路径。
//!
//! - `Frame::Control(Envelope)`: 低频控制消息(JSON)
//! - `Frame::Data(RawChunk)`: 高频数据块(裸字节,不经 base64)

use crate::protocol::raw_frame;
use crate::protocol::serde::Envelope;
use anyhow::Result;
use quinn::{RecvStream, SendStream};

/// 统一传输帧
pub enum Frame {
    /// 控制帧(JSON Envelope,低频)
    Control(Envelope),
    /// 数据帧(裸字节,高频)
    Data(raw_frame::RawChunk),
}

impl Frame {
    /// 从 RecvStream 读取一帧并解析为 Control 或 Data
    pub async fn read(recv: &mut RecvStream) -> Result<Frame> {
        let (type_byte, body_len) = raw_frame::read_frame(recv).await?;
        match type_byte {
            raw_frame::TYPE_CONTROL => {
                let body = raw_frame::read_control_body(recv, body_len).await?;
                let env = Envelope::decode(&body).map_err(|e| anyhow::anyhow!("{}", e))?;
                Ok(Frame::Control(env))
            }
            raw_frame::TYPE_DATA => {
                let chunk = raw_frame::read_data_chunk_body(recv, body_len).await?;
                Ok(Frame::Data(chunk))
            }
            _ => anyhow::bail!("未知帧类型: {}", type_byte),
        }
    }

    /// 写控制帧(JSON Envelope body)
    pub async fn write_control(send: &mut SendStream, env: &Envelope) -> Result<()> {
        let body = env.encode().map_err(|e| anyhow::anyhow!("{}", e))?;
        raw_frame::write_control_frame(send, &body).await
    }

    /// 写数据帧(裸字节 chunk)
    pub async fn write_data(send: &mut SendStream, chunk: &raw_frame::RawChunk) -> Result<()> {
        raw_frame::write_data_chunk(send, chunk).await
    }
}
