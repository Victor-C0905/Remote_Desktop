# 文件格式识别与打开系统 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 双击远程文件时按格式智能路由到对应应用（图片查看器 / PDF 阅读器 / 文本编辑器 / 十六进制回退），替代 FileManager 中"所有文件一律打开 TextEditor"的硬编码。

**Architecture:** 四层架构——检测层（FileFormatRegistry + 插件）、网关层（FileOpener）、渲染层（新应用 ImageViewer/PDFViewer/HexViewer）、数据层（Agent 新增 `file_info` 命令返回 magic bytes）。协议变更遵循 quirel-protocol 单一真相源 + wire_compat golden 测试锁定。

**Tech Stack:** Rust (quirel-protocol / agent / src-tauri)、Protobuf (Manager↔Worker IPC)、React + TypeScript、pdfjs-dist（PDF 渲染）、encoding_rs（GBK 回退）。

**⚠️ 用户规则覆盖（最高优先级）:**
- **禁止执行任何 git 提交/回退/推送操作**——每个任务完成后仅提示用户需要提交，由用户操作。
- 保留所有代码注释；新增文件按项目结构分类归入对应目录。
- 协议变更必须同步更新 `quirel-protocol`（单一真相源），禁止双端独立维护协议类型。

---

## 文件结构总览

```
新增/修改文件清单：
├── quirel-protocol/src/envelope.rs                       [修改] Payload 加 FileInfo 变体
├── quirel-protocol/tests/wire_compat.rs                  [修改] golden 测试
├── agent/protocol/agent.proto                            [修改] IPC 消息定义
├── agent/src/manager/protocol_adapter.rs                  [修改] 双向适配 + 单元测试
├── agent/src/worker/mod.rs                               [修改] dispatch 分支
├── agent/src/worker/handlers/file.rs                     [修改] handle_file_info 实现
├── src-tauri/src/connection.rs                           [修改] remote_file_info / remote_read_file_binary / GBK 回退
├── src-tauri/src/lib.rs                                  [修改] 命令注册
├── src-tauri/Cargo.toml                                  [修改] encoding_rs 依赖
├── src-tauri/tauri.conf.json                             [修改] CSP worker-src
├── package.json                                          [修改] pdfjs-dist 依赖
├── src/file-formats/types.ts                             [新增] 类型定义
├── src/file-formats/registry.ts                          [新增] 检测层注册表
├── src/file-formats/FileOpener.ts                        [新增] 网关层
├── src/file-formats/plugins/{pdf,image,text}.ts          [新增] 格式插件
├── src/file-formats/__tests__/*.test.ts                  [新增] 检测层测试
├── src/apps/ImageViewer/{ImageViewer.tsx,ImageViewer.css}    [新增] 图片应用
├── src/apps/PDFViewer/{PDFViewer.tsx,PDFViewer.css}           [新增] PDF 应用
├── src/apps/HexViewer/{HexViewer.tsx,HexViewer.css}           [新增] 十六进制回退应用
└── src/window-system/init.ts                             [修改] 注册三个新应用
```

**验证命令速查：**
- 协议 crate 测试：`cargo test -p quirel-protocol`（根目录）
- Agent 测试（WSL）：`wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && cargo test"`
- 客户端 Rust 检查（Windows，src-tauri 目录）：`cargo check`
- 前端类型检查：`npx tsc --noEmit`
- 前端测试：`npm run test:run`

---

### Task 1: 协议层 — FileInfo 消息（quirel-protocol）

**Files:**
- Modify: `quirel-protocol/src/envelope.rs`（ReadFileResponse 定义之后，约 L133）
- Modify: `quirel-protocol/tests/wire_compat.rs`（文件末尾追加测试）

- [ ] **Step 1: 写失败的 golden 测试**

在 `quirel-protocol/tests/wire_compat.rs` 末尾追加：

```rust
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
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p quirel-protocol`
Expected: 编译失败，`FileInfoRequest` / `FileInfoResponse` 变体不存在。

- [ ] **Step 3: 实现 Payload 变体**

在 `quirel-protocol/src/envelope.rs` 的 `ReadFileResponse` 变体（约 L133）之后插入：

```rust
    /// 文件格式探测请求（双击文件时先于 read_file 调用，用于格式路由）
    #[serde(rename = "file_info")]
    FileInfoRequest { path: String },

    /// 文件格式探测响应
    /// - magic_bytes: 文件头部字节（前 512 字节），用于 magic number 检测
    /// - is_text: Agent 端启发式判定（BOM/NUL/控制字符比例）
    #[serde(rename = "file_info_resp")]
    FileInfoResponse {
        path: String,
        size: u64,
        is_dir: bool,
        is_text: bool,
        extension: String,
        magic_bytes: Vec<u8>,
    },
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p quirel-protocol`
Expected: 全部测试通过（原有测试 + 2 个新测试）。

- [ ] **Step 5: 提示用户提交**

告知用户：协议层 FileInfo 消息已完成，建议提交（如 `feat(protocol): 新增 file_info 格式探测消息`）。**不执行 git 命令。**

---

### Task 2: Agent IPC — proto 消息 + protocol_adapter 双向适配

**Files:**
- Modify: `agent/protocol/agent.proto`
- Modify: `agent/src/manager/protocol_adapter.rs`

- [ ] **Step 1: 写失败的单元测试**

在 `agent/src/manager/protocol_adapter.rs` 的 `mod tests` 中追加：

```rust
    #[test]
    fn test_file_info_request_conversion() {
        let payload = Payload::FileInfoRequest { path: "/a.png".to_string() };
        let user = test_user_context();

        let result = serde_to_worker_request(&payload, &user);

        match result {
            Some(manager_request::Payload::FileInfo(req)) => {
                assert_eq!(req.path, "/a.png");
                assert_eq!(req.uid, user.uid);
                assert_eq!(req.username, user.username);
            }
            _ => panic!("Expected FileInfo conversion"),
        }
    }

    #[test]
    fn test_file_info_result_response_conversion() {
        let resp = WorkerResponse {
            payload: Some(worker_response::Payload::FileInfoResult(FileInfoResult {
                path: "/a.png".to_string(),
                size: 1024,
                is_dir: false,
                is_text: false,
                extension: "png".to_string(),
                magic_bytes: vec![0x89, 0x50, 0x4E, 0x47],
            })),
            ..Default::default()
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
```

在文件顶部 `use crate::protocol::generated::{...}` 现有导入清单中追加 `FileInfo, FileInfoResult`（保持现有导入风格）。

- [ ] **Step 2: 运行测试确认失败**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && cargo test protocol_adapter"`
Expected: 编译失败，`FileInfo` / `FileInfoResult` 类型不存在。

- [ ] **Step 3: 修改 agent.proto**

`agent/protocol/agent.proto`：

(a) `ManagerRequest` 的 `oneof payload` 内（`ApplyDiff apply_diff = 17;` 之后）追加：

```proto
        FileInfo file_info = 18;
```

(b) `WorkerResponse` 的 `oneof payload` 内（`ApplyDiffResult apply_diff_result = 16;` 之后）追加：

```proto
        FileInfoResult file_info_result = 17;
```

(c) 文件操作区（`ReadFile` 消息定义之后）追加消息体：

```proto
// 文件格式探测（读取元数据 + 头部字节，用于客户端格式路由）
message FileInfo {
    string path = 1;
    // 用户上下文（与其他文件操作一致）
    uint32 uid = 2;
    uint32 gid = 3;
    string username = 4;
    string home_dir = 5;
}

message FileInfoResult {
    string path = 1;
    uint64 size = 2;
    bool is_dir = 3;
    bool is_text = 4;
    string extension = 5;
    bytes magic_bytes = 6;
}
```

- [ ] **Step 4: 实现 protocol_adapter 双向适配**

`agent/src/manager/protocol_adapter.rs`：

(a) 顶部 `use crate::protocol::generated::{...}` 导入清单追加 `FileInfo, FileInfoResult`。

(b) `serde_to_worker_request` 函数中，`ReadFileRequest` 分支之后追加：

```rust
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
```

(c) `worker_response_to_serde` 函数中，`FileContent`（ReadFileResponse）分支之后追加：

```rust
        worker_response::Payload::FileInfoResult(r) => {
            Some(Payload::FileInfoResponse {
                path: r.path.clone(),
                size: r.size,
                is_dir: r.is_dir,
                is_text: r.is_text,
                extension: r.extension.clone(),
                magic_bytes: r.magic_bytes.to_vec(),
            })
        }
```

- [ ] **Step 5: 运行测试确认通过**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && cargo test protocol_adapter"`
Expected: 全部通过（含 2 个新测试）。

- [ ] **Step 6: 提示用户提交**

告知用户：IPC proto 与适配层完成，建议提交。**不执行 git 命令。**

---

### Task 3: Worker handler — handle_file_info 实现

**Files:**
- Modify: `agent/src/worker/mod.rs`（dispatch 加分支，约 L172 ReadFile 分支之后）
- Modify: `agent/src/worker/handlers/file.rs`

- [ ] **Step 1: 修改 dispatch**

