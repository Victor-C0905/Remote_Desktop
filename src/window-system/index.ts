// src/window-system/index.ts

// Core exports
export { Window } from './core/Window';
export { WindowManager } from './core/WindowManager';
export { WindowCollection } from './core/WindowCollection';
export { WindowRegistry } from './core/WindowRegistry';

// Event exports
export { WindowEventBus } from './events/WindowEventBus';

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

// Type exports
export * from './types';

// Init exports
export { initWindowRegistry } from './init';