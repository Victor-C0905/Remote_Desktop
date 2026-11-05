//! 压缩包命令构造（Agent 端唯一入口）
//!
//! 客户端只发送语义化请求（路径、格式、条目、覆盖策略），argv 由 Agent 用
//! 固定模板拼装，避免客户端注入 tar 危险选项（--checkpoint-action=exec 等）。
//! 所有路径/条目在此做服务端校验，不依赖前端检查。

use anyhow::{bail, Context, Result};
use quirel_protocol::archive::ArchiveExtractPolicy;

/// 单次列表超时（秒）
const LIST_TIMEOUT_SECS: u32 = 60;
/// 单次解压超时（秒）
const EXTRACT_TIMEOUT_SECS: u32 = 600;
/// 条目数量上限（与客户端配额一致，服务端再兜底）
const MAX_ENTRIES: usize = 1000;

/// 解压前校验列表的输出形态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListingKind {
    /// tar -tvf：含类型位与链接目标
    TarVerbose,
    /// 7z l -slt：key=value（含 Symbolic Link/Folder）
    SevenZipSlt,
    /// 仅文件名（unzip -Z1 / unrar lb）
    Names,
}

/// 解压前校验用的列表命令（固定 argv）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationList {
    pub command: BuiltArchiveCommand,
    pub kind: ListingKind,
}

/// 构造好的待执行命令（交给 Worker 以登录用户身份执行）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltArchiveCommand {
    pub command: String,
    pub args: Vec<String>,
    pub working_directory: String,
    pub timeout_secs: u32,
}

fn has_tool(name: &str) -> bool {
    which::which(name).is_ok()
}

/// 按格式选择实际工具（与能力探测同一策略：zip 可回退 7z）
fn resolve_tool(format_id: &str) -> Result<String> {
    let tool = match format_id {
        "zip" => {
            if has_tool("unzip") {
                "unzip".to_string()
            } else if has_tool("7z") {
                "7z".to_string()
            } else if has_tool("7zz") {
                "7zz".to_string()
            } else {
                bail!("服务器缺少 unzip/7z，无法处理 zip");
            }
        }
        "tar" | "tar.gz" | "tar.bz2" | "tar.xz" | "tar.zst" => {
            if !has_tool("tar") {
                bail!("服务器缺少 tar，无法处理 tar 格式");
            }
            "tar".to_string()
        }
        "7z" => {
            if has_tool("7z") {
                "7z".to_string()
            } else if has_tool("7zz") {
                "7zz".to_string()
            } else {
                bail!("服务器缺少 7z/7zz");
            }
        }
        "rar" => {
            if !has_tool("unrar") {
                bail!("服务器缺少 unrar，无法处理 rar");
            }
            "unrar".to_string()
        }
        other => bail!("未知压缩格式: {other}"),
    };
    Ok(tool)
}

fn check_absolute(path: &str, label: &str) -> Result<()> {
    if path.is_empty() {
        bail!("{label}不能为空");
    }
    if path.contains('\0') {
        bail!("{label}包含非法字符");
    }
    if !path.starts_with('/') {
        bail!("{label}必须是绝对路径: {path}");
    }
    if path.split('/').any(|part| part == "..") {
        bail!("{label}不能包含 ..: {path}");
    }
    Ok(())
}

fn check_entry(entry: &str, allow_glob: bool) -> Result<()> {
    if entry.is_empty() {
        bail!("条目路径不能为空");
    }
    if entry.contains('\0') || entry.starts_with('/') {
        bail!("条目路径非法: {entry}");
    }
    if entry.split('/').any(|part| part == "..") {
        bail!("条目路径含 ..: {entry}");
    }
    let base = entry.rsplit('/').next().unwrap_or(entry);
    if base.starts_with('-') {
        bail!("条目名不能以 - 开头: {entry}");
    }
    if !allow_glob && entry.contains(['*', '?', '[']) {
        bail!("该格式不支持含通配符的条目: {entry}");
    }
    Ok(())
}