`agent/src/worker/mod.rs` 中 `ReadFile` 分支（约 L172）之后追加：

```rust
        Some(crate::protocol::generated::manager_request::Payload::FileInfo(req)) => {
            handlers::file::handle_file_info(req).await
        }
```

- [ ] **Step 2: 实现 handler**

`agent/src/worker/handlers/file.rs`：

(a) 顶部 `use crate::protocol::generated::{...}` 导入清单追加 `FileInfo, FileInfoResult`。

(b) 模块头部注释的功能列表追加一行：`//! - 探测文件格式（FileInfo）`。

(c) `handle_read_file` 之后追加实现：

```rust
/// is_text 启发式判定（参考 git 的 binary 检测思路）
///
/// 规则（检查前 8000 字节）：
/// 1. UTF-8/UTF-16 BOM → 文本
/// 2. 含 NUL(0x00) → 二进制
/// 3. 控制字符（除 \t \n \r）占比 >= 5% → 二进制
/// 4. 其余 → 文本（空文件视为文本）
fn detect_is_text(bytes: &[u8]) -> bool {
    // BOM 检测：UTF-8 (EF BB BF)、UTF-16LE (FF FE)、UTF-16BE (FE FF)
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF])
        || bytes.starts_with(&[0xFF, 0xFE])
        || bytes.starts_with(&[0xFE, 0xFF])
    {
        return true;
    }

    let check_len = bytes.len().min(8000);
    if check_len == 0 {
        return true; // 空文件视为文本
    }

    let mut control_count = 0usize;
    for &b in &bytes[..check_len] {
        if b == 0 {
            return false; // NUL 字节 → 二进制
        }
        // 非法控制字符（除 Tab/LF/CR）
        if b < 32 && b != b'\t' && b != b'\n' && b != b'\r' {
            control_count += 1;
        }
    }

    // 控制字符占比 < 5% 视为文本
    control_count * 100 / check_len < 5
}

/// 处理 FileInfo 请求：读取文件元数据 + 头部 512 字节
///
/// 在目标用户上下文中执行（fork+setuid 用户隔离），
/// 返回格式路由所需的全部信息（size/is_text/extension/magic_bytes）。
#[tracing::instrument(fields(path = %req.path, uid = req.uid))]
pub async fn handle_file_info(req: FileInfo) -> WorkerResponse {
    tracing::info!("处理 FileInfo 请求: path={}, uid={}", req.path, req.uid);

    // 构造用户会话
    let session = build_user_session(req.uid, req.gid, &req.username, &req.home_dir);
    // 路径安全校验：防目录穿越、防符号链接攻击
    let safe_path = match crate::auth::validate_path(&req.path, session.home_dir.as_path(), session.uid) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("路径校验失败: path={}, error={}", req.path, e);
            let (code, message) = error_to_code_message(&e);
            return WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error { code, message })),
                ..Default::default()
            };
        }
    };
    let executor = UserExecutor::new(&session);

    // 序列化中间结果：(size, is_dir, extension, magic_bytes)
    let path = safe_path.as_str().to_string();
    let result: AnyhowResult<(u64, bool, String, Vec<u8>)> = executor.execute_as_user(move || {
        let p = Path::new(&path);

        let metadata = fs::metadata(p)
            .map_err(|e| anyhow::anyhow!("读取文件元数据失败 '{}': {}", path, e))?;

        if !metadata.is_file() {
            // 目录或特殊文件（FIFO/socket/device）：返回目录标记，无头部字节
            return Ok((metadata.len(), metadata.is_dir(), String::new(), Vec::new()));
        }

        // 扩展名（小写、不含点）
        let extension = p
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();

        // 读取头部 512 字节（所有常见 magic number 都在前 16 字节内，512 留足余量）
        let mut file = fs::File::open(p)
            .map_err(|e| anyhow::anyhow!("打开文件失败 '{}': {}", path, e))?;
        let mut head = vec![0u8; 512];
        use std::io::Read;
        let n = file.read(&mut head)
            .map_err(|e| anyhow::anyhow!("读取文件头失败 '{}': {}", path, e))?;
        head.truncate(n);

        Ok((metadata.len(), false, extension, head))
    });

    match result {
        Ok((size, is_dir, extension, head)) => {
            let is_text = if is_dir { false } else { detect_is_text(&head) };
            WorkerResponse {
                payload: Some(worker_response::Payload::FileInfoResult(FileInfoResult {
                    path: req.path,
                    size,
                    is_dir,
                    is_text,
                    extension,
                    magic_bytes: head,
                })),
                ..Default::default()
            }
        }
        Err(e) => {
            tracing::error!("FileInfo 处理失败: {}", e);
            WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error {
                    code: 500,
                    message: e.to_string(),
                })),
                ..Default::default()
            }
        }
    }
}
```

**注意：** 若 `error_to_code_message` 不存在于 file.rs，按 `handle_read_dir` 的现有错误处理模式保持一致——错误路径统一返回 `worker_response::Payload::Error(Error { code: 500, message })`。

- [ ] **Step 3: 编译并运行 Agent 全量测试**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && cargo test"`
Expected: 编译零错误，全部测试通过。

- [ ] **Step 4: 提示用户提交**

告知用户：Agent 端 file_info 完成，建议提交。**不执行 git 命令。**

---

### Task 4: 客户端 Rust — remote_file_info / remote_read_file_binary / GBK 回退

**Files:**
- Modify: `src-tauri/src/connection.rs`
- Modify: `src-tauri/src/lib.rs`（invoke_handler 注册，约 L597）
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: 加依赖**

`src-tauri/Cargo.toml` 的 `[dependencies]` 追加：

```toml
encoding_rs = "0.8"
```

- [ ] **Step 2: 实现两个新命令**

`src-tauri/src/connection.rs` 中 `RemoteReadFileResponse` 定义（约 L791）之后追加：

```rust
/// 文件格式探测结果（camelCase 序列化，与前端风格对齐）
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFileInfo {
    pub path: String,
    pub size: u64,
    pub is_dir: bool,
    pub is_text: bool,
    pub extension: String,
    /// 文件头部字节（JSON 数组传输，前端用于 magic number 检测）
    pub magic_bytes: Vec<u8>,
}

#[tauri::command]
#[tracing::instrument(skip(app), fields(server_id = %server_id, path = %path))]
pub async fn remote_file_info(server_id: String, path: String, app: tauri::AppHandle) -> Result<RemoteFileInfo, String> {
    tracing::debug!("[FileInfo] server_id={}, path={}", server_id, path);
    let resp = remote_send(server_id, Payload::FileInfoRequest { path }, app).await?;
    match resp.payload {
        Payload::FileInfoResponse { path, size, is_dir, is_text, extension, magic_bytes } => {
            Ok(RemoteFileInfo { path, size, is_dir, is_text, extension, magic_bytes })
        }
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

/// 二进制文件读取响应（base64 传输，图片/PDF/十六进制查看器使用）
#[derive(Debug, Serialize)]
pub struct RemoteBinaryFile {
    pub path: String,
    /// 文件原始字节的 base64 编码（前端 atob 解码）
    pub base64: String,
    pub mtime: u64,
    pub size: u64,
}

/// 读取二进制文件（图片/PDF/十六进制视图）
///
/// 与 remote_read_file 的区别：不做 UTF-8 转换，直接透传 base64，
/// 前端按用途解码（data URI / pdfjs Uint8Array / hex dump）。
#[tauri::command]
#[tracing::instrument(skip(app), fields(server_id = %server_id, path = %path))]
pub async fn remote_read_file_binary(server_id: String, path: String, app: tauri::AppHandle) -> Result<RemoteBinaryFile, String> {
    tracing::debug!("[ReadFileBinary] server_id={}, path={}", server_id, path);
    let resp = remote_send(server_id, Payload::ReadFileRequest { path }, app).await?;
    match resp.payload {
        Payload::ReadFileResponse { path, content, mtime, size } => {
            Ok(RemoteBinaryFile { path, base64: content, mtime, size })
        }
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}
```

- [ ] **Step 3: 改造 remote_read_file 的 GBK 回退**

`src-tauri/src/connection.rs` 约 L776，将：

```rust
            let content_text = String::from_utf8(content_bytes)
                .map_err(|e| format!("文件不是有效的 UTF-8 文本（可能为二进制或非 UTF-8 编码）: {}", e))?;
```

替换为：

```rust
            // UTF-8 优先；失败时回退 GB18030（GBK 超集，覆盖中文服务器常见编码）
            // 注：二进制文件不会走到这里——FileOpener 先经 file_info 路由，is_text=false 不进编辑器
            let content_text = match String::from_utf8(content_bytes) {
                Ok(s) => s,
                Err(e) => {
                    let (decoded, _, had_errors) = encoding_rs::GB18030.decode(&content_bytes);
                    if had_errors {
                        return Err(format!("文件不是有效的 UTF-8 文本（可能为二进制文件）: {}", e));
                    }
                    tracing::info!("[ReadFile] UTF-8 解码失败，已按 GB18030 回退解码");
                    decoded.into_owned()
                }
            };
```

