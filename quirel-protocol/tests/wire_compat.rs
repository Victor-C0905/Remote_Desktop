//! 线上格式兼容性 golden 测试
//!
//! 这些断言的字节序列是当前线上真实格式（取自 agent 端 serde.rs 的序列化行为）。
//! 任何导致这些断言失败的改动都是线上破坏性变更，禁止合入。

use quirel_protocol::{ConnectionStatsSnapshot, Envelope, Payload, StatsResponse};

fn roundtrip(env: &Envelope) -> Envelope {
    let bytes = env.encode().expect("编码失败");
    Envelope::decode(&bytes).expect("解码失败")
}

#[test]
fn ping_wire_format() {
    let env = Envelope::new(42, Payload::Ping { timestamp: 1700000000 });
    let json = serde_json::to_string(&env).unwrap();
    assert_eq!(
        json,
        r#"{"request_id":42,"payload":{"type":"ping","data":{"timestamp":1700000000}}}"#
    );
}

#[test]
fn auth_password_request_wire_format() {
    let env = Envelope::new(
        7,
        Payload::AuthPasswordRequest {
            username: "root".to_string(),
            password: "pw".to_string(),
        },
    );
    let json = serde_json::to_string(&env).unwrap();
    assert_eq!(
        json,
        r#"{"request_id":7,"payload":{"type":"auth_password_request","data":{"username":"root","password":"pw"}}}"#
    );
}

#[test]
fn file_transfer_request_defaults_preserved() {
    // 旧客户端不带 frame_mode/stream_count 字段 → 反序列化回退 default
    // 这是已部署的兼容性行为，golden 必须锁死
    let raw = r#"{"request_id":1,"payload":{"type":"file_transfer","data":{"direction":"upload","path":"/tmp/f"}}}"#;
    let env: Envelope = serde_json::from_str(raw).expect("旧格式必须可解码");
    match env.payload {
        Payload::FileTransferRequest { frame_mode, stream_count, .. } => {
            assert_eq!(frame_mode, "json");
            assert_eq!(stream_count, None);
        }
        _ => panic!("变体不匹配"),
    }
}

#[test]
fn current_user_resp_without_home_dir_compat() {
    // home_dir 为后加字段：旧版 Agent 响应不含此字段，必须可解码且回退为空串
    // （前端据此按 Linux 惯例推导初始目录：root→/root，普通用户→/home/用户名）
    let raw = r#"{"request_id":1,"payload":{"type":"current_user_resp","data":{"username":"root"}}}"#;
    let env: Envelope = serde_json::from_str(raw).expect("旧格式必须可解码");
    match env.payload {
        Payload::CurrentUserResponse { username, home_dir } => {
            assert_eq!(username, "root");
            assert_eq!(home_dir, "");
        }
        _ => panic!("变体不匹配"),
    }
}

#[test]
fn read_file_resp_wire_format() {
    let env = Envelope::new(
        3,
        Payload::ReadFileResponse {
            path: "/a.txt".to_string(),
            content: "aGk=".to_string(),
            mtime: 100,
            size: 2,
        },
    );
    let json = serde_json::to_string(&env).unwrap();
    assert_eq!(
        json,
        r#"{"request_id":3,"payload":{"type":"read_file_resp","data":{"path":"/a.txt","content":"aGk=","mtime":100,"size":2}}}"#
    );
}

#[test]
fn envelope_roundtrip_all_directions() {
    let env = Envelope::new(9, Payload::FileChunk {
        session_id: "s1".to_string(),
        seq: 1,
        data: vec![0xde, 0xad],
        size: 2,
    });
    let back = roundtrip(&env);
    assert_eq!(back.request_id, 9);
    match back.payload {
        Payload::FileChunk { session_id, seq, data, size } => {
            assert_eq!((session_id.as_str(), seq, data.as_slice(), size),
                       ("s1", 1, [0xde, 0xad].as_slice(), 2));
        }
        _ => panic!("变体不匹配"),
    }
}