fn normalise_entries<'a>(
    format_id: &str,
    entries: Option<&'a [String]>,
) -> Result<Option<Vec<String>>> {
    let Some(list) = entries.filter(|list| !list.is_empty()) else {
        return Ok(None);
    };
    if list.len() > MAX_ENTRIES {
        bail!("条目数量超过上限 {MAX_ENTRIES}");
    }
    let allow_glob = format_id.starts_with("tar");
    for entry in list {
        check_entry(entry, allow_glob)?;
    }
    Ok(Some(list.to_vec()))
}

/// tar 列表参数必须是 verbose（-tv*）：客户端解析器与解压前校验都依赖类型位/大小列
fn tar_list_flags(format_id: &str) -> Result<Vec<&'static str>> {
    match format_id {
        "tar" => Ok(vec!["-tvf"]),
        "tar.gz" => Ok(vec!["-tvzf"]),
        "tar.bz2" => Ok(vec!["-tvjf"]),
        "tar.xz" => Ok(vec!["-tvJf"]),
        "tar.zst" => Ok(vec!["--zstd", "-tvf"]),
        _ => bail!("不是 tar 格式: {format_id}"),
    }
}

fn tar_extract_flags(format_id: &str) -> Result<Vec<&'static str>> {
    match format_id {
        "tar" => Ok(vec!["-xf"]),
        "tar.gz" => Ok(vec!["-xzf"]),
        "tar.bz2" => Ok(vec!["-xjf"]),
        "tar.xz" => Ok(vec!["-xJf"]),
        "tar.zst" => Ok(vec!["--zstd", "-xf"]),
        _ => bail!("不是 tar 格式: {format_id}"),
    }
}

fn build_list_args(format_id: &str, tool: &str, archive: &str) -> Result<Vec<String>> {
    if format_id.starts_with("tar") {
        let mut args: Vec<String> = tar_list_flags(format_id)?.into_iter().map(str::to_string).collect();
        args.push(archive.to_string());
        return Ok(args);
    }
    let args = match format_id {
        "zip" if tool == "unzip" => vec!["-l".into(), archive.into()],
        "zip" | "7z" => vec!["l".into(), "-slt".into(), archive.into()],
        "rar" => vec!["l".into(), archive.into()],
        other => bail!("未知压缩格式: {other}"),
    };
    Ok(args)
}

fn build_extract_args(
    format_id: &str,
    tool: &str,
    archive: &str,
    target: &str,
    entries: Option<&[String]>,
    policy: ArchiveExtractPolicy,
) -> Result<Vec<String>> {
    let skip = matches!(policy, ArchiveExtractPolicy::Skip);
    let selected = normalise_entries(format_id, entries)?;

    if format_id.starts_with("tar") {
        // `-f` 必须紧跟档案名；安全选项放在档案名之后
        let mut args: Vec<String> = tar_extract_flags(format_id)?.into_iter().map(str::to_string).collect();
        args.push(archive.to_string());
        args.push("--no-same-owner".into());
        args.push("--no-same-permissions".into());
        if skip {
            args.push("--skip-old-files".into());
        }
        args.push("-C".into());
        args.push(target.to_string());
        if let Some(list) = selected {
            // --no-wildcards：条目名按字面匹配，避免 * ? [ 误提取
            args.push("--no-wildcards".into());
            args.push("--".into());
            args.extend(list);
        }
        return Ok(args);
    }

    match format_id {
        "zip" if tool == "unzip" => {
            let mut args = vec![if skip { "-n" } else { "-o" }.to_string(), archive.to_string()];
            if let Some(list) = selected { args.extend(list); }
            args.push("-d".into());
            args.push(target.into());
            Ok(args)
        }
        "zip" | "7z" => {
            let mut args = vec![
                "x".to_string(),
                archive.to_string(),
                format!("-o{target}"),
            ];
            if let Some(list) = selected { args.extend(list); }
            args.push("-y".into());
            if skip { args.push("-aos".into()); }
            Ok(args)
        }
        "rar" => {
            let mut args = vec![
                "x".to_string(),
                if skip { "-o-" } else { "-o+" }.to_string(),
                archive.to_string(),
            ];
            if let Some(list) = selected { args.extend(list); }
            args.push(format!("{}/", target.trim_end_matches('/')));
            Ok(args)
        }
        other => bail!("未知压缩格式: {other}"),
    }
}

