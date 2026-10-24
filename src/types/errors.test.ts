import { describe, it, expect } from "vitest";
import {
  AuthErrorCode,
  getErrorInfo,
  parseConnectError,
  buildConnectFailureText,
  describeTerminalFailure,
  isAppError,
  isPermissionDenied,
  formatOpError,
  formatTaskError,
  formatUnknownError,
} from "./errors";
import i18n from "../i18n";

describe("ERROR_SPEC 全覆盖（每个 code 必有条目）", () => {
  for (const code of Object.values(AuthErrorCode)) {
    it(`code ${code} 有映射条目`, () => {
      const info = getErrorInfo(code);
      expect(info.title.length).toBeGreaterThan(0);
      expect(info.message.length).toBeGreaterThan(0);
      expect(info.action.length).toBeGreaterThan(0);
      expect(typeof info.retryable).toBe("boolean");
    });
  }
});

describe("parseConnectError", () => {
  it("解析结构化 ConnectError JSON", () => {
    expect(parseConnectError({ code: 201, detail: "prod-1" })).toEqual({ code: 201, detail: "prod-1" });
  });
  it("未知数值回退 Unknown", () => {
    expect(parseConnectError({ code: 12345 }).code).toBe(AuthErrorCode.Unknown);
  });
  it("裸字符串 → Unknown + detail", () => {
    expect(parseConnectError("任意旧文本")).toEqual({ code: AuthErrorCode.Unknown, detail: "任意旧文本" });
  });
  it("Error 对象 → Unknown + message", () => {
    expect(parseConnectError(new Error("boom")).detail).toBe("boom");
  });
  it("结构化操作错误（AppError）→ Unknown + 词典文案", () => {
    expect(parseConnectError({ code: "permission_denied", params: {}, detail: undefined }))
      .toEqual({ code: AuthErrorCode.Unknown, detail: "权限不足" });
  });
});

describe("buildConnectFailureText", () => {
  it("包含标题/消息/建议", () => {
    const text = buildConnectFailureText({ code: AuthErrorCode.AccountLocked }, 0);
    expect(text).toContain("账户已临时锁定");
    expect(text).toContain("15 分钟");
    expect(text).toContain("建议：");
  });
  it("其他服务器正常时附加对比提示", () => {
    const text = buildConnectFailureText({ code: AuthErrorCode.ConnectionLost }, 2);
    expect(text).toContain("其他 2 台服务器连接正常");
  });
  it("无其他服务器时不附加对比提示", () => {
    expect(buildConnectFailureText({ code: AuthErrorCode.ConnectionLost }, 0)).not.toContain("台服务器连接正常");
  });
});

describe("describeTerminalFailure", () => {
  it("未连接时给出连接指引", () => {
    expect(describeTerminalFailure("未找到连接")).toContain("先连接");
  });
  it("未知错误保留原文", () => {
    expect(describeTerminalFailure("启动终端失败: xyz")).toBe("启动终端失败: xyz");
  });
  it("结构化 connection_not_found → 连接指引", () => {
    expect(describeTerminalFailure({ code: "connection_not_found", params: {}, detail: undefined }))
      .toContain("先连接");
  });
  it("结构化链路错误（connection_lost）→ 词典文案", () => {
    expect(describeTerminalFailure({ code: "connection_lost", params: {}, detail: undefined }))
      .toBe("与服务器的连接已断开");
  });
});

describe("isPermissionDenied", () => {
  it("permission_denied 直接判定", () => {
    expect(isPermissionDenied({ code: "permission_denied", params: {}, detail: undefined })).toBe(true);
  });
  it("agent_rejected 按透传 message 子串判定（中文/英文）", () => {
    expect(isPermissionDenied({ code: "agent_rejected", params: { message: "权限不足：/etc/shadow" }, detail: undefined })).toBe(true);
    expect(isPermissionDenied({ code: "agent_rejected", params: { message: "Permission denied: /etc/shadow" }, detail: undefined })).toBe(true);
    expect(isPermissionDenied({ code: "agent_rejected", params: { message: "文件不存在" }, detail: undefined })).toBe(false);
  });
  it("其他 code 不是权限错误", () => {
    expect(isPermissionDenied({ code: "file_not_found", params: {}, detail: undefined })).toBe(false);
  });
  it("字符串兜底：旧版纯文本子串匹配", () => {
    expect(isPermissionDenied("chmod 失败: 权限不足")).toBe(true);
    expect(isPermissionDenied("chmod failed: Permission denied")).toBe(true);
    expect(isPermissionDenied("路径不存在")).toBe(false);
  });
});

