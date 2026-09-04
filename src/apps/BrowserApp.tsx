// src/apps/BrowserApp.tsx

import { useState, useEffect, useRef, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useServerManager } from "../context/ServerManager";
import { useSettingsStore } from "../stores/settingsStore";
import { createLogger } from "../utils/logger";
import "./BrowserApp.css";

const log = createLogger("BrowserApp");

/** 代理会话信息（Rust 端 ProxySessionInfo） */
interface ProxySessionInfo {
  port: number;
  browser: string;
}

/**
 * 浏览器应用（服务器视角网页浏览）
 *
 * 窗口即会话控制面板：
 * - 打开窗口自动启动代理会话（SOCKS5 监听 + 系统浏览器）
 * - 关闭窗口即停止会话（浏览器与监听一并关闭）
 * - 浏览器是独立系统进程，窗口内只展示状态与操作
 * - 可选择用哪个浏览器打开（自动 / Edge / Chrome / Firefox / 自定义路径）
 */
export function BrowserApp() {
  const { activeServer } = useServerManager();
  // 默认浏览器取全局设置（设置 → 浏览器 中配置，此处只读）
  const pref = useSettingsStore((s) => s.browserId);
  // SOCKS5 固定端口（断线重连后浏览器无需重开）
  const proxyPort = useSettingsStore((s) => s.browserProxyPort);
  const [session, setSession] = useState<ProxySessionInfo | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const activeServerIdRef = useRef<string | null>(null);
  // 在飞守卫：ref 不受渲染周期影响，StrictMode 双触发/并发 effect 都拦得住。
  // busy 是 state，同一次提交周期内来不及生效，单靠它防不住并发调用
  const startingRef = useRef(false);

  const connected = activeServer?.status === "connected";

  // 启动会话（幂等：已有会话时仅再开一个浏览器窗口）
  const startSession = useCallback(
    async (browserId: string) => {
      if (!activeServer || busy || startingRef.current) return;
      startingRef.current = true;
      setBusy(true);
      setError(null);
      try {
        const info = await invoke<ProxySessionInfo>("proxy_start_session", {
          serverId: activeServer.id,
          browserId: browserId === "auto" ? null : browserId,
          port: proxyPort,
        });
        setSession(info);
        log.info(`浏览会话已启动: ${info.browser}, SOCKS5 端口 ${info.port}`);
      } catch (e) {
        setError(String(e));
        log.error("浏览会话启动失败:", e);
      } finally {
        startingRef.current = false;
        setBusy(false);
      }
    },
    [activeServer, busy, proxyPort],
  );

  // 打开窗口且已连接 → 自动启动会话；服务器断开 → 状态归位
  useEffect(() => {
    const serverId = activeServer?.id ?? null;
    if (serverId !== activeServerIdRef.current) {
      activeServerIdRef.current = serverId;
      setSession(null);
      setError(null);
    }
    if (connected && !session && !busy && !error) {
      startSession(pref);
    }
  }, [connected, session, busy, error, startSession, pref]);

  // 断线 → 清空前端会话态（Rust 端浏览器保留、监听已挂起）；
  // 重连后上方 effect 自动重启监听，浏览器在固定端口上无感恢复
  useEffect(() => {
    if (!connected) setSession(null);
  }, [connected]);

  // 关闭窗口 → 停止会话（浏览器与 SOCKS5 监听一并关闭）
  useEffect(() => {
    return () => {
      const serverId = activeServerIdRef.current;
      if (serverId) {
        invoke("proxy_stop_session", { serverId })
          .then(() => log.info("窗口关闭，浏览会话已停止"))
          .catch((e) => log.error("停止浏览会话失败:", e));
      }
    };
  }, []);

  // ── 未连接：占位提示 ────────────────────────────────────
  if (!connected) {
    return (
      <div className="ba">
        <div className="ba-empty">
          <div className="ba-empty-icon">🌐</div>
          <div className="ba-empty-title">未连接服务器</div>
          <div className="ba-empty-desc">连接服务器后，可通过服务器网络浏览网页</div>
        </div>
      </div>
    );
  }

  // ── 会话面板 ──────────────────────────────────────────
  return (
    <div className="ba">
      <div className="ba-headerbar">
        <span className="ba-title">远程浏览</span>
        <span className="ba-badge">{session ? "运行中" : busy ? "启动中…" : "未运行"}</span>
      </div>

      <div className="ba-body">
        {error && (
          <div className="ba-error">
            <div className="ba-error-title">会话启动失败</div>
            <div className="ba-error-desc">{error}</div>
            <button
              type="button"
              className="ba-btn"
              onClick={() => startSession(pref)}
              disabled={busy}
            >
              重试
            </button>
          </div>
        )}

        {!error && (
          <>
            <div className="ba-status">
              <div className="ba-status-row">
                <span className="ba-status-label">服务器</span>
                <span className="ba-status-value">{activeServer?.name || activeServer?.host}</span>
              </div>
              <div className="ba-status-row">
                <span className="ba-status-label">浏览器</span>
                <span className="ba-status-value">
                  {session
                    ? session.browser === "existing"
                      ? "已打开"
                      : session.browser
                    : "—"}
                </span>
              </div>
              <div className="ba-status-row">
                <span className="ba-status-label">SOCKS5 端口</span>
                <span className="ba-status-value">{session ? session.port : "—"}</span>
              </div>
            </div>

            <div className="ba-hint">
              浏览器以独立窗口打开，流量经服务器网络转发（目标网站看到的是服务器 IP）。
              断线重连后浏览器自动恢复网络，无需重开；关闭本窗口将同时关闭浏览器与代理。
              默认浏览器与端口可在 设置 → 浏览器 中修改。
            </div>

            <button
              type="button"
              className="ba-btn"
              onClick={() => startSession(pref)}
              disabled={busy || !session}
            >
              再开一个浏览器窗口
            </button>
          </>
        )}
      </div>
    </div>
  );
}
