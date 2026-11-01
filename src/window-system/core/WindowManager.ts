// src/window-system/core/WindowManager.ts

import { Window } from './Window';
import { WindowCollection } from './WindowCollection';
import { WindowRegistry } from './WindowRegistry';
import { WindowEventBus } from '../events/WindowEventBus';
import {
  CreateWindowOptions,
  CreateContext,
  WindowEvent,
  WindowEventType,
  IWindowManager,
} from '../types';
import { createLogger } from '../../utils/logger';
import type { SnapZone } from '../../components/window-shell/aeroSnap';
import { computeSnapRect } from '../../components/window-shell/aeroSnap';
import { Window as WindowClass, type PersistedWindowData } from './Window';

const log = createLogger('WindowManager');

/** 窗口会话快照键（sessionStorage：刷新存活、关闭项目随窗口销亡） */
const SESSION_KEY = 'wm.session';

/**
 * WindowManager - singleton orchestrator for window system
 * Manages window lifecycle and events
 */
export class WindowManager implements IWindowManager {
  private windows: WindowCollection;
  private registry: WindowRegistry;
  private eventBus: WindowEventBus;
  private activeWindowId: string | null = null;
  // ✅ MRU 层叠：激活顺序栈（末尾=栈顶=最近激活）。
  // focus=移除+压顶、create=压顶、close=移除——纯栈操作，
  // 不依赖时间戳，天然无同毫秒并列问题。
  private _activationOrder: string[] = [];

  constructor(registry: WindowRegistry) {
    this.windows = new WindowCollection();
    this.registry = registry;
    this.eventBus = new WindowEventBus();
  }

  /**
   * Generate unique window ID
   */
  private generateWindowId(appId: string): string {
    return `win-${appId}-${Date.now()}-${Math.random().toString(36).slice(2, 9)}`;
  }

  /**
   * ✅ MRU 层叠：激活顺序栈（只读视图，末尾=最近激活）。
   * 消费方（快照层）用 indexOf 得到窗口的层叠排名。
   */
  get activationOrder(): readonly string[] {
    return this._activationOrder;
  }

  /**
   * 把窗口移到激活栈顶（focus/restore/create 共用）
   */
  private pushActivationTop(windowId: string): void {
    this._activationOrder = this._activationOrder.filter(id => id !== windowId);
    this._activationOrder.push(windowId);
  }

  /**
   * 从激活栈顶向下找下一个可激活窗口（排除 excludeId 与最小化窗口）。
   * 用于关闭/最小化当前激活窗口时挑选继任者。
   */
  private pickNextActive(excludeId: string): string | null {
    for (let i = this._activationOrder.length - 1; i >= 0; i--) {
      const id = this._activationOrder[i];
      if (id === excludeId) continue;
      const w = this.windows.get(id);
      if (w && !w.minimized) return id;
    }
    return null;
  }

  /**
   * Get the app definition for a window
   */
  getApp(appId: string): ReturnType<WindowRegistry['get']> {
    return this.registry.get(appId);
  }

  /**
   * Get all registered app definitions
   * Desktop/Dock/Overview 从此派生显示列表（单一数据源）
   */
  getRegisteredApps(): ReturnType<WindowRegistry['getAll']> {
    return this.registry.getAll();
  }

