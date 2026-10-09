// agent/src/protocol/raw_frame.rs
//! 裸二进制帧编解码(数据平面)
//!
//! 帧格式: [4B length LE][1B type][body]
//!   length = 1 + body.len()
//!   type = 0x01 控制(JSON Envelope) / 0x02 数据块

use anyhow::{Result, anyhow};
use quinn::{RecvStream, SendStream};

/// 帧类型
pub const TYPE_CONTROL: u8 = 0x01;
pub const TYPE_DATA: u8 = 0x02;

/// 单帧最大长度(含 type + body),防止内存攻击
const MAX_FRAME_LEN: u32 = 10 * 1024 * 1024;

/// 数据块(裸帧承载)
#[derive(Debug, Clone, PartialEq)]
pub struct RawChunk {
    pub seq: u32,
    pub data: Vec<u8>, // 原始字节,不经 base64
}

/// 编码帧头: [4B len LE][1B type],len = 1 + body_len
fn encode_frame_header(type_byte: u8, body_len: usize) -> [u8; 5] {
    let len = (1 + body_len as u32).to_le_bytes();
    [len[0], len[1], len[2], len[3], type_byte]
}

/// 解码帧头: [4B len LE][1B type],校验长度合法性
fn decode_frame_header(buf: &[u8; 5]) -> Result<(u32, u8)> {
    let len = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    if len == 0 || len > MAX_FRAME_LEN {
        return Err(anyhow!("帧长度非法: {}", len));
    }
    Ok((len, buf[4]))
}

/// 读取 4B LE 长度 + 1B type
async fn read_frame_header(recv: &mut RecvStream) -> Result<(u32, u8)> {
    let mut buf = [0u8; 5];
    recv.read_exact(&mut buf).await?;
    decode_frame_header(&buf)
}

/// 写帧: [4B len][1B type][body]
async fn write_frame(send: &mut SendStream, type_byte: u8, body: &[u8]) -> Result<()> {
    let header = encode_frame_header(type_byte, body.len());
    send.write_all(&header).await?;
    send.write_all(body).await?;
    Ok(())
}

/// 编码数据块 body: [4B seq][4B size][data]
fn encode_data_chunk_body(chunk: &RawChunk) -> Vec<u8> {
    let mut body = Vec::with_capacity(8 + chunk.data.len());
    body.extend_from_slice(&chunk.seq.to_le_bytes());
    body.extend_from_slice(&(chunk.data.len() as u32).to_le_bytes());
    body.extend_from_slice(&chunk.data);
    body
}

/// 解码数据块 body: [4B seq][4B size][data],校验 size 与实际长度
fn decode_data_chunk_body(buf: &[u8]) -> Result<RawChunk> {
    if buf.len() < 8 {
        return Err(anyhow!("数据帧 body 过短: {}", buf.len()));
    }
    let seq = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    let size = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
    let data = buf[8..].to_vec();
    if data.len() as u32 != size {
        return Err(anyhow!("数据帧 size 不匹配: 声明 {} 实际 {}", size, data.len()));
    }
    Ok(RawChunk { seq, data })
}

/// 写数据块帧
pub async fn write_data_chunk(send: &mut SendStream, chunk: &RawChunk) -> Result<()> {
    let body = encode_data_chunk_body(chunk);
    write_frame(send, TYPE_DATA, &body).await
}

/// 读数据块帧(假设 header 已读为 TYPE_DATA)
pub async fn read_data_chunk_body(recv: &mut RecvStream, body_len: usize) -> Result<RawChunk> {
    let mut buf = vec![0u8; body_len];
    recv.read_exact(&mut buf).await?;
    decode_data_chunk_body(&buf)
}

/// 读任意帧(返回 type 与剩余 body 长度,由调用方按 type 分发)
pub async fn read_frame(recv: &mut RecvStream) -> Result<(u8, usize)> {
    let (len, type_byte) = read_frame_header(recv).await?;
    Ok((type_byte, len as usize - 1))
}

/// 读控制帧 body(JSON,调用方再 Envelope::decode)
pub async fn read_control_body(recv: &mut RecvStream, body_len: usize) -> Result<Vec<u8>> {
    let mut buf = vec![0u8; body_len];
    recv.read_exact(&mut buf).await?;
    Ok(buf)
}

/// 写控制帧(JSON Envelope body)
pub async fn write_control_frame(send: &mut SendStream, body: &[u8]) -> Result<()> {
    write_frame(send, TYPE_CONTROL, body).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trip: encode -> decode 应保持等价
    #[test]
    fn test_data_chunk_round_trip() {
        let chunk = RawChunk { seq: 42, data: vec![0xDE, 0xAD, 0xBE, 0xEF] };
        let body = encode_data_chunk_body(&chunk);
        let decoded = decode_data_chunk_body(&body).expect("decode 应成功");
        assert_eq!(decoded, chunk);
    }

    /// 空数据(data.len()==0,size 字段==0,body_len 必须 >= 8)应正确编解码
    #[test]
    fn test_data_chunk_empty_data() {
        let chunk = RawChunk { seq: 0, data: vec![] };
        let body = encode_data_chunk_body(&chunk);
        // body 至少 8 字节(seq + size),size == 0
        assert_eq!(body.len(), 8);
        let decoded = decode_data_chunk_body(&body).expect("空数据应正确解码");
        assert_eq!(decoded, chunk);
        assert!(decoded.data.is_empty());
    }

    /// 声明 size 与 data.len() 不匹配应返回错误
    #[test]
    fn test_data_chunk_size_mismatch() {
        // 声明 size=10,但实际 data 只有 4 字节
        let mut body = Vec::new();
        body.extend_from_slice(&1u32.to_le_bytes()); // seq
        body.extend_from_slice(&10u32.to_le_bytes()); // size = 10 (错误)
        body.extend_from_slice(&[0xAA; 4]); // 实际 4 字节
        let err = decode_data_chunk_body(&body).err();
        assert!(err.is_some(), "size 不匹配应返回错误");
    }

    /// body 过短(< 8 字节)应返回错误
    #[test]
    fn test_data_chunk_body_too_short() {
        let body = [0u8; 4]; // < 8
        let err = decode_data_chunk_body(&body).err();
        assert!(err.is_some(), "body 过短应返回错误");
    }

    /// 帧长度 == 0 应非法
    #[test]
    fn test_frame_header_zero_length_invalid() {
        let mut buf = [0u8; 5];
        buf[0..4].copy_from_slice(&0u32.to_le_bytes());
        buf[4] = TYPE_CONTROL;
        assert!(decode_frame_header(&buf).is_err(), "len=0 应非法");
    }

    /// 帧长度 > MAX_FRAME_LEN 应非法
    #[test]
    fn test_frame_header_over_max_invalid() {
        let mut buf = [0u8; 5];
        let over = MAX_FRAME_LEN + 1;
        buf[0..4].copy_from_slice(&over.to_le_bytes());
        buf[4] = TYPE_DATA;
        assert!(decode_frame_header(&buf).is_err(), "len > MAX 应非法");
    }

    /// 合法帧头应正确编解码
    #[test]
    fn test_frame_header_round_trip() {
        let buf = encode_frame_header(TYPE_CONTROL, 100);
        let (len, type_byte) = decode_frame_header(&buf).expect("合法 header 应解码成功");
        assert_eq!(len, 101); // 1 + 100
        assert_eq!(type_byte, TYPE_CONTROL);
    }
}