#[test]
fn file_transfer_accept_stream_count_default() {
    // 旧版 file_transfer_accept 消息不带 stream_count 字段
    // → 解码回退默认值 1（单流），而非 `#[serde(default)]` 解码出的 0
    let raw = r#"{"request_id":2,"payload":{"type":"file_transfer_accept","data":{"session_id":"s1","file_size":100,"chunk_size":65536}}}"#;
    let env: Envelope = serde_json::from_str(raw).expect("旧格式必须可解码");
    match env.payload {
        Payload::FileTransferAccept { stream_count, .. } => {
            assert_eq!(stream_count, 1);
        }
        _ => panic!("变体不匹配"),
    }
}

#[test]
fn stats_response_newtype_wire_format() {
    // Payload::StatsResponse 为 newtype 变体（复用 crate::stats::StatsResponse），
    // tag/content 下与原内联 struct 变体序列化字节完全相同，此测试锁死该字节形状
    // （auth/performance 是无 skip 属性的 Option，序列化输出 null）
    let env = Envelope::new(
        5,
        Payload::StatsResponse(StatsResponse {
            auth: None,
            connection: ConnectionStatsSnapshot {
                active_connections: 1,
                total_connections: 2,
                normal_disconnects: 3,
                timeout_disconnects: 4,
                error_disconnects: 5,
            },
            performance: None,
        }),
    );
    let json = serde_json::to_string(&env).unwrap();
    assert_eq!(
        json,
        r#"{"request_id":5,"payload":{"type":"stats_response","data":{"auth":null,"connection":{"active_connections":1,"total_connections":2,"normal_disconnects":3,"timeout_disconnects":4,"error_disconnects":5},"performance":null}}}"#
    );
}

#[test]
fn proxy_open_wire_format() {
    let env = Envelope::new(9, Payload::ProxyOpen { host: "example.com".to_string(), port: 443 });
    let json = serde_json::to_string(&env).unwrap();
    assert_eq!(
        json,
        r#"{"request_id":9,"payload":{"type":"proxy_open","data":{"host":"example.com","port":443}}}"#
    );
    // 线上往返：编码后必须可解码（代理流首帧的实际路径）
    let env2 = roundtrip(&env);
    match env2.payload {
        Payload::ProxyOpen { host, port } => {
            assert_eq!(host, "example.com");
            assert_eq!(port, 443);
        }
        _ => panic!("变体不匹配"),
    }
}

#[test]
fn proxy_open_response_wire_format() {
    let env = Envelope::new(9, Payload::ProxyOpenResponse { success: true, error: None });
    let json = serde_json::to_string(&env).unwrap();
    assert_eq!(
        json,
        r#"{"request_id":9,"payload":{"type":"proxy_open_resp","data":{"success":true}}}"#
    );
    // 失败场景：golden 锁定字节形状（含 error 键）+ 往返
    let fail = Envelope::new(9, Payload::ProxyOpenResponse {
        success: false,
        error: Some("connection refused".to_string()),
    });
    let fail_json = serde_json::to_string(&fail).unwrap();
    assert_eq!(
        fail_json,
        r#"{"request_id":9,"payload":{"type":"proxy_open_resp","data":{"success":false,"error":"connection refused"}}}"#
    );
    let fail2 = roundtrip(&fail);
    match fail2.payload {
        Payload::ProxyOpenResponse { success, error } => {
            assert!(!success);
            assert_eq!(error.as_deref(), Some("connection refused"));
        }
        _ => panic!("变体不匹配"),
    }

    // 缺 error 字段的裸 JSON 解码（default 回退为 None）
    let raw = r#"{"request_id":9,"payload":{"type":"proxy_open_resp","data":{"success":false}}}"#;
    let env3: Envelope = serde_json::from_str(raw).expect("缺 error 字段必须可解码");
    match env3.payload {
        Payload::ProxyOpenResponse { success, error } => {
            assert!(!success);
            assert_eq!(error, None);
        }
        _ => panic!("变体不匹配"),
    }
}

#[test]
fn file_info_request_wire_format() {
    let env = Envelope::new(
        11,
        Payload::FileInfoRequest { path: "/a.png".to_string() },
    );
    let json = serde_json::to_string(&env).unwrap();
    assert_eq!(
        json,
        r#"{"request_id":11,"payload":{"type":"file_info","data":{"path":"/a.png"}}}"#
    );
}

