// agent/examples/probe.rs
// 协议级探针：Windows 宿主直连 WSL 中的 quireld，做真机自动测试。
//
// 用法（仓库根，Windows）：
//   cargo run --example probe -p quireld -- auth --host 127.0.0.1 --user probeuser --password ***
//   cargo run --example probe -p quireld -- readdir --host ... --path /usr/lib
//   cargo run --example probe -p quireld -- terminal --host ...
//   cargo run --example probe -p quireld -- transfer-down --host ... --path /bin/bash
//   cargo run --example probe -p quireld -- transfer-up --host ... --size 1048576
//   cargo run --example probe -p quireld -- smoke --host ... --user ... --password ...
//
// 每个子命令输出 `PROBE PASS/FAIL: <名>` 并以退出码反映结果，供脚本聚合。
// 证书校验：测试工具跳过（对端为自签名 quireld）。

use std::sync::Arc;
use std::time::{Duration, Instant};

use quinn::RecvStream;
use rustls::pki_types::CertificateDer;

use quirel_protocol::{Envelope, Payload};

// ── 连接层 ──────────────────────────────────────────────────────────────

#[derive(Debug)]
struct ProbeVerifier;

impl rustls::client::danger::ServerCertVerifier for ProbeVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        // 测试探针：自签名对端，跳过链校验
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        // 与产品客户端一致：真实验证握手签名（S1 修复语义）
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

async fn connect(host: &str, port: u16) -> Result<quinn::Connection, String> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let crypto = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(ProbeVerifier))
        .with_no_client_auth();
    let mut transport = quinn::TransportConfig::default();
    transport.max_idle_timeout(Some(
        quinn::IdleTimeout::try_from(Duration::from_secs(30)).unwrap(),
    ));
    let mut client = quinn::ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(crypto)
            .map_err(|e| format!("tls config: {e}"))?,
    ));
    client.transport_config(Arc::new(transport));

    let mut endpoint = quinn::Endpoint::client("0.0.0.0:0".parse().unwrap())
        .map_err(|e| format!("endpoint: {e}"))?;
    endpoint.set_default_client_config(client);

    let addr: std::net::SocketAddr = format!("{host}:{port}")
        .parse()
        .map_err(|e| format!("addr: {e}"))?;
    let connecting = endpoint
        .connect(addr, "quirel")
        .map_err(|e| format!("connect: {e}"))?;
    let conn = tokio::time::timeout(Duration::from_secs(10), connecting)
        .await
        .map_err(|_| "连接超时(10s)".to_string())?
        .map_err(|e| format!("握手失败: {e}"))?;
    Ok(conn)
}

/// 长度前缀帧读写（[4B LE len][JSON envelope]，与 agent write_message 一致）
async fn write_msg(
    send: &mut quinn::SendStream,
    env: &Envelope,
) -> Result<(), String> {
    let bytes = env.encode().map_err(|e| e)?;
    let len = (bytes.len() as u32).to_le_bytes();
    send.write_all(&len).await.map_err(|e| e.to_string())?;
    send.write_all(&bytes).await.map_err(|e| e.to_string())?;
    Ok(())
}

async fn read_msg(recv: &mut RecvStream) -> Result<Option<Envelope>, String> {
    let mut len_buf = [0u8; 4];
    match recv.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(quinn::ReadExactError::FinishedEarly(_)) => return Ok(None),
        Err(e) => return Err(format!("读长度: {e}")),
    }
    let len = u32::from_le_bytes(len_buf) as usize;
    if len == 0 || len > 16 * 1024 * 1024 {
        return Err(format!("帧长度非法: {len}"));
    }
    let mut buf = vec![0u8; len];
    recv.read_exact(&mut buf).await.map_err(|e| format!("读体: {e}"))?;
    Envelope::decode(&buf).map(Some).map_err(|e| format!("解码: {e}"))
}

fn parse_args() -> (String, u16, String, String) {
    let mut host = "127.0.0.1".into();
    let mut port = 8443u16;
    let mut user = "probeuser".into();
    let mut password = String::new();
    let mut it = std::env::args().skip(1).peekable();
    // 第一个位置参数是子命令，其余 --k v
    while let Some(a) = it.next() {
        match a.as_str() {
            "--host" => host = it.next().unwrap_or(host),
            "--port" => port = it.next().and_then(|v| v.parse().ok()).unwrap_or(port),
            "--user" => user = it.next().unwrap_or(user),
            "--password" => password = it.next().unwrap_or_default(),
            _ => {}
        }
    }
    (host, port, user, password)
}

