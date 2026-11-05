//! 解压前条目校验 + 解压后 setuid/setgid 清理
//!
//! 客户端的前端检查不可信；解压前由 Agent 读取归档清单并拒绝：
//! - 绝对路径 / 含 `..` 的条目名
//! - 指向归档外部的符号链接 / 硬链接
//! - 设备、FIFO、socket 等特殊文件
//!
//! 解压后再按清单清理已解出文件/目录的 setuid/setgid 位（tar 已由
//! `--no-same-permissions` 处理，其余工具需要这一步兜底）。

use anyhow::{bail, Result};
use std::collections::HashSet;
use std::path::Path;
use std::os::unix::fs::PermissionsExt;

use super::commands::ListingKind;

/// 条目是否落在本次解压范围（None = 全部；Some = 精确文件及其祖先目录）
///
/// 目录条目在清单里带尾部 `/`，与客户端选择路径（无尾部 `/`）比对前统一去掉。
fn in_scope(name: &str, selected: Option<&[String]>) -> bool {
    let Some(list) = selected else { return true };
    let name = name.trim_end_matches('/');
    list.iter().any(|sel| {
        let sel = sel.trim_end_matches('/');
        sel == name || sel.starts_with(&format!("{name}/"))
    })
}

fn check_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("压缩包包含空条目名");
    }
    if name.contains('\0') {
        bail!("压缩包条目名含非法字符");
    }
    if name.starts_with('/') {
        bail!("压缩包包含绝对路径条目: {name}");
    }
    if name.split('/').any(|part| part == "..") {
        bail!("压缩包条目包含 ..: {name}");
    }
    Ok(())
}

/// 校验链接目标不逃出解压目录
///
/// - 硬链接目标按归档根解析（tar 语义），任何 `..` 都视为逃逸；
/// - 符号链接目标按链接所在目录解析，`../` 只在越过解压根时才拒绝，
///   避免误伤 `dir/link -> ../other` 这类常见合法归档。
fn check_link_target(target: &str, name: &str, root_relative: bool) -> Result<()> {
    if target.is_empty() || target.contains('\0') {
        bail!("链接目标非法: {name}");
    }
    if target.starts_with('/') {
        bail!("链接指向绝对路径: {name} -> {target}");
    }
    if root_relative {
        if target.split('/').any(|part| part == "..") {
            bail!("硬链接目标逃出解压目录: {name} -> {target}");
        }
        return Ok(());
    }

    // 从链接所在目录出发做词法解析；栈空时再遇到 .. 即为逃逸
    let mut stack: Vec<&str> = name
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect();
    stack.pop();
    for part in target.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            if stack.pop().is_none() {
                bail!("符号链接目标逃出解压目录: {name} -> {target}");
            }
        } else {
            stack.push(part);
        }
    }
    Ok(())
}

/// 跳过前 n 个空白分隔字段，返回剩余内容（tar verbose 的 name 部分）
fn skip_fields(line: &str, n: usize) -> Option<&str> {
    let mut rest = line;
    for _ in 0..n {
        rest = rest.trim_start();
        if rest.is_empty() {
            return None;
        }
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        rest = &rest[end..];
    }
    Some(rest.trim_start())
}

