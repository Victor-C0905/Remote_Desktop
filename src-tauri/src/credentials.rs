//! 服务器凭据的安全存储——信封加密架构（密码管理器标准模式）。
//!
//! 背景：Windows Credential Manager 单条凭据上限 2560 字符，SSH 私钥
//! PEM 全文（1700~4000+ 字符）塞不进去。因此：
//! - OS 凭据库（keyring）只存一把 32 字节**主密钥**（永远够短）
//! - 凭据载荷（密码/私钥/口令打包 JSON）用主密钥 AES-GCM 加密后
//!   存 `app_data_dir/credentials/<serverId>.bin`（nonce ‖ 密文）
//! - 拿不到主密钥（keyring 不可用）时密文不可解——安全性等同 keyring 本身
//!
//! command 签名与旧实现保持一致，前端（utils/credentials.ts）零改动。

use base64::Engine as _;
use keyring::Entry;
use tauri::Manager;
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
use ring::rand::{SecureRandom, SystemRandom};

use std::fs;
use std::path::PathBuf;

const SERVICE: &str = "quirel-server-credentials";
/// keyring 中的主密钥条目 account（凭据本身不再进 keyring）
const MASTER_ACCOUNT: &str = "__master__";
const NONCE_LEN: usize = 12;

fn entry(account: &str) -> Result<Entry, String> {
    Entry::new(SERVICE, account).map_err(|e| format!("凭据存储不可用: {}", e))
}

/// 主密钥：优先读取，不存在则生成并写入 keyring（首次初始化）
fn master_key() -> Result<[u8; 32], String> {
    let e = entry(MASTER_ACCOUNT)?;
    match e.get_password() {
        Ok(b64) => {
            let raw = base64::engine::general_purpose::STANDARD
                .decode(b64.as_bytes())
                .map_err(|e| format!("主密钥解码失败: {}", e))?;
            if raw.len() != 32 {
                return Err("主密钥长度异常".to_string());
            }
            let mut key = [0u8; 32];
            key.copy_from_slice(&raw);
            Ok(key)
        }
        Err(keyring::Error::NoEntry) => {
            // 首次：生成 32 字节随机主密钥并入库
            let rng = SystemRandom::new();
            let mut key = [0u8; 32];
            rng.fill(&mut key).map_err(|e| format!("随机源失败: {}", e))?;
            let b64 = base64::engine::general_purpose::STANDARD.encode(key);
            e.set_password(&b64).map_err(|e| format!("主密钥写入失败: {}", e))?;
            tracing::info!("[Credentials] 主密钥首次初始化完成");
            Ok(key)
        }
        Err(e) => Err(format!("主密钥读取失败: {}", e)),
    }
}

/// 凭据密文文件路径（account 仅允许安全字符，双保险防路径穿越）
fn cred_path(app: &tauri::AppHandle, account: &str) -> Result<PathBuf, String> {
    let safe: String = account
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法定位应用数据目录: {}", e))?
        .join("credentials");
    Ok(dir.join(format!("{}.bin", safe)))
}

fn seal(plaintext: &str, key: &[u8; 32]) -> Result<Vec<u8>, String> {
    let rng = SystemRandom::new();
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rng.fill(&mut nonce_bytes).map_err(|e| format!("随机源失败: {}", e))?;
    let key_obj = LessSafeKey::new(
        UnboundKey::new(&AES_256_GCM, key).map_err(|e| format!("密钥初始化失败: {:?}", e))?,
    );
    let mut in_out = plaintext.as_bytes().to_vec();
    key_obj
        .seal_in_place_append_tag(Nonce::assume_unique_for_key(nonce_bytes), Aad::empty(), &mut in_out)
        .map_err(|e| format!("加密失败: {:?}", e))?;
    let mut out = nonce_bytes.to_vec();
    out.extend_from_slice(&in_out);
    Ok(out)
}

fn open(sealed: &[u8], key: &[u8; 32]) -> Result<String, String> {
    if sealed.len() <= NONCE_LEN {
        return Err("密文数据损坏".to_string());
    }
    let (nonce_bytes, ct) = sealed.split_at(NONCE_LEN);
    let mut nonce_arr = [0u8; NONCE_LEN];
    nonce_arr.copy_from_slice(nonce_bytes);
    let key_obj = LessSafeKey::new(
        UnboundKey::new(&AES_256_GCM, key).map_err(|e| format!("密钥初始化失败: {:?}", e))?,
    );
    let mut in_out = ct.to_vec();
    let plain = key_obj
        .open_in_place(Nonce::assume_unique_for_key(nonce_arr), Aad::empty(), &mut in_out)
        .map_err(|e| format!("解密失败: {:?}", e))?;
    String::from_utf8(plain.to_vec()).map_err(|e| format!("明文非 UTF-8: {}", e))
}

#[tauri::command]
pub fn credential_set(account: String, value: String, app: tauri::AppHandle) -> Result<(), String> {
    let key = master_key()?;
    let sealed = seal(&value, &key)?;
    let path = cred_path(&app, &account)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建凭据目录失败: {}", e))?;
    }
    fs::write(&path, sealed).map_err(|e| format!("写入凭据文件失败: {}", e))
}

#[tauri::command]
pub fn credential_get(account: String, app: tauri::AppHandle) -> Result<Option<String>, String> {
    let path = cred_path(&app, &account)?;
    if !path.exists() {
        return Ok(None);
    }
    let sealed = fs::read(&path).map_err(|e| format!("读取凭据文件失败: {}", e))?;
    let key = master_key()?;
    Ok(Some(open(&sealed, &key)?))
}

#[tauri::command]
pub fn credential_delete(account: String, app: tauri::AppHandle) -> Result<(), String> {
    let path = cred_path(&app, &account)?;
    if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("删除凭据文件失败: {}", e))?;
    }
    Ok(())
}

/// 删除凭据（按 serverId）与主密钥（完全清除用：密文随数据目录删除，
/// 主密钥在系统凭据库须显式清）
#[tauri::command]
pub fn credential_delete_all(accounts: Vec<String>, app: tauri::AppHandle) -> Result<(), String> {
    for a in accounts {
        let _ = credential_delete(a, app.clone());
    }
    let _ = entry(MASTER_ACCOUNT).and_then(|e| match e.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("主密钥删除失败: {}", e)),
    });
    Ok(())
}

/// keyring 可用性探针：主密钥生成/读取 + 加解密往返 + 目录可写。
/// 前端启动时调用——不可用时立即暴露（表单阻止保存凭据）。
#[tauri::command]
pub fn credential_probe(app: tauri::AppHandle) -> Result<(), String> {
    let key = master_key()?;
    let probe = "quirel-probe-往返校验";
    let sealed = seal(probe, &key)?;
    if open(&sealed, &key)? != probe {
        return Err("加解密往返校验失败".to_string());
    }
    let dir = cred_path(&app, "__probe__")?
        .parent()
        .ok_or("路径异常")?
        .to_path_buf();
    fs::create_dir_all(&dir).map_err(|e| format!("凭据目录不可写: {}", e))?;
    Ok(())
}
