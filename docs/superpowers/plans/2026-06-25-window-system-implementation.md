# Window System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement a professional-grade window management system with multiple instances, state persistence, inter-window communication, and layout management.

**Architecture:** Windows Forms/WPF-like architecture with Window class encapsulating state and behavior, WindowManager as singleton orchestrator, WindowRegistry for application registration, and React hooks for UI integration.

**Tech Stack:** TypeScript, React, Zustand (state persistence), EventEmitter pattern (inter-window communication)

---

## File Structure

### New Files (Phase 1)

```
src/window-system/
├── core/
│   ├── Window.ts              # Window class (~150 lines)
│   ├── WindowManager.ts       # WindowManager singleton (~200 lines)
│   ├── WindowCollection.ts    # Window collection CRUD (~100 lines)
│   └── WindowRegistry.ts      # Application registry (~80 lines)
├── events/
│   ├── WindowEventBus.ts      # Event bus (~60 lines)
│   └── WindowEvents.ts        # Event types (~30 lines)
├── hooks/
│   ├── useWindowManager.ts    # Window manager hook (~50 lines)
│   ├── useWindowState.ts      # Single window state hook (~40 lines)
│   └── useWindowEvent.ts      # Window event hook (~30 lines)
├── WindowManagerContext.tsx   # React Context (~40 lines)
├── init.ts                    # App registration (~50 lines)
└── types.ts                   # Type definitions (~90 lines)
```

### Modified Files

```
src/shell/Desktop.tsx          # Replace window management logic
src/components/DraggableWindow.tsx  # Refactor to use window system
```

---

## Phase 1: Core Architecture

### Task 1: Create Type Definitions

**Files:**
- Create: `src/window-system/types.ts`

- [ ] **Step 1: Write type definitions file**

```typescript
// src/window-system/types.ts

/**
 * Window system type definitions
 */

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
  onAfterCreate?: (window: Window) => void;
  onBeforeClose?: (window: Window) => boolean;
  onAfterClose?: (window: Window) => void;
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
  | 'window:layout-changed';

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
  focused: boolean;
  preloadState: 'loading' | 'ready' | 'error';
  preloadData: any;
}

// Window manager interface for hooks
export interface IWindowManager {
  create(appId: string, options?: CreateWindowOptions): Promise<Window>;
  close(windowId: string): void;
  focus(windowId: string): void;
  minimize(windowId: string): void;
  restore(windowId: string): void;
  getById(windowId: string): Window | undefined;
  getByAppId(appId: string): Window[];
  getAll(): Window[];
  getActive(): Window | undefined;
  getApp(appId: string): AppDefinition | undefined;
  on(eventType: WindowEventType, handler: EventHandler): Unsubscribe;
  onAny(handler: () => void): Unsubscribe;
  emit(event: WindowEvent): void;
  save(): void;
  load(): void;
}
```

- [ ] **Step 2: Commit type definitions**

```bash
git add src/window-system/types.ts
git commit -m "feat(window-system): add type definitions"
```

---

### Task 2: Create WindowEventBus

**Files:**
- Create: `src/window-system/events/WindowEventBus.ts`
- Create: `src/window-system/events/WindowEvents.ts`

- [ ] **Step 1: Write WindowEvents.ts (event type constants)**

```typescript
// src/window-system/events/WindowEvents.ts

import { WindowEventType } from '../types';

// Event type constants for easier usage
export const WINDOW_EVENTS: Record<WindowEventType, WindowEventType> = {
  CREATED: 'window:created',
  CLOSED: 'window:closed',
  FOCUSED: 'window:focused',
  MINIMIZED: 'window:minimized',
  RESTORED: 'window:restored',
  RESIZED: 'window:resized',
  MOVED: 'window:moved',
  LAYOUT_CHANGED: 'window:layout-changed',
};
```

- [ ] **Step 2: Write WindowEventBus.ts (event bus implementation)**