#[test]
fn file_info_resp_wire_format() {
    // magic_bytes 为 Vec<u8>：serde_json 序列化为字节数值数组
    let env = Envelope::new(
        12,
        Payload::FileInfoResponse {
            path: "/a.png".to_string(),
            size: 1024,
            is_dir: false,
            is_text: false,
            extension: "png".to_string(),
            magic_bytes: vec![0x89, 0x50, 0x4E, 0x47],
        },
    );
    let json = serde_json::to_string(&env).unwrap();
    assert_eq!(
        json,
        r#"{"request_id":12,"payload":{"type":"file_info_resp","data":{"path":"/a.png","size":1024,"is_dir":false,"is_text":false,"extension":"png","magic_bytes":[137,80,78,71]}}}"#
    );
    // 线上往返：编码后必须可解码
    let env2 = roundtrip(&env);
    match env2.payload {
        Payload::FileInfoResponse { path, size, is_dir, is_text, extension, magic_bytes } => {
            assert_eq!(path, "/a.png");
            assert_eq!(size, 1024);
            assert!(!is_dir);
            assert!(!is_text);
            assert_eq!(extension, "png");
            assert_eq!(magic_bytes, vec![0x89, 0x50, 0x4E, 0x47]);
        }
        _ => panic!("变体不匹配"),
    }
}

#[test]
fn execute_command_wire_format() {
    let env = Envelope::new(
        21,
        Payload::ExecuteCommandRequest {
            command: "unzip".to_string(),
            args: vec!["-o".to_string(), "/tmp/a.zip".to_string(), "-d".to_string(), "/tmp/out".to_string()],
            working_directory: Some("/tmp".to_string()),
            timeout_secs: 600,
        },
    );
    let json = serde_json::to_string(&env).unwrap();
    assert_eq!(
        json,
        r#"{"request_id":21,"payload":{"type":"execute_command","data":{"command":"unzip","args":["-o","/tmp/a.zip","-d","/tmp/out"],"working_directory":"/tmp","timeout_secs":600}}}"#
    );
    // 线上往返：编码后必须可解码
    let env2 = roundtrip(&env);
    match env2.payload {
        Payload::ExecuteCommandRequest { command, args, working_directory, timeout_secs } => {
            assert_eq!(command, "unzip");
            assert_eq!(args, vec!["-o", "/tmp/a.zip", "-d", "/tmp/out"]);
            assert_eq!(working_directory.as_deref(), Some("/tmp"));
            assert_eq!(timeout_secs, 600);
        }
        _ => panic!("变体不匹配"),
    }
}

#[test]
fn execute_command_defaults_compat() {
    // working_directory/timeout_secs 带 #[serde(default)]：缺失字段可解码
    let raw = r#"{"request_id":22,"payload":{"type":"execute_command","data":{"command":"tar","args":["-xf","/a.tar"]}}}"#;
    let env: Envelope = serde_json::from_str(raw).expect("缺省字段必须可解码");
    match env.payload {
        Payload::ExecuteCommandRequest { working_directory, timeout_secs, .. } => {
            assert_eq!(working_directory, None);
            assert_eq!(timeout_secs, 0);
        }
        _ => panic!("变体不匹配"),
    }
}

#[test]
fn command_output_resp_wire_format() {
    let env = Envelope::new(
        23,
        Payload::CommandOutputResponse {
            stdout: "aGk=".to_string(),
            stderr: String::new(),
            exit_code: 0,
        },
    );
    let json = serde_json::to_string(&env).unwrap();
    assert_eq!(
        json,
        r#"{"request_id":23,"payload":{"type":"command_output_resp","data":{"stdout":"aGk=","stderr":"","exit_code":0}}}"#
    );
    // 线上往返：编码后必须可解码
    let env2 = roundtrip(&env);
    match env2.payload {
        Payload::CommandOutputResponse { stdout, stderr, exit_code } => {
            assert_eq!(stdout, "aGk=");
            assert_eq!(stderr, "");
            assert_eq!(exit_code, 0);
        }
        _ => panic!("变体不匹配"),
    }
}

// ── 登录通知回应体系：AuthResponse.code 字段（2026-09-15 设计） ──

