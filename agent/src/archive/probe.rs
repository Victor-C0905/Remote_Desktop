//! 压缩格式能力探测（仅 Unix Agent 实际执行）
//!
//! 设计要点：
//! - **真实样本**：内嵌极小压缩包，执行「列表 + 解压」并检查产物，而不是只看命令存在；
//! - **同一身份**：通过 `UserExecutor` 以登录用户身份执行，与真实解压条件一致；
//! - **不解析本地化输出**：只依据 exit code 与产物存在性，避免中文 locale 误判；
//! - **缓存 + 指纹**：工具路径/大小/mtime 变化自动失效，默认 10 分钟，支持强制刷新。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use lazy_static::lazy_static;
use quirel_protocol::{ArchiveCapabilities, ArchiveFormatCapability, ArchiveHostOs, ArchiveTool};
use tokio::sync::Mutex;

use crate::auth::{UserExecutor, UserSession};

/// 单条探测命令超时
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
/// 整轮探测总预算（防止异常工具拖住请求）
const PROBE_BUDGET: Duration = Duration::from_secs(12);
/// 能力缓存有效期
const CACHE_TTL: Duration = Duration::from_secs(600);
/// detail 截断长度（技术细节只进日志/折叠区）
const DETAIL_LIMIT: usize = 240;

const ZIP_FIXTURE: &[u8] = include_bytes!("../../assets/archive-probes/sample.zip");
const TAR_FIXTURE: &[u8] = include_bytes!("../../assets/archive-probes/sample.tar");
const TAR_GZ_FIXTURE: &[u8] = include_bytes!("../../assets/archive-probes/sample.tar.gz");
const TAR_BZ2_FIXTURE: &[u8] = include_bytes!("../../assets/archive-probes/sample.tar.bz2");
const TAR_XZ_FIXTURE: &[u8] = include_bytes!("../../assets/archive-probes/sample.tar.xz");
const TAR_ZST_FIXTURE: &[u8] = include_bytes!("../../assets/archive-probes/sample.tar.zst");

/// 探测产物文件名（样本包内固定包含该文件）
const PROBE_ENTRY: &str = "probe.txt";

lazy_static! {
    static ref CACHE: Mutex<Option<CachedCapabilities>> = Mutex::new(None);
}

struct CachedCapabilities {
    /// 探测身份（能力可能受用户权限/安全策略影响，按 uid 隔离缓存）
    uid: u32,
    fingerprint: u64,
    capabilities: ArchiveCapabilities,
    at: Instant,
}

/// 查询压缩能力（refresh=true 强制重新探测）
pub async fn capabilities(session: &UserSession, refresh: bool) -> ArchiveCapabilities {
    #[cfg(not(unix))]
    {
        let _ = (session, refresh);
        return unavailable_capabilities("platform_unsupported", None);
    }

    #[cfg(unix)]
    {
        let fingerprint = tool_fingerprint();
        let uid = session.uid;

        if !refresh {
            let cache = CACHE.lock().await;
            if let Some(cached) = cache.as_ref() {
                if cached.uid == uid
                    && cached.fingerprint == fingerprint
                    && cached.at.elapsed() < CACHE_TTL
                {
                    return cached.capabilities.clone();
                }
            }
        }

        // UserExecutor 的 fork + wait 是同步阻塞，放 blocking 线程池
        let session = session.clone();
        let capabilities = match tokio::task::spawn_blocking(move || probe_as_user(&session)).await {
            Ok(caps) => caps,
            Err(e) => unavailable_capabilities("probe_failed", Some(format!("probe thread failed: {e}"))),
        };

        let mut cache = CACHE.lock().await;
        *cache = Some(CachedCapabilities {
            uid,
            fingerprint,
            capabilities: capabilities.clone(),
            at: Instant::now(),
        });
        capabilities
    }
}

#[cfg(unix)]
fn probe_as_user(session: &UserSession) -> ArchiveCapabilities {
    let executor = UserExecutor::new(session);
    match executor.execute_as_user(|| Ok(probe_all())) {
        Ok(caps) => caps,
        Err(e) => unavailable_capabilities("probe_failed", Some(format!("execute as user failed: {e}"))),
    }
}

