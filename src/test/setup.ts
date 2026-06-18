import { vi, beforeEach, afterEach } from 'vitest';
import '@testing-library/jest-dom';

// Mock Tauri API
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

// 全局测试配置
beforeEach(() => {
  // 清除所有 mock
  vi.clearAllMocks();
});

afterEach(() => {
  // 清理 DOM
  document.body.innerHTML = '';
});