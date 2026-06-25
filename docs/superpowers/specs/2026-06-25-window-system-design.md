# Window System Architecture Design

## Overview

This document describes the design for a professional-grade window management system for the GNOME Remote desktop application. The system provides a Windows Forms/WPF-like architecture with support for multiple window instances, state persistence, inter-window communication, and layout management.

## Goals

1. **Maintainability**: Decouple window management logic from UI components
2. **Extensibility**: Easy to add new applications without modifying core code
3. **Professional Architecture**: Windows Forms/WPF-like window management
4. **Core Features**:

   * Multiple window instances per application

   * Window state persistence across sessions

   * Inter-window communication via event bus

   * Layout system (snap, split)

## Architecture

### Directory Structure

```
src/window-system/
├── core/
│   ├── Window.ts              # Window class (state + behavior)
│   ├── WindowManager.ts       # Window manager (singleton)
│   ├── WindowCollection.ts    # Window collection (CRUD operations)
│   └── WindowRegistry.ts      # Application registry
├── layout/
│   ├── LayoutEngine.ts        # Base layout engine
│   ├── FreeLayout.ts          # Free positioning (current)
│   ├── SnapLayout.ts          # Snap to edges
│   └── SplitLayout.ts         # Split screen layout
├── persistence/
│   ├── WindowSerializer.ts    # State serialization
│   └── StorageAdapter.ts      # Storage adapter (localStorage/file)
├── events/
│   ├── WindowEventBus.ts      # Event bus
│   └── WindowEvents.ts        # Event type definitions
├── hooks/
│   ├── useWindowManager.ts    # Window manager hook
│   ├── useWindowState.ts      # Single window state hook
│   └── useWindowEvent.ts      # Window event hook
└── types.ts                   # Type definitions
```

### Core Classes

#### Window Class

```typescript
export class Window {
  readonly id: string;
  readonly appId: string;

  // Persistable state
  private _position: { x: number; y: number };
  private _size: { width: number; height: number };
  private _minimized: boolean;
  private _layout: 'free' | 'snap' | 'split';

  // Runtime state
  private _activatedAt: number;
  private _preloadState: 'loading' | 'ready' | 'error';
  private _preloadData: any;

  // Lifecycle hooks
  readonly lifecycle: WindowLifecycle;

  // Methods
  focus(): void;
  minimize(): void;
  restore(): void;
  close(): void;
  resize(size: { width: number; height: number }): void;
  move(position: { x: number; y: number }): void;

  // Serialization
  serialize(): PersistedWindowData;
  static deserialize(data: PersistedWindowData): Window;
}
```

#### WindowManager Class

```typescript
export class WindowManager {
  private windows: WindowCollection;
  private registry: WindowRegistry;
  private layoutEngine: LayoutEngine;
  private eventBus: WindowEventBus;
  private serializer: WindowSerializer;

  // Window operations
  create(appId: string, options?: CreateWindowOptions): Promise<Window>;
  close(windowId: string): void;
  focus(windowId: string): void;
  minimize(windowId: string): void;

  // Queries
  getById(windowId: string): Window | undefined;
  getByAppId(appId: string): Window[];
  getAll(): Window[];
  getActive(): Window | undefined;

  // Layout
  setLayout(windowId: string, layout: LayoutType): void;
  snapToEdge(windowId: string, edge: 'left' | 'right' | 'top' | 'bottom'): void;

  // Persistence
  save(): void;
  load(): void;

  // Events
  on(event: WindowEvent, handler: EventHandler): Unsubscribe;
  emit(event: WindowEvent): void;
}
```

#### WindowRegistry

```typescript
export interface AppDefinition {
  id: string;
  title: string;
  icon: string;

  // Window configuration
  defaultSize: { width: number; height: number };
  minSize: { width: number; height: number };
  allowMultipleInstances: boolean;

  // Lifecycle hooks
  onBeforeCreate?: (context: CreateContext) => Promise<any>;
  onAfterCreate?: (window: Window) => void;
  onBeforeClose?: (window: Window) => boolean;
  onAfterClose?: (window: Window) => void;

  // Component
  component: React.ComponentType<{ windowId: string; preloadData?: any }>;
}

export interface CreateContext {
  serverId?: string;
  existingPreloadData?: any;  // From CreateWindowOptions
}

export class WindowRegistry {
  private apps: Map<string, AppDefinition> = new Map();

  register(app: AppDefinition): void;
  get(appId: string): AppDefinition | undefined;
  getAll(): AppDefinition[];
}
```

