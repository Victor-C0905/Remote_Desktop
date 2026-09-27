//! 协议适配层
//!
//! 在 Manager 内部转换客户端 JSON 协议(protocol::serde)与 Worker Protobuf 协议(protocol::generated)。
//!
//! ## 职责
//! - 将客户端 serde Payload 转换为 Worker manager_request::Payload(附用户上下文)
//! - 将 Worker WorkerResponse 转换为客户端 serde Payload
//!
//! ## 不在职责范围
//! - request_id 管理(由 IpcServer 负责)
//! - FD 传递(仅用于 CreateSession,不经过此适配层)
//! - 文件传输(FileTransfer 保持走 quic.rs 直接处理)

use base64::{Engine, engine::general_purpose::STANDARD as BASE64};

use crate::auth::UserSession;
use crate::protocol::generated::{
    manager_request, worker_response,
    ReadDir, ReadFile, WriteFile,
    Delete, Mkdir, Rename, Copy, Move,
    FileExists as FileExistsReq, ApplyDiff as ApplyDiffReq,
    FileDiff as ProtoFileDiff, FileInfo, ExecuteCommand, Chmod, Chown,
};
use crate::protocol::{Payload, FileEntry};

/// 用户上下文信息(从 UserSession 提取,用于填充 protobuf 消息)
///
/// 所有需要用户隔离的 Worker 请求都携带此信息,
/// Worker 据此构造 UserSession → UserExecutor(fork+setuid)执行操作
pub struct UserContext {
    pub uid: u32,
    pub gid: u32,
    pub username: String,
    pub home_dir: String,
}

impl From<&UserSession> for UserContext {
    fn from(session: &UserSession) -> Self {
        Self {
            uid: session.uid,
            gid: session.gid,
            username: session.username.clone(),
            home_dir: session.home_dir.to_string_lossy().to_string(),
        }
    }
}

