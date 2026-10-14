/**
 * 连接错误码（与 quirel-protocol 的 AuthErrorCode #[repr(u16)] 数值对应）
 * 数值语义不可变更：新增只能追加
 */

import i18n from "../i18n";

/* ── 错误码常量 ──────────────────────────────────────── */

export const AuthErrorCode = {
  // 1-99：客户端本地错误
  MissingCredentials: 1,
  InvalidKeyFormat: 2,
  KeyParseFailed: 3,
  CertificateRejected: 4,
  // 100-199：网络/传输阶段
  DnsFailed: 101,
  ConnectTimeout: 102,
  TlsHandshakeFailed: 103,
  NetworkUnreachable: 104,
  StreamTimeout: 105,
  ConnectionLost: 106,
  // 200-299：Agent 认证拒绝
  RateLimited: 200,
  AccountLocked: 201,
  InvalidCredentials: 202,
  PubkeyNotAuthorized: 203,
  SignatureVerificationFailed: 204,
  ChallengeExpired: 205,
  AuthServiceUnavailable: 206,
  ProtocolError: 207,
  // 300-399：会话生命周期
  SessionExpired: 301,
  // 兜底
  Unknown: 999,
} as const;

export type AuthErrorCode = (typeof AuthErrorCode)[keyof typeof AuthErrorCode];

/* ── 映射表条目类型 ──────────────────────────────────── */

export interface ErrorInfo {
  /** 通知标题 */
  title: string;
  /** 用户可读消息（不含实现细节） */
  message: string;
  /** 行动建议 */
  action: string;
  /** true → 自动重连状态机可续期；false → 确定性失败不重试（避免触发服务端锁定） */
  retryable: boolean;
  /** 通知严重度（映射到通知中心 urgency） */
  severity: "critical" | "normal" | "low";
}

/* ── 映射表（非文案元数据；title/message/action 文案在词典 errors.auth.<key>.*） ── */

/** 每个错误码的非文案元数据 */
const ERROR_SPEC: Record<AuthErrorCode, { key: string; retryable: boolean; severity: ErrorInfo["severity"] }> = {
  [AuthErrorCode.MissingCredentials]: { key: "missingCredentials", retryable: false, severity: "normal" },
  [AuthErrorCode.InvalidKeyFormat]: { key: "invalidKeyFormat", retryable: false, severity: "normal" },
  [AuthErrorCode.KeyParseFailed]: { key: "keyParseFailed", retryable: false, severity: "normal" },
  [AuthErrorCode.CertificateRejected]: { key: "certificateRejected", retryable: false, severity: "normal" },
  [AuthErrorCode.DnsFailed]: { key: "dnsFailed", retryable: false, severity: "critical" },
  [AuthErrorCode.ConnectTimeout]: { key: "connectTimeout", retryable: true, severity: "normal" },
  [AuthErrorCode.TlsHandshakeFailed]: { key: "tlsHandshakeFailed", retryable: true, severity: "normal" },
  [AuthErrorCode.NetworkUnreachable]: { key: "networkUnreachable", retryable: true, severity: "normal" },
  [AuthErrorCode.StreamTimeout]: { key: "streamTimeout", retryable: true, severity: "low" },
  [AuthErrorCode.ConnectionLost]: { key: "connectionLost", retryable: true, severity: "normal" },
  [AuthErrorCode.RateLimited]: { key: "rateLimited", retryable: true, severity: "normal" },
  [AuthErrorCode.AccountLocked]: { key: "accountLocked", retryable: false, severity: "critical" },
  [AuthErrorCode.InvalidCredentials]: { key: "invalidCredentials", retryable: false, severity: "critical" },
  [AuthErrorCode.PubkeyNotAuthorized]: { key: "pubkeyNotAuthorized", retryable: false, severity: "critical" },
  [AuthErrorCode.SignatureVerificationFailed]: { key: "signatureVerificationFailed", retryable: false, severity: "critical" },
  [AuthErrorCode.ChallengeExpired]: { key: "challengeExpired", retryable: true, severity: "low" },
  [AuthErrorCode.AuthServiceUnavailable]: { key: "authServiceUnavailable", retryable: true, severity: "normal" },
  [AuthErrorCode.ProtocolError]: { key: "protocolError", retryable: false, severity: "critical" },
  [AuthErrorCode.SessionExpired]: { key: "sessionExpired", retryable: true, severity: "low" },
  [AuthErrorCode.Unknown]: { key: "unknown", retryable: false, severity: "critical" },
};