describe("i18n 集成", () => {
  it("语言切换后错误文案跟随（en-US）", async () => {
    await i18n.changeLanguage("en-US");
    expect(getErrorInfo(AuthErrorCode.DnsFailed).title).toBe("Cannot resolve server address");
    await i18n.changeLanguage("zh-CN");
    expect(getErrorInfo(AuthErrorCode.DnsFailed).title).toBe("服务器地址无法解析");
  });
});

describe("isAppError", () => {
  it("字符串 code 识别为 AppError", () => {
    expect(isAppError({ code: "transfer_timeout" })).toBe(true);
    expect(isAppError({ code: "transfer_timeout", params: { length: "42" }, detail: "x" })).toBe(true);
  });
  it("数值 code（ConnectError）不是 AppError", () => {
    expect(isAppError({ code: 201 })).toBe(false);
  });
  it("字符串/Error/null 不是 AppError", () => {
    expect(isAppError("plain")).toBe(false);
    expect(isAppError(new Error("boom"))).toBe(false);
    expect(isAppError(null)).toBe(false);
  });
});

describe("formatOpError", () => {
  it("已知 code → 词典文案", () => {
    expect(formatOpError({ code: "transfer_timeout" })).toBe("服务器响应超时，传输已停止");
    expect(formatOpError({ code: "transfer_network_failed" })).toBe("与服务器之间的数据传输中断");
  });
  it("params 参与插值", () => {
    expect(formatOpError({ code: "transfer_invalid_direction", params: { direction: "sideways" } }))
      .toBe("无效的传输方向（sideways）");
  });
  it("Agent 拒绝经 message 参数透传远端语义", () => {
    expect(formatOpError({ code: "transfer_agent_rejected", params: { message: "远程文件不存在: /a/b" } }))
      .toBe("服务器拒绝了传输请求：远程文件不存在: /a/b");
  });
  it("未知 code 兜底 operationFailed", () => {
    expect(formatOpError({ code: "some_future_code" })).toBe("操作失败");
  });
  it("detail 不进用户正文", () => {
    const text = formatOpError({ code: "transfer_timeout", detail: "elapsed 60s on recv" });
    expect(text).not.toContain("60s");
  });
});

describe("formatTaskError", () => {
  it("AppError JSON → 词典文案", () => {
    expect(formatTaskError('{"code":"transfer_interrupted"}'))
      .toBe("传输已中断，可切回该服务器后重试");
  });
  it("旧版纯文本记录原样透传", () => {
    expect(formatTaskError("读取数据帧失败: connection closed")).toBe("读取数据帧失败: connection closed");
  });
  it("空值返回 undefined", () => {
    expect(formatTaskError(undefined)).toBeUndefined();
    expect(formatTaskError(null)).toBeUndefined();
    expect(formatTaskError("")).toBeUndefined();
  });
  it("普通 JSON 字符串（非 AppError）原样透传", () => {
    expect(formatTaskError('{"foo":1}')).toBe('{"foo":1}');
  });
});

describe("formatUnknownError", () => {
  it("AppError 对象 → 词典文案（不再产生 [object Object]）", () => {
    expect(formatUnknownError({ code: "transfer_task_not_found" })).toBe("传输任务不存在");
  });
  it("裸字符串/Error 保留原文", () => {
    expect(formatUnknownError("旧文本")).toBe("旧文本");
    expect(formatUnknownError(new Error("boom"))).toBe("boom");
  });
  it("结构化 ConnectError → 连接失败结构化文案", () => {
    expect(formatUnknownError({ code: AuthErrorCode.AccountLocked })).toContain("账户已临时锁定");
  });
});