/// 以当前（已降权）身份执行全部探测
#[cfg(unix)]
fn probe_all() -> ArchiveCapabilities {
    let os = read_os_info();
    let tools = resolve_tools();
    let tool_list = collect_tool_list(&tools);

    // tempdir 由 RAII 清理，避免 /tmp 下可预测目录被替换
    let formats = match tempfile::Builder::new()
        .prefix("quireld-probe-")
        .tempdir()
    {
        Ok(guard) => probe_formats(&tools, guard.path()),
        Err(e) => unavailable_all(
            "probe_failed",
            Some(format!("无法创建探测临时目录: {e}")),
        ),
    };

    ArchiveCapabilities {
        probed_at: now_ms(),
        os,
        tools: tool_list,
        formats,
    }
}

/// 解析后的系统工具集合
#[cfg(unix)]
struct Tool {
    name: String,
    path: PathBuf,
    version: String,
}

#[cfg(unix)]
#[derive(Default)]
struct ToolSet {
    unzip: Option<Tool>,
    tar: Option<Tool>,
    seven_zip: Option<Tool>,
    unrar: Option<Tool>,
}

#[cfg(unix)]
fn resolve_tools() -> ToolSet {
    ToolSet {
        unzip: resolve_tool("unzip", &["-v"]),
        tar: resolve_tool("tar", &["--version"]),
        seven_zip: resolve_tool("7z", &[]).or_else(|| resolve_tool("7zz", &[])),
        unrar: resolve_tool("unrar", &[]),
    }
}

#[cfg(unix)]
fn resolve_tool(name: &str, version_args: &[&str]) -> Option<Tool> {
    let path = which::which(name).ok()?;
    let version = if version_args.is_empty() {
        String::new()
    } else {
        let args: Vec<String> = version_args.iter().map(|s| s.to_string()).collect();
        let outcome = run_tool(name, &args, None, Instant::now() + PROBE_TIMEOUT);
        outcome.stdout.lines().next().unwrap_or_default().trim().to_string()
    };
    Some(Tool { name: name.to_string(), path, version })
}

#[cfg(unix)]
fn collect_tool_list(tools: &ToolSet) -> Vec<ArchiveTool> {
    let mut out = Vec::new();
    for tool in [&tools.unzip, &tools.tar, &tools.seven_zip, &tools.unrar].into_iter().flatten() {
        out.push(ArchiveTool {
            name: tool.name.clone(),
            path: tool.path.to_string_lossy().to_string(),
            version: tool.version.clone(),
        });
    }
    out
}

/// 按格式执行样本探测
#[cfg(unix)]
fn probe_formats(tools: &ToolSet, temp: &Path) -> Vec<ArchiveFormatCapability> {
    let deadline = Instant::now() + PROBE_BUDGET;

    let fixtures = [
        ("sample.zip", ZIP_FIXTURE),
        ("sample.tar", TAR_FIXTURE),
        ("sample.tar.gz", TAR_GZ_FIXTURE),
        ("sample.tar.bz2", TAR_BZ2_FIXTURE),
        ("sample.tar.xz", TAR_XZ_FIXTURE),
        ("sample.tar.zst", TAR_ZST_FIXTURE),
    ];
    for (name, bytes) in fixtures {
        let _ = std::fs::write(temp.join(name), bytes);
    }
    let _ = std::fs::write(temp.join(PROBE_ENTRY), b"quirel probe\n");

    vec![
        probe_zip(tools, temp, deadline),
        probe_tar(tools, temp, "tar", "sample.tar", &["-tf"], &["-xf"], deadline),
        probe_tar(tools, temp, "tar.gz", "sample.tar.gz", &["-tzf"], &["-xzf"], deadline),
        probe_tar(tools, temp, "tar.bz2", "sample.tar.bz2", &["-tjf"], &["-xjf"], deadline),
        probe_tar(tools, temp, "tar.xz", "sample.tar.xz", &["-tJf"], &["-xJf"], deadline),
        probe_tar(
            tools, temp, "tar.zst", "sample.tar.zst", &["--zstd", "-tf"],
            &["--zstd", "-xf"], deadline,
        ),
        probe_7z(tools, temp, deadline),
        probe_rar(tools),
    ]
}