use quirel_protocol::AuthErrorCode;

#[test]
fn auth_response_with_code_wire_format() {
    // 新版 Agent 发送带结构化错误码的认证失败响应
    let env = Envelope::new(
        7,
        Payload::AuthResponse {
            success: false,
            error: Some("用户名或密码错误".to_string()),
            session_id: None,
            code: Some(AuthErrorCode::InvalidCredentials),
            capabilities: None,
        },
    );
    let json = serde_json::to_string(&env).unwrap();
    assert_eq!(
        json,
        r#"{"request_id":7,"payload":{"type":"auth_response","data":{"success":false,"error":"用户名或密码错误","session_id":null,"code":"InvalidCredentials"}}}"#
    );
}

#[test]
fn auth_response_without_code_compat() {
    // 旧版 Agent 的响应不含 code 字段 → 新客户端必须可解码且回退 None
    let raw = r#"{"request_id":7,"payload":{"type":"auth_response","data":{"success":false,"error":"账户暂时锁定，请15分钟后再试","session_id":null}}}"#;
    let env: Envelope = serde_json::from_str(raw).expect("旧格式必须可解码");
    match env.payload {
        Payload::AuthResponse { success, error, session_id: _, code, capabilities: _ } => {
            assert!(!success);
            assert_eq!(error.as_deref(), Some("账户暂时锁定，请15分钟后再试"));
            assert_eq!(code, None);
        }
        _ => panic!("变体不匹配"),
    }
}

#[test]
fn auth_error_code_numeric_roundtrip() {
    // AuthErrorCode 数值一旦发布不可变更：as_i32/from_i32 必须稳定往返
    for code in [
        AuthErrorCode::MissingCredentials,
        AuthErrorCode::InvalidKeyFormat,
        AuthErrorCode::KeyParseFailed,
        AuthErrorCode::CertificateRejected,
        AuthErrorCode::DnsFailed,
        AuthErrorCode::ConnectTimeout,
        AuthErrorCode::TlsHandshakeFailed,
        AuthErrorCode::NetworkUnreachable,
        AuthErrorCode::StreamTimeout,
        AuthErrorCode::ConnectionLost,
        AuthErrorCode::RateLimited,
        AuthErrorCode::AccountLocked,
        AuthErrorCode::InvalidCredentials,
        AuthErrorCode::PubkeyNotAuthorized,
        AuthErrorCode::SignatureVerificationFailed,
        AuthErrorCode::ChallengeExpired,
        AuthErrorCode::AuthServiceUnavailable,
        AuthErrorCode::ProtocolError,
        AuthErrorCode::SessionExpired,
        AuthErrorCode::Unknown,
    ] {
        assert_eq!(AuthErrorCode::from_i32(code.as_i32()), Some(code), "code={:?}", code);
    }
    // 既有 HTTP 风格码（≥400）不属于 AuthErrorCode 空间
    for legacy in [400, 401, 403, 404, 408, 500, 501] {
        assert_eq!(AuthErrorCode::from_i32(legacy), None);
    }
}

#[test]
fn payload_error_code_carries_auth_error_code() {
    // Payload::Error.code 保持 i32 类型：归因黑洞补丁复用该字段填 AuthErrorCode 数值
    let env = Envelope::new(0, Payload::Error {
        code: AuthErrorCode::ConnectionLost.as_i32(),
        message: "网络连接异常".to_string(),
    });
    let back = roundtrip(&env);
    match back.payload {
        Payload::Error { code, message } => {
            assert_eq!(AuthErrorCode::from_i32(code), Some(AuthErrorCode::ConnectionLost));
            assert_eq!(message, "网络连接异常");
        }
        _ => panic!("变体不匹配"),
    }
}

// ── chmod/chown：文件属主/权限管理（2026-09-16 设计） ──