fn pass(name: &str, detail: &str) -> i32 {
    println!("PROBE PASS: {name} ({detail})");
    0
}
fn fail(name: &str, detail: &str) -> i32 {
    eprintln!("PROBE FAIL: {name} ({detail})");
    1
}

// ── 场景 ────────────────────────────────────────────────────────────────

/// 连接后先在独立首流完成认证（quireld 协议：认证流只做认证，
/// 后续每个请求各开新流——与产品客户端 connection.rs 一致）
async fn auth_conn(host: &str, port: u16, user: &str, password: &str) -> Result<quinn::Connection, String> {
    let conn = connect(host, port).await?;
    let (mut send, mut recv) = conn.open_bi().await.map_err(|e| format!("open auth stream: {e}"))?;
    write_msg(
        &mut send,
        &Envelope::new(1, Payload::AuthPasswordRequest { username: user.into(), password: password.into() }),
    )
    .await?;
    let Some(env) = tokio::time::timeout(Duration::from_secs(10), read_msg(&mut recv))
        .await.map_err(|_| "认证超时")?? else {
        return Err("认证流关闭".into());
    };
    match env.payload {
        Payload::AuthResponse { success: true, session_id: Some(_), .. } => Ok(conn),
        Payload::AuthResponse { success: false, error, .. } => {
            Err(format!("认证被拒: {}", error.unwrap_or_default()))
        }
        other => Err(format!("认证意外响应: {}", other.type_name())),
    }
}

async fn auth_scenario(host: &str, port: u16, user: &str, password: &str) -> Result<Duration, String> {
    let conn = connect(host, port).await?;
    let t0 = Instant::now();
    let (mut send, mut recv) = conn.open_bi().await.map_err(|e| e.to_string())?;
    write_msg(
        &mut send,
        &Envelope::new(
            1,
            Payload::AuthPasswordRequest {
                username: user.into(),
                password: password.into(),
            },
        ),
    )
    .await?;
    let resp = tokio::time::timeout(Duration::from_secs(10), read_msg(&mut recv))
        .await
        .map_err(|_| "认证响应超时")??;
    let Some(env) = resp else { return Err("认证流提前关闭".into()) };
    match env.payload {
        Payload::AuthResponse { success: true, session_id: Some(_), .. } => {
            Ok(t0.elapsed())
        }
        Payload::AuthResponse { success: false, error, .. } => {
            Err(format!("认证被拒: {}", error.unwrap_or_default()))
        }
        other => Err(format!("意外响应: {}", other.type_name())),
    }
}

async fn readdir_scenario(host: &str, port: u16, user: &str, password: &str, path: &str) -> Result<(usize, Duration), String> {
    let conn = auth_conn(host, port, user, password).await?;
    let (mut send, mut recv) = conn.open_bi().await.map_err(|e| e.to_string())?;

    let t0 = Instant::now();
    write_msg(&mut send, &Envelope::new(2, Payload::ReadDirRequest { path: path.into() })).await?;
    let Some(env) = tokio::time::timeout(Duration::from_secs(30), read_msg(&mut recv)).await.map_err(|_| "readdir 超时")?? else {
        return Err("readdir 流关闭".into());
    };
    let dt = t0.elapsed();
    match env.payload {
        Payload::ReadDirResponse { entries, .. } => Ok((entries.len(), dt)),
        Payload::Error { message, .. } => Err(format!("Agent 错误: {message}")),
        other => Err(format!("意外响应: {}", other.type_name())),
    }
}

