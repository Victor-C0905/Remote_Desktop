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

/** 解析 invoke reject 的错误（结构化 ConnectError JSON 或裸字符串） */
export function parseConnectError(err: unknown): { code: number; detail?: string } {
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

/** 终端初始化失败的降级文案（含未连接场景指引） */
export function describeTerminalFailure(err: unknown): string {
  const msg = err instanceof Error ? err.message : String(err);
  // 注意：此处中文子串匹配是运行时逻辑而非文案，Task 17 改为按错误码判断，本任务保持行为不变
  if (msg.includes("未找到连接") || msg.includes("未找到该服务器的连接")) {
    return i18n.t("errors.terminalNotConnected");
  }
  // 结构化连接错误 → 完整结构化文案；终端本地错误（如进程启动失败）保留原文
  if (isStructuredConnectError(err)) {
    return buildConnectFailureText(err, 0);
  }
  return msg;
}
