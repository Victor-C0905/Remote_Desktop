// src/window-system/WindowManagerContext.tsx

import { createContext, useContext, useMemo, useEffect, useSyncExternalStore, ReactNode } from 'react';
import { WindowManager } from './core/WindowManager';
import { WindowRegistry } from './core/WindowRegistry';
import { IWindowManager, WindowEventType, WindowEvent } from './types';
import { initWindowRegistry } from './init';
import { useSettingsStore } from '../stores/settingsStore';

// ─── 全局状态类型 ──────────────────────────────────────
// 仅包含"窗口列表 + 激活窗口 ID"，用于 Dock/Overview 等低频消费
interface GlobalState {
  windowList: Array<{ id: string; appId: string; isMinimized: boolean; isMaximized: boolean }>;
  activeWindowId: string | null;
}

// ─── Context 类型 ─────────────────────────────────────
interface WindowManagerContextValue {
  manager: IWindowManager;
}

// ─── Context ──────────────────────────────────────────
export const WindowManagerContext = createContext<WindowManagerContextValue | null>(null);

// ─── Registry 单例 ────────────────────────────────────
let registryInstance: WindowRegistry | null = null;

// ─── 全局状态缓存（useSyncExternalStore 的数据源）──────
// ⚠️ 关键：getSnapshot 必须幂等——数据不变时返回同一引用
let globalStateCache: GlobalState = {
  windowList: [],
  activeWindowId: null,
};

// 全局状态订阅者集合
const globalStateListeners = new Set<() => void>();

/**
 * 通知全局状态变化
 * 只在"窗口列表变化"（创建/关闭/最小化/恢复/最大化/聚焦）时触发
 */
function notifyGlobalStateChange() {
  globalStateListeners.forEach(listener => listener());
}

/**
 * 浅比较两个 GlobalState 是否相等
 */
function globalStateEqual(a: GlobalState, b: GlobalState): boolean {
  if (a.activeWindowId !== b.activeWindowId) return false;
  if (a.windowList.length !== b.windowList.length) return false;
  return a.windowList.every((wa, i) => {
    const wb = b.windowList[i];
    return wa.id === wb.id &&
      wa.appId === wb.appId &&
      wa.isMinimized === wb.isMinimized &&
      wa.isMaximized === wb.isMaximized;
  });
}

/**
 * 从 WindowManager 同步读取最新全局状态
 * ⚠️ 幂等：数据未变时返回同一缓存引用，避免 useSyncExternalStore 无限循环
 */
function getGlobalStateSnapshot(manager: WindowManager): GlobalState {
  const windows = manager.getAll();
  const activeWindow = manager.getActive();

  const newState: GlobalState = {
    windowList: windows.map(w => ({
      id: w.id,
      appId: w.appId,
      isMinimized: w.minimized,
      isMaximized: w.maximized,
    })),
    activeWindowId: activeWindow?.id ?? null,
  };

  // 数据没变 → 返回旧引用（Object.is 相等 → 不触发重渲染）
  if (globalStateEqual(globalStateCache, newState)) {
    return globalStateCache;
  }

  // 数据变了 → 更新缓存并返回新引用
  globalStateCache = newState;
  return globalStateCache;
}