  /**
   * Create a new window
   * Handles lifecycle hooks and preloading
   */
  async create(appId: string, options?: CreateWindowOptions): Promise<Window> {
    const app = this.registry.get(appId);
    if (!app) {
      throw new Error(`[WindowManager] App "${appId}" not registered`);
    }

    // Check if app allows multiple instances
    if (!app.allowMultipleInstances) {
      const existingWindows = this.windows.getByAppId(appId);
      if (existingWindows.length > 0) {
        // Focus existing window instead of creating new one
        const existing = existingWindows[0];
        this.focus(existing.id);
        return existing;
      }
    }

    // Calculate initial position and size
    const containerWidth = window.innerWidth;
    const containerHeight = window.innerHeight - 32; // 减去 TopBar 高度

    // ── 边界约束逻辑（符合标准窗口设计）──────────────
    // 1. 顶部边界：工作区内 y >= 0（TopBar 已由 shell flex 布局分离）
    const minY = 0;

    // 2. 左、右、下边界：可以穿越，但保留最小可见区域
    const MIN_VISIBLE = 100; // 最小可见区域（像素）

    const defaultX = Math.min(100 + this.windows.count() * 30, containerWidth - app.defaultSize.width - 100);
    const defaultY = Math.min(100 + this.windows.count() * 30, containerHeight - app.defaultSize.height - 100);

    // 确保默认位置在可视范围内（窗口创建时应该完全可见）
    const constrainedDefaultX = Math.max(0, Math.min(defaultX, containerWidth - app.defaultSize.width));
    const constrainedDefaultY = Math.max(minY, Math.min(defaultY, containerHeight - app.defaultSize.height));

    // 如果提供了自定义位置，应用边界约束
    let position = options?.position ?? { x: constrainedDefaultX, y: constrainedDefaultY };

    if (options?.position) {
      // 应用边界约束（允许部分移出屏幕，但保留最小可见区域）
      const minX = -(app.defaultSize.width - MIN_VISIBLE);
      const maxX = containerWidth - MIN_VISIBLE;
      const maxY = containerHeight - MIN_VISIBLE;

      position = {
        x: Math.max(minX, Math.min(options.position.x, maxX)),
        y: Math.max(minY, Math.min(options.position.y, maxY)),
      };
    }

    const size = options?.size ?? app.defaultSize;

    // Create window instance
    const windowId = this.generateWindowId(appId);
    const newWindow = new Window(windowId, appId, position, size);

    // Add to collection
    this.windows.add(newWindow);
    this._activationOrder.push(windowId); // ✅ 新窗口压入激活栈顶（MRU 层叠）
    this.activeWindowId = windowId;

    // Emit created event
    this.eventBus.emit({
      type: 'window:created',
      windowId,
      timestamp: Date.now(),
    });

    // Handle preloading
    if (options?.preloadData) {
      // Use provided preload data (skip onBeforeCreate)
      newWindow.setPreloadData(options.preloadData);
    } else if (app.lifecycle?.onBeforeCreate) {
      // Call onBeforeCreate hook
      const context: CreateContext = {
        serverId: options?.serverId,
        existingPreloadData: options?.preloadData,
      };
      try {
        const preloadData = await app.lifecycle.onBeforeCreate(context);
        newWindow.setPreloadData(preloadData);
      } catch (error) {
        log.error(`Preload failed for "${appId}"`, error);
        newWindow.setPreloadState('error');
      }
    } else {
      // No preloading needed
      newWindow.setPreloadState('ready');
    }

    // Call onAfterCreate hook
    if (app.lifecycle?.onAfterCreate) {
      app.lifecycle.onAfterCreate(newWindow);
    }

    return newWindow;
  }

  /**
   * Close a window
   */
  close(windowId: string): void {
    const window = this.windows.get(windowId);
    if (!window) return;

    const app = this.registry.get(window.appId);
    if (app?.lifecycle?.onBeforeClose) {
      const shouldClose = app.lifecycle.onBeforeClose(window);
      if (!shouldClose) return; // Cancel close
    }

    this.windows.remove(windowId);
    this._activationOrder = this._activationOrder.filter(id => id !== windowId); // ✅ 出栈

    // Update active window
    if (this.activeWindowId === windowId) {
      this.activeWindowId = this.pickNextActive(windowId);
    }

    // Emit closed event
    this.eventBus.emit({
      type: 'window:closed',
      windowId,
      timestamp: Date.now(),
    });

    // Call onAfterClose hook
    if (app?.lifecycle?.onAfterClose) {
      app.lifecycle.onAfterClose(window);
    }
  }

