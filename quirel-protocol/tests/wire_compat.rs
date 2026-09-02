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
