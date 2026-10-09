// agent/src/auth/path_guard.rs
//! 文件路径规范化校验
//!
//! 设计原则:与终端访问权限保持一致。
//! 用户隔离由 UserExecutor(fork+setuid) 实现,文件操作在目标用户上下文中执行,
//! Linux 文件系统权限自动生效(与 PTY 终端登录后的权限模型完全一致)。
//!
//! 本模块仅做路径规范化(canonicalize 解析符号链接),不叠加额外限制:
//! - 不限制家目录范围(终端可访问 /,文件管理器同样可访问)
//! - 不拒绝 .. (终端允许 cd ..,文件操作同样允许)
//! - 访问控制完全依赖 Linux 文件系统权限 + UserExecutor 用户降权

use std::path::{Path, PathBuf};
use anyhow::{Result, bail};

/// 校验后的安全路径(newtype,防止绕过)
#[derive(Debug, Clone)]
pub struct SafePath(PathBuf);

impl SafePath {
    pub fn as_str(&self) -> &str {
        // 不变式: validate_path 已校验 UTF-8,此处不应触发。
        // 若触发,说明 SafePath 被绕过构造,需排查构造路径。
        debug_assert!(self.0.to_str().is_some(), "SafePath 内部不变式违反: 非 UTF-8 路径");
        self.0.to_str().unwrap_or("")
    }
    pub fn as_path(&self) -> &Path { &self.0 }
}