  /**
   * Focus a window (bring to front)
   */
  focus(windowId: string): void {
    const window = this.windows.get(windowId);
    if (!window) return;

    // Restore if minimized
    if (window.minimized) {
      window.setMinimized(false);
    }

    // ✅ MRU 层叠：聚焦=移到激活栈顶
    this.pushActivationTop(windowId);
    this.activeWindowId = windowId;

    // Emit focused event
    this.eventBus.emit({
      type: 'window:focused',
      windowId,
      timestamp: Date.now(),
    });
  }

  /**
   * Minimize a window
   */
  minimize(windowId: string): void {
    const window = this.windows.get(windowId);
    if (!window) return;

    window.setMinimized(true);

    // Update active window
    if (this.activeWindowId === windowId) {
      this.activeWindowId = this.pickNextActive(windowId);
    }

    // Emit minimized event
    this.eventBus.emit({
      type: 'window:minimized',
      windowId,
      timestamp: Date.now(),
    });
  }

  /**
   * Restore a minimized window
   */
  restore(windowId: string): void {
    const window = this.windows.get(windowId);
    if (!window) return;

    window.setMinimized(false);
    // ✅ MRU 层叠：恢复=移到激活栈顶
    this.pushActivationTop(windowId);
    this.activeWindowId = windowId;

    // Emit restored event
    this.eventBus.emit({
      type: 'window:restored',
      windowId,
      timestamp: Date.now(),
    });
  }

  /**
   * ✅ Maximize a window
   * Saves current position/size, then applies maximized dimensions
   */
  maximize(windowId: string, maxPosition: { x: number; y: number }, maxSize: { width: number; height: number }): void {
    const window = this.windows.get(windowId);
    if (!window) return;

    // ✅ setMaximized 内部会自动保存当前位置/尺寸
    window.setMaximized(true);
    window.setPosition(maxPosition);
    window.setSize(maxSize);

    // Emit maximized event
    this.eventBus.emit({
      type: 'window:maximized',
      windowId,
      timestamp: Date.now(),
    });
  }

  /**
   * ✅ Unmaximize a window
   * Restores saved position/size (or defaults to center)
   */
  unmaximize(windowId: string): void {
    const window = this.windows.get(windowId);
    if (!window) return;

    // ✅ setMaximized(false) 内部会自动恢复保存的位置/尺寸
    window.setMaximized(false);

    // Emit unmaximized event
    this.eventBus.emit({
      type: 'window:unmaximized',
      windowId,
      timestamp: Date.now(),
    });
  }

