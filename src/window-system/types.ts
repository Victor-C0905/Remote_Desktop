// src/window-system/types.ts

/**
 * Window system type definitions
 */

import { Window as AppWindow } from './core/Window';

// Persistable window data (stored in localStorage)
export interface PersistedWindowData {
  id: string;
  appId: string;
  position: { x: number; y: number };
  size: { width: number; height: number };
  minimized: boolean;
  layout: 'free' | 'snap' | 'split';
  createdAt: number;
  updatedAt: number;
}

// Window creation options
export interface CreateWindowOptions {
  position?: { x: number; y: number };
  size?: { width: number; height: number };
  serverId?: string;
  preloadData?: any;  // If provided, skips onBeforeCreate hook
}

// Context passed to onBeforeCreate lifecycle hook
export interface CreateContext {
  serverId?: string;
  existingPreloadData?: any;
}

// Window lifecycle hooks
export interface WindowLifecycle {
  onBeforeCreate?: (context: CreateContext) => Promise<any>;
  onAfterCreate?: (window: AppWindow) => void;
  onBeforeClose?: (window: AppWindow) => boolean;
  onAfterClose?: (window: AppWindow) => void;
}

// Application definition for registry
export interface AppDefinition {
  id: string;
  title: string;
  icon: string;
  defaultSize: { width: number; height: number };
  minSize: { width: number; height: number };
  allowMultipleInstances: boolean;
  lifecycle?: WindowLifecycle;
  component: React.ComponentType<{ windowId: string; preloadData?: any }>;

  // 桌面/Dock 显示配置（单一数据源，Desktop/Dock/Overview 从此派生）
  showOnDesktop?: boolean;    // 是否显示在桌面图标区，默认 true
  showOnDock?: boolean;       // 是否显示在 Dock，默认 true
  desktopLabel?: string;      // 桌面图标标签（默认用 title）
  dockLabel?: string;         // Dock 标签（默认用 title）
}

// Layout types
export type LayoutType = 'free' | 'snap' | 'split';

// Window event types
export type WindowEventType =
  | 'window:created'
  | 'window:closed'
  | 'window:focused'
  | 'window:minimized'
  | 'window:restored'
  | 'window:resized'
  | 'window:moved'
  | 'window:layout-changed'
  | 'window:maximized'      // ✅ 新增：窗口最大化事件
  | 'window:unmaximized';   // ✅ 新增：窗口取消最大化事件

// Window event structure
export interface WindowEvent {
  type: WindowEventType;
  windowId: string;
  timestamp: number;
  payload?: any;
}

// Event handler type
export type EventHandler = (event: WindowEvent) => void;

// Unsubscribe function type
export type Unsubscribe = () => void;

// Window state for React hooks
export interface WindowState {
  id: string;
  appId: string;
  title: string;
  position: { x: number; y: number };
  size: { width: number; height: number };
  minimized: boolean;
  maximized: boolean; // ✅ 新增：最大化状态
  focused: boolean;
  preloadState: 'loading' | 'ready' | 'error';
  preloadData: any;
}

// Window manager interface for hooks
export interface IWindowManager {
  create(appId: string, options?: CreateWindowOptions): Promise<AppWindow>;
  close(windowId: string): void;
  focus(windowId: string): void;
  minimize(windowId: string): void;
  restore(windowId: string): void;
  maximize(windowId: string, maxPosition: { x: number; y: number }, maxSize: { width: number; height: number }): void;
  unmaximize(windowId: string): void;
  getById(windowId: string): AppWindow | undefined;
  getByAppId(appId: string): AppWindow[];
  getAll(): AppWindow[];
  getActive(): AppWindow | undefined;
  getApp(appId: string): AppDefinition | undefined;
  getRegisteredApps(): AppDefinition[];
  on(eventType: WindowEventType, handler: EventHandler): Unsubscribe;
  onAny(handler: () => void): Unsubscribe;
  emit(event: WindowEvent): void;
  save(): void;
  load(): void;
}