#[test]
fn chmod_request_wire_format() {
    // mode 为八进制数值（0o755 = 493），避免字符串解析歧义
    let env = Envelope::new(
        31,
        Payload::ChmodRequest {
            path: "/home/user/dir".to_string(),
            mode: 493,
            recursive: true,
        },
    );
    let json = serde_json::to_string(&env).unwrap();
    assert_eq!(
        json,
        r#"{"request_id":31,"payload":{"type":"chmod","data":{"path":"/home/user/dir","mode":493,"recursive":true}}}"#
    );
    // 线上往返：编码后必须可解码
    let env2 = roundtrip(&env);
    match env2.payload {
        Payload::ChmodRequest { path, mode, recursive } => {
            assert_eq!(path, "/home/user/dir");
            assert_eq!(mode, 493);
            assert!(recursive);
        }
        _ => panic!("变体不匹配"),
    }
}

#[test]
fn chmod_resp_wire_format() {
    let env = Envelope::new(32, Payload::ChmodResponse { success: true });
    let json = serde_json::to_string(&env).unwrap();
    assert_eq!(
        json,
        r#"{"request_id":32,"payload":{"type":"chmod_resp","data":{"success":true}}}"#
    );
    let back = roundtrip(&env);
    match back.payload {
        Payload::ChmodResponse { success } => assert!(success),
        _ => panic!("变体不匹配"),
    }
}

#[test]
fn chown_request_wire_format() {
    // owner/group 为用户名/组名字符串（如 "www-data"），Agent 端解析为 uid/gid
    let env = Envelope::new(
        33,
        Payload::ChownRequest {
            path: "/srv/www".to_string(),
            owner: "www-data".to_string(),
            group: "www-data".to_string(),
            recursive: false,
        },
    );
    let json = serde_json::to_string(&env).unwrap();
    assert_eq!(
        json,
        r#"{"request_id":33,"payload":{"type":"chown","data":{"path":"/srv/www","owner":"www-data","group":"www-data","recursive":false}}}"#
    );
    let env2 = roundtrip(&env);
    match env2.payload {
        Payload::ChownRequest { path, owner, group, recursive } => {
            assert_eq!(path, "/srv/www");
            assert_eq!(owner, "www-data");
            assert_eq!(group, "www-data");
            assert!(!recursive);
        }
        _ => panic!("变体不匹配"),
    }
}

#[test]
fn chown_resp_wire_format() {
    let env = Envelope::new(34, Payload::ChownResponse { success: true });
    let json = serde_json::to_string(&env).unwrap();
    assert_eq!(
        json,
        r#"{"request_id":34,"payload":{"type":"chown_resp","data":{"success":true}}}"#
    );
    let back = roundtrip(&env);
    match back.payload {
        Payload::ChownResponse { success } => assert!(success),
        _ => panic!("变体不匹配"),
    }
}

#[test]
fn read_dir_resp_without_owner_group_compat() {
    // FileEntry.owner/group 为后加字段：旧版 Agent 的 read_dir_resp 不含这两字段，
    // 必须可解码且回退空串（与 CurrentUserResponse.home_dir 的 default 先例一致）
    let raw = r#"{"request_id":1,"payload":{"type":"read_dir_resp","data":{"path":"/tmp","entries":[{"name":"a.txt","is_dir":false,"size":1,"mtime":"2026-08-05T10:00:00Z","permissions":"rw-r--r--"}]}}}"#;
    let env: Envelope = serde_json::from_str(raw).expect("旧格式必须可解码");
    match env.payload {
        Payload::ReadDirResponse { path, entries } => {
            assert_eq!(path, "/tmp");
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].name, "a.txt");
            assert_eq!(entries[0].owner, "");
            assert_eq!(entries[0].group, "");
        }
        _ => panic!("变体不匹配"),
    }
}

#[test]
fn read_dir_resp_with_owner_group_wire_format() {
    // 新版 Agent 的 read_dir_resp：FileEntry 携带 owner/group 字段
    let env = Envelope::new(
        35,
        Payload::ReadDirResponse {
            path: "/srv/www".to_string(),
            entries: vec![quirel_protocol::FileEntry {
                name: "index.html".to_string(),
                is_dir: false,
                size: 1024,
                mtime: "2026-09-16T00:00:00Z".to_string(),
                permissions: "rw-r--r--".to_string(),
                owner: "www-data".to_string(),
                group: "www-data".to_string(),
            }],
        },
    );
    let json = serde_json::to_string(&env).unwrap();
    assert_eq!(
        json,
        r#"{"request_id":35,"payload":{"type":"read_dir_resp","data":{"path":"/srv/www","entries":[{"name":"index.html","is_dir":false,"size":1024,"mtime":"2026-09-16T00:00:00Z","permissions":"rw-r--r--","owner":"www-data","group":"www-data"}]}}}"#
    );
}