#[cfg(unix)]
fn probe_zip(tools: &ToolSet, temp: &Path, deadline: Instant) -> ArchiveFormatCapability {
    let archive = temp.join("sample.zip");

    if let Some(unzip) = &tools.unzip {
        let out = temp.join("out_zip_unzip");
        let _ = std::fs::create_dir_all(&out);
        let list = run_tool(
            &unzip.name,
            &["-l".into(), archive.to_string_lossy().to_string()],
            None,
            deadline,
        );
        if list.success {
            let extract = run_tool(
                &unzip.name,
                &[
                    "-o".into(),
                    archive.to_string_lossy().to_string(),
                    "-d".into(),
                    out.to_string_lossy().to_string(),
                ],
                None,
                deadline,
            );
            let extracted = out.join(PROBE_ENTRY).is_file();
            if extract.success && extracted {
                return format_cap("zip", true, true, Some(&unzip.name), None, None, None);
            }
            return format_cap(
                "zip", true, false, Some(&unzip.name),
                Some("probe_failed"), None, detail_of(&extract),
            );
        }
    }

    if let Some(seven) = &tools.seven_zip {
        let out = temp.join("out_zip_7z");
        let _ = std::fs::create_dir_all(&out);
        let list = run_tool(
            &seven.name,
            &["l".into(), "-slt".into(), archive.to_string_lossy().to_string()],
            None,
            deadline,
        );
        if list.success {
            let extract = run_tool(
                &seven.name,
                &[
                    "x".into(),
                    "-y".into(),
                    format!("-o{}", out.to_string_lossy()),
                    archive.to_string_lossy().to_string(),
                ],
                None,
                deadline,
            );
            if extract.success && out.join(PROBE_ENTRY).is_file() {
                return format_cap("zip", true, true, Some(&seven.name), None, None, None);
            }
            return format_cap(
                "zip", true, false, Some(&seven.name),
                Some("probe_failed"), None, detail_of(&extract),
            );
        }
        return format_cap(
            "zip", false, false, Some(&seven.name),
            Some("probe_failed"), None, detail_of(&list),
        );
    }

    format_cap(
        "zip", false, false, None,
        Some("tool_missing"), Some("unzip"), None,
    )
}

/// tar 系格式样本探测
///
/// `extract_flags` 只含操作/压缩开关（如 `-xzf`），不含档案名与安全选项：
/// `-f` 必须紧跟档案名，拼装顺序为 flags → archive → --no-same-* → -C。
#[cfg(unix)]
fn probe_tar(
    tools: &ToolSet,
    temp: &Path,
    id: &str,
    fixture: &str,
    list_flags: &[&str],
    extract_flags: &[&str],
    deadline: Instant,
) -> ArchiveFormatCapability {
    let Some(tar) = &tools.tar else {
        return format_cap(id, false, false, None, Some("tool_missing"), Some("tar"), None);
    };

    let archive = temp.join(fixture);
    let out = temp.join(format!("out_{}", id.replace('.', "_")));
    // tar -C 要求目标目录已存在（真实解压流程会先 mkdir，探测必须对齐）
    let _ = std::fs::create_dir_all(&out);

    let mut list_args: Vec<String> = list_flags.iter().map(|s| s.to_string()).collect();
    list_args.push(archive.to_string_lossy().to_string());
    let list = run_tool(&tar.name, &list_args, None, deadline);

    if !list.success {
        let reason = if looks_like_missing_decompressor(&list.stderr) {
            "decompressor_missing"
        } else {
            "probe_failed"
        };
        return format_cap(
            id, false, false, Some(&tar.name),
            Some(reason), missing_decompressor_of(&list.stderr), detail_of(&list),
        );
    }

    // `-f` 取紧邻 argv 作为档案名：操作/压缩开关在前 → archive → 安全选项。
    // 若安全选项夹在 `-xzf` 与 archive 之间，tar 会把选项误当压缩包名。
    let mut extract_args: Vec<String> = extract_flags.iter().map(|s| s.to_string()).collect();
    extract_args.push(archive.to_string_lossy().to_string());
    extract_args.push("--no-same-owner".into());
    extract_args.push("--no-same-permissions".into());
    extract_args.push("-C".into());
    extract_args.push(out.to_string_lossy().to_string());
    let extract = run_tool(&tar.name, &extract_args, None, deadline);

    if extract.success && out.join(PROBE_ENTRY).is_file() {
        // 单条目提取使用 --no-wildcards/-- 字面匹配（busybox tar 不支持），
        // 与全部解压分开探测，避免"能全部解压但单条目必失败"被误报为可用
        let out_one = temp.join(format!("out_one_{}", id.replace('.', "_")));
        let _ = std::fs::create_dir_all(&out_one);
        let mut one_args: Vec<String> = extract_flags.iter().map(|s| s.to_string()).collect();
        one_args.push(archive.to_string_lossy().to_string());
        one_args.push("--no-same-owner".into());
        one_args.push("--no-same-permissions".into());
        one_args.push("-C".into());
        one_args.push(out_one.to_string_lossy().to_string());
        one_args.push("--no-wildcards".into());
        one_args.push("--".into());
        one_args.push(PROBE_ENTRY.into());
        let one = run_tool(&tar.name, &one_args, None, deadline);
        let one_ok = one.success && out_one.join(PROBE_ENTRY).is_file();

        return format_cap_modes(
            id, true, one_ok, true, Some(&tar.name),
            if one_ok { None } else { Some("probe_failed") },
            None,
            if one_ok { None } else { detail_of(&one) },
        );
    }

    let reason = if looks_like_missing_decompressor(&extract.stderr) {
        "decompressor_missing"
    } else {
        "probe_failed"
    };
    format_cap_modes(
        id, true, false, false, Some(&tar.name),
        Some(reason), missing_decompressor_of(&extract.stderr), detail_of(&extract),
    )
}

