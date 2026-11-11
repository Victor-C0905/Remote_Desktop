// src/window-system/core/Window.ts

import { LayoutType } from '../types';
import type { SnapZone } from '../../components/window-shell/aeroSnap';

/**
 * 会话快照条目（存 sessionStorage：刷新存活、关闭项目随窗口销亡）
 */
export interface PersistedWindowData {
  id: string;
  appId: string;
  position: { x: number; y: number };
  size: { width: number; height: number };
  minimized: boolean;
  layout: LayoutType;
  /** 应用上下文（文件管理器目录、编辑器路径等；纯 JSON 深拷贝） */
  preloadData: any;
  /** 运行时布局状态（刷新恢复时保留；旧快照缺省为默认值） */
  maximized?: boolean;
  snapZone?: SnapZone | null;
  preMaximizeState?: { position: { x: number; y: number }; size: { width: number; height: number } } | null;
  preSnapState?: { position: { x: number; y: number }; size: { width: number; height: number } } | null;
}

/**
 * Window class - encapsulates window state and behavior
 */
export class Window {
  readonly id: string;
  readonly appId: string;

  private _position: { x: number; y: number };
  private _size: { width: number; height: number };
  private _minimized: boolean;
  private _layout: LayoutType;

  // 运行时状态
  private _preloadState: 'loading' | 'ready' | 'error';
  private _preloadData: any;
  private _maximized: boolean; // ✅ 最大化状态
  private _preMaximizeState: { // ✅ 最大化前的位置和尺寸
    position: { x: number; y: number };
    size: { width: number; height: number };
  } | null;
  private _snapZone: SnapZone | null = null; // ✅ snap 区状态(半屏/四分之一屏)
  private _preSnapState: { // ✅ snap 前的位置/尺寸
    position: { x: number; y: number };
    size: { width: number; height: number };
  } | null;

  // MRU 层叠不依赖窗口自身字段：激活顺序由 WindowManager._activationOrder
  // 栈统一维护（末尾=最近激活），Window 只保留窗口本体状态。

  constructor(
    id: string,
    appId: string,
    position: { x: number; y: number },
    size: { width: number; height: number }
  ) {
    this.id = id;
    this.appId = appId;
    this._position = position;
    this._size = size;
    this._minimized = false;
    this._layout = 'free';
    this._preloadState = 'loading';
    this._preloadData = null;
    this._maximized = false;
    this._preMaximizeState = null;
    this._snapZone = null;
    this._preSnapState = null;
  }

  // Position getters/setters
  get position(): { x: number; y: number } {
    return this._position;
  }

  setPosition(pos: { x: number; y: number }): void {
    this._position = pos;
  }

  // Size getters/setters
  get size(): { width: number; height: number } {
    return this._size;
  }

  setSize(size: { width: number; height: number }): void {
    this._size = size;
  }

  // Minimized state
  get minimized(): boolean {
    return this._minimized;
  }

  setMinimized(minimized: boolean): void {
    this._minimized = minimized;
  }

  // Layout state
  get layout(): LayoutType {
    return this._layout;
  }

  setLayout(layout: LayoutType): void {
    this._layout = layout;
  }

  // Preload state
  get preloadState(): 'loading' | 'ready' | 'error' {
    return this._preloadState;
  }

  setPreloadState(state: 'loading' | 'ready' | 'error'): void {
    this._preloadState = state;
  }

  get preloadData(): any {
    return this._preloadData;
  }

  setPreloadData(data: any): void {
    this._preloadData = data;
    this._preloadState = 'ready';
  }

  // ✅ Maximized state (runtime state, not persisted)
  get maximized(): boolean {
    return this._maximized;
  }

  /**
   * Set maximized state
   * If maximizing, save current position/size for later restoration
   * If unmaximizing, restore saved position/size (if available)
   */
  setMaximized(maximized: boolean): void {
    if (maximized && !this._maximized) {
      // ✅ 互斥:snap 与 maximize 互斥。从 snap 状态进入 maximize 时,
      //    复用 snap 已保存的 preSnapState(原始正常态)作为 preMaximizeState,
      //    避免把 snap rect(半屏/四分之一尺寸)误当作正常态保存,
      //    并清空 snap 状态(_snapZone / _preSnapState)。
      if (this._snapZone !== null && this._preSnapState) {
        this._preMaximizeState = {
          position: { ...this._preSnapState.position },
          size: { ...this._preSnapState.size },
        };
        this._snapZone = null;
        this._preSnapState = null;
      } else {
        // ✅ 最大化前：保存当前位置和尺寸(正常态)
        this._preMaximizeState = {
          position: { ...this._position },
          size: { ...this._size },
        };
      }
    } else if (!maximized && this._maximized && this._preMaximizeState) {
      // ✅ 恢复最大化前的位置和尺寸
      this._position = { ...this._preMaximizeState.position };
      this._size = { ...this._preMaximizeState.size };
      this._preMaximizeState = null; // 清除保存的状态
    }
    this._maximized = maximized;
  }