/// 终端全生命周期：spawn → resize → echo 探针串 → 收到回显 → close
async fn terminal_scenario(host: &str, port: u16, user: &str, password: &str) -> Result<Duration, String> {
    let conn = auth_conn(host, port, user, password).await?;
    // 终端隧道 = 独立流：spawn 请求在该流上，后续输入/输出同流
    let (mut send, mut recv) = conn.open_bi().await.map_err(|e| e.to_string())?;

    let t0 = Instant::now();
    let token = format!("probe-ok-{}", std::process::id());
    write_msg(
        &mut send,
        &Envelope::new(3, Payload::TerminalSpawnRequest {
            shell: "/bin/bash".into(),
            cols: 80,
            rows: 24,
            working_directory: None,
        }),
    )
    .await?;
    let Some(env) = tokio::time::timeout(Duration::from_secs(15), read_msg(&mut recv)).await.map_err(|_| "spawn 响应超时")?? else {
        return Err("spawn 流关闭".into());
    };
    let session_id = match env.payload {
        Payload::TerminalSpawnResponse { session_id } => session_id,
        Payload::Error { message, .. } => return Err(format!("spawn 失败: {message}")),
        other => return Err(format!("spawn 意外响应: {}", other.type_name())),
    };

    // 隧道协议（与产品客户端一致）：spawn 响应是唯一的 envelope，
    // 此后输入/输出均为 [4B LE len][裸字节]；resize 走独立流
    let (mut rz_send, mut rz_recv) = conn.open_bi().await.map_err(|e| e.to_string())?;
    write_msg(&mut rz_send, &Envelope::new(4, Payload::TerminalResizeRequest { session_id: session_id.clone(), cols: 100, rows: 30 })).await?;
    let _ = tokio::time::timeout(Duration::from_secs(5), read_msg(&mut rz_recv)).await;

    // 输入 echo 探针串（裸字节）
    let input = format!("echo {token}
").into_bytes();
    send.write_all(&(input.len() as u32).to_le_bytes()).await.map_err(|e| e.to_string())?;
    send.write_all(&input).await.map_err(|e| e.to_string())?;

    // 输出循环：读 [4B len][裸字节]，扫探针串（bash 回显 + 执行结果都会出现）
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut seen = false;
    while Instant::now() < deadline {
        let mut len_buf = [0u8; 4];
        match tokio::time::timeout(Duration::from_secs(5), recv.read_exact(&mut len_buf)).await {
            Err(_) => continue, // 短暂无输出，继续等
            Ok(Err(quinn::ReadExactError::FinishedEarly(_))) => break,
            Ok(Err(e)) => return Err(format!("输出长度: {e}")),
            Ok(Ok(())) => {}
        }
        let len = u32::from_le_bytes(len_buf) as usize;
        if len == 0 || len > 1024 * 1024 {
            return Err(format!("隧道帧长度非法: {len}"));
        }
        let mut data = vec![0u8; len];
        tokio::time::timeout(Duration::from_secs(5), recv.read_exact(&mut data))
            .await.map_err(|_| "输出体超时")?
            .map_err(|e| format!("输出体: {e}"))?;
        if String::from_utf8_lossy(&data).contains(&token) {
            seen = true;
            break;
        }
    }
    if !seen {
        return Err("15s 内未在终端输出中看到探针串".into());
    }

    // 关闭：断开整个连接（连接清理会注销 PTY 会话）
    let _ = conn.close(0u32.into(), b"probe done");
    Ok(t0.elapsed())
}

/// 下载场景：raw 帧接收全量 chunk 直到 Complete，校验字节数与声明一致
async fn transfer_down_scenario(host: &str, port: u16, user: &str, password: &str, path: &str) -> Result<(u64, Duration), String> {
    let conn = auth_conn(host, port, user, password).await?;
    let (mut send, mut recv) = conn.open_bi().await.map_err(|e| e.to_string())?;

    let t0 = Instant::now();
    write_msg(
        &mut send,
        &Envelope::new(2, Payload::FileTransferRequest {
            direction: "download".into(),
            path: path.into(),
            file_size: None,
            chunk_size: None,
            resume_from: None,
            frame_mode: "raw".into(),
            stream_count: None,
        }),
    )
    .await?;
    let Some(env) = tokio::time::timeout(Duration::from_secs(15), read_msg(&mut recv)).await.map_err(|_| "accept 超时")?? else {
        return Err("accept 流关闭".into());
    };
    let (session_id, file_size) = match env.payload {
        Payload::FileTransferAccept { session_id, file_size, .. } => (session_id, file_size),
        Payload::Error { message, .. } => return Err(format!("accept 失败: {message}")),
        other => return Err(format!("accept 意外响应: {}", other.type_name())),
    };

    // raw 帧：[4B len][1B type][body]；type 0x02=数据 [4B seq][4B size][data]、0x01=控制(JSON)
    let mut received = 0u64;
    loop {
        let mut hdr = [0u8; 5];
        tokio::time::timeout(Duration::from_secs(30), recv.read_exact(&mut hdr))
            .await.map_err(|_| "chunk 头超时")?
            .map_err(|e| format!("chunk 头: {e}"))?;
        let frame_len = u32::from_le_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]) as usize;
        if frame_len == 0 || frame_len > 10 * 1024 * 1024 {
            return Err(format!("raw 帧长度非法: {frame_len}"));
        }
        let mut body = vec![0u8; frame_len - 1];
        recv.read_exact(&mut body).await.map_err(|e| format!("chunk 体: {e}"))?;
        match hdr[4] {
            0x02 => {
                if body.len() >= 8 {
                    received += (body.len() - 8) as u64;
                }
            }
            0x01 => {
                let env = Envelope::decode(&body).map_err(|e| format!("控制帧解码: {e}"))?;
                match env.payload {
                    Payload::FileTransferComplete { success: true, .. } => break,
                    Payload::FileTransferComplete { success: false, error, .. } => {
                        return Err(format!("传输失败: {}", error.unwrap_or_default()))
                    }
                    _ => {}
                }
            }
            _ => return Err(format!("未知帧类型: {}", hdr[4])),
        }
    }
    let _ = session_id;
    if received != file_size {
        return Err(format!("字节数不符: 收 {received} / 声明 {file_size}"));
    }
    Ok((received, t0.elapsed()))
}