  /**
   * ✅ Snap 窗口(半屏/四分之一屏,非 maximize)
   * 保存当前位置/尺寸,应用目标 rect
   */
  /**
   * ✅ 主窗口（viewport）尺寸变化后的全量窗口自适应：
   * - maximized：重新铺满新工作区（maximized 的实现是 bounds=工作区尺寸，
   *   viewport 变化后不重设会残留旧尺寸/溢出）
   * - snap：按 snapZone 在新工作区重算几何（computeSnapRect）
   * - 普通窗口：clamp 位置保证最小可见区（与恢复会话时的约束逻辑一致）
   */
  handleViewportResize(): void {
    const vw = window.innerWidth;
    const vh = window.innerHeight - 32; // 工作区高度（扣除 TopBar，与 createApp 同款）
    const MIN_VISIBLE = 100;

    this.windows.getAll().forEach(win => {
      if (win.minimized) return;

      if (win.maximized) {
        win.setPosition({ x: 0, y: 0 });
        win.setSize({ width: vw, height: vh });
        this.eventBus.emit({ type: 'window:resized', windowId: win.id, timestamp: Date.now() });
        return;
      }

      const zone = win.snapZone;
      if (zone) {
        // 'top' zone 即最大化通道，正常不会同时存在于非 maximized 窗口；
        // 其余 zone 按新工作区重算半屏/四分之一屏几何
        const rect = computeSnapRect(zone, { width: vw, height: vh });
        win.setPosition({ x: rect.x, y: rect.y });
        win.setSize({ width: rect.width, height: rect.height });
        this.eventBus.emit({ type: 'window:resized', windowId: win.id, timestamp: Date.now() });
        return;
      }

      // 普通窗口：尺寸收缩 + 位置 clamp。
      // 尺寸：超过新工作区则收缩（不高于应用注册的 minSize 下限）；
      // 放大方向不跟随——保留用户设定的窗口尺寸（桌面系统惯例）。
      const app = this.getApp(win.appId);
      const minW = app?.minSize.width ?? 400;
      const minH = app?.minSize.height ?? 300;
      const { x, y } = win.position;
      const { width, height } = win.size;
      const newW = Math.max(minW, Math.min(width, vw));
      const newH = Math.max(minH, Math.min(height, vh));
      if (newW !== width || newH !== height) {
        win.setSize({ width: newW, height: newH });
        this.eventBus.emit({ type: 'window:resized', windowId: win.id, timestamp: Date.now() });
      }
      const nx = Math.max(-(newW - MIN_VISIBLE), Math.min(x, vw - MIN_VISIBLE));
      const ny = Math.max(0, Math.min(y, vh - MIN_VISIBLE));
      if (nx !== x || ny !== y) {
        win.setPosition({ x: nx, y: ny });
        this.eventBus.emit({ type: 'window:moved', windowId: win.id, timestamp: Date.now() });
      }
    });
  }

  snap(windowId: string, zone: SnapZone, rect: { x: number; y: number; width: number; height: number }): void {
    const window = this.windows.get(windowId);
    if (!window) return;

    window.setSnap(zone, rect);

    this.eventBus.emit({
      type: 'window:snapped',
      windowId,
      timestamp: Date.now(),
    });
  }

  /**
   * ✅ 取消 snap,恢复保存的位置/尺寸
   */
  unsnap(windowId: string): void {
    const window = this.windows.get(windowId);
    if (!window) return;

    window.unsetSnap();

    this.eventBus.emit({
      type: 'window:unsnapped',
      windowId,
      timestamp: Date.now(),
    });
  }

  /**
   * Get window by ID
   */
  getById(windowId: string): Window | undefined {
    return this.windows.get(windowId);
  }

  /**
   * Get windows by app ID
   */
  getByAppId(appId: string): Window[] {
    return this.windows.getByAppId(appId);
  }

  /**
   * Get all windows
   */
  getAll(): Window[] {
    return this.windows.getAll();
  }

  /**
   * Get the active window
   */
  getActive(): Window | undefined {
    return this.activeWindowId ? this.windows.get(this.activeWindowId) : undefined;
  }

  /**
   * Subscribe to window events
   */
  on(eventType: WindowEventType, handler: (event: WindowEvent) => void): () => void {
    return this.eventBus.on(eventType, handler);
  }

  /**
   * Subscribe to any event (for React force update)
   */
  onAny(handler: () => void): () => void {
    return this.eventBus.onAny(handler);
  }

  /**
   * Emit an event
   */
  emit(event: WindowEvent): void {
    this.eventBus.emit(event);
  }

  // ── 会话快照（刷新存活、关闭项目随窗口销亡）──────────
  // 与被移除的磁盘持久化（b8b3e28）不同：存储介质是 sessionStorage，
  // 语义为「仅会话内」——刷新恢复、关项目干净桌面，无需设置开关

  private saveTimer: number | null = null;
  private sessionLoaded = false;