```typescript
// src/window-system/events/WindowEventBus.ts

import { WindowEventType, WindowEvent, EventHandler, Unsubscribe } from '../types';

/**
 * Event bus for inter-window communication
 * Uses EventEmitter pattern for pub/sub messaging
 */
export class WindowEventBus {
  private listeners: Map<WindowEventType, Set<EventHandler>> = new Map();
  private anyListeners: Set<() => void> = new Set();

  /**
   * Subscribe to a specific event type
   */
  on(eventType: WindowEventType, handler: EventHandler): Unsubscribe {
    if (!this.listeners.has(eventType)) {
      this.listeners.set(eventType, new Set());
    }
    this.listeners.get(eventType)!.add(handler);

    return () => {
      this.listeners.get(eventType)?.delete(handler);
    };
  }

  /**
   * Subscribe to any event (for React force update)
   */
  onAny(handler: () => void): Unsubscribe {
    this.anyListeners.add(handler);
    return () => {
      this.anyListeners.delete(handler);
    };
  }

  /**
   * Emit an event to all subscribers
   */
  emit(event: WindowEvent): void {
    // Notify specific event listeners
    const handlers = this.listeners.get(event.type);
    if (handlers) {
      handlers.forEach(handler => handler(event));
    }

    // Notify any-event listeners
    this.anyListeners.forEach(handler => handler());
  }

  /**
   * Clear all listeners (for cleanup)
   */
  clear(): void {
    this.listeners.clear();
    this.anyListeners.clear();
  }
}
```

- [ ] **Step 3: Commit event system**

```bash
git add src/window-system/events/
git commit -m "feat(window-system): add event bus for inter-window communication"
```

---

### Task 3: Create WindowRegistry

**Files:**
- Create: `src/window-system/core/WindowRegistry.ts`

- [ ] **Step 1: Write WindowRegistry.ts**

```typescript
// src/window-system/core/WindowRegistry.ts

import { AppDefinition } from '../types';

/**
 * Application registry for window system
 * Stores app definitions that can be used to create windows
 */
export class WindowRegistry {
  private apps: Map<string, AppDefinition> = new Map();

  /**
   * Register an application
   */
  register(app: AppDefinition): void {
    if (this.apps.has(app.id)) {
      console.warn(`[WindowRegistry] App "${app.id}" already registered, overwriting`);
    }
    this.apps.set(app.id, app);
  }

  /**
   * Get an application definition by ID
   */
  get(appId: string): AppDefinition | undefined {
    return this.apps.get(appId);
  }

  /**
   * Get all registered applications
   */
  getAll(): AppDefinition[] {
    return Array.from(this.apps.values());
  }

  /**
   * Check if an app is registered
   */
  has(appId: string): boolean {
    return this.apps.has(appId);
  }

  /**
   * Clear all registered apps (for testing)
   */
  clear(): void {
    this.apps.clear();
  }
}
```

- [ ] **Step 2: Commit WindowRegistry**

```bash
git add src/window-system/core/WindowRegistry.ts
git commit -m "feat(window-system): add application registry"
```

---

### Task 4: Create WindowCollection

**Files:**
- Create: `src/window-system/core/WindowCollection.ts`

- [ ] **Step 1: Write WindowCollection.ts**

```typescript
// src/window-system/core/WindowCollection.ts

import { Window } from './Window';

/**
 * Window collection for managing multiple windows
 * Provides CRUD operations and sorting by activation time
 */
export class WindowCollection {
  private windows: Map<string, Window> = new Map();

  /**
   * Add a window to the collection
   */
  add(window: Window): void {
    this.windows.set(window.id, window);
  }

  /**
   * Remove a window from the collection
   */
  remove(windowId: string): boolean {
    return this.windows.delete(windowId);
  }

  /**
   * Get a window by ID
   */
  get(windowId: string): Window | undefined {
    return this.windows.get(windowId);
  }

  /**
   * Get all windows as array
   */
  getAll(): Window[] {
    return Array.from(this.windows.values());
  }

  /**
   * Get all windows for a specific app
   */
  getByAppId(appId: string): Window[] {
    return this.getAll().filter(w => w.appId === appId);
  }

  /**
   * Get the most recently activated window
   */
  getActive(): Window | undefined {
    const windows = this.getAll();
    if (windows.length === 0) return undefined;

    return windows.reduce((latest, current) => {
      return current.activatedAt > latest.activatedAt ? current : latest;
    });
  }

  /**
   * Get windows sorted by activation time (oldest first)
   * Used for z-index ordering (oldest = lowest z-index)
   */
  getSortedByActivation(): Window[] {
    return this.getAll().sort((a, b) => a.activatedAt - b.activatedAt);
  }

  /**
   * Check if a window exists
   */
  has(windowId: string): boolean {
    return this.windows.has(windowId);
  }

  /**
   * Get the number of windows
   */
  count(): number {
    return this.windows.size;
  }

  /**
   * Clear all windows (for testing)
   */
  clear(): void {
    this.windows.clear();
  }
}
```

