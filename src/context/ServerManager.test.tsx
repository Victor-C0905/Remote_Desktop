// src/context/ServerManager.test.tsx
// 回归：StrictMode 双跑 effect 时，异步 listen 的 cleanup 会先于 resolve 执行。
// 首次挂载的监听器必须在 resolve 时立即注销（cancelled 竞态防护），否则
// connection-lost 会残留两份监听器，同一断连事件被处理两次——通知中心的
// 同源合并会把 count 累加到 2，用户看到「×2」。
//
// 回归：两台服务器并发连接时，慢速连接的迟到成功会留下第二条连接。
// connectServer 只按 activeServerId 断开旧连接，而「连接中」的服务器不在
// activeServerId 里；旧连接的迟到成功又不校验意图，最终后端同时存在两条
// QUIC 连接，列表出现两台「已连接」。
import { StrictMode } from "react";
import { act, render, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";

const listenMock = vi.hoisted(() => {
  const calls: Array<{ event: string; unlisten: ReturnType<typeof vi.fn> }> = [];
  const listen = vi.fn(async (event: string) => {
    const unlisten = vi.fn();
    calls.push({ event, unlisten });
    return unlisten;
  });
  return { listen, calls };
});

const invokeMock = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock.listen }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ ask: vi.fn(async () => true) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import { ServerManagerProvider, useServerManager } from "./ServerManager";
import { useServersStore } from "../stores/serversStore";
import { AuthMethod, type ServerConfig } from "../types/server";

describe("ServerManagerProvider 事件监听生命周期", () => {
  it("StrictMode 双跑后 connection-lost 只保留一份有效监听器", async () => {
    render(
      <StrictMode>
        <ServerManagerProvider>
          <div />
        </ServerManagerProvider>
      </StrictMode>,
    );

    // StrictMode 双跑 mount effect → 注册两次
    await waitFor(() => {
      expect(listenMock.calls.filter((c) => c.event === "connection-lost")).toHaveLength(2);
    });

    const lost = listenMock.calls.filter((c) => c.event === "connection-lost");
    // 首次挂载的监听器必须已注销，只保留第二次挂载的有效监听
    expect(lost.filter((c) => c.unlisten.mock.calls.length > 0)).toHaveLength(1);
    expect(lost.filter((c) => c.unlisten.mock.calls.length === 0)).toHaveLength(1);
  });
});

/* ── 单连接模型：并发连接的迟到成功必须被清理 ───────────── */

interface PendingCall {
  resolve: (value: unknown) => void;
  reject: (reason: unknown) => void;
}

const makeServer = (id: string, host: string): ServerConfig => ({
  id,
  name: id,
  host,
  port: 9443,
  auth: { method: AuthMethod.PUBKEY, username: "root" },
  status: "disconnected",
});

const connInfo = (id: string, host: string) => ({
  serverId: id,
  host,
  port: 9443,
  transport: "quic",
  status: "connected",
  rttMs: 10,
  connectedAt: Date.now(),
});

describe("ServerManagerProvider 单连接模型", () => {
  beforeEach(() => {
    // 默认返回 Promise：logger 等 fire-and-forget 调用会链 .then
    invokeMock.mockResolvedValue(undefined);
    // performConnect 以 isTauri() 短路，测试环境需注入 Tauri 标记
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  });

  afterEach(() => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  });

  it("慢连接迟到成功不得留下第二条连接", async () => {
    const pending = new Map<string, PendingCall[]>();
    const disconnectCalls: string[] = [];

    invokeMock.mockImplementation((cmd: string, args?: Record<string, unknown>) => {
      switch (cmd) {
        case "remote_connect": {
          const serverId = String(args?.serverId);
          const list = pending.get(serverId) ?? [];
          list.push({ resolve: () => {}, reject: () => {} });
          pending.set(serverId, list);
          return new Promise((resolve, reject) => {
            list[list.length - 1] = { resolve, reject };
          });
        }
        case "remote_disconnect":
          disconnectCalls.push(String(args?.serverId));
          return Promise.resolve(undefined);
        case "get_active_transfer_count":
          return Promise.resolve(0);
        case "app_process_epoch":
          return Promise.resolve("epoch-test");
        default:
          return Promise.resolve(undefined);
      }
    });

    useServersStore.setState({
      servers: [makeServer("srv-a", "a.test"), makeServer("srv-b", "b.test")],
      activeServerId: null,
    });

    let api: ReturnType<typeof useServerManager> | null = null;
    function Probe() {
      api = useServerManager();
      return null;
    }

    render(
      <ServerManagerProvider>
        <Probe />
      </ServerManagerProvider>,
    );

    // A 先发起，远端迟迟不响应（认证阶段挂起）
    await act(async () => {
      void api!.connectServer("srv-a");
    });
    await waitFor(() => expect(pending.get("srv-a")).toHaveLength(1));

    // A 仍未返回时用户切换 B：A 属于旧意图，必须被清理
    await act(async () => {
      void api!.connectServer("srv-b");
    });
    await waitFor(() => expect(pending.get("srv-b")).toHaveLength(1));
    expect(disconnectCalls).toContain("srv-a");

    // B 先成功并成为当前连接
    await act(async () => {
      pending.get("srv-b")![0].resolve(connInfo("srv-b", "b.test"));
    });
    await waitFor(() => {
      expect(useServersStore.getState().activeServerId).toBe("srv-b");
    });

    // A 的迟到成功响应此刻才返回
    await act(async () => {
      pending.get("srv-a")![0].resolve(connInfo("srv-a", "a.test"));
    });

    // 最终只允许一条连接存活，且迟到的 A 被主动断开
    await waitFor(() => {
      const connected = useServersStore
        .getState()
        .servers.filter((s) => s.status === "connected")
        .map((s) => s.id);
      expect(connected).toEqual(["srv-b"]);
    });
    expect(disconnectCalls.filter((id) => id === "srv-a").length).toBeGreaterThanOrEqual(2);
  });
});