  /**
   * 快照当前全部窗口到 sessionStorage（300ms 防抖；Context 的 onAny 驱动）
   */
  saveSession(): void {
    if (this.saveTimer !== null) clearTimeout(this.saveTimer);
    this.saveTimer = window.setTimeout(() => {
      this.saveTimer = null;
      try {
        // v2 快照：windows + order（激活栈=焦点顺序，恢复时末位即当时激活窗口）
        const snapshot = {
          windows: this.windows.getAll().map((w) => w.serialize()),
          order: this._activationOrder.slice(),
        };
        sessionStorage.setItem(SESSION_KEY, JSON.stringify(snapshot));
      } catch (error) {
        log.error('保存窗口会话快照失败:', error);
      }
    }, 300);
  }

  /**
   * 从 sessionStorage 恢复窗口（几何 + 应用上下文 preloadData）。
   * 位置按工作区边界钳制（沿用旧持久化实现的约束）；每实例只恢复一次。
   * @returns 恢复的窗口 id 列表（调用方据此通知订阅方重渲染）
   */
  loadSession(): string[] {
    if (this.sessionLoaded) return [];
    this.sessionLoaded = true;
    try {
      const stored = sessionStorage.getItem(SESSION_KEY);
      if (!stored) return [];
      // v1 快照为裸数组（无焦点顺序）；v2 为 { windows, order }
      const parsed = JSON.parse(stored) as
        | { windows?: PersistedWindowData[]; order?: string[] }
        | PersistedWindowData[];
      const data = Array.isArray(parsed) ? parsed : (parsed.windows ?? []);
      const orderHint = Array.isArray(parsed)
        ? data.map((d) => d.id)
        : (parsed.order ?? data.map((d) => d.id));
      if (data.length === 0) return [];

      // 边界约束（沿用旧持久化实现）：恢复位置不得超出工作区
      const containerWidth = window.innerWidth;
      const containerHeight = window.innerHeight - 32; // TopBar 高度
      const MIN_VISIBLE = 100;
      const restored = new Set<string>();
      const restoredPerApp = new Map<string, number>();

      for (const windowData of data) {
        if (!windowData?.id || !windowData?.appId) continue;
        // 单实例约束在恢复路径同样生效：历史快照可能包含重复窗口（create
        // 不强制单实例，约束在调用方），全量复活会让多个实例互抢同一会话
        // （browser 的收养/关闭链路被对端重建打断）
        const restoredApp = this.registry.get(windowData.appId);
        const allowed = restoredApp?.allowMultipleInstances ? Number.POSITIVE_INFINITY : 1;
        const sameAppCount = restoredPerApp.get(windowData.appId) ?? 0;
        if (sameAppCount >= allowed) continue;
        const window = WindowClass.deserialize(windowData);
        const minX = -(windowData.size.width - MIN_VISIBLE);
        const maxX = containerWidth - MIN_VISIBLE;
        const maxY = containerHeight - MIN_VISIBLE;
        window.setPosition({
          x: Math.max(minX, Math.min(windowData.position.x, maxX)),
          y: Math.max(0, Math.min(windowData.position.y, maxY)),
        });
        this.windows.add(window);
        restoredPerApp.set(windowData.appId, sameAppCount + 1);
        restored.add(window.id);
      }
      if (restored.size === 0) return [];

      // 激活栈按快照的焦点顺序重建（末位=快照时的激活窗口），不按创建序；
      // order 之外仍有恢复窗口时按创建序补齐（部分损坏兜底）
      for (const id of orderHint) {
        if (restored.has(id) && !this._activationOrder.includes(id)) {
          this._activationOrder.push(id);
        }
      }
      for (const w of this.windows.getAll()) {
        if (!this._activationOrder.includes(w.id)) this._activationOrder.push(w.id);
      }
      if (this._activationOrder.length > 0) {
        this.activeWindowId = this._activationOrder[this._activationOrder.length - 1];
      }
      log.info(`已从会话快照恢复 ${restored.size} 个窗口`);
      return [...restored];
    } catch (error) {
      log.error('恢复窗口会话快照失败:', error);
      // 损坏快照清除，避免每次刷新都失败
      sessionStorage.removeItem(SESSION_KEY);
      return [];
    }
  }
}