/// 校验路径安全性
///
/// 设计:与终端访问权限一致。不限制家目录、不拒绝 ..,
/// 仅 canonicalize 解析符号链接(与 bash 路径解析行为一致)。
/// 实际访问控制由 UserExecutor(fork+setuid) + Linux 文件系统权限保证。
///
/// # 参数
/// - `path`: 目标路径
/// - `home_dir`: 保留用于兼容调用签名(不再用于范围限制)
/// - `uid`: 保留用于兼容调用签名(不再区分 root,统一靠 Linux 权限)
pub fn validate_path(path: &str, _home_dir: &Path, _uid: u32) -> Result<SafePath> {
    // 空路径拒绝: Path::new("") 会产生无意义空 Path,提前拦截
    if path.is_empty() {
        bail!("路径不能为空");
    }
    let target = Path::new(path);

    // canonicalize 解析符号链接(与 bash 路径解析一致)。
    // 路径不存在时(如新建文件)canonicalize 失败,回退原始路径,
    // 由 UserExecutor 降权后 Linux 权限决定是否可写。
    let canon = std::fs::canonicalize(target).unwrap_or_else(|_| target.to_path_buf());

    // 校验 UTF-8: SafePath 不变式要求路径为有效 UTF-8
    if canon.to_str().is_none() {
        bail!("路径含非 UTF-8 字符,不支持: {}", canon.display());
    }

    Ok(SafePath(canon))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// 测试:空路径应被拒绝
    ///
    /// 空路径会绕过 Path::new 产生无意义空 Path,应在入口直接拒绝。
    #[test]
    fn test_reject_empty_path() {
        let home = std::env::temp_dir();
        let result = validate_path("", &home, 1000);
        assert!(result.is_err(), "空路径应被拒绝");
    }

    /// 测试:路径含 .. 应被允许(与终端一致)
    ///
    /// 终端允许 `cd ..`,文件操作同样允许。canonicalize 会解析 .. 到真实路径。
    #[test]
    fn test_traversal_allowed() {
        let temp = tempfile::tempdir().expect("创建临时目录失败");
        let temp_canon = fs::canonicalize(temp.path()).expect("解析临时目录失败");

        // 创建子目录,构造含 .. 的路径
        let subdir = temp_canon.join("sub");
        fs::create_dir(&subdir).expect("创建子目录失败");
        let path_with_parent = subdir.join("..");

        // 非 root 用户:.. 允许,canonicalize 解析到 temp_canon
        let result = validate_path(path_with_parent.to_str().unwrap(), &temp_canon, 1000);
        assert!(result.is_ok(), "含 .. 的路径应通过(与终端一致): {:?}", result.err());
        // canonicalize 后应等于 temp_canon
        let safe = result.unwrap();
        assert_eq!(safe.as_path(), temp_canon);
    }

    /// 测试:家目录外路径应被允许(与终端一致)
    ///
    /// 终端可访问 /,文件管理器同样可访问家目录外的系统目录,
    /// 实际访问由 UserExecutor + Linux 权限决定。
    #[test]
    fn test_out_of_home_allowed() {
        let home = tempfile::tempdir().expect("创建家目录临时目录失败");
        let outside = tempfile::tempdir().expect("创建外部临时目录失败");

        let home_canon = fs::canonicalize(home.path()).expect("解析家目录失败");
        let outside_canon = fs::canonicalize(outside.path()).expect("解析外部目录失败");

        // 在家目录外创建文件
        let outside_file = outside_canon.join("data.txt");
        fs::write(&outside_file, "data").expect("写入外部文件失败");

        // 非 root 用户:家目录外路径允许(靠 Linux 权限限制)
        let result = validate_path(outside_file.to_str().unwrap(), &home_canon, 1000);
        assert!(result.is_ok(), "家目录外路径应通过(与终端一致): {:?}", result.err());
    }

    /// 测试:符号链接应被 canonicalize 解析(与 bash 一致)
    ///
    /// canonicalize 解析符号链接到真实目标路径,与 bash 路径解析行为一致。
    /// 不再因链接目标越界家目录而拒绝。
    #[cfg(unix)]
    #[test]
    fn test_symlink_resolved() {
        let home = tempfile::tempdir().expect("创建家目录临时目录失败");
        let outside = tempfile::tempdir().expect("创建外部临时目录失败");

        let home_canon = fs::canonicalize(home.path()).expect("解析家目录失败");
        let outside_canon = fs::canonicalize(outside.path()).expect("解析外部目录失败");

        // 在家目录内创建符号链接,指向家目录外
        let symlink_path = home_canon.join("escape_link");
        std::os::unix::fs::symlink(&outside_canon, &symlink_path)
            .expect("创建符号链接失败");

        // 符号链接被 canonicalize 解析到 outside_canon,允许通过
        let result = validate_path(symlink_path.to_str().unwrap(), &home_canon, 1000);
        assert!(result.is_ok(), "符号链接应被解析通过: {:?}", result.err());
        let safe = result.unwrap();
        assert_eq!(safe.as_path(), outside_canon, "应解析到链接目标");
    }

    /// 测试:不存在的路径回退原始路径(新建文件场景)
    ///
    /// 路径不存在时 canonicalize 失败(如 WriteFile 新建文件),
    /// 回退原始路径由 UserExecutor 降权后 Linux 权限决定可写性。
    #[test]
    fn test_nonexistent_path_falls_back() {
        let home = std::env::temp_dir();
        let nonexistent = home.join("definitely_does_not_exist_xyz123/new_file.txt");
        let result = validate_path(
            nonexistent.to_str().unwrap(),
            &home,
            1000,
        );
        assert!(result.is_ok(), "不存在路径应回退原始路径(新建文件场景): {:?}", result.err());
        // 返回的应是原始路径(canonicalize 失败回退)
        let safe = result.unwrap();
        assert_eq!(safe.as_path(), nonexistent);
    }

    /// 测试:有效路径返回 canonicalize 后的路径
    #[test]
    fn test_valid_path_canonicalized() {
        let home = tempfile::tempdir().expect("创建家目录临时目录失败");
        let home_canon = fs::canonicalize(home.path()).expect("解析家目录失败");

        // 在家目录内创建文件
        let file_path = home_canon.join("test.txt");
        fs::write(&file_path, "content").expect("写入文件失败");

        let result = validate_path(file_path.to_str().unwrap(), &home_canon, 1000);
        assert!(result.is_ok(), "有效路径应通过校验: {:?}", result.err());

        // 验证返回的路径是 canonicalize 后的
        let safe = result.unwrap();
        assert_eq!(safe.as_path(), file_path);
    }

    /// 测试:含 CurDir(`.`)组件的路径应通过
    ///
    /// `./subdir/file` 形式路径,canonicalize 后解析为真实路径。
    #[test]
    fn test_accept_curdir_component() {
        let tmp = tempfile::tempdir().expect("创建 tempdir 失败");
        let subdir = tmp.path().join("sub");
        std::fs::create_dir(&subdir).expect("创建子目录失败");
        let path_with_dot = subdir.join("./file.txt");
        std::fs::write(&path_with_dot, "test").expect("创建文件失败");
        let result = validate_path(
            path_with_dot.to_str().unwrap(),
            tmp.path(),
            1000,
        );
        assert!(result.is_ok(), "含 . 组件的合法路径应通过: {:?}", result.err());
    }

    /// 测试:root 与非 root 行为一致(不再区分)
    ///
    /// 新模型下 root 和非 root 都靠 UserExecutor + Linux 权限,
    /// validate_path 不再区分 uid,行为一致。
    #[test]
    fn test_root_non_root_consistent() {
        let temp = tempfile::tempdir().expect("创建临时目录失败");
        let temp_canon = fs::canonicalize(temp.path()).expect("解析临时目录失败");
        let file_path = temp_canon.join("file.txt");
        fs::write(&file_path, "x").expect("写入文件失败");

        let r_root = validate_path(file_path.to_str().unwrap(), &temp_canon, 0);
        let r_user = validate_path(file_path.to_str().unwrap(), &temp_canon, 1000);
        assert!(r_root.is_ok() && r_user.is_ok(), "root 与非 root 行为应一致");
    }
}