fn parse_tar_verbose(stdout: &str, selected: Option<&[String]>) -> Result<Vec<String>> {
    let mut paths = Vec::new();
    for line in stdout.lines() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        let Some((fields, rest)) = skip_fields(line, 5).map(|rest| (line, rest)) else {
            bail!("无法解析 tar 列表行: {line}");
        };
        let mode = fields.split_whitespace().next().unwrap_or("");
        let kind = mode.chars().next().unwrap_or('-');
        match kind {
            '-' | 'd' => {
                if in_scope(rest, selected) {
                    check_name(rest)?;
                    paths.push(rest.to_string());
                }
            }
            'l' => {
                let Some(idx) = rest.rfind(" -> ") else {
                    bail!("无法解析符号链接行: {line}");
                };
                let name = rest[..idx].trim();
                let target = rest[idx + 4..].trim();
                if in_scope(name, selected) {
                    check_name(name)?;
                    check_link_target(target, name, false)?;
                }
            }
            'h' => {
                let Some(idx) = rest.rfind(" link to ") else {
                    bail!("无法解析硬链接行: {line}");
                };
                let name = rest[..idx].trim();
                let target = rest[idx + 9..].trim();
                if in_scope(name, selected) {
                    check_name(name)?;
                    check_link_target(target, name, true)?;
                }
            }
            'b' | 'c' | 'p' | 's' => {
                if in_scope(rest, selected) {
                    bail!("压缩包包含特殊文件，已拒绝解压: {rest}");
                }
            }
            other => {
                if in_scope(rest, selected) {
                    bail!("压缩包包含未知条目类型 {other}: {line}");
                }
            }
        }
    }
    Ok(paths)
}

/// 取 7z 属性列里的 Unix 权限串（形如 `_ lrwxrwxrwx` / `A_ -rw-r--r--`）
///
/// 7z 从 zip/tar 读取链接时不会输出 Symbolic Link 键，类型位只能从这里判断。
fn attribute_mode(value: &str) -> Option<&str> {
    value
        .split_whitespace()
        .last()
        .filter(|token| token.len() >= 10)
}

fn parse_7z_slt(
    stdout: &str,
    archive_path: &str,
    selected: Option<&[String]>,
) -> Result<Vec<String>> {
    let mut paths = Vec::new();
    let mut block: Vec<(String, String)> = Vec::new();

    let mut flush = |block: &mut Vec<(String, String)>| -> Result<()> {
        let get = |key: &str| {
            block
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
        };
        let Some(path) = get("Path") else {
            block.clear();
            return Ok(());
        };
        // 首个块是归档自身（含 Type），跳过
        if get("Type").is_some() || path == archive_path {
            block.clear();
            return Ok(());
        }
        if !in_scope(path, selected) {
            block.clear();
            return Ok(());
        }
        check_name(path)?;
        if let Some(target) = get("Symbolic Link").filter(|v| !v.is_empty()) {
            check_link_target(target, path, false)?;
        } else if let Some(target) = get("Hard Link").filter(|v| !v.is_empty()) {
            check_link_target(target, path, true)?;
        } else {
            // 链接没有目标信息（zip/tar 经 7z 读取），按属性类型位判断：
            // 符号链接不会穿过写入，交由解压工具处理；设备/FIFO 等直接拒绝。
            match get("Attributes").and_then(attribute_mode).and_then(|m| m.chars().next()) {
                Some('l') => {}
                Some('b' | 'c' | 'p' | 's') => {
                    bail!("压缩包包含特殊文件，已拒绝解压: {path}");
                }
                // 普通文件与目录都纳入事后权限清理范围（目录的 setgid 也需清除）
                _ => paths.push(path.to_string()),
            }
        }
        block.clear();
        Ok(())
    };

    for line in stdout.lines() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() {
            flush(&mut block)?;
            continue;
        }
        if let Some((key, value)) = line.split_once(" = ") {
            block.push((key.trim().to_string(), value.to_string()));
        }
    }
    flush(&mut block)?;
    Ok(paths)
}

fn parse_names(stdout: &str, selected: Option<&[String]>) -> Result<Vec<String>> {
    let mut paths = Vec::new();
    for line in stdout.lines() {
        let name = line.trim_end_matches('\r');
        if name.is_empty() || !in_scope(name, selected) {
            continue;
        }
        check_name(name)?;
        paths.push(name.to_string());
    }
    Ok(paths)
}

/// 校验归档清单；返回可安全解出的条目名（用于事后权限清理）
pub fn validate_listing(
    kind: ListingKind,
    stdout: &str,
    archive_path: &str,
    selected: Option<&[String]>,
) -> Result<Vec<String>> {
    match kind {
        ListingKind::TarVerbose => parse_tar_verbose(stdout, selected),
        ListingKind::SevenZipSlt => parse_7z_slt(stdout, archive_path, selected),
        ListingKind::Names => parse_names(stdout, selected),
    }
}