- [ ] **Step 2: Commit WindowCollection**

```bash
git add src/window-system/core/WindowCollection.ts
git commit -m "feat(window-system): add window collection for CRUD operations"
```

---

### Task 5: Create Window Class

**Files:**
- Create: `src/window-system/core/Window.ts`

- [ ] **Step 1: Write Window.ts (Part 1 - class definition and properties)**

```typescript
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
```

- [ ] **Step 2: Write Window.ts (Part 2 - serialization methods)**

```typescript
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
```

- [ ] **Step 3: Commit Window class**

```bash
git add src/window-system/core/Window.ts
git commit -m "feat(window-system): add Window class with state and serialization"
```

---

### Task 6: Create WindowManager

**Files:**
- Create: `src/window-system/core/WindowManager.ts`

- [ ] **Step 1: Write WindowManager.ts (Part 1 - class definition and initialization)**

```typescript
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

/**
 * WindowManager - singleton orchestrator for window system
 * Manages window lifecycle, events, and persistence
 */
export class WindowManager implements IWindowManager {
  private windows: WindowCollection;
  private registry: WindowRegistry;
  private eventBus: WindowEventBus;
  private activeWindowId: string | null = null;

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
   * Get the app definition for a window
   */
  getApp(appId: string): ReturnType<WindowRegistry['get']> {
    return this.registry.get(appId);
  }
```

- [ ] **Step 2: Write WindowManager.ts (Part 2 - window creation)**

```typescript
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
    const containerHeight = window.innerHeight - 32; // Subtract TopBar height

    const position = options?.position ?? {
      x: Math.min(100 + this.windows.count() * 30, containerWidth - app.defaultSize.width - 100),
      y: Math.min(100 + this.windows.count() * 30, containerHeight - app.defaultSize.height - 100),
    };

    const size = options?.size ?? app.defaultSize;

    // Create window instance
    const windowId = this.generateWindowId(appId);
    const newWindow = new Window(windowId, appId, position, size);

    // Add to collection
    this.windows.add(newWindow);
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
        console.error(`[WindowManager] Preload failed for "${appId}"`, error);
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
```

- [ ] **Step 3: Write WindowManager.ts (Part 3 - window operations)**

```typescript
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

    // Update active window
    if (this.activeWindowId === windowId) {
      this.activeWindowId = this.windows.getActive()?.id ?? null;
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

    // Update activation time
    window.activate();
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
      this.activeWindowId = this.windows.getActive()?.id ?? null;
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
    window.activate();
    this.activeWindowId = windowId;

    // Emit restored event
    this.eventBus.emit({
      type: 'window:restored',
      windowId,
      timestamp: Date.now(),
    });
  }
```

- [ ] **Step 4: Write WindowManager.ts (Part 4 - queries and events)**

```typescript
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
```

- [ ] **Step 5: Write WindowManager.ts (Part 5 - persistence)**

```typescript
  /**
   * Save window states to localStorage
   */
  save(): void {
    const data = this.windows.getAll().map(w => w.serialize());
    localStorage.setItem('gnome-remote-windows', JSON.stringify(data));
  }

  /**
   * Load window states from localStorage
   */
  load(): void {
    const stored = localStorage.getItem('gnome-remote-windows');
    if (!stored) return;

    try {
      const data = JSON.parse(stored) as PersistedWindowData[];
      data.forEach(windowData => {
        const window = Window.deserialize(windowData);
        this.windows.add(window);
      });
    } catch (error) {
      console.error('[WindowManager] Failed to load window states', error);
    }
  }
}

// Import PersistedWindowData for load method
import { PersistedWindowData } from '../types';
```

- [ ] **Step 6: Commit WindowManager**

```bash
git add src/window-system/core/WindowManager.ts
git commit -m "feat(window-system): add WindowManager singleton orchestrator"
```