/// 校验压缩包存在且为普通文件（以 Manager 身份做存在性检查；实际读取仍以用户身份）
fn check_archive(archive: &str) -> Result<()> {
    let meta = std::fs::metadata(archive)
        .with_context(|| format!("压缩包不存在或不可读: {archive}"))?;
    if !meta.is_file() {
        bail!("压缩包不是普通文件: {archive}");
    }
    Ok(())
}

/// 目标目录必须已存在（创建由客户端以用户身份完成，服务端不生成 root 目录）
fn check_target_dir(target: &str) -> Result<()> {
    check_absolute(target, "目标目录")?;
    if target == "/" {
        bail!("目标目录不能是根目录");
    }
    let meta = std::fs::metadata(target)
        .with_context(|| format!("目标目录不存在: {target}"))?;
    if !meta.is_dir() {
        bail!("目标不是目录: {target}");
    }
    Ok(())
}

pub fn build_list(archive_path: &str, format_id: &str) -> Result<BuiltArchiveCommand> {
    check_absolute(archive_path, "压缩包路径")?;
    check_archive(archive_path)?;
    let tool = resolve_tool(format_id)?;
    let args = build_list_args(format_id, &tool, archive_path)?;
    Ok(BuiltArchiveCommand {
        command: tool,
        args,
        working_directory: "/".into(),
        timeout_secs: LIST_TIMEOUT_SECS,
    })
}

/// 解压前条目校验命令：tar 用 verbose、7z 用 -slt、zip/rar 退化为文件名列表
pub fn build_validation_list(
    archive_path: &str,
    format_id: &str,
) -> Result<ValidationList> {
    check_absolute(archive_path, "压缩包路径")?;
    check_archive(archive_path)?;
    let tool = resolve_tool(format_id)?;
    let (args, kind) = if format_id.starts_with("tar") {
        let mut args: Vec<String> = tar_list_flags(format_id)?
            .into_iter()
            .map(str::to_string)
            .collect();
        args.push(archive_path.to_string());
        (args, ListingKind::TarVerbose)
    } else {
        match format_id {
            "zip" if tool == "unzip" => (
                vec!["-Z".into(), "-1".into(), archive_path.into()],
                ListingKind::Names,
            ),
            "zip" | "7z" => (
                vec!["l".into(), "-slt".into(), archive_path.into()],
                ListingKind::SevenZipSlt,
            ),
            "rar" => (
                vec!["lb".into(), archive_path.into()],
                ListingKind::Names,
            ),
            other => bail!("未知压缩格式: {other}"),
        }
    };
    Ok(ValidationList {
        command: BuiltArchiveCommand {
            command: tool,
            args,
            working_directory: "/".into(),
            timeout_secs: LIST_TIMEOUT_SECS,
        },
        kind,
    })
}