/// 上传场景：声明 N 字节、raw 帧发送、Complete，断言 rename 成功
async fn transfer_up_scenario(host: &str, port: u16, user: &str, password: &str, size: u64) -> Result<Duration, String> {
    let conn = auth_conn(host, port, user, password).await?;
    let (mut send, mut recv) = conn.open_bi().await.map_err(|e| e.to_string())?;

    let t0 = Instant::now();
    let path = format!("/tmp/quirel-probe-up-{}.bin", std::process::id());
    write_msg(
        &mut send,
        &Envelope::new(2, Payload::FileTransferRequest {
            direction: "upload".into(),
            path: path.clone(),
            file_size: Some(size),
            chunk_size: Some(256 * 1024),
            resume_from: None,
            frame_mode: "raw".into(),
            stream_count: None,
        }),
    )
    .await?;
    let Some(env) = tokio::time::timeout(Duration::from_secs(15), read_msg(&mut recv)).await.map_err(|_| "accept 超时")?? else {
        return Err("accept 流关闭".into());
    };
    let session_id = match env.payload {
        Payload::FileTransferAccept { session_id, .. } => session_id,
        Payload::Error { message, .. } => return Err(format!("accept 失败: {message}")),
        other => return Err(format!("accept 意外响应: {}", other.type_name())),
    };

    // raw 数据帧
    let chunk = vec![0xABu8; 256 * 1024];
    let mut sent = 0u64;
    let mut seq = 1u32;
    while sent < size {
        let n = std::cmp::min(chunk.len() as u64, size - sent) as usize;
        let mut frame = Vec::with_capacity(9 + n);
        frame.extend_from_slice(&((9 + n) as u32).to_le_bytes());
        frame.push(0x02);
        frame.extend_from_slice(&seq.to_le_bytes());
        frame.extend_from_slice(&(n as u32).to_le_bytes());
        frame.extend_from_slice(&chunk[..n]);
        send.write_all(&frame).await.map_err(|e| e.to_string())?;
        sent += n as u64;
        seq += 1;
    }
    // Complete 控制帧
    let complete = Envelope::new(0, Payload::FileTransferComplete {
        session_id,
        success: true,
        mtime: None,
        error: None,
    })
    .encode()
    .map_err(|e| e)?;
    let mut frame = Vec::with_capacity(5 + complete.len());
    frame.extend_from_slice(&((1 + complete.len()) as u32).to_le_bytes());
    frame.push(0x01);
    frame.extend_from_slice(&complete);
    send.write_all(&frame).await.map_err(|e| e.to_string())?;

    // 收尾：服务端单流上传不回 Complete 帧——handler 返回即流结束。
    // finish 发送半，等对端收尾后读到 FinishedEarly 即视为服务端确认
    send.finish().map_err(|e| e.to_string())?;
    let mut drain = [0u8; 512];
    loop {
        // read 返回 Ok(None) = 对端结束发送半（服务端 handler 收尾）
        match tokio::time::timeout(Duration::from_secs(15), recv.read(&mut drain)).await {
            Ok(Ok(None)) | Err(_) => break,
            Ok(Ok(Some(_))) => {}
            Ok(Err(e)) => return Err(format!("收尾读取: {e}")),
        }
    }

    // FileInfo 验证落盘与大小
    let (mut fs_send, mut fs_recv) = conn.open_bi().await.map_err(|e| e.to_string())?;
    write_msg(&mut fs_send, &Envelope::new(9, Payload::FileInfoRequest { path: path.clone() })).await?;
    let Some(env) = tokio::time::timeout(Duration::from_secs(10), read_msg(&mut fs_recv)).await.map_err(|_| "file_info 超时")?? else {
        return Err("file_info 流关闭".into());
    };
    match env.payload {
        Payload::FileInfoResponse { size: actual, .. } if actual == size => {}
        Payload::FileInfoResponse { size: actual, .. } => {
            return Err(format!("落盘大小不符: {actual} / {size}"))
        }
        Payload::Error { message, .. } => return Err(format!("file_info 失败: {message}")),
        other => return Err(format!("file_info 意外: {}", other.type_name())),
    }

    // 自清理：删除测试文件（探针可能被指向生产 quireld——/tmp 不留残留，
    // 审计日志中的 upload/delete 记录成对可对账）
    let (mut del_send, mut del_recv) = conn.open_bi().await.map_err(|e| e.to_string())?;
    write_msg(&mut del_send, &Envelope::new(10, Payload::DeleteRequest { path: path.clone() })).await?;
    let Some(env) = tokio::time::timeout(Duration::from_secs(10), read_msg(&mut del_recv)).await.map_err(|_| "delete 超时")?? else {
        return Err("delete 流关闭".into());
    };
    if let Payload::DeleteResponse { success: false } = env.payload {
        return Err(format!("测试文件清理失败: {path}（请手动删除）"));
    }
    Ok(t0.elapsed())
}