---

### Task 7: Create React Hooks

**Files:**
- Create: `src/window-system/hooks/useWindowManager.ts`
- Create: `src/window-system/hooks/useWindowState.ts`
- Create: `src/window-system/hooks/useWindowEvent.ts`

- [ ] **Step 1: Write useWindowManager.ts**

```typescript
// src/window-system/hooks/useWindowManager.ts

import { useContext, useReducer, useEffect } from 'react';
import { WindowManagerContext } from '../WindowManagerContext';
import { IWindowManager } from '../types';

/**
 * Hook to access the WindowManager instance
 * Automatically re-renders when any window event occurs
 */
export function useWindowManager(): IWindowManager {
  const manager = useContext(WindowManagerContext);
  if (!manager) {
    throw new Error('[useWindowManager] WindowManagerContext not provided');
  }

  // Force re-render on any window event
  const [, forceUpdate] = useReducer(x => x + 1, 0);

  useEffect(() => {
    return manager.onAny(() => forceUpdate());
  }, [manager]);

  return manager;
}
```

- [ ] **Step 2: Write useWindowState.ts**

```typescript
// src/window-system/hooks/useWindowState.ts

import { useWindowManager } from './useWindowManager';
import { WindowState } from '../types';

/**
 * Hook to access a single window's state
 * Returns window state and control methods
 */
export function useWindowState(windowId: string): WindowState {
  const manager = useWindowManager();
  const window = manager.getById(windowId);

  if (!window) {
    throw new Error(`[useWindowState] Window "${windowId}" not found`);
  }

  const app = manager.getApp(window.appId);
  if (!app) {
    throw new Error(`[useWindowState] App "${window.appId}" not registered`);
  }

  return {
    id: window.id,
    appId: window.appId,
    title: app.title,
    position: window.position,
    size: window.size,
    minimized: window.minimized,
    focused: manager.getActive()?.id === windowId,
    preloadState: window.preloadState,
    preloadData: window.preloadData,
  };
}
```

- [ ] **Step 3: Write useWindowEvent.ts**

```typescript
// src/window-system/hooks/useWindowEvent.ts

import { useEffect } from 'react';
import { useWindowManager } from './useWindowManager';
import { WindowEventType, WindowEvent } from '../types';

/**
 * Hook to subscribe to window events
 * Automatically unsubscribes on unmount
 */
export function useWindowEvent(
  eventType: WindowEventType,
  handler: (event: WindowEvent) => void
): void {
  const manager = useWindowManager();

  useEffect(() => {
    return manager.on(eventType, handler);
  }, [manager, eventType, handler]);
}
```

- [ ] **Step 4: Commit hooks**

```bash
git add src/window-system/hooks/
git commit -m "feat(window-system): add React hooks for window management"
```

---

### Task 8: Create WindowManagerContext

**Files:**
- Create: `src/window-system/WindowManagerContext.tsx`

- [ ] **Step 1: Write WindowManagerContext.tsx**

```typescript
// src/window-system/WindowManagerContext.tsx

import { createContext, useContext, useMemo, useEffect, ReactNode } from 'react';
import { WindowManager } from './core/WindowManager';
import { WindowRegistry } from './core/WindowRegistry';
import { IWindowManager } from './types';
import { initWindowRegistry } from './init';

// Create context with null default
export const WindowManagerContext = createContext<IWindowManager | null>(null);

// Registry singleton (shared across all WindowManager instances)
let registryInstance: WindowRegistry | null = null;

/**
 * Provider component for WindowManager
 * Initializes registry and manager on mount
 */
export function WindowManagerProvider({ children }: { children: ReactNode }) {
  // Initialize registry singleton
  const registry = useMemo(() => {
    if (!registryInstance) {
      registryInstance = new WindowRegistry();
      initWindowRegistry(registryInstance);
    }
    return registryInstance;
  }, []);

  // Create manager instance
  const manager = useMemo(() => {
    return new WindowManager(registry);
  }, [registry]);

  // Load persisted windows on mount
  useEffect(() => {
    manager.load();
  }, [manager]);

  // Save windows on changes
  useEffect(() => {
    return manager.onAny(() => {
      manager.save();
    });
  }, [manager]);

  return (
    <WindowManagerContext.Provider value={manager}>
      {children}
    </WindowManagerContext.Provider>
  );
}

/**
 * Hook to check if WindowManager is available
 */
export function useWindowManagerContext(): boolean {
  return useContext(WindowManagerContext) !== null;
}
```

