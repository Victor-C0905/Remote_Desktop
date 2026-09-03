//! SOCKS5 服务端协议实现（RFC 1928 子集）
//!
//! 只实现：无认证 + CONNECT 命令 + IPv4/域名/IPv6 目标地址。
//! BIND / UDP ASSOCIATE / 用户名密码认证均不支持（浏览器场景用不到，YAGNI）。

use std::io::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// CONNECT 请求解析出的目标地址
#[derive(Debug, Clone, PartialEq)]
pub struct Socks5Target {
    pub host: String,
    pub port: u16,
}

const SOCKS5_VERSION: u8 = 0x05;

/// 读 greeting 并回复"无需认证"。
/// 浏览器发起：[VER=5, NMETHODS, METHODS...]
pub async fn socks5_greeting(stream: &mut TcpStream) -> Result<(), String> {
    let ver = stream.read_u8().await.map_err(err("读协议版本"))?;
    if ver != SOCKS5_VERSION {
        return Err(format!("仅支持 SOCKS5，收到版本 {}", ver));
    }
    let nmethods = stream.read_u8().await.map_err(err("读方法数"))?;
    let mut methods = vec![0u8; nmethods as usize];
    stream.read_exact(&mut methods).await.map_err(err("读方法列表"))?;
    // 回复：无需认证（0x00）
    stream.write_all(&[SOCKS5_VERSION, 0x00]).await.map_err(err("回复 greeting"))?;
    Ok(())
}

/// 读 CONNECT 请求并解析目标地址（域名场景直接透传域名 → 远程 DNS）。
pub async fn socks5_read_connect(stream: &mut TcpStream) -> Result<Socks5Target, String> {
    let ver = stream.read_u8().await.map_err(err("读版本"))?;
    if ver != SOCKS5_VERSION {
        return Err(format!("CONNECT 版本错误: {}", ver));
    }
    let cmd = stream.read_u8().await.map_err(err("读命令"))?;
    if cmd != 0x01 {
        // 仅支持 CONNECT
        socks5_reply(stream, 0x07).await?; // 0x07 command not supported
        return Err(format!("仅支持 CONNECT，收到命令 0x{:02x}", cmd));
    }
    let _rsv = stream.read_u8().await.map_err(err("读保留位"))?;
    let atyp = stream.read_u8().await.map_err(err("读地址类型"))?;

    let host = match atyp {
        0x01 => {
            // IPv4：4 字节，转点分十进制
            let mut b = [0u8; 4];
            stream.read_exact(&mut b).await.map_err(err("读 IPv4"))?;
            format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3])
        }
        0x03 => {
            // 域名：1 字节长度 + 域名（远程解析的关键路径）
            let len = stream.read_u8().await.map_err(err("读域名长度"))?;
            let mut b = vec![0u8; len as usize];
            stream.read_exact(&mut b).await.map_err(err("读域名"))?;
            String::from_utf8(b).map_err(|_| "域名不是合法 UTF-8".to_string())?
        }
        0x04 => {
            // IPv6：16 字节，转标准格式
            let mut b = [0u8; 16];
            stream.read_exact(&mut b).await.map_err(err("读 IPv6"))?;
            let ip: std::net::Ipv6Addr = b.into();
            ip.to_string()
        }
        other => return Err(format!("未知地址类型 0x{:02x}", other)),
    };

    let port = stream.read_u16().await.map_err(err("读端口"))?;

    Ok(Socks5Target { host, port })
}

/// 回复 CONNECT 结果。code: 0x00 成功，其余为 RFC 1928 错误码。
pub async fn socks5_reply(stream: &mut TcpStream, code: u8) -> Result<(), String> {
    // 回复格式：VER, REP, RSV, ATYP=IPv4, BND.ADDR=0.0.0.0, BND.PORT=0
    stream
        .write_all(&[SOCKS5_VERSION, code, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
        .await
        .map_err(err("回复 CONNECT 结果"))?;
    Ok(())
}

fn err(ctx: &str) -> impl Fn(Error) -> String + '_ {
    move |e| format!("{}: {}", ctx, e)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn greeting_and_connect_domain() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mut a = tokio::net::TcpStream::connect(addr).await.unwrap();
        let (mut b, _) = listener.accept().await.unwrap();

        // 浏览器发 greeting：支持无认证
        a.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
        socks5_greeting(&mut b).await.unwrap();
        let mut buf = [0u8; 2];
        a.read_exact(&mut buf).await.unwrap();
        assert_eq!(buf, [0x05, 0x00]);

        // 浏览器发 CONNECT example.com:443（域名类型）
        a.write_all(&[0x05, 0x01, 0x00, 0x03, 11]).await.unwrap();
        a.write_all(b"example.com").await.unwrap();
        a.write_all(&443u16.to_be_bytes()).await.unwrap();
        let target = socks5_read_connect(&mut b).await.unwrap();
        assert_eq!(target, Socks5Target { host: "example.com".to_string(), port: 443 });
    }

    #[tokio::test]
    async fn connect_ipv4() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mut a = tokio::net::TcpStream::connect(addr).await.unwrap();
        let (mut b, _) = listener.accept().await.unwrap();

        a.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
        socks5_greeting(&mut b).await.unwrap();
        let _ = a.read_exact(&mut [0u8; 2]).await;

        // CONNECT 10.0.0.1:80
        a.write_all(&[0x05, 0x01, 0x00, 0x01, 10, 0, 0, 1]).await.unwrap();
        a.write_all(&80u16.to_be_bytes()).await.unwrap();
        let target = socks5_read_connect(&mut b).await.unwrap();
        assert_eq!(target, Socks5Target { host: "10.0.0.1".to_string(), port: 80 });
    }

    #[tokio::test]
    async fn reject_non_connect_command() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mut a = tokio::net::TcpStream::connect(addr).await.unwrap();
        let (mut b, _) = listener.accept().await.unwrap();

        a.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
        socks5_greeting(&mut b).await.unwrap();
        let _ = a.read_exact(&mut [0u8; 2]).await;

        // BIND 命令（0x02）应被拒绝并回复 0x07
        a.write_all(&[0x05, 0x02, 0x00, 0x01, 0, 0, 0, 0]).await.unwrap();
        a.write_all(&0u16.to_be_bytes()).await.unwrap();
        assert!(socks5_read_connect(&mut b).await.is_err());
        let mut reply = [0u8; 10];
        a.read_exact(&mut reply).await.unwrap();
        assert_eq!(reply[1], 0x07);
    }
}