- [ ] **Step 4: 注册命令**

`src-tauri/src/lib.rs` L597 `connection::remote_read_file,` 之后追加两行：

```rust
            connection::remote_file_info,
            connection::remote_read_file_binary,
```

- [ ] **Step 5: 编译验证**

Run: `cargo check`（src-tauri 目录，Windows 原生）
Expected: 零错误。

- [ ] **Step 6: 提示用户提交**

告知用户：客户端命令层完成，建议提交。**不执行 git 命令。**

---

### Task 5: 前端检测层 — types + registry

**Files:**
- Create: `src/file-formats/types.ts`
- Create: `src/file-formats/registry.ts`
- Create: `src/file-formats/__tests__/registry.test.ts`

- [ ] **Step 1: 写失败的测试**

创建 `src/file-formats/__tests__/registry.test.ts`：

```typescript
/**
 * FileFormatRegistry 单元测试
 *
 * 验证插件注册、按优先级检测、无匹配时的空返回
 */
import { describe, it, expect } from 'vitest';
import { FileFormatRegistry } from '../registry';
import type { RemoteFileInfo } from '../types';

/** 构造测试用文件信息 */
function makeInfo(overrides: Partial<RemoteFileInfo> = {}): RemoteFileInfo {
  return {
    path: '/tmp/f',
    size: 100,
    isDir: false,
    isText: false,
    extension: '',
    magicBytes: [],
    ...overrides,
  };
}

describe('FileFormatRegistry', () => {
  it('按注册顺序返回首个匹配的插件结果', () => {
    const registry = new FileFormatRegistry();
    registry.register({ id: 'a', detect: () => null });
    registry.register({ id: 'b', detect: () => ({ pluginId: 'b', category: 'text', confidence: 1 }) });

    const match = registry.detect(makeInfo());
    expect(match).not.toBeNull();
    expect(match!.pluginId).toBe('b');
  });

  it('所有插件都不匹配时返回 null', () => {
    const registry = new FileFormatRegistry();
    registry.register({ id: 'a', detect: () => null });

    expect(registry.detect(makeInfo())).toBeNull();
  });

  it('空注册表返回 null', () => {
    const registry = new FileFormatRegistry();
    expect(registry.detect(makeInfo())).toBeNull();
  });
});
```

- [ ] **Step 2: 运行测试确认失败**

Run: `npm run test:run -- src/file-formats`
Expected: 失败，模块不存在。

- [ ] **Step 3: 实现 types.ts**

创建 `src/file-formats/types.ts`：

```typescript
/**
 * 文件格式系统类型定义
 *
 * 四层架构：检测层（插件 + 注册表）→ 网关层（FileOpener）→ 渲染层（各应用）→ 数据层（file_info 协议）
 * 本文件是检测层的类型单一真相源。
 */

/** Agent file_info 命令返回的文件探测结果（与 Rust RemoteFileInfo 对应，camelCase） */
export interface RemoteFileInfo {
  path: string;
  /** 文件大小（字节） */
  size: number;
  isDir: boolean;
  /** Agent 端启发式判定是否为文本（BOM/NUL/控制字符比例） */
  isText: boolean;
  /** 小写扩展名，不含点；无扩展名为空串 */
  extension: string;
  /** 文件头部字节（前 512 字节），用于 magic number 检测 */
  magicBytes: number[];
}

/** 格式分类（决定路由到哪个应用） */
export type FormatCategory = 'image' | 'pdf' | 'text' | 'hex';

/** 单个插件的检测结果 */
export interface FormatMatch {
  /** 命中的插件 ID */
  pluginId: string;
  category: FormatCategory;
  /** 置信度 0-1（magic 命中 > 扩展名命中） */
  confidence: number;
  /** 图片类的 MIME 类型（data URI 用），其他类为空 */
  mimeType?: string;
}

/** 格式检测插件接口（检测层扩展点：新格式 = 新插件文件） */
export interface FileFormatPlugin {
  /** 插件唯一 ID（如 'image'、'pdf'） */
  id: string;
  /**
   * 检测文件格式
   * @param info Agent 返回的探测信息（magicBytes + 扩展名双信号）
   * @returns 命中返回 FormatMatch；不匹配返回 null
   */
  detect(info: RemoteFileInfo): FormatMatch | null;
}
```

- [ ] **Step 4: 实现 registry.ts**

创建 `src/file-formats/registry.ts`：

```typescript
/**
 * 文件格式注册表（检测层核心）
 *
 * 按注册顺序遍历插件，返回首个命中结果。
 * 插件注册顺序即优先级：高置信度格式（magic number 检测）在前。
 */
import type { FileFormatPlugin, FormatMatch, RemoteFileInfo } from './types';

export class FileFormatRegistry {
  private plugins: FileFormatPlugin[] = [];

  /** 注册插件（顺序即优先级） */
  register(plugin: FileFormatPlugin): void {
    this.plugins.push(plugin);
  }

  /** 依次遍历插件检测，返回首个命中结果；全部不匹配返回 null */
  detect(info: RemoteFileInfo): FormatMatch | null {
    for (const plugin of this.plugins) {
      const match = plugin.detect(info);
      if (match !== null) {
        return match;
      }
    }
    return null;
  }
}
```

- [ ] **Step 5: 运行测试确认通过**

Run: `npm run test:run -- src/file-formats`
Expected: 3 项测试通过。

- [ ] **Step 6: 提示用户提交**

告知用户：检测层基础设施完成，建议提交。**不执行 git 命令。**

---

### Task 6: 前端格式插件 — pdf / image / text

**Files:**
- Create: `src/file-formats/plugins/pdf.ts`
- Create: `src/file-formats/plugins/image.ts`
- Create: `src/file-formats/plugins/text.ts`
- Create: `src/file-formats/__tests__/plugins.test.ts`

- [ ] **Step 1: 写失败的测试**

创建 `src/file-formats/__tests__/plugins.test.ts`：

```typescript
/**
 * 格式插件单元测试
 *
 * 验证 magic bytes 检测、扩展名检测、优先级语义
 */
import { describe, it, expect } from 'vitest';
import { pdfPlugin } from '../plugins/pdf';
import { imagePlugin } from '../plugins/image';
import { textPlugin } from '../plugins/text';
import type { RemoteFileInfo } from '../types';

function makeInfo(overrides: Partial<RemoteFileInfo> = {}): RemoteFileInfo {
  return {
    path: '/tmp/f',
    size: 100,
    isDir: false,
    isText: false,
    extension: '',
    magicBytes: [],
    ...overrides,
  };
}

describe('pdfPlugin', () => {
  it('magic bytes 命中 %PDF-', () => {
    const match = pdfPlugin.detect(makeInfo({ magicBytes: [0x25, 0x50, 0x44, 0x46, 0x2d] }));
    expect(match).toEqual({ pluginId: 'pdf', category: 'pdf', confidence: 0.95 });
  });

  it('扩展名 .pdf 低置信度命中', () => {
    const match = pdfPlugin.detect(makeInfo({ extension: 'pdf', magicBytes: [1, 2, 3] }));
    expect(match).toEqual({ pluginId: 'pdf', category: 'pdf', confidence: 0.6 });
  });

  it('普通文件不匹配', () => {
    expect(pdfPlugin.detect(makeInfo({ extension: 'txt' }))).toBeNull();
  });
});

describe('imagePlugin', () => {
  it('PNG magic 命中', () => {
    const match = imagePlugin.detect(
      makeInfo({ magicBytes: [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a] }),
    );
    expect(match).toEqual({ pluginId: 'image', category: 'image', mimeType: 'image/png', confidence: 0.95 });
  });

  it('JPEG magic 命中', () => {
    const match = imagePlugin.detect(makeInfo({ magicBytes: [0xff, 0xd8, 0xff, 0xe0] }));
    expect(match).toEqual({ pluginId: 'image', category: 'image', mimeType: 'image/jpeg', confidence: 0.95 });
  });

  it('WebP magic 命中（RIFF....WEBP）', () => {
    const match = imagePlugin.detect(
      makeInfo({ magicBytes: [0x52, 0x49, 0x46, 0x46, 0x00, 0x00, 0x00, 0x00, 0x57, 0x45, 0x42, 0x50] }),
    );
    expect(match).toEqual({ pluginId: 'image', category: 'image', mimeType: 'image/webp', confidence: 0.95 });
  });

  it('SVG 扩展名命中（SVG 是文本，无 binary magic）', () => {
    const match = imagePlugin.detect(makeInfo({ extension: 'svg', isText: true }));
    expect(match).toEqual({ pluginId: 'image', category: 'image', mimeType: 'image/svg+xml', confidence: 0.6 });
  });

  it('图片扩展名低置信度命中', () => {
    const match = imagePlugin.detect(makeInfo({ extension: 'jpg', magicBytes: [1, 2, 3] }));
    expect(match).toEqual({ pluginId: 'image', category: 'image', mimeType: 'image/jpeg', confidence: 0.6 });
  });

  it('普通文本不匹配', () => {
    expect(imagePlugin.detect(makeInfo({ isText: true, extension: 'txt' }))).toBeNull();
  });
});

describe('textPlugin', () => {
  it('isText=true 且非 SVG 扩展名 → 文本', () => {
    const match = textPlugin.detect(makeInfo({ isText: true, extension: 'rs' }));
    expect(match).toEqual({ pluginId: 'text', category: 'text', confidence: 0.9 });
  });

  it('isText=true 但扩展名为 svg → 不匹配（交给 image 插件）', () => {
    expect(textPlugin.detect(makeInfo({ isText: true, extension: 'svg' }))).toBeNull();
  });

  it('isText=false 不匹配', () => {
    expect(textPlugin.detect(makeInfo({ isText: false, extension: 'txt' }))).toBeNull();
  });
});
```

