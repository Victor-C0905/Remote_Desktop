// quic_test/src/main.rs
// QUIC 客户端测试

use anyhow::Result;
use quinn::{ClientConfig, Endpoint, RecvStream, SendStream};
use rustls::pki_types::{CertificateDer, ServerName};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// Agent 配置
// 注意：在 WSL 中运行时使用 localhost
const AGENT_HOST: &str = "127.0.0.1";  // localhost
const AGENT_PORT: u16 = 8443;
const AGENT_TOKEN: &str = "gmr_1ecb954c-9a8c-4a5e-a3b4-f9a77af0d036";

// 消息信封
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Envelope {
    request_id: u32,
    payload: Payload,
}

// 消息 payload
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
enum Payload {
    #[serde(rename = "ping")]
    Ping { timestamp: u64 },

    #[serde(rename = "pong")]
    Pong { timestamp: u64, server_time: u64 },

    #[serde(rename = "auth_request")]
    AuthRequest { token: String },

    #[serde(rename = "auth_response")]
    AuthResponse { success: bool, error: Option<String> },

    #[serde(rename = "metrics_subscribe")]
    MetricsSubscribeRequest {},

    #[serde(rename = "metrics_data")]
    MetricsData(MetricsSnapshot),

    #[serde(rename = "read_dir")]
    ReadDirRequest { path: String },

    #[serde(rename = "read_dir_resp")]
    ReadDirResponse { entries: Vec<FileEntry> },

    #[serde(rename = "read_file")]
    ReadFileRequest { path: String },

    #[serde(rename = "read_file_resp")]
    ReadFileResponse { content: String, size: u64 },
}

// 系统指标
#[derive(Debug, Clone, Serialize, Deserialize)]
struct MetricsSnapshot {
    cpu_percent: f32,
    mem_used_bytes: u64,
    mem_total_bytes: u64,
    disks: Vec<DiskInfo>,
    network_rx_bytes: u64,
    network_tx_bytes: u64,
    uptime_secs: u64,
}

// 磁盘信息
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiskInfo {
    mount: String,
    total_bytes: u64,
    used_bytes: u64,
}

// 文件条目
#[derive(Debug, Clone, Serialize, Deserialize)]
struct FileEntry {
    name: String,
    is_dir: bool,
    size: u64,
    permissions: String,
}

impl Envelope {
    fn encode(&self) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(self)?)
    }

    fn decode(data: &[u8]) -> Result<Self> {
        Ok(serde_json::from_slice(data)?)
    }
}

// 创建客户端配置（跳过证书验证）
fn create_client_config() -> Result<ClientConfig> {
    // 创建自定义证书验证器（跳过验证）
    let verifier = SkipServerVerification::new();

    // 创建 rustls ClientConfig
    let rustls_config = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();

    // 转换为 quinn QuicClientConfig
    let quic_config = quinn::crypto::rustls::QuicClientConfig::try_from(rustls_config)?;

    // 创建 quinn ClientConfig
    let client_config = ClientConfig::new(Arc::new(quic_config));

    Ok(client_config)
}

// 自定义证书验证器（跳过验证）
#[derive(Debug)]
struct SkipServerVerification;

impl SkipServerVerification {
    fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

impl rustls::client::danger::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        // 跳过验证，直接返回成功
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
        ]
    }
}

// 发送请求并接收响应
async fn send_request(stream: &mut (SendStream, RecvStream), envelope: &Envelope) -> Result<Envelope> {
    let (send, recv) = stream;

    // 编码请求
    let data = envelope.encode()?;

    // 发送长度（4 字节）
    let len = data.len() as u32;
    send.write_all(&len.to_le_bytes()).await?;

    // 发送数据
    send.write_all(&data).await?;

    // 接收响应长度（4 字节）
    let mut len_buf = [0u8; 4];
    recv.read_exact(&mut len_buf).await?;
    let resp_len = u32::from_le_bytes(len_buf) as usize;

    // 接收响应数据
    let mut resp_data = vec![0u8; resp_len];
    recv.read_exact(&mut resp_data).await?;

    // 解码响应
    let response = Envelope::decode(&resp_data)?;

    Ok(response)
}

// 测试 Ping/Pong
async fn test_ping(stream: &mut (SendStream, RecvStream)) -> Result<bool> {
    println!("\n=== 测试 Ping/Pong ===");

    let envelope = Envelope {
        request_id: 1,
        payload: Payload::Ping {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
        },
    };

    println!("发送 Ping: {:?}", envelope);

    let response = send_request(stream, &envelope).await?;

    println!("收到响应: {:?}", response);

    if let Payload::Pong { timestamp, server_time } = response.payload {
        println!("✅ Ping/Pong 成功");
        println!("  客户端时间: {}", timestamp);
        println!("  服务器时间: {}", server_time);
        Ok(true)
    } else {
        println!("❌ Ping/Pong 失败");
        Ok(false)
    }
}

// 测试认证
async fn test_auth(stream: &mut (SendStream, RecvStream)) -> Result<bool> {
    println!("\n=== 测试认证 ===");

    let envelope = Envelope {
        request_id: 2,
        payload: Payload::AuthRequest {
            token: AGENT_TOKEN.to_string(),
        },
    };

    println!("发送认证请求");

    let response = send_request(stream, &envelope).await?;

    println!("收到响应: {:?}", response);

    if let Payload::AuthResponse { success, error } = response.payload {
        if success {
            println!("✅ 认证成功");
            Ok(true)
        } else {
            println!("❌ 认证失败: {:?}", error);
            Ok(false)
        }
    } else {
        println!("❌ 认证失败");
        Ok(false)
    }
}