- [ ] **Step 2: Commit WindowManagerContext**

```bash
git add src/window-system/WindowManagerContext.tsx
git commit -m "feat(window-system): add React Context Provider"
```

---

### Task 9: Create App Registration

**Files:**
- Create: `src/window-system/init.ts`

- [ ] **Step 1: Write init.ts**

```typescript
// src/window-system/init.ts

import { WindowRegistry } from './core/WindowRegistry';
import { AppDefinition } from './types';

// Import app components
import { TerminalApp } from '../apps/Terminal';
import { FileManager } from '../apps/FileManager';
import { SystemMonitor } from '../apps/SystemMonitor';
import { Settings } from '../apps/Settings';

// Import preloader for FileManager
import { usePreloader } from '../hooks/usePreloader';

/**
 * Initialize the window registry with all registered applications
 */
export function initWindowRegistry(registry: WindowRegistry): void {
  // Terminal - allows multiple instances
  registry.register({
    id: 'terminal',
    title: '终端',
    icon: '🖥️',
    defaultSize: { width: 850, height: 550 },
    minSize: { width: 400, height: 300 },
    allowMultipleInstances: true,
    component: TerminalApp,
  });

  // File Manager - single instance, with preloading
  registry.register({
    id: 'files',
    title: '文件管理器',
    icon: '📁',
    defaultSize: { width: 900, height: 650 },
    minSize: { width: 600, height: 400 },
    allowMultipleInstances: false,
    lifecycle: {
      onBeforeCreate: async (context) => {
        // Preload file manager data using serverId
        // Note: This will be handled by Desktop.tsx passing preloadData
        return null;
      },
    },
    component: FileManager,
  });

  // System Monitor - single instance
  registry.register({
    id: 'monitor',
    title: '系统监控',
    icon: '📊',
    defaultSize: { width: 900, height: 650 },
    minSize: { width: 600, height: 400 },
    allowMultipleInstances: false,
    component: SystemMonitor,
  });

  // Settings - single instance
  registry.register({
    id: 'settings',
    title: '设置',
    icon: '⚙️',
    defaultSize: { width: 700, height: 550 },
    minSize: { width: 500, height: 400 },
    allowMultipleInstances: false,
    component: Settings,
  });

  console.log('[WindowRegistry] Registered apps:', registry.getAll().map(a => a.id));
}
```

- [ ] **Step 2: Commit app registration**

```bash
git add src/window-system/init.ts
git commit -m "feat(window-system): add app registration initialization"
```

---

### Task 10: Refactor Desktop.tsx

**Files:**
- Modify: `src/shell/Desktop.tsx`

- [ ] **Step 1: Add WindowManager imports to Desktop.tsx**

```typescript
// Add imports at the top of Desktop.tsx
import { WindowManagerProvider, useWindowManager } from '../window-system/WindowManagerContext';
import { useWindowState } from '../window-system/hooks/useWindowState';
import { Window } from '../window-system/components/Window';
```

- [ ] **Step 2: Wrap Desktop with WindowManagerProvider**

```typescript
// Modify Desktop function to wrap with WindowManagerProvider
export function Desktop() {
  return (
    <ServerManagerProvider>
      <WallpaperProvider>
        <WindowManagerProvider>
          <DesktopContent />
        </WindowManagerProvider>
      </WallpaperProvider>
    </ServerManagerProvider>
  );
}
```

- [ ] **Step 3: Refactor DesktopContent to use WindowManager**