#[cfg(unix)]
fn probe_7z(tools: &ToolSet, temp: &Path, deadline: Instant) -> ArchiveFormatCapability {
    let Some(seven) = &tools.seven_zip else {
        return format_cap("7z", false, false, None, Some("tool_missing"), Some("7z"), None);
    };

    // 无预置 7z 样本：用 7z 自己创建再读取，验证二进制与 7z 格式能力
    let archive = temp.join("sample.7z");
    let create = run_tool(
        &seven.name,
        &[
            "a".into(),
            "-t7z".into(),
            "-y".into(),
            archive.to_string_lossy().to_string(),
            PROBE_ENTRY.into(),
        ],
        Some(temp),
        deadline,
    );
    if !create.success {
        return format_cap(
            "7z", false, false, Some(&seven.name),
            Some("probe_failed"), None, detail_of(&create),
        );
    }

    let list = run_tool(
        &seven.name,
        &["l".into(), "-slt".into(), archive.to_string_lossy().to_string()],
        None,
        deadline,
    );
    if !list.success {
        return format_cap(
            "7z", false, false, Some(&seven.name),
            Some("probe_failed"), None, detail_of(&list),
        );
    }

    let out = temp.join("out_7z");
    let _ = std::fs::create_dir_all(&out);
    let extract = run_tool(
        &seven.name,
        &[
            "x".into(),
            "-y".into(),
            format!("-o{}", out.to_string_lossy()),
            archive.to_string_lossy().to_string(),
        ],
        None,
        deadline,
    );
    if extract.success && out.join(PROBE_ENTRY).is_file() {
        format_cap("7z", true, true, Some(&seven.name), None, None, None)
    } else {
        format_cap(
            "7z", true, false, Some(&seven.name),
            Some("probe_failed"), None, detail_of(&extract),
        )
    }
}

#[cfg(unix)]
fn probe_rar(tools: &ToolSet) -> ArchiveFormatCapability {
    let Some(unrar) = &tools.unrar else {
        return format_cap("rar", false, false, None, Some("tool_missing"), Some("unrar"), None);
    };
    // RAR 创建工具为非自由软件，仓库不内置 RAR 样本；unrar 为 RAR 专用工具，
    // 检测到可执行文件即认为具备 RAR 解压能力（具体 RAR5 支持以运行时结果为准）。
    format_cap(
        "rar", true, true, Some(&unrar.name), None, None,
        Some("仅检测到 unrar，未做 RAR 样本验证".to_string()),
    )
}

#[cfg(unix)]
fn format_cap(
    id: &str,
    can_list: bool,
    can_extract: bool,
    tool: Option<&str>,
    reason_code: Option<&str>,
    missing_tool: Option<&str>,
    detail: Option<String>,
) -> ArchiveFormatCapability {
    format_cap_modes(
        id, can_list, can_extract, can_extract, tool, reason_code, missing_tool, detail,
    )
}

#[cfg(unix)]
fn format_cap_modes(
    id: &str,
    can_list: bool,
    can_extract_one: bool,
    can_extract_all: bool,
    tool: Option<&str>,
    reason_code: Option<&str>,
    missing_tool: Option<&str>,
    detail: Option<String>,
) -> ArchiveFormatCapability {
    ArchiveFormatCapability {
        id: id.to_string(),
        can_list,
        can_extract_one,
        can_extract_all,
        tool: tool.map(|s| s.to_string()),
        reason_code: reason_code.map(|s| s.to_string()),
        missing_tool: missing_tool.map(|s| s.to_string()),
        detail,
    }
}

