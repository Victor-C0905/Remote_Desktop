import { vi, beforeEach, afterEach } from 'vitest';
import '@testing-library/jest-dom';

// Mock Tauri API
// invoke 必须返回 Promise：jsdom 下无 Tauri runtime，若返回 undefined，
// 调用方的 .then/.catch/.finally 会抛 Uncaught TypeError（如 logger.persist）
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(() => Promise.resolve(undefined)),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

// Mock Tauri plugin-store：内存版 Store，让 zustand persist（settings/servers/windows）
// 的读写不触真实 IPC。每次 load() 返回独立实例，模拟不同 store 文件互不干扰
vi.mock('@tauri-apps/plugin-store', () => {
  const createMemoryStore = () => {
    const map = new Map<string, unknown>();
    return {
      get: async (key: string) => (map.has(key) ? map.get(key) : null),
      set: async (key: string, value: unknown) => { map.set(key, value); },
      delete: async (key: string) => { map.delete(key); },
      clear: async () => { map.clear(); },
      save: async () => {},
      reload: async () => {},
      entries: async () => Array.from(map.entries()),
      keys: async () => Array.from(map.keys()),
      values: async () => Array.from(map.values()),
      length: 0,
      resource: 0,
    };
  };
  return {
    load: vi.fn(async () => createMemoryStore()),
    Store: vi.fn(),
  };
});

// 全局测试配置
beforeEach(() => {
  // 清除所有 mock
  vi.clearAllMocks();
});

afterEach(() => {
  // 清理 DOM
  document.body.innerHTML = '';
});
