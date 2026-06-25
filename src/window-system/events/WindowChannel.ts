// src/window-system/events/WindowChannel.ts

import { WindowEventBus } from './WindowEventBus';

/**
 * Typed event channel for inter-window communication
 * Provides a structured way for windows to communicate
 */

// Custom event types (extend WindowEventType)
export type CustomEventType =
  | 'file:created'
  | 'file:deleted'
  | 'file:renamed'
  | 'terminal:command-executed'
  | 'settings:changed'
  | 'server:connected'
  | 'server:disconnected';

// Typed event payload map
export interface EventPayloadMap {
  'file:created': { path: string; content?: string };
  'file:deleted': { path: string };
  'file:renamed': { oldPath: string; newPath: string };
  'terminal:command-executed': { command: string; output: string };
  'settings:changed': { key: string; value: any };
  'server:connected': { serverId: string; serverName: string };
  'server:disconnected': { serverId: string };
}

// Typed event structure
export interface TypedWindowEvent<T extends CustomEventType> {
  type: T;
  sourceWindowId: string;
  targetWindowId?: string; // Optional: for direct messaging
  timestamp: number;
  payload: EventPayloadMap[T];
}

/**
 * Window channel for typed event communication
 */
export class WindowChannel {
  private eventBus: WindowEventBus;
  private customListeners: Map<CustomEventType, Set<(event: any) => void>> = new Map();

  constructor(eventBus: WindowEventBus) {
    this.eventBus = eventBus;
  }

  /**
   * Subscribe to a typed event
   */
  on<T extends CustomEventType>(
    eventType: T,
    handler: (event: TypedWindowEvent<T>) => void
  ): () => void {
    if (!this.customListeners.has(eventType)) {
      this.customListeners.set(eventType, new Set());
    }
    this.customListeners.get(eventType)!.add(handler);

    return () => {
      this.customListeners.get(eventType)?.delete(handler);
    };
  }

  /**
   * Emit a typed event
   */
  emit<T extends CustomEventType>(event: TypedWindowEvent<T>): void {
    // Notify specific event listeners
    const handlers = this.customListeners.get(event.type);
    if (handlers) {
      handlers.forEach(handler => handler(event));
    }

    // Also emit to event bus for any-event listeners
    this.eventBus.emit({
      type: event.type as any,
      windowId: event.sourceWindowId,
      timestamp: event.timestamp,
      payload: event.payload,
    });
  }

  /**
   * Send a message to a specific window
   */
  sendTo<T extends CustomEventType>(
    targetWindowId: string,
    event: TypedWindowEvent<T>
  ): void {
    event.targetWindowId = targetWindowId;
    this.emit(event);
  }

  /**
   * Broadcast to all windows
   */
  broadcast<T extends CustomEventType>(event: TypedWindowEvent<T>): void {
    this.emit(event);
  }

  /**
   * Clear all listeners
   */
  clear(): void {
    this.customListeners.clear();
  }
}