/// 将客户端 serde 请求转换为 Worker protobuf 请求 payload
///
/// # 参数
/// - `payload`: 客户端发送的 serde Payload
/// - `user`: 用户上下文(uid/gid/username/home_dir)
///
/// # 返回
/// - `Some(manager_request::Payload)`: 需要路由到 Worker 的请求
/// - `None`: 不需要路由到 Worker(如 Ping、Subscribe 等),由调用方直接处理
pub fn serde_to_worker_request(payload: &Payload, user: &UserContext) -> Option<manager_request::Payload> {
    match payload {
        Payload::ReadDirRequest { path } => {
            tracing::debug!("适配 ReadDirRequest: path={}", path);
            Some(manager_request::Payload::ReadDir(ReadDir {
                path: path.clone(),
                uid: user.uid,
                gid: user.gid,
                username: user.username.clone(),
                home_dir: user.home_dir.clone(),
            }))
        }

        Payload::ReadFileRequest { path } => {
            tracing::debug!("适配 ReadFileRequest: path={}", path);
            Some(manager_request::Payload::ReadFile(ReadFile {
                path: path.clone(),
                uid: user.uid,
                gid: user.gid,
                username: user.username.clone(),
                home_dir: user.home_dir.clone(),
            }))
        }

        Payload::FileInfoRequest { path } => {
            tracing::debug!("适配 FileInfoRequest: path={}", path);
            Some(manager_request::Payload::FileInfo(FileInfo {
                path: path.clone(),
                uid: user.uid,
                gid: user.gid,
                username: user.username.clone(),
                home_dir: user.home_dir.clone(),
            }))
        }

        Payload::ExecuteCommandRequest { command, args, working_directory, timeout_secs } => {
            tracing::debug!("适配 ExecuteCommandRequest: command={}, args={:?}, timeout={}s", command, args, timeout_secs);
            Some(manager_request::Payload::ExecuteCommand(ExecuteCommand {
                command: command.clone(),
                args: args.clone(),
                working_directory: working_directory.clone().unwrap_or_default(),
                env: Default::default(),
                timeout_secs: *timeout_secs,
                uid: user.uid,
                gid: user.gid,
                username: user.username.clone(),
                home_dir: user.home_dir.clone(),
            }))
        }

        Payload::WriteFileRequest { path, content } => {
            tracing::debug!("适配 WriteFileRequest: path={}, content_len={}", path, content.len());
            // 将 base64 字符串解码为原始字节
            let content_bytes = BASE64.decode(content.as_bytes())
                .unwrap_or_else(|_| {
                    // 如果 base64 解码失败,尝试直接转换为字节(兼容旧客户端发送原始文本)
                    tracing::warn!("WriteFile base64 解码失败,使用原始字节: path={}", path);
                    content.as_bytes().to_vec()
                });
            Some(manager_request::Payload::WriteFile(WriteFile {
                path: path.clone(),
                content: content_bytes,
                uid: user.uid,
                gid: user.gid,
                username: user.username.clone(),
                home_dir: user.home_dir.clone(),
            }))
        }

        Payload::DeleteRequest { path } => {
            tracing::debug!("适配 DeleteRequest: path={}", path);
            Some(manager_request::Payload::Delete(Delete {
                path: path.clone(),
                uid: user.uid,
                gid: user.gid,
                username: user.username.clone(),
                home_dir: user.home_dir.clone(),
            }))
        }

        Payload::MkdirRequest { path } => {
            tracing::debug!("适配 MkdirRequest: path={}", path);
            Some(manager_request::Payload::Mkdir(Mkdir {
                path: path.clone(),
                uid: user.uid,
                gid: user.gid,
                username: user.username.clone(),
                home_dir: user.home_dir.clone(),
            }))
        }

        Payload::RenameRequest { old_path, new_path } => {
            tracing::debug!("适配 RenameRequest: {} -> {}", old_path, new_path);
            Some(manager_request::Payload::Rename(Rename {
                old_path: old_path.clone(),
                new_path: new_path.clone(),
                uid: user.uid,
                gid: user.gid,
                username: user.username.clone(),
                home_dir: user.home_dir.clone(),
            }))
        }

        Payload::CopyRequest { src, dst } => {
            tracing::debug!("适配 CopyRequest: {} -> {}", src, dst);
            Some(manager_request::Payload::Copy(Copy {
                src: src.clone(),
                dst: dst.clone(),
                uid: user.uid,
                gid: user.gid,
                username: user.username.clone(),
                home_dir: user.home_dir.clone(),
            }))
        }

        Payload::MoveRequest { src, dst } => {
            tracing::debug!("适配 MoveRequest: {} -> {}", src, dst);
            Some(manager_request::Payload::Move(Move {
                src: src.clone(),
                dst: dst.clone(),
                uid: user.uid,
                gid: user.gid,
                username: user.username.clone(),
                home_dir: user.home_dir.clone(),
            }))
        }

        Payload::ChmodRequest { path, mode, recursive } => {
            tracing::debug!("适配 ChmodRequest: path={}, mode={:o}, recursive={}", path, mode, recursive);
            Some(manager_request::Payload::Chmod(Chmod {
                path: path.clone(),
                mode: *mode,
                recursive: *recursive,
                uid: user.uid,
                gid: user.gid,
                username: user.username.clone(),
                home_dir: user.home_dir.clone(),
            }))
        }

        Payload::ChownRequest { path, owner, group, recursive } => {
            tracing::debug!("适配 ChownRequest: path={}, owner={}, group={}, recursive={}", path, owner, group, recursive);
            Some(manager_request::Payload::Chown(Chown {
                path: path.clone(),
                owner: owner.clone(),
                group: group.clone(),
                recursive: *recursive,
                uid: user.uid,
                gid: user.gid,
                username: user.username.clone(),
                home_dir: user.home_dir.clone(),
            }))
        }

        Payload::FileExistsRequest { path } => {
            tracing::debug!("适配 FileExistsRequest: path={}", path);
            Some(manager_request::Payload::FileExists(FileExistsReq {
                path: path.clone(),
                uid: user.uid,
                gid: user.gid,
                username: user.username.clone(),
                home_dir: user.home_dir.clone(),
            }))
        }

        Payload::ApplyDiffRequest { path, base_mtime, diffs } => {
            tracing::debug!("适配 ApplyDiffRequest: path={}, diffs={}", path, diffs.len());
            // 转换 serde FileDiff 为 protobuf FileDiff
            let proto_diffs: Vec<ProtoFileDiff> = diffs.iter().map(|d| ProtoFileDiff {
                diff_type: match d.diff_type {
                    crate::diff::DiffType::Insert => 0,
                    crate::diff::DiffType::Delete => 1,
                    crate::diff::DiffType::Replace => 2,
                },
                line_number: d.line_number as u32,
                old_content: d.old_content.clone().unwrap_or_default(),
                new_content: d.new_content.clone().unwrap_or_default(),
            }).collect();
            Some(manager_request::Payload::ApplyDiff(ApplyDiffReq {
                path: path.clone(),
                base_mtime: *base_mtime,
                diffs: proto_diffs,
                uid: user.uid,
                gid: user.gid,
                username: user.username.clone(),
                home_dir: user.home_dir.clone(),
            }))
        }

        // 其他 payload 不路由到 Worker
        _ => None,
    }
}