- [ ] **Step 2: 运行测试确认失败**

Run: `npm run test:run -- src/file-formats`
Expected: 新增测试全部失败（插件不存在）。

- [ ] **Step 3: 实现 pdf.ts**

创建 `src/file-formats/plugins/pdf.ts`：

```typescript
/**
 * PDF 格式插件
 *
 * magic number: %PDF-（25 50 44 46 2D）
 */
import type { FileFormatPlugin, FormatMatch, RemoteFileInfo } from '../types';

/** PDF 文件头 "%PDF-" */
const PDF_MAGIC = [0x25, 0x50, 0x44, 0x46, 0x2d];

export const pdfPlugin: FileFormatPlugin = {
  id: 'pdf',
  detect(info: RemoteFileInfo): FormatMatch | null {
    // magic 命中：高置信度
    if (info.magicBytes.length >= PDF_MAGIC.length && PDF_MAGIC.every((b, i) => info.magicBytes[i] === b)) {
      return { pluginId: 'pdf', category: 'pdf', confidence: 0.95 };
    }
    // 扩展名命中：低置信度（文件可能改错了扩展名，但 read 后 pdfjs 会报错兜底）
    if (info.extension === 'pdf') {
      return { pluginId: 'pdf', category: 'pdf', confidence: 0.6 };
    }
    return null;
  },
};
```

- [ ] **Step 4: 实现 image.ts**

创建 `src/file-formats/plugins/image.ts`：

```typescript
/**
 * 图片格式插件
 *
 * magic number 检测（浏览器原生解码的所有格式）+ 扩展名回退。
 * SVG 特例：文本格式无 binary magic，仅扩展名判定（在 <img> 中渲染是静态安全的，脚本不执行）。
 */
import type { FileFormatPlugin, FormatMatch, RemoteFileInfo } from '../types';

/** 图片 magic number 表 */
const MAGIC_TABLE: Array<{ prefix: number[]; mime: string }> = [
  { prefix: [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a], mime: 'image/png' }, // PNG
  { prefix: [0xff, 0xd8, 0xff], mime: 'image/jpeg' }, // JPEG
  { prefix: [0x47, 0x49, 0x46, 0x38], mime: 'image/gif' }, // GIF87a/GIF89a
  { prefix: [0x42, 0x4d], mime: 'image/bmp' }, // BMP
  { prefix: [0x00, 0x00, 0x01, 0x00], mime: 'image/x-icon' }, // ICO
];

/** WebP: "RIFF" + 4 字节长度 + "WEBP"（偏移 8-11） */
const RIFF = [0x52, 0x49, 0x46, 0x46];
const WEBP = [0x57, 0x45, 0x42, 0x50];

/** 扩展名 → MIME 映射（回退路径） */
const EXTENSION_TABLE: Record<string, string> = {
  png: 'image/png',
  jpg: 'image/jpeg',
  jpeg: 'image/jpeg',
  gif: 'image/gif',
  webp: 'image/webp',
  bmp: 'image/bmp',
  ico: 'image/x-icon',
  svg: 'image/svg+xml',
};

/** 判断前缀是否匹配 */
function startsWith(bytes: number[], prefix: number[], offset = 0): boolean {
  if (bytes.length < offset + prefix.length) return false;
  return prefix.every((b, i) => bytes[offset + i] === b);
}

export const imagePlugin: FileFormatPlugin = {
  id: 'image',
  detect(info: RemoteFileInfo): FormatMatch | null {
    const b = info.magicBytes;

    // magic 命中：高置信度
    for (const { prefix, mime } of MAGIC_TABLE) {
      if (startsWith(b, prefix)) {
        return { pluginId: 'image', category: 'image', mimeType: mime, confidence: 0.95 };
      }
    }
    // WebP 特殊结构
    if (startsWith(b, RIFF) && startsWith(b, WEBP, 8)) {
      return { pluginId: 'image', category: 'image', mimeType: 'image/webp', confidence: 0.95 };
    }

    // 扩展名回退：低置信度
    const mime = EXTENSION_TABLE[info.extension];
    if (mime) {
      return { pluginId: 'image', category: 'image', mimeType: mime, confidence: 0.6 };
    }
    return null;
  },
};
```

- [ ] **Step 5: 实现 text.ts**

创建 `src/file-formats/plugins/text.ts`：

```typescript
/**
 * 文本格式插件
 *
 * 依赖 Agent 端 isText 启发式（BOM/NUL/控制字符比例）。
 * SVG 例外：文本格式但路由到图片应用（image 插件负责）。
 */
import type { FileFormatPlugin, FormatMatch, RemoteFileInfo } from '../types';

export const textPlugin: FileFormatPlugin = {
  id: 'text',
  detect(info: RemoteFileInfo): FormatMatch | null {
    // Agent 判定非文本（含二进制 magic）→ 不匹配
    if (!info.isText) {
      return null;
    }
    // SVG 是文本，但应路由到图片查看器
    if (info.extension === 'svg') {
      return null;
    }
    return { pluginId: 'text', category: 'text', confidence: 0.9 };
  },
};
```

- [ ] **Step 6: 运行测试确认通过**

Run: `npm run test:run -- src/file-formats`
Expected: 全部通过。

- [ ] **Step 7: 提示用户提交**

告知用户：格式插件完成，建议提交。**不执行 git 命令。**

---

### Task 7: 网关层 — FileOpener

**Files:**
- Create: `src/file-formats/FileOpener.ts`
- Create: `src/file-formats/__tests__/opener.test.ts`

- [ ] **Step 1: 写失败的测试**

创建 `src/file-formats/__tests__/opener.test.ts`：

```typescript
/**
 * FileOpener 网关层单元测试
 *
 * 验证格式检测 → 打开目标的决策逻辑（大文件确认阈值、未知格式 hex 回退）
 */
import { describe, it, expect } from 'vitest';
import { decideOpenTarget, createDefaultRegistry, SIZE_LIMITS } from '../FileOpener';
import type { RemoteFileInfo } from '../types';

function makeInfo(overrides: Partial<RemoteFileInfo> = {}): RemoteFileInfo {
  return {
    path: '/tmp/f',
    size: 100,
    isDir: false,
    isText: false,
    extension: '',
    magicBytes: [],
    ...overrides,
  };
}

describe('decideOpenTarget', () => {
  it('PNG 文件路由到 image 且小文件无需确认', () => {
    const d = decideOpenTarget(
      makeInfo({ magicBytes: [0x89, 0x50, 0x4e, 0x47], extension: 'png' }),
      createDefaultRegistry(),
    );
    expect(d.kind).toBe('image');
    expect(d.mimeType).toBe('image/png');
    expect(d.needsSizeConfirm).toBe(false);
  });

  it('超限图片需要大小确认', () => {
    const d = decideOpenTarget(
      makeInfo({ magicBytes: [0x89, 0x50, 0x4e, 0x47], size: SIZE_LIMITS.image + 1 }),
      createDefaultRegistry(),
    );
    expect(d.kind).toBe('image');
    expect(d.needsSizeConfirm).toBe(true);
  });

  it('PDF 文件路由到 pdf', () => {
    const d = decideOpenTarget(
      makeInfo({ magicBytes: [0x25, 0x50, 0x44, 0x46, 0x2d] }),
      createDefaultRegistry(),
    );
    expect(d.kind).toBe('pdf');
  });

  it('文本文件路由到 text', () => {
    const d = decideOpenTarget(
      makeInfo({ isText: true, extension: 'rs' }),
      createDefaultRegistry(),
    );
    expect(d.kind).toBe('text');
  });

  it('未知二进制格式回退到 hex', () => {
    const d = decideOpenTarget(
      makeInfo({ magicBytes: [0x01, 0x02, 0x03], extension: 'bin' }),
      createDefaultRegistry(),
    );
    expect(d.kind).toBe('hex');
    expect(d.reason).toContain('未知格式');
  });

  it('目录返回 directory 目标', () => {
    const d = decideOpenTarget(
      makeInfo({ isDir: true }),
      createDefaultRegistry(),
    );
    expect(d.kind).toBe('directory');
  });
});
```

- [ ] **Step 2: 运行测试确认失败**

Run: `npm run test:run -- src/file-formats`
Expected: 新增测试失败（FileOpener 不存在）。

