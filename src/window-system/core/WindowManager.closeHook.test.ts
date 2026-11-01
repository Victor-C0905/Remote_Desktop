// src/window-system/core/WindowManager.closeHook.test.ts
// 浏览器窗口关闭 → 会话停止契约（§5.1 销毁单点的回归锚）。
//
// 回归背景：窗口创建路径（Desktop/useOpenApp → manager.create）不经过
// onBeforeCreate，window.preloadData 恒为 undefined；关闭钩子若读
// preloadData.serverId 会静默跳过 → proxy_stop_session 从未执行 →
// webview 仅靠 3s 心跳兜底隐藏（表现为「延迟关闭」），SOCKS5 监听与
// host 全部泄漏至退出。钩子的 serverId 必须与 BrowserApp 同源
// （activeServerId）。

import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));
vi.mock('../../utils/storage', () => ({
  serversStorage: { getItem: vi.fn(), setItem: vi.fn(), removeItem: vi.fn() },
}));
// 契约只关心注册表元数据与生命周期钩子，组件全部置空
vi.mock('../../apps/Terminal', () => ({ TerminalApp: () => null }));
vi.mock('../../apps/FileManager', () => ({ FileManager: () => null }));
vi.mock('../../apps/SystemMonitor', () => ({ SystemMonitor: () => null }));
vi.mock('../../apps/Settings', () => ({ Settings: () => null }));
vi.mock('../../apps/TextEditor/TextEditor', () => ({ TextEditor: () => null }));
vi.mock('../../apps/ImageViewer/ImageViewer', () => ({ ImageViewer: () => null }));
vi.mock('../../apps/HexViewer/HexViewer', () => ({ HexViewer: () => null }));
vi.mock('../../apps/PDFViewer/PDFViewer', () => ({ PDFViewer: () => null }));
vi.mock('../../apps/ArchiveViewer/ArchiveViewer', () => ({ ArchiveViewer: () => null }));
vi.mock('../../apps/BrowserApp', () => ({ BrowserApp: () => null }));

import { invoke } from '@tauri-apps/api/core';
import { initWindowRegistry } from '../init';
import { WindowRegistry } from './WindowRegistry';
import { WindowManager } from './WindowManager';
import { useServersStore } from '../../stores/serversStore';

describe('浏览器窗口关闭 → 会话停止契约', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useServersStore.setState({ servers: [], activeServerId: null });
  });

  it('关闭时以 activeServerId 触发 proxy_stop_session（真实创建路径无 preloadData）', async () => {
    useServersStore.setState({ activeServerId: 'srv-A' });

    const registry = new WindowRegistry();
    initWindowRegistry(registry);
    const manager = new WindowManager(registry);

    const win = await manager.create('browser');
    // 前提锚定：真实创建路径不给 preloadData（Desktop/useOpenApp 均如此，
    // 无 onBeforeCreate 时窗口保持初始 null）
    expect(win.preloadData).toBeNull();

    manager.close(win.id);

    expect(invoke).toHaveBeenCalledWith('proxy_stop_session', { serverId: 'srv-A' });
  });

  it('无活动服务器时不触发停止', async () => {
    useServersStore.setState({ activeServerId: null });

    const registry = new WindowRegistry();
    initWindowRegistry(registry);
    const manager = new WindowManager(registry);

    const win = await manager.create('browser');
    manager.close(win.id);

    expect(invoke).not.toHaveBeenCalledWith('proxy_stop_session', expect.anything());
  });
});