#[cfg(unix)]
fn unavailable_all(reason_code: &str, detail: Option<String>) -> Vec<ArchiveFormatCapability> {
    ["zip", "tar", "tar.gz", "tar.bz2", "tar.xz", "tar.zst", "7z", "rar"]
        .iter()
        .map(|id| format_cap(id, false, false, None, Some(reason_code), None, detail.clone()))
        .collect()
}

fn unavailable_capabilities(reason_code: &str, detail: Option<String>) -> ArchiveCapabilities {
    ArchiveCapabilities {
        probed_at: now_ms(),
        os: ArchiveHostOs::default(),
        tools: Vec::new(),
        formats: unavailable_all_for_platform(reason_code, detail),
    }
}

fn unavailable_all_for_platform(reason_code: &str, detail: Option<String>) -> Vec<ArchiveFormatCapability> {
    ["zip", "tar", "tar.gz", "tar.bz2", "tar.xz", "tar.zst", "7z", "rar"]
        .iter()
        .map(|id| ArchiveFormatCapability {
            id: id.to_string(),
            can_list: false,
            can_extract_one: false,
            can_extract_all: false,
            tool: None,
            reason_code: Some(reason_code.to_string()),
            missing_tool: None,
            detail: detail.clone(),
        })
        .collect()
}

/// 工具路径/大小/mtime 指纹：变化即缓存失效
#[cfg(unix)]
fn tool_fingerprint() -> u64 {
    let mut hasher = DefaultHasher::new();
    std::env::var_os("PATH")
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
        .hash(&mut hasher);
    for name in ["unzip", "tar", "7z", "7zz", "unrar"] {
        name.hash(&mut hasher);
        match which::which(name) {
            Ok(path) => {
                path.hash(&mut hasher);
                if let Ok(meta) = std::fs::metadata(&path) {
                    meta.len().hash(&mut hasher);
                    if let Ok(modified) = meta.modified() {
                        modified.hash(&mut hasher);
                    }
                }
            }
            Err(_) => {
                // 未找到也参与指纹：安装后路径出现即变化
                0u8.hash(&mut hasher);
            }
        }
    }
    hasher.finish()
}

#[cfg(unix)]
struct CmdOutcome {
    success: bool,
    timed_out: bool,
    stdout: String,
    stderr: String,
}

#[cfg(unix)]
fn run_tool(program: &str, args: &[String], cwd: Option<&Path>, deadline: Instant) -> CmdOutcome {
    if Instant::now() >= deadline {
        return CmdOutcome {
            success: false,
            timed_out: true,
            stdout: String::new(),
            stderr: "probe budget exceeded".to_string(),
        };
    }

    let timeout = PROBE_TIMEOUT.min(deadline.saturating_duration_since(Instant::now()));
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            return CmdOutcome {
                success: false,
                timed_out: false,
                stdout: String::new(),
                stderr: e.to_string(),
            }
        }
    };

    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = read_pipe(child.stdout.take());
                let stderr = read_pipe(child.stderr.take());
                return CmdOutcome {
                    success: status.success(),
                    timed_out: false,
                    stdout,
                    stderr,
                };
            }
            Ok(None) => {
                if started.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return CmdOutcome {
                        success: false,
                        timed_out: true,
                        stdout: String::new(),
                        stderr: "probe timeout".to_string(),
                    };
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => {
                return CmdOutcome {
                    success: false,
                    timed_out: false,
                    stdout: String::new(),
                    stderr: e.to_string(),
                };
            }
        }
    }
}

#[cfg(unix)]
fn read_pipe<R: Read>(pipe: Option<R>) -> String {
    let mut buf = Vec::new();
    if let Some(mut pipe) = pipe {
        let _ = pipe.read_to_end(&mut buf);
    }
    String::from_utf8_lossy(&buf).to_string()
}

#[cfg(unix)]
fn detail_of(outcome: &CmdOutcome) -> Option<String> {
    if outcome.timed_out {
        return Some("探测超时".to_string());
    }
    let stderr = outcome.stderr.trim();
    let text = if stderr.is_empty() { outcome.stdout.trim() } else { stderr };
    if text.is_empty() {
        None
    } else {
        Some(text.chars().take(DETAIL_LIMIT).collect())
    }
}

#[cfg(unix)]
fn looks_like_missing_decompressor(stderr: &str) -> bool {
    let lower = stderr.to_lowercase();
    ["gzip", "bzip2", "xz", "zstd", "lz4"].iter().any(|name| lower.contains(name))
        && (lower.contains("not found")
            || lower.contains("no such file")
            || lower.contains("cannot")
            || lower.contains("error"))
}