### Layout System

```typescript
export abstract class LayoutEngine {
  abstract calculateWindows(
    windows: Window[],
    containerSize: { width: number; height: number }
  ): Map<string, { x: number; y: number; width: number; height: number }>;

  abstract handleResize(
    window: Window,
    delta: { x: number; y: number },
    containerSize: { width: number; height: number }
  ): void;
}

// SnapLayout: Windows snap to screen edges
export class SnapLayout extends LayoutEngine {
  private snapThreshold = 20;

  calculateWindows(windows, containerSize) {
    // Detect if window is near edge (within snapThreshold pixels)
    // Auto-snap and resize to fill half screen
  }

  // Layout switching:
  // - User manually triggers via manager.snapToEdge()
  // - Or window automatically snaps when dragged near edge
}

// SplitLayout: Split screen like VS Code
export class SplitLayout extends LayoutEngine {
  private splits: Map<string, { direction: 'horizontal' | 'vertical'; ratio: number }>;

  split(windowId: string, direction: 'horizontal' | 'vertical'): void;
  unsplit(windowId: string): void;
}
```

### Persistence System

```typescript
export class WindowSerializer {
  serialize(windows: Window[]): string {
    return JSON.stringify(windows.map(w => w.serialize()));
  }

  deserialize(data: string): PersistedWindowData[] {
    return JSON.parse(data);
  }
}

export interface StorageAdapter {
  save(key: string, data: string): void;
  load(key: string): string | null;
}

export class LocalStorageAdapter implements StorageAdapter {
  save(key: string, data: string) {
    localStorage.setItem(key, data);
  }

  load(key: string) {
    return localStorage.getItem(key);
  }
}
```

### Event System

```typescript
export type WindowEventType =
  | 'window:created'
  | 'window:closed'
  | 'window:focused'
  | 'window:minimized'
  | 'window:restored'
  | 'window:resized'
  | 'window:moved'
  | 'window:layout-changed';

export interface WindowEvent {
  type: WindowEventType;
  windowId: string;
  timestamp: number;
  payload?: any;
}

export class WindowEventBus {
  private listeners: Map<WindowEventType, Set<EventHandler>> = new Map();

  on(eventType: WindowEventType, handler: EventHandler): Unsubscribe;
  emit(event: WindowEvent): void;
}
```

### React Integration

```typescript
// useWindowManager: Access window manager
export function useWindowManager() {
  const manager = useContext(WindowManagerContext);
  const [, forceUpdate] = useReducer(x => x + 1, 0);

  useEffect(() => {
    return manager.onAny(() => forceUpdate());
  }, [manager]);

  return manager;
}

// useWindowState: Access single window state
export function useWindowState(windowId: string) {
  const manager = useWindowManager();
  const window = manager.getById(windowId);

  if (!window) throw new Error(`Window ${windowId} not found`);

  return {
    position: window.position,
    size: window.size,
    minimized: window.minimized,
    focused: window.focused,
    focus: () => window.focus(),
    minimize: () => window.minimize(),
    close: () => window.close(),
  };
}

// useWindowEvent: Subscribe to window events
export function useWindowEvent(
  eventType: WindowEventType,
  handler: (event: WindowEvent) => void
) {
  const manager = useWindowManager();

  useEffect(() => {
    return manager.on(eventType, handler);
  }, [manager, eventType, handler]);
}
```

## Migration Plan

### Phase 1: Core Architecture (\~800 lines)

**Goal**: Establish window system core, maintain existing functionality

**New Files**:

```
src/window-system/
├── core/
│   ├── Window.ts              # ~150 lines
│   ├── WindowManager.ts       # ~200 lines
│   ├── WindowCollection.ts    # ~100 lines
│   └── WindowRegistry.ts      # ~80 lines
├── events/
│   ├── WindowEventBus.ts      # ~60 lines
│   └── WindowEvents.ts        # ~30 lines
├── hooks/
│   ├── useWindowManager.ts    # ~50 lines
│   └── useWindowState.ts      # ~40 lines
└── types.ts                   # ~90 lines
```