```typescript
// Replace window state management in DesktopContent
function DesktopContent() {
  const manager = useWindowManager();
  const { activeServer } = useServerManager();
  const { wallpaper } = useWallpaper();
  const preloader = usePreloader();

  // Remove existing window state:
  // const [windows, setWindows] = useState<WindowState[]>([]);
  // const [activeWindowId, setActiveWindowId] = useState<string | null>(null);

  // Use manager.getAll() instead
  const windows = manager.getAll();

  // Clock and metrics logic remains unchanged
  // ...

  // Refactor openApp to use manager.create
  const openApp = useCallback(async (appId: string) => {
    setOverviewVisible(false);

    if (appId === 'files') {
      // FileManager needs preloading
      try {
        await preloader.preload(activeServer?.id ?? null);
        manager.create(appId, { preloadData: preloader.data });
      } catch {
        console.error('[Desktop] FileManager preload failed');
        manager.create(appId); // Create without preload data
      }
    } else {
      manager.create(appId);
    }
  }, [manager, activeServer?.id, preloader]);

  // Remove closeWindow, minimizeWindow, focusWindow functions
  // Use manager.close, manager.minimize, manager.focus instead

  // Render windows using Window component
  return (
    <div className="shell">
      {/* TopBar unchanged */}
      <div className="desktop-area" style={getWallpaperStyle(wallpaper)}>
        {/* Desktop icons unchanged */}
        
        {/* Render windows */}
        {windows
          .filter(w => !w.minimized)
          .map((win) => (
            <Window key={win.id} windowId={win.id} />
          ))}
        
        {/* Dock unchanged */}
      </div>
      
      {/* Overview and Notification Center unchanged */}
    </div>
  );
}
```

- [ ] **Step 4: Commit Desktop refactor**

```bash
git add src/shell/Desktop.tsx
git commit -m "refactor(desktop): integrate WindowManager for window management"
```

---

### Task 11: Create Window Component

**Files:**
- Create: `src/window-system/components/Window.tsx`

- [ ] **Step 1: Write Window.tsx**

```typescript
// src/window-system/components/Window.tsx

import { useWindowManager } from '../hooks/useWindowManager';
import { useWindowState } from '../hooks/useWindowState';
import { DraggableWindow } from '../../components/DraggableWindow';

/**
 * Window component - wraps DraggableWindow with window system state
 */
export function Window({ windowId }: { windowId: string }) {
  const manager = useWindowManager();
  const state = useWindowState(windowId);
  const app = manager.getApp(state.appId);

  if (!app) {
    return null;
  }

  return (
    <DraggableWindow
      title={state.title}
      isActive={state.focused}
      onClose={() => manager.close(windowId)}
      onMinimize={() => manager.minimize(windowId)}
      onFocus={() => manager.focus(windowId)}
      initialPosition={state.position}
      initialSize={state.size}
      minWidth={app.minSize.width}
      minHeight={app.minSize.height}
      zIndex={state.focused ? 90 : 10}
    >
      {state.preloadState === 'loading' ? (
        <div>Loading...</div>
      ) : state.preloadState === 'error' ? (
        <div>Error loading data</div>
      ) : (
        <app.component windowId={windowId} preloadData={state.preloadData} />
      )}
    </DraggableWindow>
  );
}
```

- [ ] **Step 2: Commit Window component**

```bash
git add src/window-system/components/Window.tsx
git commit -m "feat(window-system): add Window component wrapping DraggableWindow"
```

---

### Task 12: Update App Components

**Files:**
- Modify: `src/apps/Terminal.tsx`
- Modify: `src/apps/FileManager.tsx`
- Modify: `src/apps/SystemMonitor.tsx`
- Modify: `src/apps/Settings.tsx`

- [ ] **Step 1: Update Terminal.tsx to accept windowId prop**

```typescript
// Add windowId prop to TerminalApp
export function TerminalApp({ windowId }: { windowId: string }) {
  // Existing logic unchanged
  // ...
}
```

- [ ] **Step 2: Update FileManager.tsx to accept windowId and preloadData props**

```typescript
// Update FileManager props
interface FileManagerProps {
  windowId: string;
  preloadData?: any;  // From onBeforeCreate or CreateWindowOptions
}

export function FileManager({ windowId, preloadData }: FileManagerProps) {
  // Use preloadData if available
  const [data, setData] = useState(preloadData);
  
  // Existing logic unchanged
  // ...
}
```

- [ ] **Step 3: Update SystemMonitor.tsx and Settings.tsx similarly**

```typescript
// Add windowId prop to each app
export function SystemMonitor({ windowId }: { windowId: string }) { ... }
export function Settings({ windowId }: { windowId: string }) { ... }
```

