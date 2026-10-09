// src/components/TransferLimitCard.tsx
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { createLogger } from "../utils/logger";

const log = createLogger("TransferLimitCard");

/** 值域与 Agent 端一致（1 MB – 100 GB；Agent 侧是权威校验，此处仅前置拦截） */
const MIN_LIMIT_MB = 1;
const MAX_LIMIT_MB = 102400;

/** Agent 返回的限制信息（snake_case 字段，与 StatsResponse 命名惯例一致） */
interface TransferLimitInfo {
  max_file_transfer_mb: number;
  editable: boolean;
  persisted: boolean;
}

/** 查询阶段状态（决定输入可用性与 hint 文案） */
type LimitState =
  | { kind: "disconnected" } // 未连接
  | { kind: "unsupported" }  // 旧 Agent（认证未上报 transfer_limit 能力）
  | { kind: "loading" }      // 查询中
  | { kind: "error"; message: string } // 查询失败
  | { kind: "ready"; editable: boolean }; // 已回填（editable=当前会话是否 root）

/** 应用结果提示（与查询状态分离，输入变更时清除） */
interface Feedback {
  text: string;
  warn: boolean;
}

export function TransferLimitCard({ serverId }: { serverId: string | null }) {
  const [inputValue, setInputValue] = useState("");
  const [state, setState] = useState<LimitState>({ kind: "disconnected" });
  const [feedback, setFeedback] = useState<Feedback | null>(null);
  const [applying, setApplying] = useState(false);

  // 进入文件分区（或连接切换）时查询：能力门控 → 实际限制值。
  // 能力门控是硬前提：旧 Agent 无法解码新 payload，盲发会断流、
  // 客户端主循环会把整条连接误判为断开并拆除
  useEffect(() => {
    // 切换服务器（含断开）时清掉上一台的反馈与回填值，避免残留泄漏
    setFeedback(null);
    if (!serverId) {
      setState({ kind: "disconnected" });
      setInputValue("");
      return;
    }
    let cancelled = false;
    setState({ kind: "loading" });
    (async () => {
      try {
        // 1) 能力门控
        const caps = await invoke<string[]>("get_agent_capabilities", { serverId });
        if (cancelled) return;
        if (!caps.includes("transfer_limit")) {
          setState({ kind: "unsupported" });
          return;
        }
        // 2) 查询当前限制（Agent 为单一事实来源，客户端不持久化）
        const info = await invoke<TransferLimitInfo>("get_transfer_limit", { serverId });
        if (cancelled) return;
        setInputValue(String(info.max_file_transfer_mb));
        setState({ kind: "ready", editable: info.editable });
      } catch (e) {
        if (!cancelled) setState({ kind: "error", message: String(e) });
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [serverId]);

  // 注意用 Number 而非 parseInt：parseInt("1e3") 截断得 1、parseInt("12.5") 得 12，
  // 会把非法输入误判为合法整数导致应用值与输入不符；Number 全量解析后由
  // isInteger 正确拦截小数。Number("")=0 会误判合法，须以非空为前置
  const parsed = Number(inputValue);
  const isValidLocal =
    inputValue !== "" && Number.isInteger(parsed) && parsed >= MIN_LIMIT_MB && parsed <= MAX_LIMIT_MB;

  const editable = state.kind === "ready" && state.editable;
  const applyDisabled = !editable || !isValidLocal;

  const handleApply = async () => {
    if (!serverId || !editable || !isValidLocal) return;
    // set 进行中按钮禁用，防止连点重复请求、响应回填覆盖等待期输入
    setApplying(true);
    try {
      const info = await invoke<TransferLimitInfo>("set_transfer_limit", {
        serverId,
        maxFileTransferMb: parsed,
      });
      setInputValue(String(info.max_file_transfer_mb));
      setFeedback(
        info.persisted
          ? { text: "已生效", warn: false }
          : { text: "已生效，但配置文件写入失败，重启后恢复原值", warn: true }
      );
    } catch (e) {
      // Agent 的 403/400 已是用户视角文案；其余为网络/连接类
      setFeedback({ text: String(e), warn: true });
      log.warn("修改上传大小限制失败:", e);
    } finally {
      setApplying(false);
    }
  };

  // hint 文案（spec 状态矩阵）
  const hint = (() => {
    switch (state.kind) {
      case "disconnected":
        return "连接服务器后可查看";
      case "unsupported":
        return "当前 Agent 版本不支持此设置，请升级 Agent";
      case "loading":
        return "正在查询…";
      case "error":
        return state.message;
      case "ready":
        return state.editable
          ? "仅对当前连接的服务器生效，修改后该服务器所有用户适用"
          : "需要以 root 用户连接才能修改";
    }
  })();

  // 本地校验失败时优先展示值域提示（应用按钮同时禁用，不会发请求）
  const invalidLocal =
    state.kind === "ready" && state.editable && inputValue !== "" && !isValidLocal;
  const hintText = invalidLocal
    ? `请输入 ${MIN_LIMIT_MB} – ${MAX_LIMIT_MB} 之间的整数`
    : hint;

  return (
    <div className="st-card">
      <div className="st-card-header text-title">传输设置</div>
      <div className="st-option-row">
        <span className="st-option-label text-body">上传大小限制</span>
        <input
          type="number"
          className="st-input"
          value={inputValue}
          onChange={(e) => {
            setInputValue(e.target.value);
            setFeedback(null);
          }}
          disabled={!editable}
        />
        <span className="st-input-unit">MB</span>
        <button
          className="st-btn st-btn-primary"
          disabled={applyDisabled || applying}
          onClick={handleApply}
        >
          应用
        </button>
      </div>
      <div className="st-hint">{hintText}</div>
      {feedback && (
        <div className={feedback.warn ? "st-hint st-hint-warn" : "st-hint"}>
          {feedback.text}
        </div>
      )}
    </div>
  );
}
