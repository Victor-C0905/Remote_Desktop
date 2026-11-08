//! 长度前缀帧读取（JSON envelope / 代理 / 订阅通道通用）
//!
//! 线上格式：`[4B LE 长度][消息体]`。长度前缀必须在分配内存前封顶校验：
//! 恶意或失步的对端发送 `len ≈ 0xFFFFFFFF` 的帧会触发 ~4GB 一次性分配
//! （OOM / 卡死），release 构建 `panic="abort"` 下越界访问更会全进程崩溃
//! （2026-11 审计 M11）。文件传输 RAW 帧另有专用上限
//! （transfer.rs `RAW_MAX_FRAME_LEN` = 10MB），不经过本模块。

use quinn::RecvStream;

/// 长度前缀帧的分配上限（64MB）。
///
/// 余量依据：JSON envelope 载荷含 base64 编码的整文件内容
/// （read_file_resp）、全量目录/进程列表等合法大载荷，64MB 可覆盖，
/// 同时把单帧最大分配从 ~4GB 压到 64MB。
pub(crate) const MAX_LEN_PREFIX_MESSAGE: usize = 64 * 1024 * 1024;

/// 校验长度前缀合法性；非法返回 Err（含原因描述）
pub(crate) fn check_len_prefix(len: usize) -> Result<(), String> {
    if len == 0 || len > MAX_LEN_PREFIX_MESSAGE {
        return Err(format!(
            "frame length invalid: {} (要求 0 < len <= {})",
            len, MAX_LEN_PREFIX_MESSAGE
        ));
    }
    Ok(())
}

/// 读取 `[4B LE 长度][消息体]` 帧：校验长度 → 分配 → 读满。
///
/// 对端正常结束发送端（FinishedEarly）返回 `Ok(None)`；
/// 其余失败返回 Err（含长度校验失败）。
pub(crate) async fn read_len_prefixed(recv: &mut RecvStream) -> Result<Option<Vec<u8>>, String> {
    let mut len_buf = [0u8; 4];
    match recv.read_exact(&mut len_buf).await {
        Ok(()) => {}
        Err(quinn::ReadExactError::FinishedEarly(_)) => return Ok(None),
        Err(e) => return Err(format!("read frame length: {e}")),
    }
    let len = u32::from_le_bytes(len_buf) as usize;
    check_len_prefix(len)?;
    let mut data = vec![0u8; len];
    recv.read_exact(&mut data)
        .await
        .map_err(|e| format!("read frame body: {e}"))?;
    Ok(Some(data))
}

#[cfg(test)]
mod tests {
    use super::check_len_prefix;

    #[test]
    fn check_len_prefix_accepts_sane_sizes() {
        assert!(check_len_prefix(1).is_ok());
        assert!(check_len_prefix(256 * 1024).is_ok());
        assert!(check_len_prefix(super::MAX_LEN_PREFIX_MESSAGE).is_ok());
    }

    #[test]
    fn check_len_prefix_rejects_zero_and_overflow() {
        // 0 长度帧非法（协议失步特征）
        assert!(check_len_prefix(0).is_err());
        // u32::from_le_bytes 全 1（= 0xFFFFFFFF as usize，64 位下约 4GB）必须被拒
        assert!(check_len_prefix(0xFFFF_FFFFusize).is_err());
        // 超上限一位也拒
        assert!(check_len_prefix(super::MAX_LEN_PREFIX_MESSAGE + 1).is_err());
    }
}