// ── 文件系统全链路场景 ──────────────────────────────────────────────────
//
// 在 /tmp/quirel-probe-fs-<pid>/ 内走完 mkdir→write→read→rename→copy→
// chmod→read_dir→move→delete 全链路，结束整目录自清理（探针可被指向
// 生产 quireld，所有落盘动作必须自含自清）。
async fn fs_scenario(host: &str, port: u16, user: &str, password: &str) -> Result<(usize, Duration), String> {
    let conn = auth_conn(host, port, user, password).await?;
    let t0 = Instant::now();
    let mut req_id = 100u32;
    let dir = format!("/tmp/quirel-probe-fs-{}", std::process::id());

    macro_rules! rpc {
        ($payload:expr) => {{
            let (mut s, mut r) = conn.open_bi().await.map_err(|e| e.to_string())?;
            req_id += 1;
            write_msg(&mut s, &Envelope::new(req_id, $payload)).await?;
            let env = tokio::time::timeout(Duration::from_secs(15), read_msg(&mut r))
                .await.map_err(|_| "rpc 超时")??;
            match env {
                Some(e) => e.payload,
                None => return Err("rpc 流关闭".into()),
            }
        }};
    }

    match rpc!(Payload::MkdirRequest { path: dir.clone() }) {
        Payload::MkdirResponse { success: true, .. } => {}
        other => return Err(format!("mkdir 失败: {}", other.type_name())),
    }

    let expected = b"probe-fs-roundtrip-content-123";
    let b64 = {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD.encode(expected)
    };
    let file = format!("{dir}/a.txt");
    match rpc!(Payload::WriteFileRequest { path: file.clone(), content: b64 }) {
        Payload::WriteFileResponse { .. } => {}
        other => return Err(format!("write 失败: {}", other.type_name())),
    }

    match rpc!(Payload::ReadFileRequest { path: file.clone() }) {
        Payload::ReadFileResponse { content, .. } => {
            use base64::Engine as _;
            let got = base64::engine::general_purpose::STANDARD.decode(&content)
                .map_err(|e| format!("read 解码: {e}"))?;
            if got != expected {
                return Err(format!("内容回读不符: {:?} / {:?}", got, expected));
            }
        }
        other => return Err(format!("read 失败: {}", other.type_name())),
    }

    let renamed = format!("{dir}/b.txt");
    match rpc!(Payload::RenameRequest { old_path: file.clone(), new_path: renamed.clone() }) {
        Payload::RenameResponse { success: true, .. } => {}
        other => return Err(format!("rename 失败: {}", other.type_name())),
    }
    let copied = format!("{dir}/c.txt");
    match rpc!(Payload::CopyRequest { src: renamed.clone(), dst: copied.clone() }) {
        Payload::CopyResponse { success: true, .. } => {}
        other => return Err(format!("copy 失败: {}", other.type_name())),
    }
    match rpc!(Payload::ChmodRequest { path: copied.clone(), mode: 0o600, recursive: false }) {
        Payload::ChmodResponse { success: true } => {}
        other => return Err(format!("chmod 失败: {}", other.type_name())),
    }

    match rpc!(Payload::ReadDirRequest { path: dir.clone() }) {
        Payload::ReadDirResponse { entries, .. } if entries.len() == 2 => {}
        Payload::ReadDirResponse { entries, .. } => {
            return Err(format!("read_dir 数量不符: {} (期望 2)", entries.len()))
        }
        other => return Err(format!("read_dir 失败: {}", other.type_name())),
    }

    match rpc!(Payload::MoveRequest { src: copied.clone(), dst: renamed.clone() }) {
        Payload::MoveResponse { success: true, .. } => {}
        other => return Err(format!("move 失败: {}", other.type_name())),
    }
    match rpc!(Payload::DeleteRequest { path: dir.clone() }) {
        Payload::DeleteResponse { success: true } => {}
        other => return Err(format!("delete 目录失败: {}", other.type_name())),
    }

    Ok((8, t0.elapsed()))
}

