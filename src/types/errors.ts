/**
 * 连接错误码（与 quirel-protocol 的 AuthErrorCode #[repr(u16)] 数值对应）
 * 数值语义不可变更：新增只能追加
 */

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

/* ── 映射表（全项目唯一用户文案源，遵循「三不暴露」：不暴露协议名词/库错误原文/内部机制） ── */

export const ERROR_MAP = {
  [AuthErrorCode.MissingCredentials]: { title: "缺少登录信息", message: "未提供所需的登录凭据", action: "请补全服务器登录配置", retryable: false, severity: "normal" },
  [AuthErrorCode.InvalidKeyFormat]: { title: "密钥格式不支持", message: "该密钥文件的格式不受支持", action: "请使用 OpenSSH 格式的密钥文件", retryable: false, severity: "normal" },
  [AuthErrorCode.KeyParseFailed]: { title: "密钥无法读取", message: "密钥文件无法解析，可能已损坏或密码错误", action: "请确认密钥文件与密码", retryable: false, severity: "normal" },
  [AuthErrorCode.CertificateRejected]: { title: "未信任服务器", message: "服务器证书未获信任，连接已取消", action: "如需连接请重新发起并确认证书", retryable: false, severity: "normal" },
  [AuthErrorCode.DnsFailed]: { title: "服务器地址无法解析", message: "找不到该服务器的网络地址", action: "请检查服务器地址配置", retryable: false, severity: "critical" },
  [AuthErrorCode.ConnectTimeout]: { title: "无法连接服务器", message: "连接超时，未能建立连接", action: "请检查网络后重试", retryable: true, severity: "normal" },
  [AuthErrorCode.TlsHandshakeFailed]: { title: "安全连接建立失败", message: "无法与服务器建立安全连接", action: "请检查网络或稍后重试", retryable: true, severity: "normal" },
  [AuthErrorCode.NetworkUnreachable]: { title: "网络不可达", message: "当前网络无法到达该服务器", action: "请检查网络连接", retryable: true, severity: "normal" },
  [AuthErrorCode.StreamTimeout]: { title: "认证超时", message: "认证过程耗时过长", action: "请重新连接", retryable: true, severity: "low" },
  [AuthErrorCode.ConnectionLost]: { title: "网络连接中断", message: "与服务器的连接已中断", action: "请检查网络后重试", retryable: true, severity: "normal" },
  [AuthErrorCode.RateLimited]: { title: "请求过于频繁", message: "短时间内连接请求过多，服务器暂时限制了访问", action: "请稍候片刻再试", retryable: true, severity: "normal" },
  [AuthErrorCode.AccountLocked]: { title: "账户已临时锁定", message: "出于安全考虑，多次认证失败后账户被暂时锁定", action: "请于 15 分钟后重试", retryable: false, severity: "critical" },
  [AuthErrorCode.InvalidCredentials]: { title: "用户名或密码错误", message: "服务器拒绝了当前凭据", action: "请检查后重试", retryable: false, severity: "critical" },
  [AuthErrorCode.PubkeyNotAuthorized]: { title: "密钥未获授权", message: "该密钥未被服务器授权登录", action: "请在服务器上添加公钥授权", retryable: false, severity: "critical" },
  [AuthErrorCode.SignatureVerificationFailed]: { title: "密钥验证失败", message: "密钥与服务器记录不匹配", action: "请确认使用正确的密钥文件", retryable: false, severity: "critical" },
  [AuthErrorCode.ChallengeExpired]: { title: "认证超时", message: "认证流程耗时过长，已失效", action: "重新连接即可", retryable: true, severity: "low" },
  [AuthErrorCode.AuthServiceUnavailable]: { title: "服务暂时不可用", message: "服务器暂时无法处理登录请求", action: "请稍后重试", retryable: true, severity: "normal" },
  [AuthErrorCode.ProtocolError]: { title: "通信异常", message: "与服务器的通信出现异常", action: "请更新客户端后重试", retryable: false, severity: "critical" },
  [AuthErrorCode.SessionExpired]: { title: "会话已超时", message: "长时间未操作，会话已结束", action: "请重新连接", retryable: true, severity: "low" },
  [AuthErrorCode.Unknown]: { title: "连接失败", message: "发生未知错误", action: "请重试", retryable: false, severity: "critical" },
} as const satisfies Record<AuthErrorCode, ErrorInfo>;

/** 查表（未识别 code 回退 Unknown） */
export function getErrorInfo(code: number): ErrorInfo {
  return (ERROR_MAP as Record<number, ErrorInfo>)[code] ?? ERROR_MAP[AuthErrorCode.Unknown];
}

/* ── 解析与格式化 ────────────────────────────────────── */

/** 解析 invoke reject 的错误（结构化 ConnectError JSON 或裸字符串） */
export function parseConnectError(err: unknown): { code: number; detail?: string } {
  if (
    typeof err === "object" && err !== null && "code" in err &&
    typeof (err as { code: unknown }).code === "number"
  ) {
    const e = err as { code: number; detail?: string | null };
    const known = (ERROR_MAP as Record<number, ErrorInfo>)[e.code] !== undefined;
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
  text += `\n建议：${info.action}`;
  if (othersConnected > 0) {
    text += `\n\nℹ️ 其他 ${othersConnected} 台服务器连接正常，仅此台无法连接\n可能是本机与该服务器之间的网络问题，建议更换网络环境后重试`;
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
  if (msg.includes("未找到连接") || msg.includes("未找到该服务器的连接")) {
    return "尚未连接到服务器，请先连接后再打开终端";
  }
  // 结构化连接错误 → 完整结构化文案；终端本地错误（如进程启动失败）保留原文
  if (isStructuredConnectError(err)) {
    return buildConnectFailureText(err, 0);
  }
  return msg;
}