**Migration Steps**:

1. Create WindowRegistry and register existing applications
2. Refactor Desktop.tsx to use WindowManager
3. Refactor DraggableWindow to Window component

### Phase 2: Persistence System (\~200 lines)

**Goal**: Window state restoration after refresh

**New Files**:

```
src/window-system/persistence/
├── WindowSerializer.ts        # ~80 lines
└── StorageAdapter.ts          # ~50 lines
```

### Phase 3: Layout System (\~400 lines)

**Goal**: Support window snapping and split screen

**New Files**:

```
src/window-system/layout/
├── LayoutEngine.ts            # ~80 lines
├── FreeLayout.ts              # ~60 lines
├── SnapLayout.ts              # ~150 lines
└── SplitLayout.ts             # ~110 lines
```

### Phase 4: Inter-Window Communication (\~100 lines)

**Goal**: Event-based communication between windows

## Timeline

| Phase     | Content                    | Lines      | Estimated Effort |
| --------- | -------------------------- | ---------- | ---------------- |
| Phase 1   | Core Architecture          | \~800      | 2-3 days         |
| Phase 2   | Persistence System         | \~200      | 0.5 day          |
| Phase 3   | Layout System              | \~400      | 1-2 days         |
| Phase 4   | Inter-Window Communication | \~100      | 0.5 day          |
| **Total** | <br />                     | **\~1500** | **4-6 days**     |

## Type Definitions

```typescript
// types.ts

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

export interface CreateWindowOptions {
  position?: { x: number; y: number };
  size?: { width: number; height: number };
  serverId?: string;
  preloadData?: any;  // If provided, skips onBeforeCreate hook
}

export interface WindowLifecycle {
  onBeforeCreate?: () => Promise<any>;
  onAfterCreate?: (window: Window) => void;
  onBeforeClose?: (window: Window) => boolean;
  onAfterClose?: (window: Window) => void;
}

export type LayoutType = 'free' | 'snap' | 'split';

export type EventHandler = (event: WindowEvent) => void;
export type Unsubscribe = () => void;
```

## Usage Examples

### Registering an Application

```typescript
// src/window-system/init.ts
import { TerminalApp } from '../apps/Terminal';
import { FileManager } from '../apps/FileManager';

export function initWindowRegistry(registry: WindowRegistry) {
  registry.register({
    id: 'terminal',
    title: '终端',
    icon: '🖥️',
    defaultSize: { width: 850, height: 550 },
    minSize: { width: 400, height: 300 },
    allowMultipleInstances: true,
    component: TerminalApp,
  });

  registry.register({
    id: 'files',
    title: '文件管理器',
    icon: '📁',
    defaultSize: { width: 900, height: 650 },
    minSize: { width: 600, height: 400 },
    allowMultipleInstances: false,
    onBeforeCreate: async (context) => {
      // Preload data using context.serverId
      return await preloadFileManagerData(context.serverId);
    },
    component: FileManager,
  });
}
```

### Using in Desktop Component

```typescript
// src/shell/Desktop.tsx
function DesktopContent() {
  const manager = useWindowManager();
  const { activeServer } = useServerManager();

  const openApp = useCallback((appId: string) => {
    manager.create(appId, { serverId: activeServer?.id });
  }, [manager, activeServer]);

  return (
    <div className="shell">
      <TopBar ... />
      <div className="desktop-area">
        {manager.getAll().map((window) => (
          <Window key={window.id} windowId={window.id} />
        ))}
        <Dock ... />
      </div>
    </div>
  );
}
```

### Inter-Window Communication

```typescript
// Terminal window: Notify file creation
useWindowEvent('file-created', (event) => {
  if (event.payload.path) {
    // Refresh file list
  }
});

// Emit event
manager.emit({
  type: 'file-created',
  windowId: terminalWindowId,
  payload: { path: '/home/user/test.txt' },
});
```

## Success Criteria

1. **Maintainability**: Window management logic isolated in window-system/
2. **Extensibility**: New applications only need to register with WindowRegistry
3. **Multiple Instances**: Terminal can open multiple windows
4. **Persistence**: Window positions/sizes restored after refresh
5. **Communication**: Windows can communicate via event bus
6. **Layout**: Windows can snap to edges and split screen