/// 将 Worker protobuf 响应转换为客户端 serde Payload
///
/// # 参数
/// - `response`: Worker 返回的 WorkerResponse
///
/// # 返回
/// - `Some(Payload)`: 成功转换的客户端响应
/// - `None`: 无法识别的响应类型(不应发生,记录警告日志)
pub fn worker_response_to_serde(response: &crate::protocol::generated::WorkerResponse) -> Option<Payload> {
    match &response.payload {
        Some(worker_response::Payload::DirListing(listing)) => {
            tracing::debug!("适配 DirListing: path={}, entries={}", listing.path, listing.entries.len());

            // 转换 FileEntry(protobuf → serde,字段类型一致,直接映射)
            let entries: Vec<FileEntry> = listing.entries.iter().map(|e| FileEntry {
                name: e.name.clone(),
                is_dir: e.is_dir,
                size: e.size,
                mtime: e.mtime.clone(),
                permissions: e.permissions.clone(),
                owner: e.owner.clone(),
                group: e.group.clone(),
            }).collect();

            Some(Payload::ReadDirResponse {
                path: listing.path.clone(),
                entries,
            })
        }

        Some(worker_response::Payload::FileContent(content)) => {
            tracing::debug!("适配 FileContent: path={}, size={}", content.path, content.content.len());

            // 将原始字节编码为 base64 字符串(支持二进制文件)
            let content_b64 = BASE64.encode(&content.content);

            Some(Payload::ReadFileResponse {
                path: content.path.clone(),
                content: content_b64,
                mtime: content.mtime,
                size: content.size,
            })
        }

        Some(worker_response::Payload::FileInfoResult(r)) => {
            tracing::debug!("适配 FileInfoResult: path={}, size={}, is_dir={}", r.path, r.size, r.is_dir);

            Some(Payload::FileInfoResponse {
                path: r.path.clone(),
                size: r.size,
                is_dir: r.is_dir,
                is_text: r.is_text,
                extension: r.extension.clone(),
                magic_bytes: r.magic_bytes.to_vec(),
            })
        }

        Some(worker_response::Payload::CommandOutput(output)) => {
            tracing::debug!("适配 CommandOutput: exit_code={}", output.exit_code);
            Some(Payload::CommandOutputResponse {
                stdout: BASE64.encode(&output.stdout),
                stderr: BASE64.encode(&output.stderr),
                exit_code: output.exit_code,
            })
        }

        Some(worker_response::Payload::WriteResult(result)) => {
            tracing::debug!("适配 WriteResult: path={}, size={}", result.path, result.size);

            Some(Payload::WriteFileResponse {
                path: result.path.clone(),
                mtime: result.mtime,
                size: result.size,
            })
        }

        Some(worker_response::Payload::Error(error)) => {
            tracing::warn!("Worker 返回错误: code={}, message={}", error.code, error.message);

            Some(Payload::Error {
                code: error.code as i32,
                message: error.message.clone(),
            })
        }

        Some(worker_response::Payload::DeleteResult(result)) => {
            tracing::debug!("适配 DeleteResult: success={}", result.success);
            Some(Payload::DeleteResponse { success: result.success })
        }

        Some(worker_response::Payload::MkdirResult(result)) => {
            tracing::debug!("适配 MkdirResult: path={}", result.path);
            Some(Payload::MkdirResponse {
                success: true,
                path: result.path.clone(),
            })
        }

        Some(worker_response::Payload::RenameResult(result)) => {
            tracing::debug!("适配 RenameResult: {} -> {}", result.old_path, result.new_path);
            Some(Payload::RenameResponse {
                success: true,
                old_path: result.old_path.clone(),
                new_path: result.new_path.clone(),
            })
        }

        Some(worker_response::Payload::CopyResult(result)) => {
            tracing::debug!("适配 CopyResult: {} -> {}", result.src, result.dst);
            Some(Payload::CopyResponse {
                success: true,
                src: result.src.clone(),
                dst: result.dst.clone(),
            })
        }

        Some(worker_response::Payload::MoveResult(result)) => {
            tracing::debug!("适配 MoveResult: {} -> {}", result.src, result.dst);
            Some(Payload::MoveResponse {
                success: true,
                src: result.src.clone(),
                dst: result.dst.clone(),
            })
        }

        Some(worker_response::Payload::ChmodResult(result)) => {
            tracing::debug!("适配 ChmodResult: success={}", result.success);
            Some(Payload::ChmodResponse { success: result.success })
        }

        Some(worker_response::Payload::ChownResult(result)) => {
            tracing::debug!("适配 ChownResult: success={}", result.success);
            Some(Payload::ChownResponse { success: result.success })
        }

        Some(worker_response::Payload::FileExistsResult(result)) => {
            tracing::debug!("适配 FileExistsResult: exists={}", result.exists);
            Some(Payload::FileExistsResponse {
                exists: result.exists,
                size: if result.exists { Some(result.size) } else { None },
                mtime: if result.exists { Some(result.mtime) } else { None },
            })
        }

        Some(worker_response::Payload::ApplyDiffResult(result)) => {
            tracing::debug!("适配 ApplyDiffResult: path={}, success={}", result.path, result.success);
            Some(Payload::ApplyDiffResponse {
                path: result.path.clone(),
                success: result.success,
                new_mtime: result.new_mtime,
                error: if result.error.is_empty() { None } else { Some(result.error.clone()) },
            })
        }

        // 其他响应类型不经过此适配层(SessionCreated 由 IpcServer 单独处理)
        other => {
            tracing::warn!("协议适配层收到未处理的响应类型: {:?}", other);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_user_context() -> UserContext {
        UserContext {
            uid: 1000,
            gid: 1000,
            username: "testuser".to_string(),
            home_dir: "/home/testuser".to_string(),
        }
    }

    #[test]
    fn test_read_dir_request_conversion() {
        let payload = Payload::ReadDirRequest { path: "/home/testuser".to_string() };
        let user = test_user_context();

        let result = serde_to_worker_request(&payload, &user);

        match result {
            Some(manager_request::Payload::ReadDir(req)) => {
                assert_eq!(req.path, "/home/testuser");
                assert_eq!(req.uid, 1000);
                assert_eq!(req.gid, 1000);
                assert_eq!(req.username, "testuser");
                assert_eq!(req.home_dir, "/home/testuser");
            }
            _ => panic!("Expected ReadDir request"),
        }
    }

    #[test]
    fn test_read_file_request_conversion() {
        let payload = Payload::ReadFileRequest { path: "/home/testuser/file.txt".to_string() };
        let user = test_user_context();

        let result = serde_to_worker_request(&payload, &user);

        match result {
            Some(manager_request::Payload::ReadFile(req)) => {
                assert_eq!(req.path, "/home/testuser/file.txt");
                assert_eq!(req.uid, 1000);
            }
            _ => panic!("Expected ReadFile request"),
        }
    }

    #[test]
    fn test_write_file_request_conversion() {
        let content = BASE64.encode(b"hello world");
        let payload = Payload::WriteFileRequest {
            path: "/home/testuser/file.txt".to_string(),
            content: content.clone(),
        };
        let user = test_user_context();

        let result = serde_to_worker_request(&payload, &user);

        match result {
            Some(manager_request::Payload::WriteFile(req)) => {
                assert_eq!(req.path, "/home/testuser/file.txt");
                assert_eq!(req.content, b"hello world");
                assert_eq!(req.uid, 1000);
            }
            _ => panic!("Expected WriteFile request"),
        }
    }

    #[test]
    fn test_non_routed_payload_returns_none() {
        let payload = Payload::Ping { timestamp: 12345 };
        let user = test_user_context();

        let result = serde_to_worker_request(&payload, &user);
        assert!(result.is_none());
    }

    #[test]
    fn test_dir_listing_response_conversion() {
        use crate::protocol::generated::{WorkerResponse, DirListing, FileEntry as ProtoFileEntry};

        let response = WorkerResponse {
            request_id: 1,
            payload: Some(worker_response::Payload::DirListing(DirListing {
                path: "/home/testuser".to_string(),
                entries: vec![ProtoFileEntry {
                    name: "file.txt".to_string(),
                    is_dir: false,
                    size: 100,
                    mtime: "2026-08-05T10:00:00Z".to_string(),
                    permissions: "rw-r--r--".to_string(),
                    owner: "testuser".to_string(),
                    group: "testuser".to_string(),
                }],
            })),
        };

        let result = worker_response_to_serde(&response);

        match result {
            Some(Payload::ReadDirResponse { path, entries }) => {
                assert_eq!(path, "/home/testuser");
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].name, "file.txt");
                assert_eq!(entries[0].size, 100);
                assert_eq!(entries[0].owner, "testuser");
                assert_eq!(entries[0].group, "testuser");
            }
            _ => panic!("Expected ReadDirResponse"),
        }
    }

    #[test]
    fn test_file_content_response_conversion() {
        use crate::protocol::generated::{WorkerResponse, FileContent};

        let response = WorkerResponse {
            request_id: 1,
            payload: Some(worker_response::Payload::FileContent(FileContent {
                path: "/home/testuser/file.txt".to_string(),
                content: b"hello world".to_vec(),
                mtime: 1722844800,
                size: 11,
            })),
        };

        let result = worker_response_to_serde(&response);

        match result {
            Some(Payload::ReadFileResponse { path, content, mtime, size }) => {
                assert_eq!(path, "/home/testuser/file.txt");
                assert_eq!(mtime, 1722844800);
                assert_eq!(size, 11);
                // 验证 base64 解码后是原始内容
                let decoded = BASE64.decode(content.as_bytes()).unwrap();
                assert_eq!(decoded, b"hello world");
            }
            _ => panic!("Expected ReadFileResponse"),
        }
    }

    #[test]
    fn test_error_response_conversion() {
        use crate::protocol::generated::{WorkerResponse, Error};

        let response = WorkerResponse {
            request_id: 1,
            payload: Some(worker_response::Payload::Error(Error {
                code: 403,
                message: "权限不足".to_string(),
            })),
        };

        let result = worker_response_to_serde(&response);

        match result {
            Some(Payload::Error { code, message }) => {
                assert_eq!(code, 403);
                assert_eq!(message, "权限不足");
            }
            _ => panic!("Expected Error payload"),
        }
    }

    #[test]
    fn test_delete_request_conversion() {
        let payload = Payload::DeleteRequest { path: "/home/testuser/trash.txt".to_string() };
        let user = test_user_context();

        let result = serde_to_worker_request(&payload, &user);

        match result {
            Some(manager_request::Payload::Delete(req)) => {
                assert_eq!(req.path, "/home/testuser/trash.txt");
                assert_eq!(req.uid, 1000);
                assert_eq!(req.gid, 1000);
                assert_eq!(req.username, "testuser");
                assert_eq!(req.home_dir, "/home/testuser");
            }
            _ => panic!("Expected Delete request"),
        }
    }

    #[test]
    fn test_delete_result_conversion() {
        use crate::protocol::generated::{WorkerResponse, DeleteResult};

        let response = WorkerResponse {
            request_id: 1,
            payload: Some(worker_response::Payload::DeleteResult(DeleteResult {
                success: true,
            })),
        };

        let result = worker_response_to_serde(&response);

        match result {
            Some(Payload::DeleteResponse { success }) => {
                assert!(success);
            }
            _ => panic!("Expected DeleteResponse"),
        }
    }

    #[test]
    fn test_apply_diff_request_conversion() {
        use crate::diff::{FileDiff, DiffType};

        let diffs = vec![
            FileDiff {
                diff_type: DiffType::Insert,
                line_number: 2,
                old_content: None,
                new_content: Some("inserted\n".to_string()),
            },
            FileDiff {
                diff_type: DiffType::Replace,
                line_number: 1,
                old_content: Some("old\n".to_string()),
                new_content: Some("new\n".to_string()),
            },
        ];
        let payload = Payload::ApplyDiffRequest {
            path: "/home/testuser/file.txt".to_string(),
            base_mtime: 1722844800,
            diffs,
        };
        let user = test_user_context();

        let result = serde_to_worker_request(&payload, &user);

        match result {
            Some(manager_request::Payload::ApplyDiff(req)) => {
                assert_eq!(req.path, "/home/testuser/file.txt");
                assert_eq!(req.base_mtime, 1722844800);
                assert_eq!(req.uid, 1000);
                assert_eq!(req.diffs.len(), 2);
                // 验证 Insert 差异转换
                assert_eq!(req.diffs[0].diff_type, 0);
                assert_eq!(req.diffs[0].line_number, 2);
                assert_eq!(req.diffs[0].new_content, "inserted\n");
                assert_eq!(req.diffs[0].old_content, "");
                // 验证 Replace 差异转换
                assert_eq!(req.diffs[1].diff_type, 2);
                assert_eq!(req.diffs[1].line_number, 1);
                assert_eq!(req.diffs[1].old_content, "old\n");
                assert_eq!(req.diffs[1].new_content, "new\n");
            }
            _ => panic!("Expected ApplyDiff request"),
        }
    }

    #[test]
    fn test_file_exists_result_conversion() {
        use crate::protocol::generated::{WorkerResponse, FileExistsResult};

        // 文件存在的情况
        let response = WorkerResponse {
            request_id: 1,
            payload: Some(worker_response::Payload::FileExistsResult(FileExistsResult {
                exists: true,
                size: 1024,
                mtime: 1722844800,
            })),
        };

        let result = worker_response_to_serde(&response);

        match result {
            Some(Payload::FileExistsResponse { exists, size, mtime }) => {
                assert!(exists);
                assert_eq!(size, Some(1024));
                assert_eq!(mtime, Some(1722844800));
            }
            _ => panic!("Expected FileExistsResponse"),
        }

        // 文件不存在的情况
        let response_missing = WorkerResponse {
            request_id: 2,
            payload: Some(worker_response::Payload::FileExistsResult(FileExistsResult {
                exists: false,
                size: 0,
                mtime: 0,
            })),
        };

        let result_missing = worker_response_to_serde(&response_missing);

        match result_missing {
            Some(Payload::FileExistsResponse { exists, size, mtime }) => {
                assert!(!exists);
                assert_eq!(size, None);
                assert_eq!(mtime, None);
            }
            _ => panic!("Expected FileExistsResponse"),
        }
    }

    #[test]
    fn test_file_info_request_conversion() {
        let payload = Payload::FileInfoRequest { path: "/a.png".to_string() };
        let user = test_user_context();

        let result = serde_to_worker_request(&payload, &user);

        match result {
            Some(manager_request::Payload::FileInfo(req)) => {
                assert_eq!(req.path, "/a.png");
                assert_eq!(req.uid, user.uid);
                assert_eq!(req.gid, user.gid);
                assert_eq!(req.username, user.username);
                assert_eq!(req.home_dir, user.home_dir);
            }
            _ => panic!("Expected FileInfo conversion"),
        }
    }

    #[test]
    fn test_file_info_result_response_conversion() {
        use crate::protocol::generated::{FileInfoResult, WorkerResponse};

        let resp = WorkerResponse {
            request_id: 1,
            payload: Some(worker_response::Payload::FileInfoResult(FileInfoResult {
                path: "/a.png".to_string(),
                size: 1024,
                is_dir: false,
                is_text: false,
                extension: "png".to_string(),
                magic_bytes: vec![0x89, 0x50, 0x4E, 0x47],
            })),
        };

        let result = worker_response_to_serde(&resp);

        match result {
            Some(Payload::FileInfoResponse { path, size, is_dir, is_text, extension, magic_bytes }) => {
                assert_eq!(path, "/a.png");
                assert_eq!(size, 1024);
                assert!(!is_dir);
                assert!(!is_text);
                assert_eq!(extension, "png");
                assert_eq!(magic_bytes, vec![0x89, 0x50, 0x4E, 0x47]);
            }
            _ => panic!("Expected FileInfoResponse"),
        }
    }

    #[test]
    fn test_execute_command_request_conversion() {
        let payload = Payload::ExecuteCommandRequest {
            command: "unzip".to_string(),
            args: vec!["-o".to_string(), "/tmp/a.zip".to_string()],
            working_directory: Some("/tmp".to_string()),
            timeout_secs: 600,
        };
        let user = test_user_context();

        let result = serde_to_worker_request(&payload, &user);

        match result {
            Some(manager_request::Payload::ExecuteCommand(req)) => {
                assert_eq!(req.command, "unzip");
                assert_eq!(req.args, vec!["-o".to_string(), "/tmp/a.zip".to_string()]);
                assert_eq!(req.working_directory, "/tmp");
                assert_eq!(req.timeout_secs, 600);
                assert_eq!(req.uid, user.uid);
                assert_eq!(req.gid, user.gid);
                assert_eq!(req.username, user.username);
                assert_eq!(req.home_dir, user.home_dir);
            }
            _ => panic!("Expected ExecuteCommand conversion"),
        }
    }

    #[test]
    fn test_command_output_response_conversion() {
        use crate::protocol::generated::{CommandOutput, WorkerResponse};

        let resp = WorkerResponse {
            request_id: 1,
            payload: Some(worker_response::Payload::CommandOutput(CommandOutput {
                stdout: b"hi".to_vec(),
                stderr: Vec::new(),
                exit_code: 0,
            })),
        };

        let result = worker_response_to_serde(&resp);

        match result {
            Some(Payload::CommandOutputResponse { stdout, stderr, exit_code }) => {
                assert_eq!(stdout, "aGk="); // base64("hi")
                assert_eq!(stderr, "");
                assert_eq!(exit_code, 0);
            }
            _ => panic!("Expected CommandOutputResponse"),
        }
    }

    #[test]
    fn test_chmod_request_conversion() {
        let payload = Payload::ChmodRequest {
            path: "/home/testuser/dir".to_string(),
            mode: 0o755, // 493
            recursive: true,
        };
        let user = test_user_context();

        let result = serde_to_worker_request(&payload, &user);

        match result {
            Some(manager_request::Payload::Chmod(req)) => {
                assert_eq!(req.path, "/home/testuser/dir");
                assert_eq!(req.mode, 493); // 0o755
                assert!(req.recursive);
                assert_eq!(req.uid, 1000);
                assert_eq!(req.gid, 1000);
                assert_eq!(req.username, "testuser");
                assert_eq!(req.home_dir, "/home/testuser");
            }
            _ => panic!("Expected Chmod request"),
        }
    }

    #[test]
    fn test_chmod_result_conversion() {
        use crate::protocol::generated::{WorkerResponse, ChmodResult};

        let response = WorkerResponse {
            request_id: 1,
            payload: Some(worker_response::Payload::ChmodResult(ChmodResult {
                success: true,
            })),
        };

        let result = worker_response_to_serde(&response);

        match result {
            Some(Payload::ChmodResponse { success }) => {
                assert!(success);
            }
            _ => panic!("Expected ChmodResponse"),
        }
    }

    #[test]
    fn test_chown_request_conversion() {
        let payload = Payload::ChownRequest {
            path: "/home/testuser/www".to_string(),
            owner: "www-data".to_string(),
            group: "www-data".to_string(),
            recursive: false,
        };
        let user = test_user_context();

        let result = serde_to_worker_request(&payload, &user);

        match result {
            Some(manager_request::Payload::Chown(req)) => {
                assert_eq!(req.path, "/home/testuser/www");
                assert_eq!(req.owner, "www-data");
                assert_eq!(req.group, "www-data");
                assert!(!req.recursive);
                assert_eq!(req.uid, 1000);
                assert_eq!(req.gid, 1000);
                assert_eq!(req.username, "testuser");
                assert_eq!(req.home_dir, "/home/testuser");
            }
            _ => panic!("Expected Chown request"),
        }
    }

    #[test]
    fn test_chown_result_conversion() {
        use crate::protocol::generated::{WorkerResponse, ChownResult};

        let response = WorkerResponse {
            request_id: 1,
            payload: Some(worker_response::Payload::ChownResult(ChownResult {
                success: true,
            })),
        };

        let result = worker_response_to_serde(&response);

        match result {
            Some(Payload::ChownResponse { success }) => {
                assert!(success);
            }
            _ => panic!("Expected ChownResponse"),
        }
    }
}
