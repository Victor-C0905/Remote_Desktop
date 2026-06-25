// src/window-system/core/Window.ts

import { PersistedWindowData, LayoutType } from '../types';

/**
 * Window class - encapsulates window state and behavior
 * Separates persistable state from runtime state
 */
export class Window {
  readonly id: string;
  readonly appId: string;

  // Persistable state (saved to localStorage)
  private _position: { x: number; y: number };
  private _size: { width: number; height: number };
  private _minimized: boolean;
  private _layout: LayoutType;
  private _createdAt: number;
  private _updatedAt: number;

  // Runtime state (not persisted)
  private _activatedAt: number;
  private _preloadState: 'loading' | 'ready' | 'error';
  private _preloadData: any;

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
    this._createdAt = Date.now();
    this._updatedAt = Date.now();
    this._activatedAt = Date.now();
    this._preloadState = 'loading';
    this._preloadData = null;
  }

  // Position getters/setters
  get position(): { x: number; y: number } {
    return this._position;
  }

  setPosition(pos: { x: number; y: number }): void {
    this._position = pos;
    this._updatedAt = Date.now();
  }

  // Size getters/setters
  get size(): { width: number; height: number } {
    return this._size;
  }

  setSize(size: { width: number; height: number }): void {
    this._size = size;
    this._updatedAt = Date.now();
  }

  // Minimized state
  get minimized(): boolean {
    return this._minimized;
  }

  setMinimized(minimized: boolean): void {
    this._minimized = minimized;
    this._updatedAt = Date.now();
  }

  // Layout state
  get layout(): LayoutType {
    return this._layout;
  }

  setLayout(layout: LayoutType): void {
    this._layout = layout;
    this._updatedAt = Date.now();
  }

  // Activation time (for z-index ordering)
  get activatedAt(): number {
    return this._activatedAt;
  }

  activate(): void {
    this._activatedAt = Date.now();
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

  // Timestamps
  get createdAt(): number {
    return this._createdAt;
  }

  get updatedAt(): number {
    return this._updatedAt;
  }

  /**
   * Serialize window state for persistence
   * Only includes persistable state (not runtime state)
   */
  serialize(): PersistedWindowData {
    return {
      id: this.id,
      appId: this.appId,
      position: this._position,
      size: this._size,
      minimized: this._minimized,
      layout: this._layout,
      createdAt: this._createdAt,
      updatedAt: this._updatedAt,
    };
  }

  /**
   * Deserialize window from persisted data
   * Runtime state is initialized to defaults
   */
  static deserialize(data: PersistedWindowData): Window {
    const window = new Window(
      data.id,
      data.appId,
      data.position,
      data.size
    );
    window._minimized = data.minimized;
    window._layout = data.layout;
    window._createdAt = data.createdAt;
    window._updatedAt = data.updatedAt;
    // Runtime state defaults
    window._activatedAt = Date.now();
    window._preloadState = 'loading';
    window._preloadData = null;
    return window;
  }
}