- [ ] **Step 3: 实现 FileOpener.ts**

创建 `src/file-formats/FileOpener.ts`：

```typescript
/**
 * FileOpener 网关层
 *
 * 职责：调用 file_info 协议探测格式 → 检测层识别 → 决定打开目标（应用 ID + 大小确认）。
 * 不直接创建窗口（窗口创建属于 FileManager 的职责，保持网关层纯逻辑可测试）。
 */
import { invoke } from '@tauri-apps/api/core';
import { FileFormatRegistry } from './registry';
import { pdfPlugin } from './plugins/pdf';
import { imagePlugin } from './plugins/image';
import { textPlugin } from './plugins/text';
import type { FormatCategory, RemoteFileInfo } from './types';

/** 大小确认阈值（字节）：超过时提示用户确认后再加载 */
export const SIZE_LIMITS = {
  /** 图片：20MB（data URI 全量加载，base64 膨胀 1.33 倍） */
  image: 20 * 1024 * 1024,
  /** PDF：30MB（pdfjs 全量加载） */
  pdf: 30 * 1024 * 1024,
  /** 文本：5MB（编辑器全量加载 + 差异计算） */
  text: 5 * 1024 * 1024,
} as const;

/** 打开决策结果 */
export interface OpenDecision {
  /**
   * 目标类型：
   * - 'directory'：目录（FileManager 自行导航）
   * - 'image' | 'pdf' | 'text' | 'hex'：对应应用
   */
  kind: 'directory' | FormatCategory;
  /** 图片类的 MIME 类型（构造 data URI 用） */
  mimeType?: string;
  /** 超过阈值需要用户确认 */
  needsSizeConfirm: boolean;
  /** hex 回退时的原因说明 */
  reason?: string;
}

/** 创建默认注册表（插件顺序即优先级：magic 精度高的在前） */
export function createDefaultRegistry(): FileFormatRegistry {
  const registry = new FileFormatRegistry();
  registry.register(pdfPlugin);    // %PDF- magic，最精确
  registry.register(imagePlugin);  // 图片 magic 表
  registry.register(textPlugin);   // isText 启发式兜底
  return registry;
}

/** 调用 file_info 协议探测远程文件格式 */
export async function detectFileFormat(serverId: string, path: string): Promise<RemoteFileInfo> {
  return invoke<RemoteFileInfo>('remote_file_info', { serverId, path });
}

/** 纯决策：根据探测结果决定打开目标（无副作用，便于测试） */
export function decideOpenTarget(info: RemoteFileInfo, registry: FileFormatRegistry): OpenDecision {
  // 目录：由 FileManager 导航，不属于应用路由
  if (info.isDir) {
    return { kind: 'directory', needsSizeConfirm: false };
  }

  const match = registry.detect(info);

  if (match === null) {
    // 未知格式回退：十六进制查看器（对齐原设计"HexViewer fallback"）
    return { kind: 'hex', needsSizeConfirm: false, reason: `未知格式（扩展名 .${info.extension || '无'}）` };
  }

  const limit = SIZE_LIMITS[match.category as keyof typeof SIZE_LIMITS];
  return {
    kind: match.category,
    mimeType: match.mimeType,
    needsSizeConfirm: info.size > limit,
  };
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `npm run test:run -- src/file-formats`
Expected: 全部通过。

- [ ] **Step 5: 提示用户提交**

告知用户：网关层完成，建议提交。**不执行 git 命令。**

---

### Task 8: FileManager 接入 FileOpener

**Files:**
- Modify: `src/apps/FileManager.tsx:756-779`（handleOpen 改造）

- [ ] **Step 1: 添加导入**

`src/apps/FileManager.tsx` 顶部 import 区追加：

```typescript
import { detectFileFormat, decideOpenTarget, createDefaultRegistry } from '../file-formats/FileOpener';
```

- [ ] **Step 2: 改造 handleOpen**

将 `src/apps/FileManager.tsx` L756-779 的 `handleOpen` 整体替换为：

```typescript
  // 双击/上下文菜单打开条目
  // 文件：经 FileOpener 网关探测格式后路由到对应应用（图片/PDF/文本/十六进制）
  const handleOpen = useCallback(async (entry: FileEntry) => {
    const sep = "/";
    const fullPath = currentPath === "/"
      ? `${currentPath}${sep}${entry.name}`
      : `${currentPath}${sep}${entry.name}`;

    // 目录：保持原有导航逻辑
    if (entry.is_dir) {
      navigateTo(fullPath);
      return;
    }

    // 未连接服务器：回退旧行为（直接开编辑器，由编辑器报错）
    if (!activeServerId) {
      manager.create('editor', { preloadData: { path: fullPath, serverId: undefined } });
      return;
    }

    try {
      // 数据层：探测格式 → 网关决策
      const info = await detectFileFormat(activeServerId, fullPath);
      const decision = decideOpenTarget(info, createDefaultRegistry());

      // 大文件确认（全量 base64 加载，内存峰值约为文件大小 × 2.3）
      if (decision.needsSizeConfirm) {
        const mb = (info.size / 1024 / 1024).toFixed(1);
        const ok = confirm(`文件较大（${mb} MB），加载可能需要一些时间。仍要打开吗？`);
        if (!ok) return;
      }

      switch (decision.kind) {
        case 'text':
          // 文本/代码 → TextEditor（保持原有行为）
          manager.create('editor', {
            serverId: activeServerId,
            preloadData: { path: fullPath, serverId: activeServerId },
          });
          break;
        case 'image':
          manager.create('image-viewer', {
            serverId: activeServerId,
            preloadData: { path: fullPath, serverId: activeServerId, mimeType: decision.mimeType },
          });
          break;
        case 'pdf':
          manager.create('pdf-viewer', {
            serverId: activeServerId,
            preloadData: { path: fullPath, serverId: activeServerId },
          });
          break;
        case 'hex':
          // 未知格式回退：十六进制查看器
          log.debug(`未知格式，回退十六进制查看器: ${fullPath} (${decision.reason})`);
          manager.create('hex-viewer', {
            serverId: activeServerId,
            preloadData: { path: fullPath, serverId: activeServerId },
          });
          break;
      }
    } catch (err) {
      log.error("格式探测失败，回退编辑器打开:", err);
      // 探测失败（如旧版 Agent 不支持 file_info）：回退旧行为，保证可用性
      manager.create('editor', {
        serverId: activeServerId,
        preloadData: { path: fullPath, serverId: activeServerId },
      });
    }
  }, [currentPath, navigateTo, manager, activeServerId]);
```

**注意：** `handleOpen` 从同步变异步。现有调用点（键盘 Enter、onDoubleClick、上下文菜单）均为 fire-and-forget 调用，async 化后无需改动调用点；若 lint 报 floating promise，调用点前缀 `void`。

- [ ] **Step 3: 类型检查**

Run: `npx tsc --noEmit`
Expected: 无类型错误（`manager.create` 的 appId 为 string 类型，应用注册在 Task 9-11 完成）。

- [ ] **Step 4: 提示用户提交**

告知用户：FileManager 已接入格式路由，建议提交。**不执行 git 命令。**

---

### Task 9: ImageViewer 应用

**Files:**
- Create: `src/apps/ImageViewer/ImageViewer.tsx`
- Create: `src/apps/ImageViewer/ImageViewer.css`
- Modify: `src/window-system/init.ts`

- [ ] **Step 1: 实现 ImageViewer.tsx**

创建 `src/apps/ImageViewer/ImageViewer.tsx`：

```typescript
/**
 * 远程图片查看器
 *
 * 数据流：remote_read_file_binary → base64 → data URI → <img> 浏览器原生解码
 * 功能：滚轮缩放、拖拽平移、适应窗口、1:1 原始尺寸
 * 支持：PNG/JPEG/GIF/WebP/BMP/ICO/SVG（浏览器原生解码格式）
 */

