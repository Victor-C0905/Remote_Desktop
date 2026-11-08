use anyhow::{Context, Result};
use rcgen::{CertificateParams, DistinguishedName, KeyPair, PKCS_ECDSA_P256_SHA256};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::fs;
use std::path::Path;

use crate::config::AgentConfig;

/// 写入敏感文件（证书/私钥）：Unix 下强制 0600 + O_NOFOLLOW。
/// .mode() 仅对新建文件生效，故对已存在的旧文件再补一次显式收紧——
/// 否则 0644 的旧私钥会被本机任意用户读取（守护进程以 root 运行时尤为严重）。
fn write_secret_file(path: &Path, contents: &str) -> Result<()> {
    #[cfg(unix)]
    {
        use std::fs::OpenOptions;
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
            .with_context(|| format!("打开文件失败: {:?}", path))?;
        f.write_all(contents.as_bytes())
            .with_context(|| format!("写入文件失败: {:?}", path))?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("收紧文件权限失败: {:?}", path))?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, contents).with_context(|| format!("写入文件失败: {:?}", path))
    }
}

pub fn ensure_certificate(cfg: &AgentConfig) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
    let cert_path = Path::new(&cfg.server.cert_path);
    let key_path = Path::new(&cfg.server.key_path);

    if cert_path.exists() && key_path.exists() {
        tracing::info!("使用已有证书: {}", cfg.server.cert_path);
        // 对已存在文件补一次权限收紧：write_secret_file 的 0600 仅对新建
        // 生效，历史遗留的 0644 私钥会被本机任意用户读取（守护进程以
        // root 运行时尤为严重；2026-11 审计 L2）
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for p in [key_path, cert_path] {
                if let Err(e) =
                    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600))
                {
                    tracing::warn!("收紧已有密钥/证书权限失败: {:?}: {}", p, e);
                }
            }
        }
        let certs = load_certs(cert_path)?;
        let key = load_key(key_path)?;
        return Ok((certs, key));
    }

    tracing::info!("生成自签名证书...");

    let mut params = CertificateParams::default();
    params.distinguished_name = DistinguishedName::new();
    params.distinguished_name.push(rcgen::DnType::CommonName, "Quireld");
    params.distinguished_name.push(rcgen::DnType::OrganizationName, "Quirel");
    params.alg = &PKCS_ECDSA_P256_SHA256;

    let key_pair = KeyPair::generate(&PKCS_ECDSA_P256_SHA256)?;
    let key_pem = key_pair.serialize_pem();
    params.key_pair = Some(key_pair);

    let cert = rcgen::Certificate::from_params(params)?;
    let cert_pem = cert.serialize_pem()?;

    write_secret_file(cert_path, &cert_pem)
        .with_context(|| format!("写入证书文件失败: {:?}", cert_path))?;
    write_secret_file(key_path, &key_pem)
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
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_secret_file_writes_content_with_private_mode() {
        let dir = std::env::temp_dir().join(format!("quireld-cert-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("key.pem");
        write_secret_file(&path, "TEST-PEM").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "TEST-PEM");
        // Unix：私钥文件必须 0600（含对已存在旧文件的收紧）
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "私钥文件必须 0600");
        }
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }
}