#[cfg(unix)]
fn missing_decompressor_of(stderr: &str) -> Option<&'static str> {
    let lower = stderr.to_lowercase();
    for name in ["gzip", "bzip2", "xz", "zstd", "lz4"] {
        if lower.contains(name) {
            return Some(match name {
                "gzip" => "gzip",
                "bzip2" => "bzip2",
                "xz" => "xz",
                "zstd" => "zstd",
                _ => "lz4",
            });
        }
    }
    None
}

/// 读取 /etc/os-release 并识别包管理器
#[cfg(unix)]
fn read_os_info() -> ArchiveHostOs {
    std::fs::read_to_string("/etc/os-release")
        .map(|content| parse_os_release(&content))
        .unwrap_or_default()
}

fn parse_os_release(content: &str) -> ArchiveHostOs {
    let mut id = String::new();
    let mut version = String::new();
    let mut id_like = String::new();

    for line in content.lines() {
        let Some((key, value)) = line.split_once('=') else { continue };
        let value = value.trim().trim_matches('"').trim_matches('\'').to_string();
        match key.trim() {
            "ID" => id = value,
            "VERSION_ID" => version = value,
            "ID_LIKE" => id_like = value,
            _ => {}
        }
    }

    let family = format!("{} {}", id, id_like).to_lowercase();
    let package_manager = if family.contains("debian") || family.contains("ubuntu") {
        "apt"
    } else if family.contains("fedora") || family.contains("rhel") || family.contains("centos") {
        "dnf"
    } else if family.contains("alpine") {
        "apk"
    } else if family.contains("arch") {
        "pacman"
    } else if family.contains("suse") {
        "zypper"
    } else if family.contains("nixos") {
        "nix"
    } else {
        "unknown"
    };

    ArchiveHostOs {
        id,
        version,
        package_manager: package_manager.to_string(),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_os_release_maps_distributions() {
        let os = parse_os_release(
            "ID=ubuntu\nVERSION_ID=\"24.04\"\nID_LIKE=debian\n",
        );
        assert_eq!(os.id, "ubuntu");
        assert_eq!(os.version, "24.04");
        assert_eq!(os.package_manager, "apt");

        let os = parse_os_release("ID=alpine\nVERSION_ID=3.20\n");
        assert_eq!(os.package_manager, "apk");

        let os = parse_os_release("ID=centos\nID_LIKE=\"rhel fedora\"\n");
        assert_eq!(os.package_manager, "dnf");
    }

    #[test]
    fn parse_os_release_unknown() {
        let os = parse_os_release("ID=linux\n");
        assert_eq!(os.package_manager, "unknown");
    }
}

#[cfg(all(test, unix))]
mod probe_execution_tests {
    use super::*;

    /// 真实执行一轮探测（Linux 测试环境）：验证样本写入、命令构造与结果结构。
    /// 不假定具体工具存在（最小容器可能只有 tar 或什么都没有）。
    #[test]
    fn probe_all_returns_full_format_matrix() {
        let caps = probe_all();
        assert_eq!(caps.formats.len(), 8);
        for cap in &caps.formats {
            assert!(!cap.id.is_empty());
            if cap.can_list {
                assert!(cap.tool.is_some(), "可用格式必须记录实际工具: {}", cap.id);
            }
        }
        // 探测结果自洽：可解压必须可列表
        for cap in &caps.formats {
            if cap.can_extract_all {
                assert!(cap.can_list, "{} 可解压但不可列表", cap.id);
            }
        }
    }

    /// 回归：`-f` 与档案名必须相邻。历史上安全选项夹在中间，GNU tar 把
    /// `--no-same-owner` 当作压缩包名，导致「可列表但不可解压」的误报。
    #[test]
    fn gnu_tar_reports_extract_capability() {
        let caps = probe_all();
        let has_gnu_tar = caps
            .tools
            .iter()
            .any(|tool| tool.name == "tar" && tool.version.contains("GNU tar"));
        if !has_gnu_tar {
            return;
        }
        let tar_cap = caps
            .formats
            .iter()
            .find(|cap| cap.id == "tar")
            .expect("探测矩阵应包含 tar");
        assert!(
            tar_cap.can_extract_all,
            "GNU tar 可列出样本时应能解压: {:?}",
            tar_cap.detail
        );
    }
}
