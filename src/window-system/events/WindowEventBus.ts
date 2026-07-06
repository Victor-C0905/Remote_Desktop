// src/window-system/events/WindowEventBus.ts

import { WindowEventType, WindowEvent, EventHandler, Unsubscribe } from '../types';

/**
 * Event bus for inter-window communication
 * Uses EventEmitter pattern for pub/sub messaging
 */
export class WindowEventBus {
  private listeners: Map<WindowEventType, Set<EventHandler>> = new Map();
  private anyListeners: Set<(event: WindowEvent) => void> = new Set();

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
  onAny(handler: (event: WindowEvent) => void): Unsubscribe {
    this.anyListeners.add(handler as any);
    return () => {
      this.anyListeners.delete(handler as any);
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
    this.anyListeners.forEach(handler => handler(event));
  }

  /**
   * Clear all listeners (for cleanup)
   */
  clear(): void {
    this.listeners.clear();
    this.anyListeners.clear();
  }
}