pub fn build_extract(
    archive_path: &str,
    target_dir: &str,
    format_id: &str,
    entries: Option<&[String]>,
    policy: ArchiveExtractPolicy,
) -> Result<BuiltArchiveCommand> {
    check_absolute(archive_path, "压缩包路径")?;
    check_target_dir(target_dir)?;
    check_archive(archive_path)?;
    let tool = resolve_tool(format_id)?;
    let args = build_extract_args(format_id, &tool, archive_path, target_dir, entries, policy)?;
    Ok(BuiltArchiveCommand {
        command: tool,
        args,
        working_directory: "/".into(),
        timeout_secs: EXTRACT_TIMEOUT_SECS,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tar_extract_args_order_and_policy() {
        let entries = vec!["dir/a.txt".to_string()];
        let args = build_extract_args(
            "tar.gz", "tar", "/x/a.tar.gz", "/out", Some(&entries),
            ArchiveExtractPolicy::Overwrite,
        ).unwrap();
        assert_eq!(args[0], "-xzf");
        assert_eq!(args[1], "/x/a.tar.gz");
        assert!(args.contains(&"--no-same-owner".to_string()));
        assert!(args.contains(&"--no-wildcards".to_string()));
        assert!(!args.iter().any(|a| a == "--skip-old-files"));

        let skip = build_extract_args(
            "tar.gz", "tar", "/x/a.tar.gz", "/out", None,
            ArchiveExtractPolicy::Skip,
        ).unwrap();
        assert!(skip.iter().any(|a| a == "--skip-old-files"));
    }

    #[test]
    fn zip_policy_and_entries() {
        let args = build_extract_args(
            "zip", "unzip", "/x/a.zip", "/out", None,
            ArchiveExtractPolicy::Skip,
        ).unwrap();
        assert_eq!(args, vec!["-n", "/x/a.zip", "-d", "/out"]);
    }

    #[test]
    fn rejects_unsafe_entries() {
        for bad in ["../x", "/abs", "-rf", "a/*"] {
            let entries = vec![bad.to_string()];
            assert!(
                build_extract_args("zip", "unzip", "/x/a.zip", "/out", Some(&entries),
                    ArchiveExtractPolicy::Overwrite).is_err(),
                "应拒绝条目: {bad}"
            );
        }
    }

    #[test]
    fn target_must_be_absolute_and_not_root() {
        assert!(check_target_dir("/").is_err());
        assert!(check_target_dir("relative").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn tar_validation_list_reads_real_sample() {
        use crate::archive::validate::validate_listing;
        use std::process::Command;

        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("sample.tar");
        std::fs::write(&archive, include_bytes!("../../assets/archive-probes/sample.tar")).unwrap();
        let archive = archive.to_string_lossy().to_string();

        let list = build_validation_list(&archive, "tar").unwrap();
        let out = Command::new(&list.command.command)
            .args(&list.command.args)
            .output()
            .expect("tar 应可执行");
        assert!(out.status.success(), "tar 列表失败: {}", String::from_utf8_lossy(&out.stderr));

        let stdout = String::from_utf8_lossy(&out.stdout);
        let names = validate_listing(list.kind, &stdout, &archive, None).unwrap();
        assert_eq!(names, vec!["probe.txt"]);
    }

    #[cfg(unix)]
    #[test]
    fn zip_validation_list_reads_real_sample() {
        use crate::archive::validate::validate_listing;
        use std::process::Command;

        // 环境缺少 unzip/7z 时跳过（Agent 实际会在能力探测阶段声明不支持 zip）
        if which::which("unzip").is_err()
            && which::which("7z").is_err()
            && which::which("7zz").is_err()
        {
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("sample.zip");
        std::fs::write(&archive, include_bytes!("../../assets/archive-probes/sample.zip")).unwrap();
        let archive = archive.to_string_lossy().to_string();

        let list = build_validation_list(&archive, "zip").unwrap();
        let out = Command::new(&list.command.command)
            .args(&list.command.args)
            .output()
            .expect("zip 列表工具应可执行");
        assert!(out.status.success(), "zip 列表失败: {}", String::from_utf8_lossy(&out.stderr));

        let stdout = String::from_utf8_lossy(&out.stdout);
        let names = validate_listing(list.kind, &stdout, &archive, None).unwrap();
        assert_eq!(names, vec!["probe.txt"]);
    }

    #[cfg(unix)]
    #[test]
    fn sevenzip_slt_reads_real_sample() {
        use crate::archive::validate::validate_listing;
        use std::process::Command;

        if which::which("7z").is_err() {
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("sample.zip");
        std::fs::write(&archive, include_bytes!("../../assets/archive-probes/sample.zip")).unwrap();
        let archive = archive.to_string_lossy().to_string();

        let out = Command::new("7z")
            .args(["l", "-slt", &archive])
            .output()
            .expect("7z 应可执行");
        assert!(out.status.success(), "7z 列表失败: {}", String::from_utf8_lossy(&out.stderr));

        let stdout = String::from_utf8_lossy(&out.stdout);
        let names = validate_listing(ListingKind::SevenZipSlt, &stdout, &archive, None).unwrap();
        assert_eq!(names, vec!["probe.txt"]);
    }
}
