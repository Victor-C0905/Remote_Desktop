// src/shell/RootErrorBoundary.tsx
// 根错误边界：单个组件的渲染/effect 崩溃不再白屏整个应用——降级为最小
// 错误屏（dev 下多为 HMR 中间态，刷新即恢复）。不做自动重试（避免
// 崩溃循环），交给用户显式重载。

import { Component, type ReactNode } from "react";
import { createLogger } from "../utils/logger";

const log = createLogger("RootErrorBoundary");

interface Props {
  children: ReactNode;
}

interface State {
  error: Error | null;
}

export class RootErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: { componentStack?: string | null }) {
    log.error("渲染崩溃:", error, "\n组件栈:", info.componentStack ?? "");
  }

  render() {
    if (this.state.error) {
      return (
        <div
          style={{
            padding: 48,
            color: "#e01b24",
            fontFamily: "var(--font-ui, sans-serif)",
            background: "var(--quirel-window-bg, #fff)",
            minHeight: "100vh",
          }}
        >
          <h2 style={{ marginBottom: 12 }}>界面发生错误</h2>
          <pre
            style={{
              whiteSpace: "pre-wrap",
              wordBreak: "break-all",
              marginBottom: 20,
            }}
          >
            {String(this.state.error.message ?? this.state.error)}
          </pre>
          <button
            type="button"
            onClick={() => this.setState({ error: null })}
            style={{
              padding: "8px 20px",
              border: "1px solid #e01b24",
              borderRadius: 6,
              background: "transparent",
              color: "#e01b24",
              cursor: "pointer",
            }}
          >
            重试
          </button>
          <button
            type="button"
            onClick={() => window.location.reload()}
            style={{
              padding: "8px 20px",
              border: "1px solid var(--quirel-text-secondary, #666)",
              borderRadius: 6,
              background: "transparent",
              color: "var(--quirel-text-secondary, #666)",
              cursor: "pointer",
            }}
          >
            重置界面
          </button>
          <span style={{ marginLeft: 12, color: "var(--quirel-text-secondary, #666)" }}>
            受控整页重载（浏览器会话不受影响）；完全恢复请重启 Quirel
          </span>
        </div>
      );
    }
    return this.props.children;
  }
}