import { useState, useRef, useCallback, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { createLogger } from '../../utils/logger';
import './ImageViewer.css';

const log = createLogger('ImageViewer');

/** 预加载数据（FileManager 路由时传入） */
interface ImageViewerProps {
  windowId: string;
  preloadData?: {
    path: string;
    serverId: string;
    mimeType?: string;
  };
}

/** 二进制读取结果（与 Rust RemoteBinaryFile 对应） */
interface RemoteBinaryFile {
  path: string;
  base64: string;
  mtime: number;
  size: number;
}

export function ImageViewer({ preloadData }: ImageViewerProps) {
  const [dataUri, setDataUri] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  // 视图变换状态
  const [scale, setScale] = useState(1);
  const [offset, setOffset] = useState({ x: 0, y: 0 });
  // 拖拽状态
  const dragRef = useRef<{ startX: number; startY: number; baseX: number; baseY: number } | null>(null);

  const path = preloadData?.path;
  const serverId = preloadData?.serverId;
  const mimeType = preloadData?.mimeType || 'image/png';

  // 加载图片数据
  useEffect(() => {
    if (!path || !serverId) {
      setError('未指定文件或未连接服务器');
      setLoading(false);
      return;
    }

    let cancelled = false;
    (async () => {
      try {
        setLoading(true);
        setError(null);
        const result = await invoke<RemoteBinaryFile>('remote_read_file_binary', { serverId, path });
        if (cancelled) return;
        setDataUri(`data:${mimeType};base64,${result.base64}`);
        // 重置视图（新图片）
        setScale(1);
        setOffset({ x: 0, y: 0 });
      } catch (err) {
        if (cancelled) return;
        log.error('读取图片失败:', err);
        setError(`无法加载图片: ${err}`);
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();

    return () => { cancelled = true; };
  }, [path, serverId, mimeType]);

  // 滚轮缩放（0.05x - 20x）
  const handleWheel = useCallback((e: React.WheelEvent) => {
    e.preventDefault();
    setScale((s) => {
      const factor = e.deltaY < 0 ? 1.1 : 1 / 1.1;
      const next = s * factor;
      return Math.min(20, Math.max(0.05, next));
    });
  }, []);

  // 拖拽平移（mousedown 记录起点，window 级 mousemove/mouseup 保证拖出画布仍有效）
  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    dragRef.current = { startX: e.clientX, startY: e.clientY, baseX: offset.x, baseY: offset.y };
  }, [offset]);

  useEffect(() => {
    const handleMouseMove = (e: MouseEvent) => {
      if (!dragRef.current) return;
      setOffset({
        x: dragRef.current.baseX + (e.clientX - dragRef.current.startX),
        y: dragRef.current.baseY + (e.clientY - dragRef.current.startY),
      });
    };
    const handleMouseUp = () => { dragRef.current = null; };
    window.addEventListener('mousemove', handleMouseMove);
    window.addEventListener('mouseup', handleMouseUp);
    return () => {
      window.removeEventListener('mousemove', handleMouseMove);
      window.removeEventListener('mouseup', handleMouseUp);
    };
  }, []);

  // 重置视图
  const resetView = useCallback(() => { setScale(1); setOffset({ x: 0, y: 0 }); }, []);

  return (
    <div className="iv-root">
      {loading && <div className="iv-status">加载中…</div>}
      {error && (
        <div className="iv-error">
          <p>{error}</p>
          <p className="iv-error-path">{path}</p>
        </div>
      )}
      {!loading && !error && dataUri && (
        <>
          <div className="iv-toolbar">
            <span className="iv-filename" title={path}>{path?.split('/').pop()}</span>
            <span className="iv-zoom-label">{Math.round(scale * 100)}%</span>
            <button className="iv-btn" onClick={resetView} title="重置视图">适应</button>
            <button className="iv-btn" onClick={() => setScale(1)} title="原始尺寸">1:1</button>
          </div>
          <div className="iv-canvas" onWheel={handleWheel} onMouseDown={handleMouseDown}>
            <img
              src={dataUri}
              alt={path ?? ''}
              className="iv-image"
              draggable={false}
              style={{
                transform: `translate(${offset.x}px, ${offset.y}px) scale(${scale})`,
              }}
            />
          </div>
        </>
      )}
    </div>
  );
}
```

- [ ] **Step 2: 实现 ImageViewer.css**

创建 `src/apps/ImageViewer/ImageViewer.css`（Adwaita 风格，`--quirel-` 前缀主题变量，含降级值）：

```css
/*
   ImageViewer — 远程图片查看器（Adwaita 风格）
   使用 --quirel- 前缀主题变量，与其他应用保持一致
*/

.iv-root {
  display: flex;
  flex-direction: column;
  width: 100%;
  height: 100%;
  background: var(--quirel-window-bg, #242424);
  overflow: hidden;
  user-select: none;
}

.iv-status {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--quirel-dim-fg, #9a9996);
  font-size: 14px;
}

.iv-error {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  color: var(--quirel-error-fg, #e01b24);
  font-size: 14px;
  padding: 24px;
  text-align: center;
}

.iv-error-path {
  color: var(--quirel-dim-fg, #9a9996);
  font-size: 12px;
  word-break: break-all;
}

.iv-toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 12px;
  background: var(--quirel-headerbar-bg, #303030);
  border-bottom: 1px solid var(--quirel-border, #3f3f3f);
  flex-shrink: 0;
}

.iv-filename {
  flex: 1;
  font-size: 13px;
  color: var(--quirel-fg, #ffffff);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.iv-zoom-label {
  font-size: 12px;
  color: var(--quirel-dim-fg, #9a9996);
  min-width: 44px;
  text-align: right;
}

.iv-btn {
  padding: 4px 10px;
  font-size: 12px;
  color: var(--quirel-fg, #ffffff);
  background: var(--quirel-button-bg, #3f3f3f);
  border: 1px solid var(--quirel-border, #4f4f4f);
  border-radius: 6px;
  cursor: pointer;
}

.iv-btn:hover {
  background: var(--quirel-button-hover-bg, #4f4f4f);
}

.iv-canvas {
  flex: 1;
  overflow: hidden;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--quirel-view-bg, #1e1e1e);
  cursor: grab;
}

.iv-canvas:active {
  cursor: grabbing;
}

.iv-image {
  max-width: 100%;
  max-height: 100%;
  transform-origin: center center;
}
```

- [ ] **Step 3: 注册应用**

`src/window-system/init.ts`：

(a) 顶部 import 区追加：

```typescript
import { ImageViewer } from '../apps/ImageViewer/ImageViewer';
```

(b) BrowserApp 注册之后追加（格式路由目标应用，非用户主动入口，不显示在桌面/Dock）：

```typescript
  // Image Viewer - 远程图片查看（文件格式路由目标，非主动入口）
  registry.register({
    id: 'image-viewer',
    title: '图片查看器',
    icon: '🖼️',
    defaultSize: { width: 800, height: 600 },
    minSize: { width: 400, height: 300 },
    allowMultipleInstances: true,
    component: ImageViewer,
    showOnDesktop: false,
    showOnDock: false,
  });
```

- [ ] **Step 4: 类型检查 + 前端测试**

Run: `npx tsc --noEmit && npm run test:run`
Expected: 零类型错误；测试全部通过。

- [ ] **Step 5: 提示用户提交**

告知用户：ImageViewer 完成，建议提交。**不执行 git 命令。**

---

### Task 10: HexViewer 回退应用

**Files:**
- Create: `src/apps/HexViewer/HexViewer.tsx`
- Create: `src/apps/HexViewer/HexViewer.css`
- Modify: `src/window-system/init.ts`

- [ ] **Step 1: 实现 HexViewer.tsx**

创建 `src/apps/HexViewer/HexViewer.tsx`：

```typescript
/**
 * 十六进制查看器（未知格式回退）
 *
 * 数据流：remote_read_file_binary → base64 → Uint8Array → hexdump 视图
 * 只加载前 64KB（大文件全量 hexdump 无意义，浏览器渲染不动）
 * 显示格式对齐 hexdump -C：偏移 + 16 字节十六进制 + ASCII 列
 */

import { useState, useEffect, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { createLogger } from '../../utils/logger';
import './HexViewer.css';

const log = createLogger('HexViewer');

/** 最多加载的字节数（64KB） */
const MAX_BYTES = 64 * 1024;
/** 每行字节数 */
const BYTES_PER_LINE = 16;

interface HexViewerProps {
  windowId: string;
  preloadData?: {
    path: string;
    serverId: string;
  };
}

interface RemoteBinaryFile {
  path: string;
  base64: string;
  mtime: number;
  size: number;
}

/** 单行 hexdump 数据 */
interface HexLine {
  offset: number;
  hex: string;
  ascii: string;
}

/** Uint8Array → hexdump 行数组（截断到 MAX_BYTES） */
function buildHexLines(bytes: Uint8Array): HexLine[] {
  const lines: HexLine[] = [];
  const total = Math.min(bytes.length, MAX_BYTES);
  for (let base = 0; base < total; base += BYTES_PER_LINE) {
    const end = Math.min(base + BYTES_PER_LINE, total);
    const lineBytes = bytes.slice(base, end);

    // 十六进制列（不足 16 字节右侧留空对齐）
    const hexParts: string[] = [];
    for (let i = 0; i < BYTES_PER_LINE; i++) {
      hexParts.push(i < lineBytes.length ? lineBytes[i].toString(16).padStart(2, '0') : '  ');
    }

    // ASCII 列（可显示字符 32-126，其余显示 .）
    let ascii = '';
    for (let i = 0; i < lineBytes.length; i++) {
      const b = lineBytes[i];
      ascii += b >= 32 && b <= 126 ? String.fromCharCode(b) : '.';
    }

    lines.push({ offset: base, hex: hexParts.join(' '), ascii });
  }
  return lines;
}

/** base64 → Uint8Array */
function base64ToBytes(b64: string): Uint8Array {
  const binary = atob(b64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

export function HexViewer({ preloadData }: HexViewerProps) {
  const [lines, setLines] = useState<HexLine[]>([]);
  const [fileSize, setFileSize] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const path = preloadData?.path;
  const serverId = preloadData?.serverId;

  useEffect(() => {
    if (!path || !serverId) {
      setError('未指定文件或未连接服务器');
      setLoading(false);
      return;
    }

    let cancelled = false;
    (async () => {
      try {
        setLoading(true);
        setError(null);
        const result = await invoke<RemoteBinaryFile>('remote_read_file_binary', { serverId, path });
        if (cancelled) return;
        const bytes = base64ToBytes(result.base64);
        setLines(buildHexLines(bytes));
        setFileSize(result.size);
      } catch (err) {
        if (cancelled) return;
        log.error('读取文件失败:', err);
        setError(`无法读取文件: ${err}`);
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();

    return () => { cancelled = true; };
  }, [path, serverId]);

  // 文件超出展示范围的提示
  const truncated = useMemo(() => fileSize > MAX_BYTES, [fileSize]);

  return (
    <div className="hv-root">
      {loading && <div className="hv-status">加载中…</div>}
      {error && (
        <div className="hv-error">
          <p>{error}</p>
          <p className="hv-error-path">{path}</p>
        </div>
      )}
      {!loading && !error && (
        <>
          <div className="hv-toolbar">
            <span className="hv-filename" title={path}>{path?.split('/').pop()}</span>
            <span className="hv-size">{fileSize.toLocaleString()} 字节</span>
          </div>
          <div className="hv-content">
            {lines.map((line) => (
              <div key={line.offset} className="hv-line">
                <span className="hv-offset">{line.offset.toString(16).padStart(8, '0')}</span>
                <span className="hv-hex">{line.hex}</span>
                <span className="hv-ascii">|{line.ascii}|</span>
              </div>
            ))}
            {truncated && (
              <div className="hv-truncated">
                … 仅显示前 {MAX_BYTES / 1024} KB（共 {fileSize.toLocaleString()} 字节）
              </div>
            )}
          </div>
        </>
      )}
    </div>
  );
}
```

- [ ] **Step 2: 实现 HexViewer.css**

创建 `src/apps/HexViewer/HexViewer.css`：

```css
/*
   HexViewer — 十六进制查看器（未知格式回退，Adwaita 风格）
   等宽字体 hexdump 布局，对齐 hexdump -C 输出格式
*/

.hv-root {
  display: flex;
  flex-direction: column;
  width: 100%;
  height: 100%;
  background: var(--quirel-window-bg, #242424);
  overflow: hidden;
}

.hv-status {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--quirel-dim-fg, #9a9996);
  font-size: 14px;
}

.hv-error {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  color: var(--quirel-error-fg, #e01b24);
  font-size: 14px;
  padding: 24px;
  text-align: center;
}

.hv-error-path {
  color: var(--quirel-dim-fg, #9a9996);
  font-size: 12px;
  word-break: break-all;
}

.hv-toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 12px;
  background: var(--quirel-headerbar-bg, #303030);
  border-bottom: 1px solid var(--quirel-border, #3f3f3f);
  flex-shrink: 0;
}

.hv-filename {
  flex: 1;
  font-size: 13px;
  color: var(--quirel-fg, #ffffff);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.hv-size {
  font-size: 12px;
  color: var(--quirel-dim-fg, #9a9996);
}

.hv-content {
  flex: 1;
  overflow: auto;
  padding: 8px 12px;
  background: var(--quirel-view-bg, #1e1e1e);
  font-family: 'JetBrains Mono', 'Cascadia Code', Consolas, monospace;
  font-size: 12px;
  line-height: 1.5;
}

.hv-line {
  display: flex;
  gap: 12px;
  white-space: pre;
}

.hv-offset {
  color: var(--quirel-dim-fg, #9a9996);
}

.hv-hex {
  color: var(--quirel-fg, #ffffff);
}

.hv-ascii {
  color: var(--quirel-accent-fg, #99c1f1);
}

.hv-truncated {
  margin-top: 8px;
  color: var(--quirel-dim-fg, #9a9996);
  font-size: 12px;
}
```

- [ ] **Step 3: 注册应用**

`src/window-system/init.ts`：

(a) 顶部 import 区追加：

```typescript
import { HexViewer } from '../apps/HexViewer/HexViewer';
```

(b) ImageViewer 注册之后追加：

```typescript
  // Hex Viewer - 十六进制查看（未知格式回退，非主动入口）
  registry.register({
    id: 'hex-viewer',
    title: '十六进制查看器',
    icon: '🔢',
    defaultSize: { width: 800, height: 550 },
    minSize: { width: 500, height: 350 },
    allowMultipleInstances: true,
    component: HexViewer,
    showOnDesktop: false,
    showOnDock: false,
  });
```

- [ ] **Step 4: 类型检查 + 前端测试**

Run: `npx tsc --noEmit && npm run test:run`
Expected: 零类型错误；测试全部通过。

- [ ] **Step 5: 提示用户提交**

告知用户：HexViewer 完成，建议提交。**不执行 git 命令。**

---

### Task 11: PDFViewer 应用（pdfjs-dist + CSP 调整）

**Files:**
- Modify: `package.json`（pdfjs-dist 依赖）
- Modify: `src-tauri/tauri.conf.json`（CSP）
- Create: `src/apps/PDFViewer/PDFViewer.tsx`
- Create: `src/apps/PDFViewer/PDFViewer.css`
- Modify: `src/window-system/init.ts`

- [ ] **Step 1: 安装依赖**

Run: `npm install pdfjs-dist@^5`
Expected: 安装成功，package.json 出现 pdfjs-dist。

- [ ] **Step 2: 调整 CSP（pdfjs worker 需要）**

`src-tauri/tauri.conf.json` L27，将：

```json
"csp": "default-src 'self'; img-src 'self' data: https:; style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; font-src 'self' https://fonts.gstatic.com; connect-src 'self' ws://localhost:1420"
```

替换为：

```json
"csp": "default-src 'self'; img-src 'self' data: blob: https:; style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; font-src 'self' https://fonts.gstatic.com; connect-src 'self' ws://localhost:1420; worker-src 'self' blob:; object-src 'none'"
```

**说明：** `worker-src 'self' blob:` 允许 pdfjs 加载 Worker（Vite 会把 `?url` 导入的 worker 打包为 self 资源，blob: 作为 pdfjs 内部回退路径保险）。

- [ ] **Step 3: 实现 PDFViewer.tsx**

创建 `src/apps/PDFViewer/PDFViewer.tsx`：

```typescript
/**
 * 远程 PDF 阅读器
 *
 * 数据流：remote_read_file_binary → base64 → Uint8Array → pdfjs 渲染 canvas
 * 功能：分页导航（上一页/下一页/页码输入）、连续滚动
 * 渲染引擎：pdfjs-dist（浏览器内置 PDF 阅读器同源引擎）
 */

import { useState, useRef, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import * as pdfjsLib from 'pdfjs-dist';
// Vite 打包 worker 资源（CSP worker-src 'self' 允许）
import workerUrl from 'pdfjs-dist/build/pdf.worker.min.mjs?url';
import { createLogger } from '../../utils/logger';
import './PDFViewer.css';

// 配置 pdfjs worker（必须在任何 getDocument 调用前执行）
pdfjsLib.GlobalWorkerOptions.workerSrc = workerUrl;

const log = createLogger('PDFViewer');

interface PDFViewerProps {
  windowId: string;
  preloadData?: {
    path: string;
    serverId: string;
  };
}

interface RemoteBinaryFile {
  path: string;
  base64: string;
  mtime: number;
  size: number;
}

/** base64 → Uint8Array */
function base64ToBytes(b64: string): Uint8Array {
  const binary = atob(b64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

export function PDFViewer({ preloadData }: PDFViewerProps) {
  const [numPages, setNumPages] = useState(0);
  const [pageNum, setPageNum] = useState(1);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  // pdfjs 文档代理（非 React 状态，避免 Proxy 干扰 pdfjs 内部机制）
  const docRef = useRef<pdfjsLib.PDFDocumentProxy | null>(null);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  // 任务版本号（防止快速翻页时旧渲染覆盖新页面）
  const renderTaskRef = useRef<{ cancel: () => void; promise: Promise<void> } | null>(null);

  const path = preloadData?.path;
  const serverId = preloadData?.serverId;

  // 加载 PDF 文档
  useEffect(() => {
    if (!path || !serverId) {
      setError('未指定文件或未连接服务器');
      setLoading(false);
      return;
    }

    let cancelled = false;
    (async () => {
      try {
        setLoading(true);
        setError(null);
        const result = await invoke<RemoteBinaryFile>('remote_read_file_binary', { serverId, path });
        if (cancelled) return;

        const data = base64ToBytes(result.base64);
        // getDocument 会 transfer 底层 buffer，必须复制避免再次渲染失败
        const doc = await pdfjsLib.getDocument({ data: data.slice() }).promise;
        if (cancelled) {
          doc.destroy();
          return;
        }
        docRef.current = doc;
        setNumPages(doc.numPages);
        setPageNum(1);
      } catch (err) {
        if (cancelled) return;
        log.error('加载 PDF 失败:', err);
        setError(`无法加载 PDF: ${err}`);
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();

    return () => {
      cancelled = true;
      docRef.current?.destroy();
      docRef.current = null;
    };
  }, [path, serverId]);

  // 渲染当前页
  useEffect(() => {
    const doc = docRef.current;
    const canvas = canvasRef.current;
    if (!doc || !canvas || pageNum < 1 || pageNum > doc.numPages) return;

    let cancelled = false;
    (async () => {
      try {
        // 取消上一次未完成的渲染任务（快速翻页保护）
        renderTaskRef.current?.cancel();

        const page = await doc.getPage(pageNum);
        if (cancelled) return;

        // 适配容器宽度（2 倍物理像素，HiDPI 清晰渲染）
        const container = canvas.parentElement;
        const containerWidth = container ? container.clientWidth : 800;
        const baseViewport = page.getViewport({ scale: 1 });
        const scale = (containerWidth / baseViewport.width) * 2;
        const viewport = page.getViewport({ scale });

        canvas.width = viewport.width;
        canvas.height = viewport.height;
        canvas.style.width = `${viewport.width / 2}px`;
        canvas.style.height = `${viewport.height / 2}px`;

        const task = page.render({
          canvas,
          canvasContext: canvas.getContext('2d')!,
          viewport,
        });
        renderTaskRef.current = task;
        await task.promise;
      } catch (err) {
        // RenderingCancelledException 是正常流程（翻页取消），不作为错误处理
        if (!cancelled && !(err instanceof Error && err.name === 'RenderingCancelledException')) {
          log.error('渲染 PDF 页面失败:', err);
        }
      }
    })();

    return () => { cancelled = true; };
  }, [pageNum, numPages, loading]);

  // 翻页操作
  const goPrev = useCallback(() => setPageNum((p) => Math.max(1, p - 1)), []);
  const goNext = useCallback(() => setPageNum((p) => Math.min(numPages, p + 1)), [numPages]);
  const handlePageInput = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
    const n = parseInt(e.target.value, 10);
    if (!Number.isNaN(n) && n >= 1 && n <= numPages) {
      setPageNum(n);
    }
  }, [numPages]);

  return (
    <div className="pv-root">
      {loading && <div className="pv-status">加载中…</div>}
      {error && (
        <div className="pv-error">
          <p>{error}</p>
          <p className="pv-error-path">{path}</p>
        </div>
      )}
      {!loading && !error && numPages > 0 && (
        <>
          <div className="pv-toolbar">
            <span className="pv-filename" title={path}>{path?.split('/').pop()}</span>
            <button className="pv-btn" onClick={goPrev} disabled={pageNum <= 1}>‹</button>
            <input
              className="pv-page-input"
              type="number"
              min={1}
              max={numPages}
              value={pageNum}
              onChange={handlePageInput}
            />
            <span className="pv-page-total">/ {numPages}</span>
            <button className="pv-btn" onClick={goNext} disabled={pageNum >= numPages}>›</button>
          </div>
          <div className="pv-canvas">
            <canvas ref={canvasRef} />
          </div>
        </>
      )}
    </div>
  );
}
```

- [ ] **Step 4: 实现 PDFViewer.css**

创建 `src/apps/PDFViewer/PDFViewer.css`（Adwaita 风格，与其他查看器变量一致）：

```css
/*
   PDFViewer — 远程 PDF 阅读器（Adwaita 风格）
   工具栏 + 滚动画布布局，与其他查看器保持一致
*/

.pv-root {
  display: flex;
  flex-direction: column;
  width: 100%;
  height: 100%;
  background: var(--quirel-window-bg, #242424);
  overflow: hidden;
}

.pv-status {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--quirel-dim-fg, #9a9996);
  font-size: 14px;
}

.pv-error {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  color: var(--quirel-error-fg, #e01b24);
  font-size: 14px;
  padding: 24px;
  text-align: center;
}

.pv-error-path {
  color: var(--quirel-dim-fg, #9a9996);
  font-size: 12px;
  word-break: break-all;
}

.pv-toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 12px;
  background: var(--quirel-headerbar-bg, #303030);
  border-bottom: 1px solid var(--quirel-border, #3f3f3f);
  flex-shrink: 0;
}

.pv-filename {
  flex: 1;
  font-size: 13px;
  color: var(--quirel-fg, #ffffff);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.pv-btn {
  padding: 4px 10px;
  font-size: 12px;
  color: var(--quirel-fg, #ffffff);
  background: var(--quirel-button-bg, #3f3f3f);
  border: 1px solid var(--quirel-border, #4f4f4f);
  border-radius: 6px;
  cursor: pointer;
}

.pv-btn:hover:not(:disabled) {
  background: var(--quirel-button-hover-bg, #4f4f4f);
}

.pv-btn:disabled {
  opacity: 0.5;
  cursor: default;
}

.pv-page-input {
  width: 56px;
  padding: 4px 6px;
  font-size: 12px;
  text-align: center;
  color: var(--quirel-fg, #ffffff);
  background: var(--quirel-view-bg, #1e1e1e);
  border: 1px solid var(--quirel-border, #4f4f4f);
  border-radius: 6px;
}

.pv-page-total {
  font-size: 12px;
  color: var(--quirel-dim-fg, #9a9996);
}

.pv-canvas {
  flex: 1;
  overflow: auto;
  background: var(--quirel-view-bg, #1e1e1e);
  display: flex;
  justify-content: center;
  padding: 8px;
}
```

- [ ] **Step 5: 注册应用**

`src/window-system/init.ts`：

(a) 顶部 import 区追加：

```typescript
import { PDFViewer } from '../apps/PDFViewer/PDFViewer';
```

(b) HexViewer 注册之后追加：

```typescript
  // PDF Viewer - 远程 PDF 阅读（文件格式路由目标，非主动入口）
  registry.register({
    id: 'pdf-viewer',
    title: 'PDF 阅读器',
    icon: '📄',
    defaultSize: { width: 900, height: 700 },
    minSize: { width: 500, height: 400 },
    allowMultipleInstances: true,
    component: PDFViewer,
    showOnDesktop: false,
    showOnDock: false,
  });
```

- [ ] **Step 6: 类型检查 + 前端测试**

Run: `npx tsc --noEmit && npm run test:run`
Expected: 零类型错误；测试全部通过。

**注意：** 若 `?url` 导入报类型错误，确认 `src/vite-env.d.ts` 存在 `/// <reference types="vite/client" />`（Vite 类型声明覆盖 `*?url` 模块）。

- [ ] **Step 7: 提示用户提交**

告知用户：PDFViewer 完成，建议提交。**不执行 git 命令。**

---

### Task 12: 全量验证与手动冒烟

- [ ] **Step 1: Rust 全量测试（3 处）**

```
cargo test -p quirel-protocol                          # 协议 crate
wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && cargo test"   # Agent
cd src-tauri && cargo check                            # 客户端 Rust
```
Expected: 全部通过 / 零错误。

- [ ] **Step 2: 前端全量验证**

Run: `npx tsc --noEmit && npm run test:run`
Expected: 零类型错误，全部测试通过。

- [ ] **Step 3: 手动冒烟测试（需用户配合）**

提示用户连接服务器后验证以下场景：

| # | 场景 | 预期 |
|---|------|------|
| 1 | 双击 .png 图片 | 打开图片查看器，滚轮缩放/拖拽平移正常 |
| 2 | 双击 .jpg/.webp | 图片查看器正常显示 |
| 3 | 双击 .svg | 路由到图片查看器（非编辑器） |
| 4 | 双击 .pdf | PDF 阅读器打开，翻页正常 |
| 5 | 双击 .rs/.txt 源码 | TextEditor 打开（原行为保持） |
| 6 | 双击 GBK 编码 .txt | 编辑器正常显示中文（GB18030 回退） |
| 7 | 双击 .tar.gz 二进制 | 十六进制查看器显示 hexdump |
| 8 | 双击 >20MB 图片 | 弹出大小确认对话框 |
| 9 | 断开服务器后双击文件 | 回退编辑器并报错（旧行为兜底） |

- [ ] **Step 4: 提示用户提交**

告知用户：全部任务完成，建议整体提交（如 `feat: 文件格式识别与智能打开系统`）。**不执行 git 命令。**

---

## Self-Review 记录

1. **Spec 覆盖检查**：四层架构（检测/网关/渲染/数据）全部有对应任务；2026-09-02 设计中的图片/PDF/文本/hex 四种路由目标全部覆盖。
2. **协议单一真相源**：所有新协议类型定义在 quirel-protocol，agent 与 src-tauri 均引用共享 crate，无重复定义；wire_compat golden 测试锁定 JSON 字节格式。
3. **占位符扫描**：无 TODO/占位符残留。
4. **类型一致性**：Rust `RemoteFileInfo` 的 `#[serde(rename_all = "camelCase")]` 与前端 `RemoteFileInfo` 接口字段一一对应（isDir/isText/magicBytes）。
5. **向后兼容**：旧版 Agent 不支持 file_info 时 handleOpen catch 回退旧编辑器行为，保证降级可用。
6. **用户规则**：全部任务仅提示提交，不执行 git 操作；注释保留。