/// 增量保存场景：WriteFile → ApplyDiff(替换一行) → ReadFile 校验 → 自清理
async fn diff_scenario(host: &str, port: u16, user: &str, password: &str) -> Result<Duration, String> {
    let conn = auth_conn(host, port, user, password).await?;
    let t0 = Instant::now();
    let mut req_id = 200u32;
    let dir = format!("/tmp/quirel-probe-diff-{}", std::process::id());
    let file = format!("{dir}/d.txt");

    macro_rules! rpc {
        ($payload:expr) => {{
            let (mut s, mut r) = conn.open_bi().await.map_err(|e| e.to_string())?;
            req_id += 1;
            write_msg(&mut s, &Envelope::new(req_id, $payload)).await?;
            let env = tokio::time::timeout(Duration::from_secs(15), read_msg(&mut r))
                .await.map_err(|_| "rpc 超时")??;
            match env {
                Some(e) => e.payload,
                None => return Err("rpc 流关闭".into()),
            }
        }};
    }

    use base64::Engine as _;
    let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
    let _ = rpc!(Payload::MkdirRequest { path: dir.clone() });
    let _ = rpc!(Payload::WriteFileRequest { path: file.clone(), content: b64(b"line1\nline2\nline3\n") });

    let diffs = vec![quirel_protocol::FileDiff {
        diff_type: quirel_protocol::DiffType::Replace,
        line_number: 2,
        old_content: Some("line2".into()),
        new_content: Some("line2-edited".into()),
    }];
    let base_mtime = match rpc!(Payload::ReadFileRequest { path: file.clone() }) {
        Payload::ReadFileResponse { mtime, .. } => mtime,
        other => return Err(format!("read 失败: {}", other.type_name())),
    };
    match rpc!(Payload::ApplyDiffRequest { path: file.clone(), base_mtime, diffs }) {
        Payload::ApplyDiffResponse { success: true, .. } => {}
        Payload::ApplyDiffResponse { success: false, error, .. } => {
            return Err(format!("apply_diff 被拒: {}", error.unwrap_or_default()))
        }
        other => return Err(format!("apply_diff 失败: {}", other.type_name())),
    }
    match rpc!(Payload::ReadFileRequest { path: file.clone() }) {
        Payload::ReadFileResponse { content, .. } => {
            let got = base64::engine::general_purpose::STANDARD.decode(&content)
                .map_err(|e| format!("read 解码: {e}"))?;
            if !String::from_utf8_lossy(&got).contains("line2-edited") {
                return Err(format!("增量结果不符: {:?}", got));
            }
        }
        other => return Err(format!("read 失败: {}", other.type_name())),
    }

    let _ = rpc!(Payload::DeleteRequest { path: dir.clone() });
    Ok(t0.elapsed())
}

/// 进程列表场景：GetProcesses 非空
async fn processes_scenario(host: &str, port: u16, user: &str, password: &str) -> Result<(usize, Duration), String> {
    let conn = auth_conn(host, port, user, password).await?;
    let t0 = Instant::now();
    let (mut send, mut recv) = conn.open_bi().await.map_err(|e| e.to_string())?;
    write_msg(&mut send, &Envelope::new(2, Payload::GetProcesses {})).await?;
    let Some(env) = tokio::time::timeout(Duration::from_secs(30), read_msg(&mut recv)).await.map_err(|_| "processes 超时")?? else {
        return Err("processes 流关闭".into());
    };
    match env.payload {
        Payload::ProcessListResponse { processes } if !processes.is_empty() => {
            Ok((processes.len(), t0.elapsed()))
        }
        Payload::ProcessListResponse { .. } => Err("进程列表为空（采集器异常）".into()),
        other => Err(format!("意外响应: {}", other.type_name())),
    }
}