/** 查表（未识别 code 回退 Unknown），文案经 i18n 取当前语言 */
export function getErrorInfo(code: number): ErrorInfo {
  const spec = (ERROR_SPEC as Record<number, (typeof ERROR_SPEC)[AuthErrorCode]>)[code] ?? ERROR_SPEC[AuthErrorCode.Unknown];
  const t = i18n.t.bind(i18n);
  return {
    title: t(`errors.auth.${spec.key}.title`),
    message: t(`errors.auth.${spec.key}.message`),
    action: t(`errors.auth.${spec.key}.action`),
    retryable: spec.retryable,
    severity: spec.severity,
  };
}

/* ── 解析与格式化 ────────────────────────────────────── */

/** 解析 invoke reject 的错误（结构化 ConnectError JSON、结构化操作错误或裸字符串） */
export function parseConnectError(err: unknown): { code: number; detail?: string } {
  if (isAppError(err)) {
    // 结构化操作错误：映射到最接近的展示路径（detail 不作正文，未知时兜底 Unknown）
    return { code: AuthErrorCode.Unknown, detail: formatOpError(err) };
  }
  if (
    typeof err === "object" && err !== null && "code" in err &&
    typeof (err as { code: unknown }).code === "number"
  ) {
    const e = err as { code: number; detail?: string | null };
    const known = (ERROR_SPEC as Record<number, unknown>)[e.code] !== undefined;
    return { code: known ? e.code : AuthErrorCode.Unknown, detail: e.detail ?? undefined };
  }
  // 裸字符串（防御路径）：保持原文本作 detail，分类 Unknown
  const msg = err instanceof Error ? err.message : String(err);
  return { code: AuthErrorCode.Unknown, detail: msg };
}

/** 判断连接错误是否可重试（自动重连状态机门控） */
export function isRetryableConnectError(err: unknown): boolean {
  return getErrorInfo(parseConnectError(err).code).retryable;
}

/**
 * 构建连接失败的展示文本（标题/消息/上下文/建议）。
 * othersConnected > 0 时附加多服务器对比提示（只读已有状态，零探测）。
 */
export function buildConnectFailureText(err: unknown, othersConnected: number): string {
  const { code, detail } = parseConnectError(err);
  const info = getErrorInfo(code);
  let text = `${info.title}\n${info.message}`;
  if (detail && detail !== info.title) text += `\n${detail}`;
  text += `\n${i18n.t("errors.actionPrefix")}：${info.action}`;
  if (othersConnected > 0) {
    text += `\n\n${i18n.t("errors.othersConnected", { count: othersConnected })}`;
  }
  return text;
}

/** 判断是否为结构化 ConnectError（含数值 code 字段） */
function isStructuredConnectError(err: unknown): err is { code: number; detail?: string | null } {
  return (
    typeof err === "object" && err !== null && "code" in err &&
    typeof (err as { code: unknown }).code === "number"
  );
}

/* ── 结构化操作错误（Rust AppError：{ code, params?, detail? }）────── */

/** AppError 跨 invoke 边界的形状（serde 序列化） */
export interface AppErrorShape {
  code: string;
  params?: Record<string, string>;
  detail?: string;
}

/** 判断是否为结构化操作错误（字符串 code 字段，区别于数值 code 的 ConnectError） */
export function isAppError(err: unknown): err is AppErrorShape {
  return (
    typeof err === "object" && err !== null && "code" in err &&
    typeof (err as { code: unknown }).code === "string"
  );
}