// ─── Provider ─────────────────────────────────────────
export function WindowManagerProvider({ children }: { children: ReactNode }) {
  // 初始化 Registry 单例
  const registry = useMemo(() => {
    if (!registryInstance) {
      registryInstance = new WindowRegistry();
      initWindowRegistry(registryInstance);
    }
    return registryInstance;
  }, []);

  // 创建 Manager 实例
  const manager = useMemo(() => {
    return new WindowManager(registry);
  }, [registry]);

  // 加载持久化窗口（受用户设置控制：关闭则每次启动干净桌面）
  // mount-only：只在启动时读一次设置值；运行时切换开关只影响 save（save 内动态读取），
  // 若把设置值放进依赖会导致切换开关时重复 load() → 窗口重复恢复。
  // persist 后端是异步 Tauri Store，mount 时可能尚未 rehydrate → 显式等待，
  // 否则用户关闭的开关可能被默认值 true 覆盖（设置失效）
  useEffect(() => {
    let cancelled = false;
    const boot = async () => {
      if (!useSettingsStore.persist.hasHydrated()) {
        await new Promise<void>((resolve) =>
          useSettingsStore.persist.onFinishHydration(() => resolve())
        );
      }
      if (cancelled) return;
      if (!useSettingsStore.getState().restoreWindowsOnStartup) return;
      manager.load();
      // 初始化事件桥接：将 Manager 事件路由到 per-window 通知
      initWindowEventBridge(manager as WindowManager);
    };
    boot();
    return () => { cancelled = true; };
  }, [manager]);

  // 选择性事件监听：只在窗口列表变化时触发全局状态更新
  // 窗口拖动（moved）和调整大小（resized）不触发全局状态更新
  useEffect(() => {
    const LIST_AFFECTING_EVENTS: WindowEventType[] = [
      'window:created',
      'window:closed',
      'window:focused',
      'window:minimized',
      'window:restored',
      'window:maximized',
      'window:unmaximized',
    ];

    const unsubscribes = LIST_AFFECTING_EVENTS.map(eventType =>
      manager.on(eventType, () => {
        notifyGlobalStateChange();
      })
    );

    // 保存事件也监听（受用户设置控制：关闭开关则不持久化，避免旧数据残留）
    const saveUnsubscribe = manager.onAny(() => {
      if (useSettingsStore.getState().restoreWindowsOnStartup) {
        manager.save();
      }
    });

    return () => {
      unsubscribes.forEach(unsub => unsub());
      saveUnsubscribe();
    };
  }, [manager]);

  return (
    <WindowManagerContext.Provider value={{ manager }}>
      {children}
    </WindowManagerContext.Provider>
  );
}

// ─── Hooks ────────────────────────────────────────────

/**
 * 检查 WindowManager 是否可用
 */
export function useWindowManagerContext(): boolean {
  return useContext(WindowManagerContext) !== null;
}

/**
 * 获取 WindowManager 实例
 */
export function useWindowManager(): WindowManagerContextValue {
  const context = useContext(WindowManagerContext);
  if (!context) {
    throw new Error('[useWindowManager] WindowManagerContext not provided');
  }
  return context;
}

/**
 * 全局状态 Hook：用 useSyncExternalStore 订阅
 * 只在窗口列表变化时重渲染（创建/关闭/最小化/恢复/聚焦）
 * 窗口拖动/调整大小不触发此 Hook 的重渲染
 */
export function useWindowGlobalState(): GlobalState {
  const { manager } = useWindowManager();

  return useSyncExternalStore(
    // subscribe: 注册全局状态监听器
    (callback) => {
      globalStateListeners.add(callback);
      return () => {
        globalStateListeners.delete(callback);
      };
    },
    // getSnapshot: 返回当前快照（幂等，数据不变返回同一引用）
    () => getGlobalStateSnapshot(manager as WindowManager),
    // getServerSnapshot: SSR 兼容
    () => globalStateCache
  );
}

// ─── Per-Window 状态订阅 ──────────────────────────────

// 窗口状态缓存（按 windowId 分片）
const windowStateCaches = new Map<string, WindowInstanceState>();
const windowStateListeners = new Map<string, Set<() => void>>();

interface WindowInstanceState {
  position: { x: number; y: number };
  size: { width: number; height: number };
  minimized: boolean;
  maximized: boolean;
  isActive: boolean;
  activatedAt: number;
}

/**
 * 通知指定窗口状态变化
 */
function notifyWindowStateChange(windowId: string) {
  windowStateListeners.get(windowId)?.forEach(listener => listener());
}

/**
 * 逐字段比较 WindowInstanceState
 */
function windowStateEqual(a: WindowInstanceState, b: WindowInstanceState): boolean {
  return a.position.x === b.position.x &&
    a.position.y === b.position.y &&
    a.size.width === b.size.width &&
    a.size.height === b.size.height &&
    a.minimized === b.minimized &&
    a.maximized === b.maximized &&
    a.isActive === b.isActive &&
    a.activatedAt === b.activatedAt;
}