  /**
   * Get pre-maximize state (for restoration)
   */
  get preMaximizeState(): { position: { x: number; y: number }; size: { width: number; height: number } } | null {
    return this._preMaximizeState;
  }

  // ✅ Snap 状态(半屏/四分之一屏,与 maximize 互斥)
  get snapZone(): SnapZone | null {
    return this._snapZone;
  }

  get preSnapState(): { position: { x: number; y: number }; size: { width: number; height: number } } | null {
    return this._preSnapState;
  }

  /**
   * 进入半屏/四分之一屏 snap。
   * 首次 snap 时保存当前位置/尺寸;已 snap 状态再换 zone 不覆盖 preSnapState。
   */
  setSnap(zone: SnapZone, rect: { x: number; y: number; width: number; height: number }): void {
    // ✅ 互斥:snap 与 maximize 互斥。从 maximize 状态进入 snap 时,
    //    复用 maximize 已保存的 preMaximizeState(原始正常态)作为 preSnapState,
    //    避免把最大化尺寸误当作正常态保存,并清空 maximize 状态。
    if (this._maximized && this._preMaximizeState) {
      this._preSnapState = {
        position: { ...this._preMaximizeState.position },
        size: { ...this._preMaximizeState.size },
      };
      this._maximized = false;
      this._preMaximizeState = null;
    } else if (this._snapZone === null) {
      // 首次 snap(从正常态):保存当前位置/尺寸
      this._preSnapState = {
        position: { ...this._position },
        size: { ...this._size },
      };
    }
    // 已 snap 状态换 zone:_preSnapState 不被覆盖,只换 rect
    this._snapZone = zone;
    this._position = { x: rect.x, y: rect.y };
    this._size = { width: rect.width, height: rect.height };
  }

  /** 退出 snap,恢复保存的位置/尺寸。 */
  unsetSnap(): void {
    if (this._preSnapState) {
      this._position = { ...this._preSnapState.position };
      this._size = { ...this._preSnapState.size };
      this._preSnapState = null;
    }
    this._snapZone = null;
  }

  /**
   * 序列化为会话快照。preloadData 深拷贝为纯 JSON（函数/循环引用丢弃——
   * 撤销闭包这类不可序列化的运行时产物不进快照）。
   */
  serialize(): PersistedWindowData {
    let preloadData: any = null;
    if (this._preloadData != null) {
      try {
        preloadData = JSON.parse(JSON.stringify(this._preloadData));
      } catch {
        preloadData = null;
      }
    }
    return {
      id: this.id,
      appId: this.appId,
      position: { ...this._position },
      size: { ...this._size },
      minimized: this._minimized,
      layout: this._layout,
      preloadData,
      // 布局状态随快照往返：否则刷新后最大化窗口会变成「普通窗口 + 旧工作区
      // 尺寸」——表现为最大化丢失且几何停在旧工作区（第二轮视觉测试 F2-A
      // 的同类症状；旧快照无这些字段时按默认值恢复，向后兼容）
      maximized: this._maximized,
      snapZone: this._snapZone,
      preMaximizeState: this._preMaximizeState
        ? {
            position: { ...this._preMaximizeState.position },
            size: { ...this._preMaximizeState.size },
          }
        : null,
      preSnapState: this._preSnapState
        ? {
            position: { ...this._preSnapState.position },
            size: { ...this._preSnapState.size },
          }
        : null,
    };
  }

  /**
   * 从会话快照反序列化。几何与最大化/吸附状态照快照恢复（最大化/吸附的
   * 实际矩形由 WindowManager 按恢复时的工作区重新归一化，不沿用旧尺寸）；
   * preloadData 存在时置 ready——应用挂载即拿到上下文（文件管理器目录、
   * 编辑器路径等），不重走加载态。
   */
  static deserialize(data: PersistedWindowData): Window {
    const window = new Window(data.id, data.appId, data.position, data.size);
    window._minimized = data.minimized;
    window._layout = data.layout;
    window._maximized = data.maximized === true;
    window._snapZone = data.snapZone ?? null;
    window._preMaximizeState = data.preMaximizeState
      ? {
          position: { ...data.preMaximizeState.position },
          size: { ...data.preMaximizeState.size },
        }
      : null;
    window._preSnapState = data.preSnapState
      ? {
          position: { ...data.preSnapState.position },
          size: { ...data.preSnapState.size },
        }
      : null;
    if (data.preloadData != null) {
      window._preloadData = data.preloadData;
      window._preloadState = 'ready';
    }
    return window;
  }
}