/** snake_case code → 词典键（errors.<key>），与 src-tauri/src/errors.rs 的 codes 模块一一对应 */
const OP_ERROR_KEY_MAP: Record<string, string> = {
  connection_not_found: "op.connectionNotFound",
  permission_denied: "op.permissionDenied",
  frame_length_invalid: "op.frameLengthInvalid",
  frame_write_failed: "op.frameWriteFailed",
  file_not_found: "op.fileNotFound",
  io_failed: "op.ioFailed",
  auth_failed: "op.authFailed",
  operation_failed: "op.operationFailed",
  transfer_task_not_found: "op.transferTaskNotFound",
  transfer_invalid_state: "op.transferInvalidState",
  transfer_server_not_connected: "op.transferServerNotConnected",
  transfer_quic_unavailable: "op.transferQuicUnavailable",
  transfer_invalid_direction: "op.transferInvalidDirection",
  transfer_network_failed: "op.transferNetworkFailed",
  transfer_timeout: "op.transferTimeout",
  transfer_local_file_failed: "op.transferLocalFileFailed",
  transfer_save_failed: "op.transferSaveFailed",
  transfer_upload_incomplete: "op.transferUploadIncomplete",
  transfer_agent_rejected: "op.transferAgentRejected",
  transfer_protocol_unexpected: "op.transferProtocolUnexpected",
  transfer_cancelled: "op.transferCancelled",
  transfer_task_deleted: "op.transferTaskDeleted",
  transfer_interrupted: "op.transferInterrupted",
  connection_lost: "op.connectionLost",
  network_failed: "op.networkFailed",
  response_timeout: "op.responseTimeout",
  agent_rejected: "op.agentRejected",
  unexpected_response: "op.unexpectedResponse",
  file_not_utf8: "op.fileNotUtf8",
  terminal_session_not_found: "op.terminalSessionNotFound",
  proxy_port_conflict: "op.proxyPortConflict",
  proxy_listen_failed: "op.proxyListenFailed",
  proxy_browser_not_found: "op.proxyBrowserNotFound",
  proxy_browser_not_installed: "op.proxyBrowserNotInstalled",
  proxy_browser_path_invalid: "op.proxyBrowserPathInvalid",
  proxy_browser_spawn_failed: "op.proxyBrowserSpawnFailed",
  proxy_browser_in_use: "op.proxyBrowserInUse",
  proxy_cleanup_failed: "op.proxyCleanupFailed",
};

/** 未识别 code 的兜底词典键（与 Rust 端 OPERATION_FAILED 语义一致） */
const OP_ERROR_FALLBACK_KEY = "op.operationFailed";

/** 将结构化操作错误格式化为用户文案（params 参与插值；detail 仅供日志，不进正文） */
export function formatOpError(err: AppErrorShape): string {
  const key = OP_ERROR_KEY_MAP[err.code] ?? OP_ERROR_FALLBACK_KEY;
  return i18n.t(`errors.${key}`, err.params ?? {});
}

/** 传输任务记录的 error 字段（AppError JSON 或旧版纯文本）→ 用户文案 */
export function formatTaskError(error: string | undefined | null): string | undefined {
  if (!error) return undefined;
  try {
    const parsed = JSON.parse(error) as unknown;
    if (isAppError(parsed)) return formatOpError(parsed);
  } catch {
    // 非合法 JSON：旧版纯文本记录，原样透传
  }
  return error;
}

/** 任意 unknown 错误的统一格式化（invoke 边界兜底：裸字符串/Error/AppError/ConnectError） */
export function formatUnknownError(err: unknown): string {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  if (isAppError(err)) return formatOpError(err);
  if (isStructuredConnectError(err)) return buildConnectFailureText(err, 0);
  return String(err);
}

/**
 * 判断错误是否为权限不足（Linux 权限语义，内核拒绝）。
 * 结构化路径：permission_denied 直接判定；agent_rejected 时按 Agent 透传的
 * 原始 message 子串判定（Agent 端权限错误随 agent_rejected 透传）。
 * 字符串兜底路径：旧版纯文本错误的子串匹配。
 * 注意：子串匹配是运行时判定逻辑（识别内核错误），非展示文案。
 */
export function isPermissionDenied(err: unknown): boolean {
  const matches = (s: string) => s.includes("权限不足") || s.includes("Permission denied") || s.includes("permission denied");
  if (isAppError(err)) {
    if (err.code === "permission_denied") return true;
    if (err.code === "agent_rejected") return matches(err.params?.message ?? "");
    return false;
  }
  const msg = err instanceof Error ? err.message : String(err);
  return matches(msg);
}

/** 终端初始化失败的降级文案（含未连接场景指引） */
export function describeTerminalFailure(err: unknown): string {
  // 结构化操作错误：未连接场景给专门指引，其余走词典渲染
  if (isAppError(err)) {
    if (err.code === "connection_not_found") {
      return i18n.t("errors.terminalNotConnected");
    }
    return formatOpError(err);
  }
  const msg = err instanceof Error ? err.message : String(err);
  // 裸字符串兜底（旧版 Agent / 非结构化路径）
  if (msg.includes("未找到连接") || msg.includes("未找到该服务器的连接")) {
    return i18n.t("errors.terminalNotConnected");
  }
  // 结构化连接错误 → 完整结构化文案；终端本地错误（如进程启动失败）保留原文
  if (isStructuredConnectError(err)) {
    return buildConnectFailureText(err, 0);
  }
  return msg;
}
