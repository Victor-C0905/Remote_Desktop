//! 本地 store 文件维护：备份、隔离（备份后移除原文件）、应用重启/退出。
//!
//! 只允许操作已知 store 文件名，防止前端传入任意路径造成穿越/误删。
//! 备份放在 `app_data_dir/backups/`，每个文件保留最近 3 份。

use std::fs;
use std::path::{Path, PathBuf};

use tauri::Manager;

/// 允许维护的 store 文件（新增持久化文件时同步追加）
const ALLOWED_STORES: &[&str] = &[
    "settings.bin",
    "servers.bin",
    "secure.bin",
    "transfers.bin",
    "notifications.bin",
    "browser-data.bin",
];

const MAX_BACKUPS_PER_STORE: usize = 3;

fn validate_name(name: &str) -> Result<(), String> {
    if ALLOWED_STORES.contains(&name) {
        Ok(())
    } else {
        Err(format!("不允许操作的 store 文件: {name}"))
    }
}

fn app_data_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|e| format!("无法定位应用数据目录: {e}"))
}

fn backup_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app_data_path(app)?.join("backups");
    fs::create_dir_all(&dir).map_err(|e| format!("创建备份目录失败: {e}"))?;
    Ok(dir)
}

/// 复制一份带时间戳的备份，返回备份路径
fn create_backup(source: &Path, backups: &Path, name: &str) -> Result<PathBuf, String> {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S-%3f").to_string();
    let dest = backups.join(format!("{name}.{stamp}.bak"));
    fs::copy(source, &dest).map_err(|e| format!("备份 {name} 失败: {e}"))?;
    Ok(dest)
}

/// 清理同一文件的历史备份，只保留最近 MAX_BACKUPS_PER_STORE 份
fn prune_backups(backups: &Path, name: &str) {
    let prefix = format!("{name}.");
    let Ok(entries) = fs::read_dir(backups) else {
        return;
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with(&prefix) && n.ends_with(".bak"))
                .unwrap_or(false)
        })
        .collect();

    // 文件名内嵌时间戳，按名称升序即时间顺序；超量时删除最旧的
    files.sort();
    let keep_from = files.len().saturating_sub(MAX_BACKUPS_PER_STORE);
    for old in files.into_iter().take(keep_from) {
        let _ = fs::remove_file(old);
    }
}

fn backup_one(app: &tauri::AppHandle, name: &str) -> Result<Option<PathBuf>, String> {
    validate_name(name)?;
    let source = app_data_path(app)?.join(name);
    if !source.is_file() {
        return Ok(None);
    }
    let dir = backup_dir(app)?;
    let dest = create_backup(&source, &dir, name)?;
    prune_backups(&dir, name);
    Ok(Some(dest))
}

/// 启动时备份全部已存在的 store：必须在 webview 加载前执行，
/// 保证前端任何写回开始前原文件已有一份留档
pub fn backup_all_on_startup(app: &tauri::AppHandle) {
    for name in ALLOWED_STORES {
        match backup_one(app, name) {
            Ok(Some(path)) => tracing::info!("[Store] 启动备份 {} -> {}", name, path.display()),
            Ok(None) => {} // 文件尚不存在（首次启动）无需备份
            Err(e) => tracing::error!("[Store] 启动备份 {} 失败: {}", name, e),
        }
    }
}

/// 备份指定 store 文件；文件不存在时返回 None（首次启动的正常路径）
#[tauri::command]
pub fn store_backup(name: String, app: tauri::AppHandle) -> Result<Option<String>, String> {
    backup_one(&app, &name).map(|p| p.map(|p| p.to_string_lossy().to_string()))
}

/// 备份并移除原文件：用于恢复对话框的「备份后重启（使用空配置）」
#[tauri::command]
pub fn store_quarantine(name: String, app: tauri::AppHandle) -> Result<Option<String>, String> {
    let backup = backup_one(&app, &name)?;
    let source = app_data_path(&app)?.join(&name);
    if source.is_file() {
        fs::remove_file(&source).map_err(|e| format!("隔离 {name} 失败: {e}"))?;
    }
    Ok(backup.map(|p| p.to_string_lossy().to_string()))
}

/// 重启应用：恢复流程在隔离文件后调用，让其以空配置重新初始化
#[tauri::command]
pub fn app_restart(app: tauri::AppHandle) -> Result<(), String> {
    app.restart();
}

/// 退出应用：用户拒绝自动隔离时使用
#[tauri::command]
pub fn app_exit(app: tauri::AppHandle) {
    app.exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 拒绝白名单外的文件名() {
        for name in ["../servers.bin", "servers.bin/../x", "unknown.bin", "/etc/passwd"] {
            assert!(validate_name(name).is_err(), "{name} 不应通过校验");
        }
        assert!(validate_name("servers.bin").is_ok());
    }

    #[test]
    fn 备份仅保留最近三份() {
        let base = std::env::temp_dir().join(format!("quirel-store-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let backups = base.join("backups");
        fs::create_dir_all(&backups).unwrap();
        let source = base.join("servers.bin");
        fs::write(&source, "v0").unwrap();

        for _ in 0..4 {
            create_backup(&source, &backups, "servers.bin").unwrap();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        prune_backups(&backups, "servers.bin");

        let count = fs::read_dir(&backups).unwrap().count();
        assert_eq!(count, MAX_BACKUPS_PER_STORE);
        let _ = fs::remove_dir_all(&base);
    }
}