// ===== 上传大小限制（设置页「文件→传输设置」）+ Agent 能力协商 =====

#[test]
fn get_transfer_limit_request_wire_format() {
    // 空 struct 变体在 tag/content 下序列化为 "data":{}
    // （若断言红灯且差异仅为 data 形状，以 serde 实际输出为准修正断言并锁死）
    let env = Envelope::new(11, Payload::GetTransferLimit {});
    let json = serde_json::to_string(&env).unwrap();
    assert_eq!(
        json,
        r#"{"request_id":11,"payload":{"type":"get_transfer_limit","data":{}}}"#
    );
    let back = roundtrip(&env);
    match back.payload {
        Payload::GetTransferLimit {} => {}
        _ => panic!("变体不匹配"),
    }
}

#[test]
fn transfer_limit_response_wire_format() {
    let env = Envelope::new(12, Payload::TransferLimitResponse {
        max_file_transfer_mb: 2048,
        editable: true,
        persisted: false,
    });
    let json = serde_json::to_string(&env).unwrap();
    assert_eq!(
        json,
        r#"{"request_id":12,"payload":{"type":"transfer_limit_resp","data":{"max_file_transfer_mb":2048,"editable":true,"persisted":false}}}"#
    );
    let back = roundtrip(&env);
    match back.payload {
        Payload::TransferLimitResponse { max_file_transfer_mb, editable, persisted } => {
            assert_eq!((max_file_transfer_mb, editable, persisted), (2048, true, false));
        }
        _ => panic!("变体不匹配"),
    }
}

#[test]
fn set_transfer_limit_request_wire_format() {
    let env = Envelope::new(13, Payload::SetTransferLimit { max_file_transfer_mb: 1024 });
    let json = serde_json::to_string(&env).unwrap();
    assert_eq!(
        json,
        r#"{"request_id":13,"payload":{"type":"set_transfer_limit","data":{"max_file_transfer_mb":1024}}}"#
    );
    let back = roundtrip(&env);
    match back.payload {
        Payload::SetTransferLimit { max_file_transfer_mb } => {
            assert_eq!(max_file_transfer_mb, 1024);
        }
        _ => panic!("变体不匹配"),
    }
}

#[test]
fn auth_response_with_capabilities_wire_format() {
    // 新 Agent 认证响应携带能力列表；旧客户端 serde 忽略未知字段，向前兼容
    let env = Envelope::new(14, Payload::AuthResponse {
        success: true,
        error: None,
        session_id: Some("sess-1".to_string()),
        code: None,
        capabilities: Some(vec!["transfer_limit".to_string()]),
    });
    let json = serde_json::to_string(&env).unwrap();
    assert_eq!(
        json,
        r#"{"request_id":14,"payload":{"type":"auth_response","data":{"success":true,"error":null,"session_id":"sess-1","code":null,"capabilities":["transfer_limit"]}}}"#
    );
    let back = roundtrip(&env);
    match back.payload {
        Payload::AuthResponse { success, capabilities, .. } => {
            assert!(success);
            assert_eq!(capabilities, Some(vec!["transfer_limit".to_string()]));
        }
        _ => panic!("变体不匹配"),
    }
}

#[test]
fn auth_response_without_capabilities_compat() {
    // 旧 Agent 响应不含 capabilities → 解码回退 None。
    // 客户端凭 None 识别旧 Agent 并门控新协议命令（旧 Agent 解码不了新 payload，盲发会断流拆连）
    let raw = r#"{"request_id":7,"payload":{"type":"auth_response","data":{"success":true,"error":null,"session_id":"s1","code":null}}}"#;
    let env: Envelope = serde_json::from_str(raw).expect("旧格式必须可解码");
    match env.payload {
        Payload::AuthResponse { success, capabilities, .. } => {
            assert!(success);
            assert_eq!(capabilities, None);
        }
        _ => panic!("变体不匹配"),
    }
}