/// 订阅场景：subscribe → 收到 ≥1 帧 metrics 事件 → unsubscribe
async fn metrics_scenario(host: &str, port: u16, user: &str, password: &str) -> Result<Duration, String> {
    let conn = auth_conn(host, port, user, password).await?;
    let t0 = Instant::now();
    let (mut send, mut recv) = conn.open_bi().await.map_err(|e| e.to_string())?;
    write_msg(&mut send, &Envelope::new(2, Payload::Subscribe {
        server_id: "probe".into(),
        types: vec![quirel_protocol::SubscriptionType::Metrics { interval_secs: Some(1) }],
    })).await?;
    let Some(env) = tokio::time::timeout(Duration::from_secs(10), read_msg(&mut recv)).await.map_err(|_| "subscribe 超时")?? else {
        return Err("subscribe 流关闭".into());
    };
    if !matches!(env.payload, Payload::SubscribeAck { success: true, .. }) {
        return Err(format!("subscribe 未确认: {}", env.payload.type_name()));
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if Instant::now() > deadline {
            return Err("10s 未收到 metrics 事件".into());
        }
        let Some(env) = tokio::time::timeout(Duration::from_secs(10), read_msg(&mut recv)).await.map_err(|_| "事件超时")?? else {
            return Err("订阅流关闭".into());
        };
        if matches!(env.payload, Payload::Event { .. }) {
            break;
        }
    }
    write_msg(&mut send, &Envelope::new(3, Payload::Unsubscribe {
        server_id: "probe".into(),
        types: vec![quirel_protocol::SubscriptionType::Metrics { interval_secs: Some(1) }],
    })).await?;
    Ok(t0.elapsed())
}

/// 错误凭据场景：错误密码必须被拒（用不存在的用户，避免锁定污染 probeuser）
async fn auth_bad_scenario(host: &str, port: u16) -> Result<Duration, String> {
    let t0 = Instant::now();
    match auth_conn(host, port, "probe-nonexistent-user", "wrong-pass").await {
        Err(e) if e.contains("认证被拒") => Ok(t0.elapsed()),
        Err(e) => Err(format!("失败原因异常（期望认证被拒）: {e}")),
        Ok(_) => Err("错误凭据竟能通过认证！".into()),
    }
}

