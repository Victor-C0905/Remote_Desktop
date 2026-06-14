#!/bin/bash
# QUIC 测试脚本
# 在 WSL 内部测试 Agent 的 QUIC 连接

set -e

echo "=== QUIC 连接测试 ==="
echo ""

# 检查 Agent 是否运行
echo "1. 检查 Agent 是否运行..."
if ! pgrep -f "target/release/agent" > /dev/null; then
    echo "❌ Agent 未运行，请先启动 Agent"
    exit 1
fi
echo "✅ Agent 正在运行"
echo ""

# 测试 QUIC 端口
echo "2. 测试 QUIC 端口 (127.0.0.1:8443)..."
if ! ss -ulnp | grep -q "127.0.0.1:8443"; then
    echo "❌ QUIC 端口未监听"
    exit 1
fi
echo "✅ QUIC 端口正常监听"
echo ""

# 编写 Rust 测试程序
echo "3. 编写 QUIC 测试程序..."
cd ~/gnome-remote/agent/tests

# 创建 Cargo.toml
cat > Cargo.toml << 'EOF'
[package]
name = "quic_test"
version = "0.1.0"
edition = "2021"

[dependencies]
quinn = "0.11"
rustls = { version = "0.23", features = ["ring"] }
tokio = { version = "1", features = ["full"] }
anyhow = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
EOF

# 创建 src 目录
mkdir -p src

# 创建测试源码
cat > src/main.rs << 'EOF'
use anyhow::Result;
use quinn::{Endpoint, RecvStream, SendStream};
use rustls::client::{ServerCertVerified, ServerCertVerifier};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

// 跳过证书验证
struct SkipCertVerification;

impl ServerCertVerifier for SkipCertVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== QUIC 客户端测试 ===");
    
    // 安装 CryptoProvider
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");
    
    // 创建客户端 Endpoint
    let mut client_config = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(SkipCertVerification))
        .with_no_client_auth();
    
    client_config.alpn_protocols = vec![b"h3".to_vec()];
    
    let client_endpoint = Endpoint::client(
        quinn::ClientConfig::new(Arc::new(client_config)),
        "[::1]:0".parse()?,
    )?;
    
    println!("✅ 客户端 Endpoint 创建成功");
    
    // 连接服务器
    println!("🔗 连接 Agent (127.0.0.1:8443)...");
    let conn = client_endpoint
        .connect("127.0.0.1:8443".parse()?, "localhost")?
        .await?;
    
    println!("✅ QUIC 连接成功");
    
    // 创建 Stream
    let (mut send, mut recv) = conn.open_bi().await?;
    println!("✅ Stream 创建成功");
    
    // 发送 Ping 消息
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    
    let ping_msg = serde_json::json!({
        "request_id": 1,
        "payload": {
            "type": "ping",
            "data": {
                "timestamp": timestamp
            }
        }
    });
    
    let ping_bytes = serde_json::to_vec(&ping_msg)?;
    
    // 发送消息长度
    let len_bytes = (ping_bytes.len() as u32).to_le_bytes();
    send.write_all(&len_bytes).await?;
    
    // 发送消息内容
    send.write_all(&ping_bytes).await?;
    println!("✅ Ping 消息发送成功");
    
    // 接收响应长度
    let mut len_buf = [0u8; 4];
    recv.read_exact(&mut len_buf).await?;
    let resp_len = u32::from_le_bytes(len_buf) as usize;
    
    // 接收响应内容
    let mut resp_buf = vec![0u8; resp_len];
    recv.read_exact(&mut resp_buf).await?;
    
    let pong_msg: serde_json::Value = serde_json::from_slice(&resp_buf)?;
    println!("✅ Pong 消息接收成功:");
    println!("{}", serde_json::to_string_pretty(&pong_msg)?);
    
    // 关闭连接
    conn.close(0u32.into(), b"test done");
    println!("✅ 连接关闭");
    
    println!("");
    println!("=== 测试完成 ===");
    
    Ok(())
}
EOF

echo "✅ 测试程序创建完成"
echo ""

# 编译测试程序
echo "4. 编译测试程序..."
cargo build --release
echo "✅ 测试程序编译完成"
echo ""

# 运行测试
echo "5. 运行 QUIC 测试..."
./target/release/quic_test
echo ""

echo "=== 测试完成 ==="