describe("操作错误词典（zh/en 结构一致 + code 全覆盖）", () => {
  const codes = [
    "connectionNotFound", "permissionDenied", "frameLengthInvalid", "frameWriteFailed",
    "fileNotFound", "ioFailed", "authFailed", "operationFailed",
    "transferTaskNotFound", "transferInvalidState", "transferServerNotConnected",
    "transferQuicUnavailable", "transferInvalidDirection", "transferNetworkFailed",
    "transferTimeout", "transferLocalFileFailed", "transferSaveFailed",
    "transferUploadIncomplete", "transferAgentRejected", "transferProtocolUnexpected",
    "transferCancelled", "transferTaskDeleted", "transferInterrupted",
    "connectionLost", "networkFailed", "responseTimeout",
    "agentRejected", "unexpectedResponse", "fileNotUtf8",
  ];
  it("errors.op 覆盖全部已发布 code 且非空（zh）", () => {
    for (const key of codes) {
      const text = i18n.t(`errors.op.${key}`);
      expect(text, `errors.op.${key} 缺失`).not.toBe(`errors.op.${key}`);
      expect(text.length).toBeGreaterThan(0);
    }
  });
  it("Rust snake_case code → 词典渲染全覆盖（经 formatOpError）", () => {
    const snakeCodes = [
      "connection_not_found", "permission_denied", "frame_length_invalid", "frame_write_failed",
      "file_not_found", "io_failed", "auth_failed", "operation_failed",
      "transfer_task_not_found", "transfer_invalid_state", "transfer_server_not_connected",
      "transfer_quic_unavailable", "transfer_invalid_direction", "transfer_network_failed",
      "transfer_timeout", "transfer_local_file_failed", "transfer_save_failed",
      "transfer_upload_incomplete", "transfer_agent_rejected", "transfer_protocol_unexpected",
      "transfer_cancelled", "transfer_task_deleted", "transfer_interrupted",
      "connection_lost", "network_failed", "response_timeout",
      "agent_rejected", "unexpected_response", "file_not_utf8",
      "terminal_session_not_found",
      "proxy_port_conflict", "proxy_listen_failed", "proxy_browser_not_found",
      "proxy_browser_not_installed", "proxy_browser_path_invalid",
      "proxy_browser_spawn_failed", "proxy_browser_in_use",
      "proxy_browser_env_failed", "proxy_browser_window_failed", "proxy_browser_window_closed",
      "proxy_cleanup_failed",
    ];
    for (const code of snakeCodes) {
      const text = formatOpError({ code, params: {}, detail: undefined });
      // operation_failed 自身就是兜底码，断言专属文案会自相矛盾，单独排除
      if (code === "operation_failed") continue;
      // 未命中词典会回退 operationFailed，此处要求每个 code 都有专属文案
      expect(text, `${code} 未命中专属词典文案`).not.toBe("操作失败");
    }
  });
  it("带参数错误经词典插值（proxy / terminal 新码）", () => {
    expect(formatOpError({ code: "proxy_port_conflict", params: { port: "1080", holder: "server-B" }, detail: undefined }))
      .toBe("端口 1080 已被服务器 server-B 的浏览会话占用，请先关闭其浏览窗口或在设置中更换端口");
    expect(formatOpError({ code: "proxy_browser_not_installed", params: { browser: "firefox" }, detail: undefined }))
      .toBe("所选浏览器（firefox）未安装");
    expect(formatOpError({ code: "terminal_session_not_found", params: {}, detail: undefined }))
      .toBe("终端会话不存在或已关闭");
  });
  it("语言切换后 op 文案跟随（en-US）", async () => {
    await i18n.changeLanguage("en-US");
    expect(i18n.t("errors.op.transferTimeout")).toBe("The server response timed out, the transfer has stopped");
    expect(i18n.t("errors.op.agentRejected", { message: "x" })).toBe("The server could not complete the operation: x");
    expect(i18n.t("errors.op.fileNotUtf8")).toBe("The file is not valid UTF-8 text (it may be binary)");
    await i18n.changeLanguage("zh-CN");
  });
});