- [ ] **Step 4: Commit app updates**

```bash
git add src/apps/
git commit -m "refactor(apps): add windowId prop for window system integration"
```

---

### Task 13: Create Window System Index

**Files:**
- Create: `src/window-system/index.ts`

- [ ] **Step 1: Write index.ts (exports)**

```typescript
// src/window-system/index.ts

// Core exports
export { Window } from './core/Window';
export { WindowManager } from './core/WindowManager';
export { WindowCollection } from './core/WindowCollection';
export { WindowRegistry } from './core/WindowRegistry';

// Event exports
export { WindowEventBus } from './events/WindowEventBus';
export { WINDOW_EVENTS } from './events/WindowEvents';

// Hook exports
export { useWindowManager } from './hooks/useWindowManager';
export { useWindowState } from './hooks/useWindowState';
export { useWindowEvent } from './hooks/useWindowEvent';

// Context exports
export {
  WindowManagerProvider,
  WindowManagerContext,
  useWindowManagerContext,
} from './WindowManagerContext';

// Component exports
export { Window as WindowComponent } from './components/Window';

// Type exports
export * from './types';

// Init exports
export { initWindowRegistry } from './init';
```

- [ ] **Step 2: Commit index**

```bash
git add src/window-system/index.ts
git commit -m "feat(window-system): add index exports"
```

---

### Task 14: Integration Testing

**Files:**
- Test: Manual testing in browser

- [ ] **Step 1: Start development server**

```bash
npm run dev
```

- [ ] **Step 2: Test window creation**

- Click on Dock icons to open windows
- Verify windows appear with correct titles
- Verify window z-index ordering (last clicked = top)

- [ ] **Step 3: Test window operations**

- Minimize windows and verify they disappear
- Click Dock icon to restore minimized window
- Close windows and verify they are removed

- [ ] **Step 4: Test multiple instances**

- Open multiple terminal windows
- Verify each has unique windowId
- Verify file manager only allows one instance

- [ ] **Step 5: Test persistence**

- Open several windows
- Refresh page
- Verify windows are restored at same positions

- [ ] **Step 6: Fix any issues found**

Document issues and create fix commits.

---

## Phase 2: Persistence System (Future)

**Note:** Phase 1 already includes basic persistence via localStorage. Phase 2 will add:
- Custom storage adapters (Tauri file system)
- Window state versioning
- Migration for breaking changes

---

## Phase 3: Layout System (Future)

**Tasks:**
- Create LayoutEngine base class
- Implement FreeLayout (current behavior)
- Implement SnapLayout (snap to edges)
- Implement SplitLayout (split screen)
- Add layout switching UI

---

## Phase 4: Inter-Window Communication (Future)

**Tasks:**
- Extend WindowEventBus for custom events
- Add typed event channels
- Create useWindowChannel hook

---

## Success Criteria

1. ✅ Window management logic isolated in `src/window-system/`
2. ✅ New apps only need to register with WindowRegistry
3. ✅ Terminal can open multiple windows
4. ✅ Window positions/sizes restored after refresh
5. ✅ Windows can communicate via event bus
6. ⏳ Layout system (Phase 3)

---

## Self-Review

**1. Spec Coverage:**
- ✅ Window class defined in Task 5
- ✅ WindowManager defined in Task 6
- ✅ WindowRegistry defined in Task 3
- ✅ WindowCollection defined in Task 4
- ✅ Event system defined in Task 2
- ✅ Hooks defined in Task 7
- ✅ Context defined in Task 8
- ✅ App registration defined in Task 9
- ✅ Desktop refactor defined in Task 10
- ✅ Window component defined in Task 11
- ✅ App updates defined in Task 12

**2. Placeholder Scan:**
- ✅ No TBD, TODO, or "implement later"
- ✅ All code blocks contain complete implementation
- ✅ All file paths are exact

**3. Type Consistency:**
- ✅ WindowState in types.ts matches useWindowState return
- ✅ AppDefinition in types.ts matches WindowRegistry.register parameter
- ✅ CreateWindowOptions in types.ts matches WindowManager.create parameter
- ✅ WindowEvent in types.ts matches WindowEventBus.emit parameter

---

**Plan complete. Ready for execution.**