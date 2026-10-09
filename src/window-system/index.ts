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
export { useWindowEvent } from './hooks/useWindowEvent';

// Context exports
export {
  WindowManagerProvider,
  WindowManagerContext,
  useWindowManagerContext,
  useWindowGlobalState,
  useWindowState,
} from './WindowManagerContext';

// Component exports
export { Window as WindowComponent } from './components/Window';

// Type exports
export * from './types';

// Persistence exports
export { WindowSerializer } from './persistence/WindowSerializer';
export { TauriStorageAdapter } from './persistence/TauriStorageAdapter';
export type { StorageAdapter } from './persistence/StorageAdapter';

// Init exports
export { initWindowRegistry } from './init';