// 测试系统指标
async fn test_metrics(stream: &mut (SendStream, RecvStream)) -> Result<bool> {
    println!("\n=== 测试系统指标采集 ===");

    let envelope = Envelope {
        request_id: 3,
        payload: Payload::MetricsSubscribeRequest {},
    };

    println!("发送指标订阅请求");

    let response = send_request(stream, &envelope).await?;

    println!("收到响应: {:?}", response);

    if let Payload::MetricsData(data) = response.payload {
        println!("✅ 系统指标采集成功");
        println!("  CPU 使用率: {:.2}%", data.cpu_percent);
        println!(
            "  内存使用: {:.2} GB / {:.2} GB",
            data.mem_used_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
            data.mem_total_bytes as f64 / (1024.0 * 1024.0 * 1024.0)
        );
        println!("  磁盘数量: {}", data.disks.len());
        println!(
            "  网络接收: {:.2} MB",
            data.network_rx_bytes as f64 / (1024.0 * 1024.0)
        );
        println!(
            "  网络发送: {:.2} MB",
            data.network_tx_bytes as f64 / (1024.0 * 1024.0)
        );
        println!("  系统运行时间: {:.2} 小时", data.uptime_secs as f64 / 3600.0);
        Ok(true)
    } else {
        println!("❌ 系统指标采集失败");
        Ok(false)
    }
}

// 测试读取目录
async fn test_read_dir(stream: &mut (SendStream, RecvStream)) -> Result<bool> {
    println!("\n=== 测试读取目录 ===");

    let envelope = Envelope {
        request_id: 4,
        payload: Payload::ReadDirRequest {
            path: "/home".to_string(),
        },
    };

    println!("发送读取目录请求: /home");

    let response = send_request(stream, &envelope).await?;

    println!("收到响应: {:?}", response);

    if let Payload::ReadDirResponse { entries } = response.payload {
        println!("✅ 读取目录成功");
        println!("  目录: /home");
        println!("  文件数量: {}", entries.len());

        println!("  文件列表:");
        for entry in entries.iter().take(10) {
            let icon = if entry.is_dir { "📁" } else { "📄" };
            println!(
                "    {} {} ({} bytes, {})",
                icon, entry.name, entry.size, entry.permissions
            );
        }

        Ok(true)
    } else {
        println!("❌ 读取目录失败");
        Ok(false)
    }
}

// 测试读取文件
async fn test_read_file(stream: &mut (SendStream, RecvStream)) -> Result<bool> {
    println!("\n=== 测试读取文件 ===");

    let envelope = Envelope {
        request_id: 5,
        payload: Payload::ReadFileRequest {
            path: "/etc/hostname".to_string(),
        },
    };

    println!("发送读取文件请求: /etc/hostname");

    let response = send_request(stream, &envelope).await?;

    println!("收到响应: {:?}", response);

    if let Payload::ReadFileResponse { content, size } = response.payload {
        println!("✅ 读取文件成功");
        println!("  文件: /etc/hostname");
        println!("  内容: {}", content.trim());
        println!("  大小: {} bytes", size);
        Ok(true)
    } else {
        println!("❌ 读取文件失败");
        Ok(false)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // 安装 CryptoProvider（aws-lc-rs）
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("Failed to install CryptoProvider");

    println!("============================================================");
    println!("QUIC 客户端测试");
    println!("============================================================");
    println!("Agent 地址: {}:{}", AGENT_HOST, AGENT_PORT);

    // 创建客户端配置
    let client_config = create_client_config()?;

    // 创建 Endpoint
    let mut endpoint = Endpoint::client("0.0.0.0:0".parse()?)?;
    endpoint.set_default_client_config(client_config);

    println!("✅ QUIC 客户端初始化成功");

    // 连接 Agent
    println!("\n=== 连接 Agent ===");
    let addr_str = format!("{}:{}", AGENT_HOST, AGENT_PORT);
    let addr: SocketAddr = addr_str.parse().map_err(|e| anyhow::anyhow!("Failed to parse address '{}': {}", addr_str, e))?;
    let conn = endpoint.connect(addr, "localhost")?.await?;

    println!("✅ QUIC 连接成功");

    // 创建双向 Stream
    let stream = conn.open_bi().await?;

    println!("✅ 创建双向 Stream 成功");

    // 运行测试
    let mut stream = stream;
    let results = vec![
        ("Ping/Pong", test_ping(&mut stream).await?),
        ("认证", test_auth(&mut stream).await?),
        ("系统指标", test_metrics(&mut stream).await?),
        ("读取目录", test_read_dir(&mut stream).await?),
        ("读取文件", test_read_file(&mut stream).await?),
    ];

    // 输出结果
    println!("\n============================================================");
    println!("测试结果总结");
    println!("============================================================");

    let passed = results.iter().filter(|(_, r)| *r).count();
    let failed = results.len() - passed;

    for (name, result) in results {
        let status = if result { "✅ 通过" } else { "❌ 失败" };
        println!("{}: {}", name, status);
    }

    println!("\n总计: {} 通过, {} 失败", passed, failed);

    // 关闭连接
    conn.close(0u32.into(), b"test complete");

    println!("\n✅ 连接已关闭");

    Ok(())
}