// ── 入口 ────────────────────────────────────────────────────────────────

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("smoke");
    let (host, port, user, password) = parse_args();

    let code = match cmd {
        "auth" => match auth_scenario(&host, port, &user, &password).await {
            Ok(dt) => pass("auth", &format!("{:.0}ms", dt.as_millis())),
            Err(e) => fail("auth", &e),
        },
        "readdir" => {
            let path = args.iter().position(|a| a == "--path").and_then(|i| args.get(i + 1)).cloned().unwrap_or_else(|| "/usr/lib".into());
            match readdir_scenario(&host, port, &user, &password, &path).await {
                Ok((n, dt)) => pass("readdir", &format!("{n} 条, {:.0}ms", dt.as_millis())),
                Err(e) => fail("readdir", &e),
            }
        }
        "terminal" => match terminal_scenario(&host, port, &user, &password).await {
            Ok(dt) => pass("terminal", &format!("{:.0}ms", dt.as_millis())),
            Err(e) => fail("terminal", &e),
        },
        "transfer-down" => {
            let path = args.iter().position(|a| a == "--path").and_then(|i| args.get(i + 1)).cloned().unwrap_or_else(|| "/bin/bash".into());
            match transfer_down_scenario(&host, port, &user, &password, &path).await {
                Ok((n, dt)) => pass("transfer-down", &format!("{n} 字节, {:.0}ms", dt.as_millis())),
                Err(e) => fail("transfer-down", &e),
            }
        }
        "transfer-up" => {
            let size: u64 = args.iter().position(|a| a == "--size").and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(1_048_576);
            match transfer_up_scenario(&host, port, &user, &password, size).await {
                Ok(dt) => pass("transfer-up", &format!("{size} 字节, {:.0}ms", dt.as_millis())),
                Err(e) => fail("transfer-up", &e),
            }
        }
        // hold：连接并认证后挂起，直到连接关闭——配合外部 SIGTERM quireld
        // 验证优雅关闭（M5）：应秒级收到关闭且 quireld 干净退出
        "hold" => {
            match auth_conn(&host, port, &user, &password).await {
                Ok(conn) => {
                    let t0 = Instant::now();
                    conn.closed().await;
                    let dt = t0.elapsed().as_millis();
                    match conn.close_reason() {
                        Some(reason) => println!("HOLD CLOSED: elapsed={dt}ms reason={reason:?}"),
                        None => println!("HOLD CLOSED: elapsed={dt}ms reason=unknown"),
                    }
                    0
                }
                Err(e) => fail("hold", &e),
            }
        }
        // resilience：认证后周期性 readdir 并逐次打点——配合外部 kill -9
        // Worker 验证崩溃自动重启（M4）：短暂 FAIL 后应恢复 OK
        "resilience" => {
            let dur: u64 = args.iter().position(|a| a == "--duration")
                .and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(30);
            match auth_conn(&host, port, &user, &password).await {
                Ok(conn) => {
                    let t0 = Instant::now();
                    let mut rc = 2; // 2 = 从未成功
                    loop {
                        if t0.elapsed().as_secs() >= dur { break; }
                        let attempt = Instant::now();
                        let (mut send, mut recv) = match conn.open_bi().await {
                            Ok(s) => s,
                            Err(e) => {
                                println!("RESILIENCE FAIL: open_bi {e}");
                                tokio::time::sleep(Duration::from_secs(2)).await;
                                continue;
                            }
                        };
                        write_msg(&mut send, &Envelope::new(2, Payload::ReadDirRequest { path: "/tmp".into() })).await.unwrap();
                        match tokio::time::timeout(Duration::from_secs(10), read_msg(&mut recv)).await {
                            Ok(Ok(Some(env))) if matches!(env.payload, Payload::ReadDirResponse { .. }) => {
                                println!("RESILIENCE OK: {:.1}s", attempt.elapsed().as_secs_f64());
                                rc = 0;
                            }
                            other => println!("RESILIENCE FAIL: {other:?}"),
                        }
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }
                    rc
                }
                Err(e) => fail("resilience", &e),
            }
        }
        "smoke" => {
            let mut rc = 0;
            for (name, r) in [
                ("auth", auth_conn(&host, port, &user, &password).await.map(|_| "ok".to_string())),
                ("readdir", readdir_scenario(&host, port, &user, &password, "/usr/lib").await.map(|(n, d)| format!("{n} 条, {:.0}ms", d.as_millis()))),
                ("terminal", terminal_scenario(&host, port, &user, &password).await.map(|d| format!("{:.0}ms", d.as_millis()))),
                ("transfer-down", transfer_down_scenario(&host, port, &user, &password, "/bin/bash").await.map(|(n, d)| format!("{n}B, {:.0}ms", d.as_millis()))),
                ("transfer-up", transfer_up_scenario(&host, port, &user, &password, 1_048_576).await.map(|d| format!("{:.0}ms", d.as_millis()))),
                ("fs", fs_scenario(&host, port, &user, &password).await.map(|(n, d)| format!("{n} 步, {:.0}ms", d.as_millis()))),
                ("diff", diff_scenario(&host, port, &user, &password).await.map(|d| format!("{:.0}ms", d.as_millis()))),
                ("processes", processes_scenario(&host, port, &user, &password).await.map(|(n, d)| format!("{n} 进程, {:.0}ms", d.as_millis()))),
                ("metrics", metrics_scenario(&host, port, &user, &password).await.map(|d| format!("{:.0}ms", d.as_millis()))),
                ("auth-bad", auth_bad_scenario(&host, port).await.map(|d| format!("正确拒绝, {:.0}ms", d.as_millis()))),
            ] {
                match r {
                    Ok(detail) => println!("PROBE PASS: {name} ({detail})"),
                    Err(e) => {
                        eprintln!("PROBE FAIL: {name} ({e})");
                        rc = 1;
                    }
                }
            }
            rc
        }
        other => {
            eprintln!("未知子命令: {other}（可用: auth/readdir/terminal/transfer-down/transfer-up/smoke/hold/resilience）");
            2
        }
    };
    std::process::exit(code);
}