/// 清除已解出文件/目录的 setuid/setgid 位（在降权后的用户上下文中调用）
pub fn sanitize_extracted_permissions(
    target: &str,
    names: &[String],
    skip_existing: &HashSet<String>,
) -> usize {
    let mut cleaned = 0usize;
    for name in names {
        // skip 策略下已存在的文件未被覆盖，不能改动其权限
        if skip_existing.contains(name) {
            continue;
        }
        let path = Path::new(target).join(name);
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        let ft = meta.file_type();
        if ft.is_symlink() || !(ft.is_file() || ft.is_dir()) {
            continue;
        }
        let mode = meta.permissions().mode();
        if mode & 0o6000 == 0 {
            continue;
        }
        let new_mode = mode & !0o6000;
        match std::fs::set_permissions(&path, std::fs::Permissions::from_mode(new_mode)) {
            Ok(()) => cleaned += 1,
            Err(e) => {
                tracing::warn!("清除 setuid/setgid 失败: path={}, error={}", path.display(), e);
            }
        }
    }
    tracing::info!("解压后权限清理: target={}, cleaned={}", target, cleaned);
    cleaned
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tar_rejects_traversal_and_special() {
        let ok = "\
-rw-r--r-- 0/0 3 2026-11-05 18:00 a.txt
drwxr-xr-x 0/0 0 2026-11-05 18:00 dir/
lrwxrwxrwx 0/0 0 2026-11-05 18:00 link -> ../x
";
        assert!(validate_listing(ListingKind::TarVerbose, ok, "/x/a.tar", None).is_err());

        let special = "c rw-r--r-- 0/0 0 2026-11-05 18:00 dev\n";
        assert!(validate_listing(ListingKind::TarVerbose, special, "/x/a.tar", None).is_err());

        let safe = "\
-rw-r--r-- 0/0 3 2026-11-05 18:00 a.txt
drwxr-xr-x 0/0 0 2026-11-05 18:00 dir/
lrwxrwxrwx 0/0 0 2026-11-05 18:00 link -> a.txt
";
        let paths = validate_listing(ListingKind::TarVerbose, safe, "/x/a.tar", None).unwrap();
        assert_eq!(paths, vec!["a.txt", "dir/"]);
    }

    #[test]
    fn tar_verbose_handles_spaces_and_directories() {
        let listing = "\
drwxr-xr-x 0/0 0 2026-11-05 18:00 dir/
-rw-r--r-- 0/0 3 2026-11-05 18:00 dir/a b.txt
lrwxrwxrwx 0/0 0 2026-11-05 18:00 link -> dir/a b.txt
";
        let paths = validate_listing(ListingKind::TarVerbose, listing, "/x/a.tar", None).unwrap();
        assert_eq!(paths, vec!["dir/", "dir/a b.txt"]);
    }

    #[test]
    fn tar_link_targets_must_stay_inside_target() {
        let inside = "\
drwxr-xr-x 0/0 0 2026-11-05 18:00 dir/
lrwxrwxrwx 0/0 0 2026-11-05 18:00 dir/link -> ../other
hrw-r--r-- 0/0 0 2026-11-05 18:00 dir/hard link to dir/orig
";
        assert!(validate_listing(ListingKind::TarVerbose, inside, "/x/a.tar", None).is_ok());

        let symlink_escape = "\
lrwxrwxrwx 0/0 0 2026-11-05 18:00 dir/link -> ../../outside
";
        assert!(validate_listing(ListingKind::TarVerbose, symlink_escape, "/x/a.tar", None).is_err());

        // 硬链接目标按归档根解析，任何 .. 都逃出解压根
        let hard_escape = "\
hrw-r--r-- 0/0 0 2026-11-05 18:00 dir/hard link to ../outside
";
        assert!(validate_listing(ListingKind::TarVerbose, hard_escape, "/x/a.tar", None).is_err());
    }

    #[test]
    fn selected_scope_covers_selected_file_and_ancestors() {
        let listing = "\
drwxr-xr-x 0/0 0 2026-11-05 18:00 dir/
-rw-r--r-- 0/0 3 2026-11-05 18:00 dir/a.txt
-rw-r--r-- 0/0 3 2026-11-05 18:00 dir/b.txt
-rw-r--r-- 0/0 3 2026-11-05 18:00 ../evil
";
        let selected = vec!["dir/a.txt".to_string()];
        let paths = validate_listing(
            ListingKind::TarVerbose, listing, "/x/a.tar", Some(&selected),
        ).unwrap();
        // 只校验选中文件与其祖先目录；未选中的不安全条目不会被解压
        assert_eq!(paths, vec!["dir/", "dir/a.txt"]);
    }

    #[test]
    fn names_reject_absolute_and_parent() {
        assert!(validate_listing(ListingKind::Names, "/etc/passwd\n", "/x/a.zip", None).is_err());
        assert!(validate_listing(ListingKind::Names, "dir/../../x\n", "/x/a.zip", None).is_err());
        let paths = validate_listing(ListingKind::Names, "a.txt\ndir/b.txt\n", "/x/a.zip", None).unwrap();
        assert_eq!(paths, vec!["a.txt", "dir/b.txt"]);
    }

    #[test]
    fn sevenzip_slt_skips_header_and_checks_links() {
        let listing = "\
Path = /x/a.7z
Type = 7z
Physical Size = 10

Path = a.txt
Size = 3
Folder = -

Path = link
Symbolic Link = /etc/passwd
";
        assert!(validate_listing(ListingKind::SevenZipSlt, listing, "/x/a.7z", None).is_err());
    }

    #[test]
    fn sevenzip_slt_attributes_detect_zip_links_and_dirs() {
        let listing = "\
Path = /x/a.zip
Type = zip
Physical Size = 10

----------
Path = dir
Folder = +
Attributes = D_ drwx---r-x

Path = link
Folder = -
Attributes = _ lrwxrwxrwx

Path = file.txt
Folder = -
Attributes = _ -rw-r--r--
";
        let paths = validate_listing(ListingKind::SevenZipSlt, listing, "/x/a.zip", None).unwrap();
        // 符号链接不进权限清理清单；目录与普通文件都要清理
        assert_eq!(paths, vec!["dir", "file.txt"]);
    }

    #[test]
    fn sevenzip_slt_rejects_special_via_attributes() {
        let listing = "\
Path = /x/a.zip
Type = zip
Physical Size = 10

Path = fifo
Folder = -
Attributes = _ prw-rw-rw-
";
        assert!(validate_listing(ListingKind::SevenZipSlt, listing, "/x/a.zip", None).is_err());
    }

    #[test]
    fn sanitize_clears_setuid_and_keeps_skipped() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().to_str().unwrap();

        let fresh = dir.path().join("fresh");
        std::fs::write(&fresh, b"x").unwrap();
        std::fs::set_permissions(&fresh, std::fs::Permissions::from_mode(0o4755)).unwrap();

        let existing = dir.path().join("existing");
        std::fs::write(&existing, b"x").unwrap();
        std::fs::set_permissions(&existing, std::fs::Permissions::from_mode(0o4755)).unwrap();

        let mut skip = HashSet::new();
        skip.insert("existing".to_string());

        let names = vec!["fresh".to_string(), "existing".to_string()];
        assert_eq!(sanitize_extracted_permissions(target, &names, &skip), 1);

        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode();
        assert_eq!(mode(&fresh) & 0o6000, 0);
        // skip 策略下未被覆盖的旧文件保持原样
        assert_eq!(mode(&existing) & 0o6000, 0o4000);
    }
}
