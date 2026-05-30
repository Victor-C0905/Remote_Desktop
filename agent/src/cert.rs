use anyhow::{Context, Result};
use rcgen::{CertificateParams, DistinguishedName, KeyPair, PKCS_ECDSA_P256_SHA256};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::fs;
use std::path::Path;

use crate::config::AgentConfig;

pub fn ensure_certificate(cfg: &AgentConfig) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
    let cert_path = Path::new(&cfg.server.cert_path);
    let key_path = Path::new(&cfg.server.key_path);

    if cert_path.exists() && key_path.exists() {
        tracing::info!("使用已有证书: {}", cfg.server.cert_path);
        let certs = load_certs(cert_path)?;
        let key = load_key(key_path)?;
        return Ok((certs, key));
    }

    tracing::info!("生成自签名证书...");

    let mut params = CertificateParams::default();
    params.distinguished_name = DistinguishedName::new();
    params.distinguished_name.push(rcgen::DnType::CommonName, "GNOME Remote Agent");
    params.distinguished_name.push(rcgen::DnType::OrganizationName, "GNOME Remote");
    params.alg = &PKCS_ECDSA_P256_SHA256;

    let key_pair = KeyPair::generate(&PKCS_ECDSA_P256_SHA256)?;
    let key_pem = key_pair.serialize_pem();
    params.key_pair = Some(key_pair);

    let cert = rcgen::Certificate::from_params(params)?;
    let cert_pem = cert.serialize_pem()?;

    fs::write(cert_path, &cert_pem)
        .with_context(|| format!("写入证书文件失败: {:?}", cert_path))?;
    fs::write(key_path, &key_pem)
        .with_context(|| format!("写入私钥文件失败: {:?}", key_path))?;

    tracing::info!("证书已生成:");
    tracing::info!("  证书: {}", cfg.server.cert_path);
    tracing::info!("  私钥: {}", cfg.server.key_path);

    let certs = parse_cert_pem(&cert_pem)?;
    let key = parse_key_pem(&key_pem)?;

    Ok((certs, key))
}

fn load_certs(path: &Path) -> Result<Vec<CertificateDer<'static>>> {
    let pem = fs::read(path).context("读取证书文件失败")?;
    parse_cert_pem(&String::from_utf8_lossy(&pem))
}

fn load_key(path: &Path) -> Result<PrivateKeyDer<'static>> {
    let pem = fs::read(path).context("读取私钥文件失败")?;
    parse_key_pem(&String::from_utf8_lossy(&pem))
}

fn parse_cert_pem(pem: &str) -> Result<Vec<CertificateDer<'static>>> {
    let mut certs = Vec::new();
    for item in rustls_pemfile::certs(&mut pem.as_bytes()) {
        certs.push(item.context("解析证书 PEM 失败")?);
    }
    if certs.is_empty() {
        anyhow::bail!("PEM 文件中未找到有效证书");
    }
    Ok(certs)
}

fn parse_key_pem(pem: &str) -> Result<PrivateKeyDer<'static>> {
    let keys = rustls_pemfile::private_key(&mut pem.as_bytes())
        .context("解析私钥 PEM 失败")?
        .context("PEM 文件中未找到有效私钥")?;
    Ok(keys)
}