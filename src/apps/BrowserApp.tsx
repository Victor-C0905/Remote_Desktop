// src/apps/BrowserApp.tsx

import { useState, useEffect, useRef, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useServerManager } from "../context/ServerManager";
import { useSettingsStore } from "../stores/settingsStore";
import { createLogger } from "../utils/logger";
import { SymbolicIcon } from "../components/symbolic";
import { ConnectionErrorState, ReconnectingState } from "../components/ConnectionState";
import { getServerErrorInfo } from "../stores/serversStore";
import { useOpenApp } from "../window-system/hooks/useOpenApp";
import "./BrowserApp.css";

const log = createLogger("BrowserApp");

/** 代理会话信息（Rust 端 ProxySessionInfo） */
interface ProxySessionInfo {
  port: number;
  browser: string;
}

/** 代理会话状态（Rust 端 ProxySessionStatus，无会话时为 null） */
interface ProxySessionStatus {
  port: number;
  browser: string;
  listener_alive: boolean;
  browser_alive: boolean;
}

/**
 * 浏览器应用（服务器视角网页浏览）
 *
 * 窗口即会话控制面板：
 * - 浏览器只在用户主动点击"打开浏览器"时启动（连接服务器不自动打开）
 * - 关闭窗口即停止会话（浏览器与监听一并关闭）
 * - 断线重连后：已打开的浏览器自动恢复网络（仅重建监听，不弹新窗口）；
 *   浏览器已关则等待用户再次主动打开
 * - 浏览器是独立系统进程，窗口内只展示状态与操作
 * - 可选择用哪个浏览器打开（自动 / Edge / Chrome / Firefox / 自定义路径）
 */
export function BrowserApp() {
  const { activeServer, servers, connectServer } = useServerManager();
  const openApp = useOpenApp();
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

  // 服务器切换 → 状态归位（会话随窗口/服务器维度，切换后需重新查询或手动打开）
  useEffect(() => {
    const serverId = activeServer?.id ?? null;
    if (serverId !== activeServerIdRef.current) {
      activeServerIdRef.current = serverId;
      setSession(null);
      setError(null);
    }
  }, [activeServer?.id]);

  // 连接后查询会话状态：浏览器还活着 → 恢复网络（重建监听，不弹新窗口）；
  // 无会话/浏览器已关 → 等待用户主动点击"打开浏览器"，连接本身不自动 spawn
  useEffect(() => {
    if (!connected || !activeServer) return;
    const serverId = activeServer.id;
    let cancelled = false;
    (async () => {
      try {
        const st = await invoke<ProxySessionStatus | null>("proxy_session_status", {
          serverId,
        });
        if (cancelled || !st) return;
        if (!st.browser_alive) return; // 浏览器已关：不自动重开，等用户主动操作
        if (st.listener_alive) {
          // 会话完好（如窗口重挂载）：直接采纳状态
          setSession({ port: st.port, browser: st.browser });
        } else {
          // 断线挂起：仅重建监听，浏览器在固定端口上无感恢复
          startSession(pref);
        }
      } catch (e) {
        log.error("查询浏览会话状态失败:", e);
      }
    })();
    return () => {
      cancelled = true;
    };
    // startSession 身份随 busy/session 变化，重跑只会重复查询（幂等）：
    // 恢复期间 busy=true 会挡住 startSession 的重复调用
  }, [connected, activeServer, startSession, pref]);

  // 断线 → 清空前端会话态（Rust 端浏览器保留、监听已挂起）；
  // 重连后上方 effect 查询状态，浏览器还开着则自动恢复网络
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
    // 断连故障服务器（activeServerId 已清空，按状态识别；reconnecting 优先于 error）
    const troubledServer =
      servers.find((s) => s.status === "reconnecting") ||
      servers.find((s) => s.status === "error") ||
      null;

    if (troubledServer) {
      return (
        <div className="ba">
          <div className="ba-empty">
            {troubledServer.status === "reconnecting" ? (
              <ReconnectingState serverName={troubledServer.name || troubledServer.host} />
            ) : (
              <ConnectionErrorState
                {...getServerErrorInfo(troubledServer)}
                onRetry={() => connectServer(troubledServer.id)}
                onOpenSettings={() => openApp("settings")}
              />
            )}
          </div>
        </div>
      );
    }

    return (
      <div className="ba">
        <div className="ba-empty">
          <div className="ba-empty-icon">
            <SymbolicIcon name="network-offline" size={32} />
          </div>
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

        {!error && !session && (
          <>
            <div className="ba-hint">
              点击下方按钮，将使用系统浏览器经服务器网络打开独立浏览窗口
              （目标网站看到的是服务器 IP）。默认浏览器与端口可在 设置 → 浏览器 中修改。
            </div>

            <button
              type="button"
              className="ba-btn"
              onClick={() => startSession(pref)}
              disabled={busy}
            >
              {busy ? "启动中…" : "打开浏览器"}
            </button>
          </>
        )}

        {!error && session && (
          <>
            <div className="ba-status">
              <div className="ba-status-row">
                <span className="ba-status-label">服务器</span>
                <span className="ba-status-value">{activeServer?.name || activeServer?.host}</span>
              </div>
              <div className="ba-status-row">
                <span className="ba-status-label">浏览器</span>
                <span className="ba-status-value">
                  {session.browser === "existing" ? "已打开" : session.browser}
                </span>
              </div>
              <div className="ba-status-row">
                <span className="ba-status-label">SOCKS5 端口</span>
                <span className="ba-status-value">{session.port}</span>
              </div>
            </div>

            <div className="ba-hint">
              浏览器以独立窗口打开，流量经服务器网络转发。断线重连后浏览器自动恢复网络，
              无需重开；关闭本窗口将同时关闭浏览器与代理。
            </div>

            <button
              type="button"
              className="ba-btn"
              onClick={() => startSession(pref)}
              disabled={busy}
            >
              再开一个浏览器窗口
            </button>
          </>
        )}
      </div>
    </div>
  );
}