/** 默认窗口状态（窗口不存在时的回退值） */
const DEFAULT_WINDOW_STATE: WindowInstanceState = {
  position: { x: 0, y: 0 },
  size: { width: 0, height: 0 },
  minimized: false,
  maximized: false,
  isActive: false,
  activatedAt: 0,
};

/**
 * 从 Window 实例同步读取最新状态
 * ⚠️ 幂等：数据未变时返回同一缓存引用，避免 useSyncExternalStore 无限循环
 */
function getWindowStateSnapshot(windowId: string, manager: WindowManager): WindowInstanceState {
  const win = manager.getById(windowId);
  if (!win) {
    return windowStateCaches.get(windowId) ?? DEFAULT_WINDOW_STATE;
  }

  const activeWindowId = manager.getActive()?.id ?? null;

  const newState: WindowInstanceState = {
    position: win.position,
    size: win.size,
    minimized: win.minimized,
    maximized: win.maximized,
    isActive: win.id === activeWindowId,
    activatedAt: win.activatedAt,
  };

  const cached = windowStateCaches.get(windowId);

  // 数据没变 → 返回旧引用
  if (cached && windowStateEqual(cached, newState)) {
    return cached;
  }

  // 数据变了 → 更新缓存并返回新引用
  windowStateCaches.set(windowId, newState);
  return newState;
}

/**
 * 窗口状态 Hook：按 windowId 分片订阅
 * 只在对应窗口状态变化时重渲染
 * 其他窗口的拖动/调整大小不影响此 Hook 的订阅者
 */
export function useWindowState(windowId: string): WindowInstanceState | null {
  const { manager } = useWindowManager();

  // 检查窗口是否存在
  const win = manager.getById(windowId);
  if (!win) return null;

  return useSyncExternalStore(
    // subscribe: 注册指定窗口的状态监听器
    (callback) => {
      if (!windowStateListeners.has(windowId)) {
        windowStateListeners.set(windowId, new Set());
      }
      windowStateListeners.get(windowId)!.add(callback);
      return () => {
        windowStateListeners.get(windowId)?.delete(callback);
      };
    },
    // getSnapshot: 返回当前窗口状态快照（幂等）
    () => getWindowStateSnapshot(windowId, manager as WindowManager),
    // getServerSnapshot: SSR 兼容
    () => windowStateCaches.get(windowId) ?? DEFAULT_WINDOW_STATE
  );
}

// ─── 初始化：将 Manager 事件路由到 per-window 通知 ────

// 全局事件到 per-window 通知的桥接（在 Provider 中设置）
let bridgeInitialized = false;

export function initWindowEventBridge(manager: WindowManager) {
  if (bridgeInitialized) return;
  bridgeInitialized = true;

  // 窗口位置/大小变化 → 只通知对应窗口的订阅者
  const PER_WINDOW_EVENTS: WindowEventType[] = [
    'window:moved',
    'window:resized',
  ];

  PER_WINDOW_EVENTS.forEach(eventType => {
    manager.on(eventType, (event: WindowEvent) => {
      notifyWindowStateChange(event.windowId);
    });
  });

  // 窗口列表变化 → 通知对应窗口 + 全局
  const LIST_EVENTS: WindowEventType[] = [
    'window:created',
    'window:closed',
    'window:focused',
    'window:minimized',
    'window:restored',
    'window:maximized',
    'window:unmaximized',
    'window:snapped',      // ✅ 新增
    'window:unsnapped',    // ✅ 新增
  ];

  LIST_EVENTS.forEach(eventType => {
    manager.on(eventType, (event: WindowEvent) => {
      // 聚焦事件：同时通知旧窗口和新窗口（isActive 变化）
      if (eventType === 'window:focused') {
        windowStateListeners.forEach((_, wId) => {
          notifyWindowStateChange(wId);
        });
      } else {
        // 其他事件：只通知对应窗口
        notifyWindowStateChange(event.windowId);
      }
    });
  });
}
